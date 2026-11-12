//! Stems: a song split into drums, bass, vocals and other by Deezer's
//! Spleeter 4stems, with each stem's loudness per second.
//!
//! # The model
//!
//! Spleeter 4stems is four U-Nets, one per stem, each estimating its stem's
//! magnitude spectrogram from the mix's. What is installed is the fp32 ONNX
//! conversion `Best-Practice/spleeter-4stems-onnx` at revision `4866430`
//! (exported through sherpa-onnx's PyTorch port of the U-Net, with the ELU
//! activations the 4stems configuration names). Its fp16 files are half the
//! download, but fp16 graphs can lack CPU kernels in ONNX Runtime and the
//! fp32 ones run everywhere it does. Each graph, as ort reports it:
//!
//! ```text
//! input   x  f32 [2, splits, 512, 1024]   channel, 512-frame segment, frame, bin
//! output  y  f32 [2, splits, 512, 1024]   that stem's magnitude estimate
//! ```
//!
//! `splits` is dynamic and the bin axis fixed: the networks model the first
//! 1024 of the STFT's 2049 bins, up to 11 kHz at 44.1 kHz.
//!
//! # The processing
//!
//! The graphs take magnitudes and give magnitudes; everything around them is
//! here, as Spleeter's own `spleeter/model/__init__.py` does it (MIT; read,
//! not copied) for the 4stems configuration:
//!
//! - 44.1 kHz stereo: a mono recording is doubled, wider ones keep their
//!   first two channels, other rates are resampled;
//! - one frame (4096 samples) of zeros before the signal, then an STFT with a
//!   periodic Hann window of 4096 and a hop of 1024, zero-padded at the end
//!   to whole frames (TensorFlow's `pad_end`);
//! - the magnitudes of the first 1024 bins, zero-padded to whole 512-frame
//!   segments, into each stem's network. Here a run is one segment, which
//!   changes nothing: the segments are independent (the networks' batch norm
//!   runs in inference mode);
//! - ratio masks across the four estimates, `(est_i² + ε/4) / (Σ est_j² + ε)`
//!   with ε = 1e-10 (`separation_exponent` 2), which sum to one;
//! - the bins above 1024 take each frame's mean mask: Spleeter's
//!   `mask_extension: "average"`. The 4stems configuration ships `"zeros"`,
//!   which drops everything above 11 kHz (cymbals, breath, sibilance) from
//!   every stem; averaging keeps it, shared as the band below is, so the four
//!   stems still add up to the mix, which a world playing them together needs;
//! - each mask times the mix's complex STFT (its phase kept), then the
//!   inverse: overlap-add of the Hann-windowed frames scaled by 2/3
//!   (Spleeter's `WINDOW_COMPENSATION_FACTOR`; the squared periodic Hann at a
//!   quarter-frame hop sums to 3/2), from past the leading frame to the
//!   input's length.
//!
//! The output streams: a segment's samples are written once no later frame
//! reaches them, so a separation holds the input and one segment's spectra,
//! not four copies of the song.
//!
//! # Loudness
//!
//! Each stem's curve has one value per second of the output (the last over
//! what remains), aligned to a soundtrack's energy curve: the RMS of the
//! stem's two channels' mean over that second, divided by the stem's 95th
//! percentile of those values, but never by less than a tenth (−20 dB) of
//! the mix's own 95th percentile, so a stem the song lacks stays near zero
//! rather than having its leakage stretched to full scale; clamped to 0..1.
//! In [`NAMES`] order they are the Open World Format's `StemCurves`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use ort::session::Session;
use ort::value::TensorRef;
use realfft::num_complex::Complex;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

use crate::decode;

/// The rate the networks work at, and the stems are written at.
pub const SAMPLE_RATE: u32 = 44_100;

/// The stems, in the order of [`Stems`]' arrays and of a soundtrack's
/// `StemCurves`. Also the model's file names (`drums.onnx`, …).
pub const NAMES: [&str; 4] = ["drums", "bass", "vocals", "other"];

/// The STFT's frame and hop, in samples.
const FRAME: usize = 4096;
const HOP: usize = 1024;
/// Bins of a frame's spectrum.
const BINS: usize = FRAME / 2 + 1;
/// The bins the networks model (Spleeter's `F`).
const MODELLED: usize = 1024;
/// Frames in a network's segment (Spleeter's `T`).
const SEGMENT: usize = 512;
const EPSILON: f32 = 1e-10;
/// Undoes the squared window's overlap: Σ hann² at a quarter-frame hop is 3/2.
const WINDOW_COMPENSATION: f32 = 2.0 / 3.0;
/// The graphs' input and output.
const INPUT: &str = "x";
const OUTPUT: &str = "y";

