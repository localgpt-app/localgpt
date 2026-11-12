//! CLAP: a recording and a description in one space (the `match` model).
//!
//! LAION's `larger_clap_music_and_speech`, as Xenova's quantized ONNX export:
//! an audio tower and a text tower trained so that a recording lands near
//! the sentences that describe it, in one 512-d space. [`match_descriptions`]
//! is the use an agent has for it: a recording, a few descriptions in plain
//! English, and how well each one fits.
//!
//! The audio frontend is HF's `ClapFeatureExtractor` for this checkpoint
//! (checked against `transformers.audio_utils` in the tests): 48 kHz mono,
//! reflect-centre-padded STFT (1024 window, 480 hop, periodic Hann), power
//! spectrum, Slaney mel filterbank (64 mels, 50 Hz–14 kHz), 10·log10. A
//! 10 s window becomes [1001, 64].
//!
//! A recording longer than a window is heard at three windows, centred at a
//! quarter, a half and three quarters of its length, and their embeddings
//! are averaged: one window is too local to a song's intro, drop or bridge.
//! A shorter one is one window, zero-padded.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use ort::session::Session;
use ort::value::Tensor;
use realfft::{RealFftPlanner, RealToComplex};
use serde::Serialize;
use tokenizers::{Tokenizer, TruncationParams};

use crate::decode::{self, Audio};

/// The rate both the frontend and the audio tower work at.
pub const SAMPLE_RATE: u32 = 48_000;
/// One window: 10 s at [`SAMPLE_RATE`].
pub const WINDOW_SAMPLES: usize = 480_000;
/// The size of an embedding.
pub const DIMENSIONS: usize = 512;
/// The temperature CLAP was trained with for audio against text: the
/// exponent of `logit_scale_a` in LAION's checkpoint (3.3119991, read from
/// `model.safetensors` of laion/larger_clap_music_and_speech, revision
/// 8a2acdbf). It is what HF's `logits_per_audio` multiplies cosines by
/// before the softmax a zero-shot classification takes.
pub const LOGIT_SCALE: f32 = 27.439_926;

/// The model's files, as the catalog names them.
pub const AUDIO_MODEL: &str = "audio_model_quantized.onnx";
pub const TEXT_MODEL: &str = "text_model_quantized.onnx";
pub const TOKENIZER: &str = "tokenizer.json";

const FRAMES: usize = 1001;
const MELS: usize = 64;
const N_FFT: usize = 1024;
const HOP: usize = 480;
const FMIN: f64 = 50.0;
const FMAX: f64 = 14_000.0;
/// Where a long recording's windows are centred, as fractions of its length.
const WINDOWS_AT: [f32; 3] = [0.25, 0.5, 0.75];
/// The text context CLAP was trained with.
const MAX_TOKENS: usize = 77;

/// How well one description fits a recording.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Score {
    pub description: String,
    /// The cosine of the recording's and the description's embeddings,
    /// −1 to 1: higher is closer. It means the same from call to call.
    pub similarity: f32,
    /// The softmax of the similarities × [`LOGIT_SCALE`] over the
    /// descriptions given: how the model splits its belief among them. It
    /// sums to 1 over a call, so it means something only beside the others.
    pub probability: f32,
}

/// Score `input` against `descriptions` with the model in `model_dir`,
/// best first.
pub fn match_descriptions(
    model_dir: &Path,
    input: &Path,
    descriptions: &[String],
) -> Result<Vec<Score>> {
    let audio = decode::read(input)?;
    Clap::load(model_dir)?.scores(&audio, descriptions)
}

/// The model. Each tower opens on its first use (or [`Clap::open_audio`],
/// [`Clap::open_text`]), so a caller that only embeds text never holds the
/// audio tower, and the other way round.
pub struct Clap {
    dir: PathBuf,
    frontend: Frontend,
    audio: Option<Session>,
    text: Option<TextTower>,
}

struct TextTower {
    session: Session,
    tokenizer: Tokenizer,
    /// Xenova's export takes `input_ids` alone; another export may also
    /// want the mask.
    attention_mask: bool,
}

