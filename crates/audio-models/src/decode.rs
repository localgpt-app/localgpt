//! Reading audio files, resampling, and writing WAV.

use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};

/// The longest recording a model is given. A model's input is held in
/// memory as `f32` (about 21 MB a minute for 44.1 kHz stereo); a longer one
/// is refused rather than half-read.
pub const MAX_SECONDS: f32 = 20.0 * 60.0;

/// A decoded recording: one `Vec` per channel, at its own sample rate.
#[derive(Debug, Clone)]
pub struct Audio {
    pub sample_rate: u32,
    pub channels: Vec<Vec<f32>>,
}

impl Audio {
    /// Its length, in seconds.
    pub fn seconds(&self) -> f32 {
        self.frames() as f32 / self.sample_rate.max(1) as f32
    }

    /// Samples per channel.
    pub fn frames(&self) -> usize {
        self.channels.first().map_or(0, Vec::len)
    }

    /// The channels averaged.
    pub fn mono(&self) -> Vec<f32> {
        let n = self.channels.len().max(1) as f32;
        (0..self.frames())
            .map(|i| self.channels.iter().map(|c| c[i]).sum::<f32>() / n)
            .collect()
    }

    /// Two channels: a mono recording doubled, anything wider cut to its
    /// first two.
    pub fn stereo(&self) -> [Vec<f32>; 2] {
        match self.channels.as_slice() {
            [] => [Vec::new(), Vec::new()],
            [one] => [one.clone(), one.clone()],
            [left, right, ..] => [left.clone(), right.clone()],
        }
    }
}

/// Decode an audio file (WAV, FLAC, Ogg Vorbis, MP3, AAC/M4A, ALAC, AIFF).
pub fn read(path: &Path) -> Result<Audio> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
    use symphonia::core::errors::Error;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .with_context(|| format!("{} is not a recording this can read", path.display()))?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| anyhow!("{} has no audio track", path.display()))?;
    let track_id = track.id;
    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| anyhow!("{} does not state its sample rate", path.display()))?;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .with_context(|| format!("no decoder for {}", path.display()))?;
    let limit = (MAX_SECONDS * sample_rate as f32) as usize;

    let mut channels: Vec<Vec<f32>> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(Error::ResetRequired) => break,
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            // A damaged packet is skipped, as players do.
            Err(Error::DecodeError(_)) => continue,
            Err(e) => return Err(e).with_context(|| format!("decoding {}", path.display())),
        };
        let spec = *decoded.spec();
        let width = spec.channels.count().max(1);
        let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        buffer.copy_interleaved_ref(decoded);
        if channels.is_empty() {
            channels = vec![Vec::new(); width];
        }
        for frame in buffer.samples().chunks_exact(width) {
            for (channel, sample) in channels.iter_mut().zip(frame) {
                channel.push(*sample);
            }
        }
        if channels[0].len() > limit {
            bail!(
                "{} is longer than {} minutes; trim it first",
                path.display(),
                MAX_SECONDS / 60.0
            );
        }
    }
    if channels.first().is_none_or(Vec::is_empty) {
        bail!("{} holds no samples", path.display());
    }
    Ok(Audio {
        sample_rate,
        channels,
    })
}

/// Resample one channel from `from` Hz to `to` Hz (windowed sinc), keeping
/// its timing: the output starts where the input does and is as long as the
/// input's duration at the new rate.
pub fn resample(samples: &[f32], from: u32, to: u32) -> Result<Vec<f32>> {
    use rubato::{
        Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
    };

    if from == to || samples.is_empty() {
        return Ok(samples.to_vec());
    }
    const CHUNK: usize = 4096;
    let params = SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 256,
        window: WindowFunction::BlackmanHarris2,
    };
    let ratio = to as f64 / from as f64;
    // rubato's sinc resampler starts half its filter before the first input
    // frame, so its output is already aligned with the input: nothing is
    // trimmed (a chirp test below holds it to that).
    let mut resampler = SincFixedIn::<f32>::new(ratio, 1.0, params, CHUNK, 1)?;
    let wanted = (samples.len() as f64 * ratio).round() as usize;
    let mut out = Vec::with_capacity(wanted + CHUNK);
    let (chunks, rest) = samples.as_chunks::<CHUNK>();
    for chunk in chunks {
        out.extend_from_slice(&resampler.process(&[&chunk[..]], None)?[0]);
    }
    if !rest.is_empty() {
        out.extend_from_slice(&resampler.process_partial(Some(&[rest]), None)?[0]);
    }
    // Flush what the filter still holds until the output covers the input.
    while out.len() < wanted {
        let tail = resampler.process_partial::<&[f32]>(None, None)?;
        if tail[0].is_empty() {
            break;
        }
        out.extend_from_slice(&tail[0]);
    }
    out.resize(wanted, 0.0);
    Ok(out)
}

/// The recording at `rate`, every channel resampled.
pub fn resample_audio(audio: &Audio, rate: u32) -> Result<Audio> {
    Ok(Audio {
        sample_rate: rate,
        channels: audio
            .channels
            .iter()
            .map(|c| resample(c, audio.sample_rate, rate))
            .collect::<Result<_>>()?,
    })
}

/// Write `channels` (equal lengths) as 16-bit PCM WAV.
pub fn write_wav(path: &Path, channels: &[Vec<f32>], sample_rate: u32) -> Result<()> {
    let spec = hound::WavSpec {
        channels: channels.len() as u16,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)
        .with_context(|| format!("creating {}", path.display()))?;
    let frames = channels.first().map_or(0, Vec::len);
    for i in 0..frames {
        for channel in channels {
            let s = channel.get(i).copied().unwrap_or(0.0).clamp(-1.0, 1.0);
            writer.write_sample((s * i16::MAX as f32).round() as i16)?;
        }
    }
    writer.finalize()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, seconds: f32, hz: f32) -> Vec<f32> {
        (0..(rate as f32 * seconds) as usize)
            .map(|i| (i as f32 / rate as f32 * hz * std::f32::consts::TAU).sin() * 0.5)
            .collect()
    }

    #[test]
    fn a_written_wav_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tone.wav");
        let left = tone(22_050, 0.5, 440.0);
        let right: Vec<f32> = left.iter().map(|s| -s).collect();
        write_wav(&path, &[left.clone(), right], 22_050).unwrap();
        let audio = read(&path).unwrap();
        assert_eq!(audio.sample_rate, 22_050);
        assert_eq!(audio.channels.len(), 2);
        assert_eq!(audio.frames(), left.len());
        assert!((audio.channels[0][100] - left[100]).abs() < 1e-3);
        assert!(
            audio.mono().iter().all(|s| s.abs() < 1e-3),
            "opposites cancel"
        );
    }

    #[test]
    fn resampling_keeps_length_and_timing() {
        // A chirp, which unlike a tone has one alignment only.
        let chirp = |rate: u32| -> Vec<f32> {
            (0..rate as usize)
                .map(|i| {
                    let t = i as f32 / rate as f32;
                    (t * t * 440.0 * std::f32::consts::TAU).sin() * 0.5
                })
                .collect()
        };
        let out = resample(&chirp(44_100), 44_100, 16_000).unwrap();
        assert_eq!(out.len(), 16_000);
        let reference = chirp(16_000);
        let error_at = |lag: i32| -> f32 {
            (2000..14_000usize)
                .map(|i| (out[(i as i32 + lag) as usize] - reference[i]).abs())
                .sum()
        };
        let best = (-50..=50)
            .min_by(|a, b| error_at(*a).total_cmp(&error_at(*b)))
            .unwrap();
        assert!(best.abs() <= 1, "the output is {best} samples off");
    }
}