/// What a separation made.
#[derive(Debug, Clone)]
pub struct Stems {
    /// The four WAVs, 16-bit stereo at [`SAMPLE_RATE`], in [`NAMES`] order.
    pub files: [PathBuf; 4],
    /// Each stem's loudness, one value in 0..1 per second, in [`NAMES`]
    /// order: a soundtrack's `StemCurves`.
    pub curves: [Vec<f32>; 4],
    /// The recording's length, in seconds.
    pub seconds: f32,
}

/// Split `input` with the model installed in `model_dir`, writing
/// `drums.wav`, `bass.wav`, `vocals.wav` and `other.wav` into `out_dir`.
pub fn separate(model_dir: &Path, input: &Path, out_dir: &Path) -> Result<Stems> {
    Model::load(model_dir)?.separate(input, out_dir)
}

/// The four networks, loaded once and run as often as wanted.
pub struct Model {
    /// In [`NAMES`] order.
    sessions: [Session; 4],
}

impl Model {
    /// Load the networks from the model's directory.
    pub fn load(dir: &Path) -> Result<Self> {
        let [drums, bass, vocals, other] = NAMES.map(|name| dir.join(format!("{name}.onnx")));
        Ok(Self {
            sessions: [open(&drums)?, open(&bass)?, open(&vocals)?, open(&other)?],
        })
    }

    /// Split `input` into the four stems, written into `out_dir`.
    pub fn separate(&mut self, input: &Path, out_dir: &Path) -> Result<Stems> {
        let audio = decode::read(input)?;
        let [left, right] = audio.stereo();
        let mix = [
            decode::resample(&left, audio.sample_rate, SAMPLE_RATE)?,
            decode::resample(&right, audio.sample_rate, SAMPLE_RATE)?,
        ];
        drop((audio, left, right));
        std::fs::create_dir_all(out_dir)
            .with_context(|| format!("creating {}", out_dir.display()))?;
        let files = NAMES.map(|name| out_dir.join(format!("{name}.wav")));
        let curves = self.write(&mix, &files).inspect_err(|_| {
            // Half-written stems are not stems.
            for file in &files {
                let _ = std::fs::remove_file(file);
            }
        })?;
        Ok(Stems {
            files,
            curves,
            seconds: mix[0].len() as f32 / SAMPLE_RATE as f32,
        })
    }

    /// Separate `mix` into `files`, answering the stems' loudness curves.
    fn write(&mut self, mix: &[Vec<f32>; 2], files: &[PathBuf; 4]) -> Result<[Vec<f32>; 4]> {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writers = Vec::with_capacity(files.len());
        for file in files {
            writers.push(
                hound::WavWriter::create(file, spec)
                    .with_context(|| format!("creating {}", file.display()))?,
            );
        }
        let mut loudness: [Loudness; 4] = Default::default();
        self.run(mix, |_, stems| {
            for ((writer, loudness), [left, right]) in
                writers.iter_mut().zip(&mut loudness).zip(stems)
            {
                for (l, r) in left.iter().zip(right) {
                    writer.write_sample(pcm(*l))?;
                    writer.write_sample(pcm(*r))?;
                }
                loudness.push(left, right);
            }
            Ok(())
        })?;
        for (writer, file) in writers.into_iter().zip(files) {
            writer
                .finalize()
                .with_context(|| format!("writing {}", file.display()))?;
        }
        let mut whole = Loudness::default();
        whole.push(&mix[0], &mix[1]);
        Ok(curves(loudness.map(|l| l.rms()), &whole.rms()))
    }