impl Clap {
    /// The model whose files are in `dir`: the installed `match` directory,
    /// or any directory holding [`AUDIO_MODEL`], [`TEXT_MODEL`] and
    /// [`TOKENIZER`].
    pub fn load(dir: &Path) -> Result<Self> {
        if !dir.is_dir() {
            bail!(
                "no CLAP model at {} (localgpt audio install match)",
                dir.display()
            );
        }
        Ok(Self {
            dir: dir.to_path_buf(),
            frontend: Frontend::new(),
            audio: None,
            text: None,
        })
    }

    /// Open the audio tower now rather than on the first recording.
    pub fn open_audio(&mut self) -> Result<()> {
        self.audio_session().map(|_| ())
    }

    /// Open the text tower and its tokenizer now rather than on the first
    /// description.
    pub fn open_text(&mut self) -> Result<()> {
        self.text_tower().map(|_| ())
    }

    /// Every description scored against `audio`, best first.
    pub fn scores(&mut self, audio: &Audio, descriptions: &[String]) -> Result<Vec<Score>> {
        if descriptions.is_empty() {
            bail!("give at least one description to match against");
        }
        let heard = self.embed_audio(audio)?;
        let texts = descriptions
            .iter()
            .map(|d| self.embed_text(d))
            .collect::<Result<Vec<_>>>()?;
        Ok(rank(&heard, descriptions, &texts))
    }

    /// The recording's embedding, L2-normed: its windows' average (see the
    /// module's documentation).
    pub fn embed_audio(&mut self, audio: &Audio) -> Result<Vec<f32>> {
        let samples = decode::resample(&audio.mono(), audio.sample_rate, SAMPLE_RATE)
            .context("resampling to 48 kHz")?;
        let mut sum = vec![0.0f32; DIMENSIONS];
        for (start, end) in windows(samples.len()) {
            let embedding = self.embed_window(&samples[start..end])?;
            for (total, value) in sum.iter_mut().zip(&embedding) {
                *total += value;
            }
        }
        Ok(normalized(sum))
    }

    /// A description's embedding, L2-normed. Past CLAP's 77-token context the
    /// text is cut, keeping its closing token.
    pub fn embed_text(&mut self, text: &str) -> Result<Vec<f32>> {
        let tower = self.text_tower()?;
        let encoding = tower
            .tokenizer
            .encode(text, true)
            .map_err(|e| anyhow!("tokenizing {text:?}: {e}"))?;
        let ids: Vec<i64> = encoding.get_ids().iter().map(|&id| i64::from(id)).collect();
        let n = ids.len();
        let input_ids = Tensor::from_array(([1usize, n], ids)).context("the text tower's input")?;
        let outputs = if tower.attention_mask {
            let mask = Tensor::from_array(([1usize, n], vec![1i64; n]))
                .context("the text tower's mask")?;
            tower
                .session
                .run(ort::inputs!["input_ids" => input_ids, "attention_mask" => mask])
        } else {
            tower.session.run(ort::inputs!["input_ids" => input_ids])
        }
        .with_context(|| format!("running CLAP's text tower on {text:?}"))?;
        let embedding = outputs
            .get("text_embeds")
            .ok_or_else(|| anyhow!("CLAP's text tower has no `text_embeds` output"))?
            .try_extract_tensor::<f32>()
            .context("the text tower's output")?
            .1
            .to_vec();
        checked(embedding, "text")
    }

    /// One window of 48 kHz mono (zero-padded to 10 s) → its embedding.
    fn embed_window(&mut self, samples: &[f32]) -> Result<Vec<f32>> {
        let mel = self.frontend.log_mel(samples);
        let session = self.audio_session()?;
        let input = Tensor::from_array(([1usize, 1, FRAMES, MELS], mel))
            .context("the audio tower's input")?;
        let outputs = session
            .run(ort::inputs!["input_features" => input])
            .context("running CLAP's audio tower")?;
        let embedding = outputs
            .get("audio_embeds")
            .ok_or_else(|| anyhow!("CLAP's audio tower has no `audio_embeds` output"))?
            .try_extract_tensor::<f32>()
            .context("the audio tower's output")?
            .1
            .to_vec();
        checked(embedding, "audio")
    }

