//! A song's waveform for the timeline: the loudest sample in each slice of
//! time, decoded with symphonia as Verse decodes songs. The soundtrack's own
//! `energy` is one value a second, too coarse to draw a song by.

use std::path::Path;

/// Slices per second.
pub const RATE: f32 = 40.0;

/// The peaks of the file at `path`, scaled so the loudest is `1`; `None`
/// when it doesn't decode.
pub fn decode(path: &Path) -> Option<Vec<f32>> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::DecoderOptions;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let file = std::fs::File::open(path).ok()?;
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
        .ok()?;
    let mut format = probed.format;
    let track = format.default_track()?;
    let track_id = track.id;
    let rate = track.codec_params.sample_rate?;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .ok()?;

    let per_slice = ((rate as f32 / RATE).round() as usize).max(1);
    let mut peaks = Vec::new();
    let (mut loudest, mut in_slice) = (0.0f32, 0usize);
    let mut buffer: Option<SampleBuffer<f32>> = None;
    while let Ok(packet) = format.next_packet() {
        if packet.track_id() != track_id {
            continue;
        }
        let Ok(decoded) = decoder.decode(&packet) else {
            continue;
        };
        let spec = *decoded.spec();
        let channels = spec.channels.count().max(1);
        let buf = match &mut buffer {
            Some(b) if b.capacity() >= decoded.capacity() * channels => b,
            _ => buffer.insert(SampleBuffer::new(decoded.capacity() as u64, spec)),
        };
        buf.copy_interleaved_ref(decoded);
        for frame in buf.samples().chunks_exact(channels) {
            loudest = frame.iter().fold(loudest, |m, s| m.max(s.abs()));
            in_slice += 1;
            if in_slice == per_slice {
                peaks.push(loudest);
                (loudest, in_slice) = (0.0, 0);
            }
        }
    }
    if in_slice > 0 {
        peaks.push(loudest);
    }
    if peaks.is_empty() {
        return None;
    }
    let top = peaks.iter().copied().fold(0.0f32, f32::max);
    if top > 0.0 {
        for peak in &mut peaks {
            *peak = (*peak / top * 1000.0).round() / 1000.0;
        }
    }
    Some(peaks)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mono 16-bit WAV: a second of a 440 Hz tone at half scale, then a
    /// second of silence.
    fn tone_then_silence(path: &Path) {
        let rate = 8_000u32;
        let mut samples: Vec<i16> = (0..rate)
            .map(|i| {
                let t = i as f32 / rate as f32;
                ((t * 440.0 * std::f32::consts::TAU).sin() * 16_384.0) as i16
            })
            .collect();
        samples.resize(2 * rate as usize, 0);
        let data_len = (samples.len() * 2) as u32;
        let mut bytes = Vec::with_capacity(44 + data_len as usize);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
        bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
        bytes.extend_from_slice(&rate.to_le_bytes());
        bytes.extend_from_slice(&(rate * 2).to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn the_waveform_is_loud_where_the_song_is_and_quiet_where_it_is_not() {
        let path =
            std::env::temp_dir().join(format!("localgpt-app-peaks-{}.wav", std::process::id()));
        tone_then_silence(&path);
        let peaks = decode(&path).expect("a WAV decodes");
        std::fs::remove_file(&path).ok();

        assert_eq!(peaks.len(), 2 * RATE as usize, "{RATE} slices a second");
        let (tone, silence) = peaks.split_at(RATE as usize);
        assert!(
            tone.iter().all(|&p| p > 0.9),
            "the tone is the loudest: {tone:?}"
        );
        assert!(silence.iter().all(|&p| p == 0.0), "then nothing");
        assert!(decode(Path::new("/nonexistent/song.mp3")).is_none());
    }
}