    /// Separate `mix`, two channels of equal length at [`SAMPLE_RATE`],
    /// handing `sink` each finished stretch of the four stems in order: the
    /// index of its first sample, and per stem (in [`NAMES`] order) its left
    /// and right samples.
    fn run(
        &mut self,
        mix: &[Vec<f32>; 2],
        mut sink: impl FnMut(usize, [[&[f32]; 2]; 4]) -> Result<()>,
    ) -> Result<()> {
        let frames = frame_count(mix[0].len());
        let mut stft = Stft::new();
        let mut spectra = vec![Complex::default(); 2 * SEGMENT * BINS];
        let mut magnitudes = vec![0f32; 2 * SEGMENT * MODELLED];
        let mut masks: [Vec<f32>; 4] = std::array::from_fn(|_| vec![0.0; BINS]);
        let mut masked = vec![Complex::default(); BINS];
        let mut ola = OverlapAdd::new(mix[0].len());
        for first in (0..frames).step_by(SEGMENT) {
            let count = SEGMENT.min(frames - first);
            // The segment's spectra, and the networks' input: the modelled
            // bins' magnitudes, zeros past the last frame.
            magnitudes.fill(0.0);
            for (channel, signal) in mix.iter().enumerate() {
                for t in 0..count {
                    let row = channel * SEGMENT + t;
                    let spectrum = &mut spectra[row * BINS..][..BINS];
                    stft.analyse(signal, first + t, spectrum)?;
                    for (m, z) in magnitudes[row * MODELLED..][..MODELLED]
                        .iter_mut()
                        .zip(spectrum.iter())
                    {
                        *m = z.norm();
                    }
                }
            }
            let estimates = self.estimate(&magnitudes)?;
            for channel in 0..2 {
                for t in 0..count {
                    let row = channel * SEGMENT + t;
                    ratio_masks(
                        std::array::from_fn(|s| &estimates[s][row * MODELLED..][..MODELLED]),
                        &mut masks,
                    );
                    let spectrum = &spectra[row * BINS..][..BINS];
                    for (stem, mask) in masks.iter().enumerate() {
                        for ((out, z), m) in masked.iter_mut().zip(spectrum).zip(mask) {
                            *out = z * m;
                        }
                        stft.synthesise(&mut masked, ola.frame(stem, channel, first + t))?;
                    }
                }
            }
            // No later frame reaches before the next one's start.
            let done = if first + count == frames {
                usize::MAX
            } else {
                (first + count) * HOP
            };
            ola.drain(done, &mut sink)?;
        }
        Ok(())
    }

    /// Each network's estimate for one segment's magnitudes
    /// (`[2, 1, SEGMENT, MODELLED]`), in [`NAMES`] order.
    fn estimate(&mut self, magnitudes: &[f32]) -> Result<[Vec<f32>; 4]> {
        let mut estimates: [Vec<f32>; 4] = Default::default();
        for ((session, estimate), name) in self.sessions.iter_mut().zip(&mut estimates).zip(NAMES) {
            let input = TensorRef::from_array_view(([2usize, 1, SEGMENT, MODELLED], magnitudes))?;
            let outputs = session
                .run(ort::inputs![INPUT => input])
                .with_context(|| format!("running the {name} network"))?;
            let (_, values) = outputs[OUTPUT]
                .try_extract_tensor::<f32>()
                .with_context(|| format!("reading the {name} network's estimate"))?;
            if values.len() != magnitudes.len() {
                bail!(
                    "the {name} network answered {} values for {}",
                    values.len(),
                    magnitudes.len()
                );
            }
            *estimate = values.to_vec();
        }
        Ok(estimates)
    }
}

/// A session for one network.
fn open(path: &Path) -> Result<Session> {
    if !path.is_file() {
        bail!(
            "{} is missing: install the model (localgpt audio install stems)",
            path.display()
        );
    }
    // Without the memory pattern the four sessions peak about 250 MB lower
    // (1.2 GB → under 1 GB on a six-minute song), at the same speed; without
    // the CPU arena too they halve again but run twice as long.
    Session::builder()
        .and_then(|builder| builder.with_memory_pattern(false))
        .and_then(|builder| builder.commit_from_file(path))
        .map_err(|e| anyhow!("{e}"))
        .with_context(|| format!("loading {}", path.display()))
}

/// Frames of the STFT of `samples` samples: one frame of zeros before, and
/// zeros after to whole frames.
fn frame_count(samples: usize) -> usize {
    (FRAME + samples).div_ceil(HOP)
}

/// The periodic Hann window (TensorFlow's `hann_window(periodic=True)`).
fn hann() -> Vec<f32> {
    (0..FRAME)
        .map(|i| (0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / FRAME as f64).cos()) as f32)
        .collect()
}

/// Spleeter's STFT and its inverse, one frame at a time.
struct Stft {
    forward: Arc<dyn RealToComplex<f32>>,
    inverse: Arc<dyn ComplexToReal<f32>>,
    window: Vec<f32>,
    frame: Vec<f32>,
    scratch: Vec<Complex<f32>>,
}