    fn audio_session(&mut self) -> Result<&mut Session> {
        let session = match self.audio.take() {
            Some(session) => session,
            None => open_session(&self.dir.join(AUDIO_MODEL))?,
        };
        Ok(self.audio.insert(session))
    }

    fn text_tower(&mut self) -> Result<&mut TextTower> {
        let tower = match self.text.take() {
            Some(tower) => tower,
            None => {
                let session = open_session(&self.dir.join(TEXT_MODEL))?;
                let path = self.dir.join(TOKENIZER);
                let mut tokenizer = Tokenizer::from_file(&path)
                    .map_err(|e| anyhow!("loading {}: {e}", path.display()))?;
                tokenizer.with_padding(None);
                tokenizer
                    .with_truncation(Some(TruncationParams {
                        max_length: MAX_TOKENS,
                        ..Default::default()
                    }))
                    .map_err(|e| anyhow!("{}: {e}", path.display()))?;
                let attention_mask = session
                    .inputs()
                    .iter()
                    .any(|input| input.name() == "attention_mask");
                TextTower {
                    session,
                    tokenizer,
                    attention_mask,
                }
            }
        };
        Ok(self.text.insert(tower))
    }
}

fn open_session(path: &Path) -> Result<Session> {
    if !path.is_file() {
        bail!(
            "{} is missing (localgpt audio install match)",
            path.display()
        );
    }
    Session::builder()
        .context("starting ONNX Runtime")?
        .commit_from_file(path)
        .with_context(|| format!("loading {}", path.display()))
}

/// An embedding of the right size, L2-normed.
fn checked(embedding: Vec<f32>, tower: &str) -> Result<Vec<f32>> {
    if embedding.len() != DIMENSIONS {
        bail!(
            "CLAP's {tower} tower gave {} values, not {DIMENSIONS}",
            embedding.len()
        );
    }
    Ok(normalized(embedding))
}

fn normalized(mut v: Vec<f32>) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
    for x in &mut v {
        *x /= norm;
    }
    v
}

/// The windows of a recording `len` samples long at 48 kHz, as (start, end):
/// one when it fits in a window, else three centred at [`WINDOWS_AT`], cut
/// to the recording (the frontend zero-pads what is cut).
fn windows(len: usize) -> Vec<(usize, usize)> {
    if len <= WINDOW_SAMPLES {
        return vec![(0, len)];
    }
    WINDOWS_AT
        .iter()
        .map(|at| {
            let centre = (len as f32 * at) as usize;
            let start = centre.saturating_sub(WINDOW_SAMPLES / 2);
            (start, (start + WINDOW_SAMPLES).min(len))
        })
        .collect()
}

/// Each description's score against the recording's embedding, best first.
/// Both sides are L2-normed, so a dot product is the cosine.
fn rank(audio: &[f32], descriptions: &[String], texts: &[Vec<f32>]) -> Vec<Score> {
    let similarities: Vec<f32> = texts
        .iter()
        .map(|t| t.iter().zip(audio).map(|(a, b)| a * b).sum())
        .collect();
    let top = similarities
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max);
    let weights: Vec<f32> = similarities
        .iter()
        .map(|s| ((s - top) * LOGIT_SCALE).exp())
        .collect();
    let total: f32 = weights.iter().sum();
    let mut scores: Vec<Score> = descriptions
        .iter()
        .zip(similarities.iter().zip(&weights))
        .map(|(description, (similarity, weight))| Score {
            description: description.clone(),
            similarity: *similarity,
            probability: weight / total,
        })
        .collect();
    scores.sort_by(|a, b| b.similarity.total_cmp(&a.similarity));
    scores
}

// ---------------------------------------------------------------------------
// The audio frontend: HF's ClapFeatureExtractor
// ---------------------------------------------------------------------------

/// The log-mel frontend, its filterbank, window and FFT plan made once.
struct Frontend {
    /// [MELS][N_FFT / 2 + 1], row-major.
    filters: Vec<f32>,
    window: Vec<f32>,
    fft: Arc<dyn RealToComplex<f32>>,
}

impl Frontend {
    fn new() -> Self {
        Self {
            filters: mel_filterbank(),
            window: hann(),
            fft: RealFftPlanner::<f32>::new().plan_fft_forward(N_FFT),
        }
    }

    /// One window of 48 kHz mono → log-mel [FRAMES, MELS], row-major
    /// (10·log10 power). Shorter input is zero-padded, longer cut.
    fn log_mel(&self, samples: &[f32]) -> Vec<f32> {
        let mut buffer = samples[..samples.len().min(WINDOW_SAMPLES)].to_vec();
        buffer.resize(WINDOW_SAMPLES, 0.0);
        let padded = reflect_pad(&buffer, N_FFT / 2);

        let mut input = self.fft.make_input_vec();
        let mut spectrum = self.fft.make_output_vec();
        let mut scratch = self.fft.make_scratch_vec();
        let bins = N_FFT / 2 + 1;
        let mut power = vec![0.0f32; bins];
        let mut out = vec![0.0f32; FRAMES * MELS];
        for (frame, row) in out.as_chunks_mut::<MELS>().0.iter_mut().enumerate() {
            let start = frame * HOP;
            let Some(slice) = padded.get(start..start + N_FFT) else {
                break;
            };
            for ((x, sample), w) in input.iter_mut().zip(slice).zip(&self.window) {
                *x = sample * w;
            }
            if self
                .fft
                .process_with_scratch(&mut input, &mut spectrum, &mut scratch)
                .is_err()
            {
                break;
            }
            for (p, c) in power.iter_mut().zip(&spectrum) {
                *p = c.norm_sqr();
            }
            for (cell, filter) in row.iter_mut().zip(self.filters.chunks_exact(bins)) {
                let energy: f32 = filter.iter().zip(&power).map(|(f, p)| f * p).sum();
                *cell = 10.0 * energy.max(1e-10).log10();
            }
        }
        out
    }
}

/// Slaney's mel scale (librosa's `htk=False`).
fn hz_to_mel(f: f64) -> f64 {
    const F_SP: f64 = 200.0 / 3.0;
    const MIN_LOG_HZ: f64 = 1000.0;
    let min_log_mel = MIN_LOG_HZ / F_SP;
    let logstep = 6.4f64.ln() / 27.0;
    if f < MIN_LOG_HZ {
        f / F_SP
    } else {
        min_log_mel + (f / MIN_LOG_HZ).ln() / logstep
    }
}

fn mel_to_hz(m: f64) -> f64 {
    const F_SP: f64 = 200.0 / 3.0;
    let min_log_mel = 1000.0 / F_SP;
    let logstep = 6.4f64.ln() / 27.0;
    if m < min_log_mel {
        m * F_SP
    } else {
        1000.0 * (logstep * (m - min_log_mel)).exp()
    }
}

/// The 64×513 filterbank (row-major, [mel][fft bin]), Slaney-scaled and
/// area-normalised, as librosa and HF build it.
fn mel_filterbank() -> Vec<f32> {
    let bins = N_FFT / 2 + 1;
    let fft_freqs: Vec<f64> = (0..bins)
        .map(|k| k as f64 * f64::from(SAMPLE_RATE) / N_FFT as f64)
        .collect();
    let lo = hz_to_mel(FMIN);
    let hi = hz_to_mel(FMAX);
    let freqs: Vec<f64> = (0..MELS + 2)
        .map(|i| mel_to_hz(lo + (hi - lo) * i as f64 / (MELS + 1) as f64))
        .collect();

    let mut weights = vec![0.0f32; MELS * bins];
    for (i, row) in weights.chunks_exact_mut(bins).enumerate() {
        let enorm = 2.0 / (freqs[i + 2] - freqs[i]);
        for (w, &f) in row.iter_mut().zip(&fft_freqs) {
            let lower = (f - freqs[i]) / (freqs[i + 1] - freqs[i]);
            let upper = (freqs[i + 2] - f) / (freqs[i + 2] - freqs[i + 1]);
            *w = (lower.min(upper).max(0.0) * enorm) as f32;
        }
    }
    weights
}

/// Periodic Hann (numpy's `hanning(N + 1)[:-1]`).
fn hann() -> Vec<f32> {
    (0..N_FFT)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / N_FFT as f32).cos())
        .collect()
}