impl Stft {
    fn new() -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        let forward = planner.plan_fft_forward(FRAME);
        let inverse = planner.plan_fft_inverse(FRAME);
        let scratch = forward.get_scratch_len().max(inverse.get_scratch_len());
        Self {
            forward,
            inverse,
            window: hann(),
            frame: vec![0.0; FRAME],
            scratch: vec![Complex::default(); scratch],
        }
    }

    /// Frame `index` of `signal`'s spectrum, the signal preceded by a frame
    /// of zeros and followed by as many as it takes.
    fn analyse(
        &mut self,
        signal: &[f32],
        index: usize,
        spectrum: &mut [Complex<f32>],
    ) -> Result<()> {
        let start = index * HOP;
        for (i, (sample, w)) in self.frame.iter_mut().zip(&self.window).enumerate() {
            let x = (start + i)
                .checked_sub(FRAME)
                .and_then(|at| signal.get(at))
                .copied()
                .unwrap_or(0.0);
            *sample = x * w;
        }
        self.forward
            .process_with_scratch(&mut self.frame, spectrum, &mut self.scratch)
            .map_err(|e| anyhow!("STFT: {e}"))
    }

    /// Add the inverse of one frame's `spectrum` (used as scratch), windowed
    /// and compensated, into `out` (the frame's place in the output).
    fn synthesise(&mut self, spectrum: &mut [Complex<f32>], out: &mut [f32]) -> Result<()> {
        // A real signal's DC and Nyquist bins are real.
        spectrum[0].im = 0.0;
        spectrum[BINS - 1].im = 0.0;
        self.inverse
            .process_with_scratch(spectrum, &mut self.frame, &mut self.scratch)
            .map_err(|e| anyhow!("inverse STFT: {e}"))?;
        // realfft's inverse is unnormalised.
        let scale = WINDOW_COMPENSATION / FRAME as f32;
        for ((o, x), w) in out.iter_mut().zip(&self.frame).zip(&self.window) {
            *o += x * w * scale;
        }
        Ok(())
    }
}

/// The four stems' overlap-add, both channels, holding only what a later
/// frame can still reach. Positions count from the start of the padded
/// signal, a frame before the input's first sample.
struct OverlapAdd {
    /// `[stem][channel]`, from `start`.
    buffers: [[Vec<f32>; 2]; 4],
    start: usize,
    /// The input's length.
    len: usize,
}

impl OverlapAdd {
    fn new(len: usize) -> Self {
        Self {
            buffers: Default::default(),
            start: 0,
            len,
        }
    }

    /// Where frame `index` of `stem`'s `channel` is added.
    fn frame(&mut self, stem: usize, channel: usize, index: usize) -> &mut [f32] {
        let buffer = &mut self.buffers[stem][channel];
        let at = index * HOP - self.start;
        if buffer.len() < at + FRAME {
            buffer.resize(at + FRAME, 0.0);
        }
        &mut buffer[at..at + FRAME]
    }

    /// Hand `sink` everything before position `done`, the input's part of it
    /// only, and forget it.
    fn drain(
        &mut self,
        done: usize,
        sink: &mut impl FnMut(usize, [[&[f32]; 2]; 4]) -> Result<()>,
    ) -> Result<()> {
        let held = self.buffers[0][0].len();
        let done = done
            .min(FRAME + self.len)
            .clamp(self.start, self.start + held);
        let count = done - self.start;
        // The leading frame of zeros is not the input's.
        let skip = FRAME.saturating_sub(self.start).min(count);
        if count > skip {
            let buffers = &self.buffers;
            sink(
                self.start + skip - FRAME,
                std::array::from_fn(|s| std::array::from_fn(|c| &buffers[s][c][skip..count])),
            )?;
        }
        for buffer in self.buffers.iter_mut().flatten() {
            buffer.drain(..count.min(buffer.len()));
        }
        self.start = done;
        Ok(())
    }
}

/// Spleeter's ratio masks for one frame of one channel: from the four
/// estimates over the modelled bins, each stem's mask over every bin, those
/// above the modelled ones taking the frame's mean. They sum to one.
fn ratio_masks(estimates: [&[f32]; 4], masks: &mut [Vec<f32>; 4]) {
    for k in 0..MODELLED {
        let powers = estimates.map(|e| e[k] * e[k]);
        let total = powers.iter().sum::<f32>() + EPSILON;
        for (mask, power) in masks.iter_mut().zip(powers) {
            let m = (power + EPSILON / 4.0) / total;
            // A network that answers NaN shares the bin out evenly.
            mask[k] = if m.is_finite() { m } else { 0.25 };
        }
    }
    for mask in masks {
        let mean = mask[..MODELLED].iter().sum::<f32>() / MODELLED as f32;
        mask[MODELLED..].fill(mean);
    }
}

/// A sample as 16-bit PCM.
fn pcm(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16
}

/// Per-second loudness of a stereo signal, gathered as it streams past.
#[derive(Debug, Default)]
struct Loudness {
    /// Per second, the sum of the squared channel means.
    squares: Vec<f64>,
    samples: usize,
}