/// Reflect-pad `n` samples on both sides (numpy's `mode="reflect"`); the
/// input is a whole window, far longer than `n`.
fn reflect_pad(samples: &[f32], n: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(samples.len() + 2 * n);
    for i in (1..=n).rev() {
        out.push(samples[i.min(samples.len() - 1)]);
    }
    out.extend_from_slice(samples);
    for i in (samples.len().saturating_sub(n + 1)..samples.len().saturating_sub(1)).rev() {
        out.push(samples[i]);
    }
    out
}

// ---------------------------------------------------------------------------
// The tool
// ---------------------------------------------------------------------------

#[cfg(feature = "mcp")]
pub(crate) use tool::MatchTool;

#[cfg(feature = "mcp")]
mod tool {
    use std::sync::{Arc, Mutex, PoisonError};

    use anyhow::{Result, anyhow, bail};
    use async_trait::async_trait;
    use localgpt_world_agent::mcp::{Tool, ToolSchema};
    use serde_json::{Value, json};

    use super::Clap;
    use crate::catalog::MATCH;
    use crate::decode;
    use crate::tools::{ToolContext, blocking};

    const NAME: &str = "audio_match";
    const MOST: usize = 32;

    /// What the tool is for, as an agent reads it.
    const DESCRIPTION: &str = "An ear for an agent that cannot hear: compare a recording against \
     descriptions written in plain English (\"rain on a tin roof\", \"a calm \
     piano piece\") and get them ranked by how well each fits. Use it to check \
     that a sound placed in a world is what was meant, or to pick a track for a \
     scene. Scores are relative among the descriptions given, so give several, \
     including contrasting ones (for rain: \"heavy rain\", \"a crackling fire\", \
     \"traffic on a wet road\"). `probability` splits 1 across the descriptions; \
     `similarity` is the cosine between recording and description (higher is \
     closer; compare it among the descriptions, not against a fixed bar). A recording longer than 10 s is heard at a \
     quarter, a half and three quarters of its length. Reads WAV, FLAC, Ogg \
     Vorbis, MP3, AAC/M4A, ALAC and AIFF. The model is LAION's CLAP \
     (larger_clap_music_and_speech), on this machine's CPU.";

    /// `audio_match`: a recording scored against descriptions. The model
    /// stays loaded between calls.
    pub(crate) struct MatchTool {
        context: ToolContext,
        model: Arc<Mutex<Option<Clap>>>,
    }

    impl MatchTool {
        pub(crate) fn new(context: &ToolContext) -> Self {
            Self {
                context: context.clone(),
                model: Arc::default(),
            }
        }
    }

    #[async_trait]
    impl Tool for MatchTool {
        fn name(&self) -> &str {
            NAME
        }

        fn schema(&self) -> ToolSchema {
            ToolSchema {
                name: NAME.to_string(),
                description: DESCRIPTION.to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "file": {
                            "type": "string",
                            "description": format!(
                                "The recording: a path relative to {}, or absolute",
                                self.context.base.display()
                            )
                        },
                        "descriptions": {
                            "type": "array",
                            "items": {"type": "string"},
                            "minItems": 1,
                            "maxItems": MOST,
                            "description": "What it might be, each a short phrase in plain English"
                        }
                    },
                    "required": ["file", "descriptions"]
                }),
            }
        }

        fn annotations(&self) -> Option<Value> {
            Some(json!({
                "readOnlyHint": true,
                "destructiveHint": false,
            }))
        }

        async fn execute(&self, arguments: &str) -> Result<String> {
            let args: Value = serde_json::from_str(arguments)
                .map_err(|e| anyhow!("the arguments aren't JSON: {e}"))?;
            let file = args
                .get("file")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("`file` must be the path of a recording"))?
                .to_string();
            let descriptions = args
                .get("descriptions")
                .and_then(Value::as_array)
                .ok_or_else(|| anyhow!("`descriptions` must be an array of phrases"))?
                .iter()
                .map(|d| {
                    d.as_str()
                        .map(str::trim)
                        .filter(|d| !d.is_empty())
                        .map(String::from)
                        .ok_or_else(|| anyhow!("every description must be a non-empty string"))
                })
                .collect::<Result<Vec<_>>>()?;
            if descriptions.is_empty() || descriptions.len() > MOST {
                bail!(
                    "give between 1 and {MOST} descriptions (got {})",
                    descriptions.len()
                );
            }
            let path = self.context.input(&file);
            let dir = self.context.models.join(MATCH.id);
            let model = self.model.clone();
            let (seconds, scores) = blocking(move || {
                let audio = decode::read(&path)?;
                let mut slot = model.lock().unwrap_or_else(PoisonError::into_inner);
                let clap = match slot.take() {
                    Some(clap) => clap,
                    None => Clap::load(&dir)?,
                };
                let clap = slot.insert(clap);
                Ok((audio.seconds(), clap.scores(&audio, &descriptions)?))
            })
            .await?;
            let ranked: Vec<Value> = scores
                .iter()
                .map(|s| {
                    json!({
                        "description": s.description,
                        "similarity": round3(s.similarity),
                        "probability": round3(s.probability),
                    })
                })
                .collect();
            Ok(serde_json::to_string_pretty(&json!({
                "file": file,
                "seconds": round3(seconds),
                "scores": ranked,
            }))?)
        }
    }

    fn round3(x: f32) -> f64 {
        (f64::from(x) * 1000.0).round() / 1000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The deterministic signal the Python reference fixture was made from.
    fn test_signal() -> Vec<f32> {
        (0..WINDOW_SAMPLES)
            .map(|i| {
                let t = i as f64 / f64::from(SAMPLE_RATE);
                let mut v = 0.30 * (std::f64::consts::TAU * 220.0 * t).sin()
                    + 0.15 * (std::f64::consts::TAU * 880.0 * t).sin();
                if t % 0.5 < 0.01 {
                    v += 0.5 * (std::f64::consts::TAU * 3000.0 * t).sin();
                }
                v as f32
            })
            .collect()
    }

    /// The installed model's directory, when it is installed.
    fn installed() -> Option<PathBuf> {
        let dir = crate::models_dir()?;
        crate::installed(&dir, &crate::catalog::MATCH).then(|| dir.join("match"))
    }

    #[test]
    fn the_catalog_names_the_files_clap_opens() {
        let names: Vec<&str> = crate::catalog::MATCH.files.iter().map(|f| f.name).collect();
        assert_eq!(names, [AUDIO_MODEL, TEXT_MODEL, TOKENIZER]);
    }

    #[test]
    fn mel_matches_python_reference() {
        let mel = Frontend::new().log_mel(&test_signal());
        let bytes = include_bytes!("testdata/clap_ref_mel.f32le");
        let expected: Vec<f32> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect();
        assert_eq!(mel.len(), expected.len());
        let max_diff = mel
            .iter()
            .zip(&expected)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(max_diff < 0.5, "mel max diff {max_diff} dB");
    }

    #[test]
    fn embedding_matches_python_reference() {
        let Some(dir) = installed() else {
            eprintln!("the match model is not installed: nothing to compare");
            return;
        };
        let mut clap = Clap::load(&dir).unwrap();
        let audio = Audio {
            sample_rate: SAMPLE_RATE,
            channels: vec![test_signal()],
        };
        let embedding = clap.embed_audio(&audio).unwrap();
        let meta: serde_json::Value =
            serde_json::from_str(include_str!("testdata/clap_ref_meta.json")).unwrap();
        let expected: Vec<f32> = meta["embedding"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap() as f32)
            .collect();
        let cos: f32 = embedding.iter().zip(&expected).map(|(a, b)| a * b).sum();
        assert!(cos > 0.995, "cosine against the reference {cos}");
    }

    #[test]
    fn a_short_recording_is_one_window_and_a_long_one_three() {
        assert_eq!(windows(1000), [(0, 1000)]);
        assert_eq!(windows(WINDOW_SAMPLES), [(0, WINDOW_SAMPLES)]);
        let long = 4 * WINDOW_SAMPLES;
        let three = windows(long);
        assert_eq!(three.len(), 3);
        assert_eq!(
            three[1],
            (long / 2 - WINDOW_SAMPLES / 2, long / 2 + WINDOW_SAMPLES / 2)
        );
        // Just over a window: every window stays inside the recording.
        for (start, end) in windows(WINDOW_SAMPLES + 10) {
            assert!(start < end && end <= WINDOW_SAMPLES + 10);
            assert!(end - start <= WINDOW_SAMPLES);
        }
    }

    #[test]
    fn scores_are_ranked_and_their_probabilities_sum_to_one() {
        let audio = normalized(vec![1.0, 0.0, 0.0]);
        let descriptions: Vec<String> = ["far", "near", "middle"].map(String::from).to_vec();
        let texts = vec![
            normalized(vec![0.0, 1.0, 0.0]),
            normalized(vec![1.0, 0.1, 0.0]),
            normalized(vec![1.0, 1.0, 0.0]),
        ];
        let scores = rank(&audio, &descriptions, &texts);
        let order: Vec<&str> = scores.iter().map(|s| s.description.as_str()).collect();
        assert_eq!(order, ["near", "middle", "far"]);
        let total: f32 = scores.iter().map(|s| s.probability).sum();
        assert!((total - 1.0).abs() < 1e-5);
        assert!((scores[2].similarity).abs() < 1e-6, "orthogonal is 0");
        // The softmax is at CLAP's temperature: the odds of two are
        // exp(scale × the difference of their cosines).
        let odds = scores[0].probability / scores[1].probability;
        let expected = ((scores[0].similarity - scores[1].similarity) * LOGIT_SCALE).exp();
        assert!((odds / expected - 1.0).abs() < 1e-3);
    }

    /// The installed model on the CC0 starter tracks and on white noise:
    /// `LOCALGPT_AUDIO_MODELS=<dir> cargo test -p localgpt-audio-models
    /// --features run -- --ignored --nocapture installed_model`. The tracks
    /// come from `$LOCALGPT_WORLD_ASSETS_SRC/music`, else a
    /// localgpt-world-assets checkout beside the workspace.
    #[test]
    #[ignore = "runs the installed match model"]
    fn installed_model_on_tracks_and_noise() {
        let dir = installed().expect("the match model is not installed");
        let mut clap = Clap::load(&dir).unwrap();
        let strings = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();

        // Ten seconds of white noise, from a fixed seed.
        let mut state = 0x5EED_u64;
        let noise: Vec<f32> = (0..WINDOW_SAMPLES)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 40) as f32 / (1u64 << 24) as f32 - 0.5
            })
            .collect();
        let noise = Audio {
            sample_rate: SAMPLE_RATE,
            channels: vec![noise],
        };
        let scores = clap
            .scores(
                &noise,
                &strings(&["white noise", "a piano melody", "birdsong"]),
            )
            .unwrap();
        print_scores("white noise (synthesized, 10 s)", &scores);
        assert_eq!(scores[0].description, "white noise");

        let music = std::env::var_os("LOCALGPT_WORLD_ASSETS_SRC")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../localgpt-world-assets")
            })
            .join("music");
        let descriptions = strings(&[
            "a calm ambient piece",
            "an energetic upbeat dance track",
            "a dark, brooding soundscape",
            "a solo piano",
            "rain on a tin roof",
            "a man speaking",
            "a dog barking",
        ]);
        for name in ["tidewater", "emberfall", "nightglass", "amber-drift"] {
            let path = music.join(format!("{name}.mp3"));
            let started = std::time::Instant::now();
            let audio = decode::read(&path).unwrap();
            let decoded = started.elapsed();
            let scores = clap.scores(&audio, &descriptions).unwrap();
            print_scores(
                &format!(
                    "{name} ({:.0} s; decoded in {:.2} s, all in {:.2} s)",
                    audio.seconds(),
                    decoded.as_secs_f32(),
                    started.elapsed().as_secs_f32()
                ),
                &scores,
            );
            // Music, whichever mood: never the rain, the speech or the dog.
            assert!(
                !["rain on a tin roof", "a man speaking", "a dog barking"]
                    .contains(&scores[0].description.as_str()),
                "{name} heard as {}",
                scores[0].description
            );
        }
    }

    fn print_scores(what: &str, scores: &[Score]) {
        eprintln!("{what}");
        for s in scores {
            eprintln!(
                "  {:>6.3}  {:>6.3}  {}",
                s.probability, s.similarity, s.description
            );
        }
    }
}