impl Loudness {
    fn push(&mut self, left: &[f32], right: &[f32]) {
        let rate = SAMPLE_RATE as usize;
        for (l, r) in left.iter().zip(right) {
            let second = self.samples / rate;
            if second == self.squares.len() {
                self.squares.push(0.0);
            }
            let mean = 0.5 * (*l as f64 + *r as f64);
            self.squares[second] += mean * mean;
            self.samples += 1;
        }
    }

    /// The RMS of each second, the last over what it holds.
    fn rms(&self) -> Vec<f32> {
        let rate = SAMPLE_RATE as usize;
        self.squares
            .iter()
            .enumerate()
            .map(|(i, squares)| {
                let n = (self.samples - i * rate).clamp(1, rate);
                (squares / n as f64).sqrt() as f32
            })
            .collect()
    }
}

/// The stems' per-second RMS as 0..1 curves (the module's "Loudness").
fn curves(stems: [Vec<f32>; 4], mix: &[f32]) -> [Vec<f32>; 4] {
    let floor = (percentile_95(mix) / 10.0).max(1e-6);
    stems.map(|curve| {
        let scale = percentile_95(&curve).max(floor);
        curve
            .iter()
            .map(|v| match v / scale {
                // A curve is numbers, which JSON's are.
                x if x.is_nan() => 0.0,
                x => x.clamp(0.0, 1.0),
            })
            .collect()
    })
}

fn percentile_95(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f32::total_cmp);
    sorted[((sorted.len() as f32 * 0.95) as usize).min(sorted.len() - 1)]
}

#[cfg(feature = "mcp")]
pub use tool::StemsTool;

#[cfg(feature = "mcp")]
mod tool {
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    use anyhow::{Context, Result};
    use async_trait::async_trait;
    use localgpt_world_agent::mcp::{Tool, ToolSchema};
    use serde::Deserialize;
    use serde_json::{Map, Value, json};

    use super::{Model, NAMES, Stems};
    use crate::tools::{ToolContext, blocking};

    /// `audio_stems`: the model as an agent's tool. The four networks hold
    /// about a gigabyte while they run, so they load for each call and go
    /// after it: loading takes about a second, a split takes seconds a
    /// minute of audio, and an agent's MCP server can run for hours.
    pub struct StemsTool {
        context: ToolContext,
        /// One split at a time: two would hold two gigabytes.
        running: Arc<Mutex<()>>,
    }

    impl StemsTool {
        pub fn new(context: ToolContext) -> Self {
            Self {
                context,
                running: Arc::default(),
            }
        }
    }

    #[derive(Deserialize)]
    struct Args {
        file: String,
        out_dir: Option<String>,
    }

    #[async_trait]
    impl Tool for StemsTool {
        fn name(&self) -> &str {
            "audio_stems"
        }

        fn schema(&self) -> ToolSchema {
            ToolSchema {
                name: "audio_stems".to_string(),
                description: "Split a song into four stems with Deezer's Spleeter (4stems), on this \
                    machine's CPU: drums, bass, vocals and other, each a 16-bit stereo 44.1 kHz WAV, \
                    plus how loud each stem is in every second. Takes seconds per minute of audio. \
                    The four files add up to the song, so placing them as separate sounds keeps it \
                    whole: each can be an entity's audio source, {\"File\": {\"path\": <the path \
                    given here>, \"looping\": true}} — the paths are relative to the world's \
                    assets/, as asset paths are. Looping files play in phase, so the four add up \
                    wherever they are placed; leave the soundtrack's `path` unset, or the song is \
                    heard twice. The desktop app plays them; the web viewer does not yet. `stems` \
                    in the answer is exactly a soundtrack's `stems` (StemCurves: one value in 0..1 \
                    per second for drums, bass, vocals and other, aligned to the soundtrack's \
                    `energy` curve); set it, with the song's `duration`, on the soundtrack and \
                    modulations bound to a stem follow that instrument as it is heard. Supported inputs: WAV, \
                    FLAC, Ogg Vorbis, MP3, AAC/M4A, ALAC, AIFF, up to 20 minutes."
                    .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "file": {
                            "type": "string",
                            "description": "The recording to split; a relative path is from the world's folder."
                        },
                        "out_dir": {
                            "type": "string",
                            "description": "Where the four WAVs go; a relative path is inside the world's assets/. Default: stems/<the file's name>/ there. Files of the same names are replaced."
                        }
                    },
                    "required": ["file"]
                }),
            }
        }

        fn annotations(&self) -> Option<Value> {
            Some(json!({
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            }))
        }

        async fn execute(&self, arguments: &str) -> Result<String> {
            self.split(arguments).await
        }
    }

    impl StemsTool {
        async fn split(&self, arguments: &str) -> Result<String> {
            let args: Args = serde_json::from_str(arguments)
                .context("audio_stems takes {\"file\": path, \"out_dir\"?: path}")?;
            let input = self.context.input(&args.file);
            let out_dir = match args.out_dir.as_deref() {
                Some(dir) if Path::new(dir).is_absolute() => PathBuf::from(dir),
                Some(dir) => self.context.out.join(dir),
                None => {
                    let name = input
                        .file_stem()
                        .map_or_else(|| "track".into(), |s| s.to_string_lossy());
                    self.context.out.join("stems").join(name.as_ref())
                }
            };
            let dir = self.context.models.join(crate::catalog::STEMS.id);
            let running = self.running.clone();
            let stems = blocking(move || {
                // A split that panicked holds nothing: the gate stays usable.
                let _one = running.lock().unwrap_or_else(|e| e.into_inner());
                Model::load(&dir)?.separate(&input, &out_dir)
            })
            .await?;
            Ok(answer(&self.context, &stems).to_string())
        }
    }

    /// The tool's answer: the files as the world names them, and the curves.
    fn answer(context: &ToolContext, stems: &Stems) -> Value {
        let mut files = Map::new();
        let mut curves = Map::new();
        for ((name, file), curve) in NAMES.iter().zip(&stems.files).zip(&stems.curves) {
            files.insert(name.to_string(), context.named(file).into());
            let rounded: Vec<Value> = curve
                .iter()
                .map(|v| ((*v as f64 * 1000.0).round() / 1000.0).into())
                .collect();
            curves.insert(name.to_string(), rounded.into());
        }
        json!({
            "files": files,
            "sample_rate": super::SAMPLE_RATE,
            "seconds": (stems.seconds as f64 * 100.0).round() / 100.0,
            "stems": curves,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A signal with something at every frequency and no periodicity.
    fn noise(len: usize, seed: u32) -> Vec<f32> {
        let mut state = seed;
        (0..len)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 8) as f32 / (1 << 24) as f32 - 0.5
            })
            .collect()
    }

    /// The separation's STFT, masking, inverse and streaming overlap-add,
    /// with fixed masks splitting every bin 10/20/30/40 % in place of the
    /// networks', in segments of three frames: each stem should come back as
    /// its share of the signal, and every sample handed on once, in order.
    fn round_trip(mix: &[Vec<f32>; 2]) -> [[Vec<f32>; 2]; 4] {
        const SHARES: [f32; 4] = [0.1, 0.2, 0.3, 0.4];
        let len = mix[0].len();
        let mut stft = Stft::new();
        let mut ola = OverlapAdd::new(len);
        let mut spectrum = vec![Complex::default(); BINS];
        let mut masked = vec![Complex::default(); BINS];
        let mut out: [[Vec<f32>; 2]; 4] = std::array::from_fn(|_| [vec![], vec![]]);
        let mut sink = |at: usize, stems: [[&[f32]; 2]; 4]| -> Result<()> {
            for (out, stem) in out.iter_mut().zip(stems) {
                for (out, samples) in out.iter_mut().zip(stem) {
                    assert_eq!(out.len(), at, "a gap or an overlap");
                    out.extend_from_slice(samples);
                }
            }
            Ok(())
        };
        let frames = frame_count(len);
        for first in (0..frames).step_by(3) {
            let count = 3.min(frames - first);
            for f in first..first + count {
                for (channel, signal) in mix.iter().enumerate() {
                    stft.analyse(signal, f, &mut spectrum).unwrap();
                    for (stem, share) in SHARES.iter().enumerate() {
                        for (m, z) in masked.iter_mut().zip(&spectrum) {
                            *m = z * share;
                        }
                        stft.synthesise(&mut masked, ola.frame(stem, channel, f))
                            .unwrap();
                    }
                }
            }
            let done = if first + count == frames {
                usize::MAX
            } else {
                (first + count) * HOP
            };
            ola.drain(done, &mut sink).unwrap();
        }
        for (stem, share) in out.iter().zip(SHARES) {
            for (back, signal) in stem.iter().zip(mix) {
                assert_eq!(back.len(), len, "every sample is handed on");
                let worst = signal
                    .iter()
                    .zip(back)
                    .map(|(a, b)| (a * share - b).abs())
                    .fold(0.0f32, f32::max);
                assert!(worst < 1e-5, "{len} samples: off by {worst}");
            }
        }
        out
    }

    #[test]
    fn the_stft_and_its_inverse_give_the_signal_back() {
        for len in [1, 1000, FRAME, 3 * FRAME + 17, 44_100] {
            let mix = [noise(len, len as u32), noise(len, len as u32 + 7)];
            let stems = round_trip(&mix);
            for (c, signal) in mix.iter().enumerate() {
                for (i, x) in signal.iter().enumerate() {
                    let sum: f32 = stems.iter().map(|s| s[c][i]).sum();
                    assert!((sum - x).abs() < 1e-5, "{len} samples: the stems miss");
                }
            }
        }
    }

    #[test]
    fn masks_sum_to_one_in_every_bin() {
        let estimates: [Vec<f32>; 4] = std::array::from_fn(|s| {
            noise(MODELLED, s as u32 + 1)
                .iter()
                .map(|v| v.abs() * 40.0)
                .collect()
        });
        let silent = vec![0.0; MODELLED];
        let mut masks: [Vec<f32>; 4] = std::array::from_fn(|_| vec![0.0; BINS]);
        for case in [
            std::array::from_fn(|s| estimates[s].as_slice()),
            [silent.as_slice(); 4],
            [estimates[0].as_slice(), &silent, &silent, &silent],
        ] {
            ratio_masks(case, &mut masks);
            for k in 0..BINS {
                let sum: f32 = masks.iter().map(|m| m[k]).sum();
                assert!((sum - 1.0).abs() < 1e-5, "bin {k} sums to {sum}");
                assert!(masks.iter().all(|m| (0.0..=1.0).contains(&m[k])));
            }
        }
        // Silence is shared evenly; a lone stem takes everything.
        ratio_masks([silent.as_slice(); 4], &mut masks);
        assert!((masks[2][7] - 0.25).abs() < 1e-6);
        ratio_masks([&silent, &silent, &estimates[0], &silent], &mut masks);
        assert!(
            masks[2][..MODELLED]
                .iter()
                .zip(&estimates[0])
                .all(|(m, e)| *e == 0.0 || *m > 0.999)
        );
    }

    #[test]
    fn loudness_has_a_value_per_second_and_silence_stays_zero() {
        let rate = SAMPLE_RATE as usize;
        let mut loud = Loudness::default();
        let tone: Vec<f32> = (0..rate * 2 + rate / 2)
            .map(|i| if i < rate { 0.5 } else { 0.0 })
            .collect();
        // Pushed in uneven pieces, as the separation streams.
        for (l, r) in tone.chunks(7919).zip(tone.chunks(7919)) {
            loud.push(l, r);
        }
        let rms = loud.rms();
        assert_eq!(rms.len(), 3, "2.5 seconds is three values");
        assert!((rms[0] - 0.5).abs() < 1e-6 && rms[1] == 0.0 && rms[2] == 0.0);

        let [drums, bass, vocals, other] = curves(
            [
                rms.clone(),
                vec![0.0; 3],
                rms.iter().map(|v| v * 0.001).collect(),
                vec![],
            ],
            &rms,
        );
        assert_eq!(drums, vec![1.0, 0.0, 0.0]);
        assert_eq!(bass, vec![0.0; 3], "silence stays zero");
        assert!(
            vocals[0] < 0.02,
            "a stem 60 dB down is not stretched to full scale"
        );
        assert!(other.is_empty());
        let [silent, ..] = curves([vec![0.0; 2], vec![], vec![], vec![]], &[0.0, 0.0]);
        assert_eq!(silent, vec![0.0; 2]);
    }

    #[test]
    fn frames_cover_the_input_after_a_leading_frame() {
        assert_eq!(frame_count(0), 4);
        assert_eq!(frame_count(1), 5);
        // The last frame reaches past the input's end.
        for n in [1, 1023, 1024, 1025, 100_000] {
            assert!((frame_count(n) - 1) * HOP + FRAME >= FRAME + n);
        }
    }

    /// The installed model's interface is the one this module feeds.
    /// `LOCALGPT_AUDIO_MODELS=… cargo test -p localgpt-audio-models --features run -- --ignored --nocapture stems`
    #[test]
    #[ignore = "needs the installed stems model"]
    fn the_installed_networks_take_and_give_magnitudes() {
        let dir = crate::models_dir()
            .expect("a models directory")
            .join("stems");
        for name in NAMES {
            let session = open(&dir.join(format!("{name}.onnx"))).unwrap();
            for (kind, outlets) in [("input", session.inputs()), ("output", session.outputs())] {
                for outlet in outlets {
                    println!("{name}: {kind} {} {}", outlet.name(), outlet.dtype());
                }
            }
            let metadata = session.metadata().unwrap();
            for key in metadata.custom_keys().unwrap() {
                println!("{name}: {key} = {:?}", metadata.custom(&key));
            }
            // The file is the stem it is named for, with 4stems' activations.
            assert_eq!(metadata.custom("comment").as_deref(), Some(name));
            assert_eq!(metadata.custom("conv_activation").as_deref(), Some("ELU"));
            assert_eq!(metadata.custom("sample_rate").as_deref(), Some("44100"));
            assert_eq!(session.inputs().len(), 1);
            assert_eq!(session.inputs()[0].name(), INPUT);
            assert!(session.outputs().iter().any(|o| o.name() == OUTPUT));
        }
    }

    /// The model on a real track: the stems add up to the mix, and none is
    /// silent. `STEMS_TRACK` picks the track; by default the shared pack's
    /// Emberfall (beside this checkout), which has drums, unlike Tidewater
    /// and Amber Drift, whose drums stem is rightly near −85 dB. No track in
    /// the pack sings, so its vocals stem is what the network hears as voice
    /// in pads and bells.
    #[test]
    #[ignore = "needs the installed stems model and a track"]
    fn the_stems_of_a_track_add_up_to_it() {
        let dir = crate::models_dir()
            .expect("a models directory")
            .join("stems");
        let track = std::env::var_os("STEMS_TRACK").map_or_else(
            || {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../../localgpt-world-assets/music/emberfall.mp3")
            },
            PathBuf::from,
        );
        let audio = decode::read(&track)
            .unwrap_or_else(|e| panic!("{e:#} (STEMS_TRACK names another track)"));
        let [left, right] = audio.stereo();
        let mix = [
            decode::resample(&left, audio.sample_rate, SAMPLE_RATE).unwrap(),
            decode::resample(&right, audio.sample_rate, SAMPLE_RATE).unwrap(),
        ];
        let mut model = Model::load(&dir).unwrap();
        let started = std::time::Instant::now();
        let mut sum = [vec![0f32; mix[0].len()], vec![0f32; mix[0].len()]];
        let mut energy = [0f64; 4];
        let mut covered = 0;
        model
            .run(&mix, |at, stems| {
                assert_eq!(at, covered, "stretches arrive in order");
                covered += stems[0][0].len();
                for (stem, channels) in stems.iter().enumerate() {
                    for (c, samples) in channels.iter().enumerate() {
                        for (i, s) in samples.iter().enumerate() {
                            sum[c][at + i] += s;
                            energy[stem] += (*s as f64).powi(2);
                        }
                    }
                }
                Ok(())
            })
            .unwrap();
        let took = started.elapsed();
        assert_eq!(covered, mix[0].len(), "every sample is handed on");
        let power = |x: &[f32]| x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>();
        let residual: f64 = (0..2)
            .map(|c| {
                let diff: Vec<f32> = sum[c].iter().zip(&mix[c]).map(|(a, b)| a - b).collect();
                power(&diff)
            })
            .sum();
        let total = power(&mix[0]) + power(&mix[1]);
        let error_db = 10.0 * (residual / total).log10();
        let seconds = mix[0].len() as f32 / SAMPLE_RATE as f32;
        println!(
            "{}: {seconds:.1} s separated in {:.1} s ({:.1}x realtime)",
            track.display(),
            took.as_secs_f32(),
            seconds / took.as_secs_f32()
        );
        println!("sum of the stems − mix: {error_db:.1} dB");
        for (name, e) in NAMES.iter().zip(energy) {
            println!("{name}: {:.1} dB of the mix", 10.0 * (e / total).log10());
        }
        assert!(
            error_db < -60.0,
            "the stems miss the mix by {error_db:.1} dB"
        );
        assert!(
            energy.iter().all(|e| 10.0 * (e / total).log10() > -40.0),
            "a stem is silent: {energy:?}"
        );

        // The files and the curves, through the public path.
        let out = tempfile::tempdir().unwrap();
        let stems = model.separate(&track, out.path()).unwrap();
        let seconds = stems.seconds.ceil() as usize;
        for (file, curve) in stems.files.iter().zip(&stems.curves) {
            let written = decode::read(file).unwrap();
            assert_eq!(written.sample_rate, SAMPLE_RATE);
            assert_eq!(written.frames(), mix[0].len());
            assert_eq!(curve.len(), seconds);
            assert!(curve.iter().all(|v| (0.0..=1.0).contains(v)));
        }
    }
}
