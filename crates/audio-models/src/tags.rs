//! What a sound or a recording contains, from AudioSet's 527 labels, with
//! Xiaomi's CED-tiny ([`crate::catalog::TAGS`]).
//!
//! The official ONNX export takes the waveform itself — `waveform`, `[batch,
//! samples]` of 16 kHz mono in [-1, 1] — and holds its own front end: a
//! centred 512-point STFT every 160 samples (periodic Hann), 64 mel bands up
//! to 8 kHz, 10·log10 and a batch norm. Nothing here computes a spectrogram.
//! The graph ends in a sigmoid, so its `logits`, `[batch, 527]`, are already
//! each label's score, independent of the others: a track can be "Music",
//! "Piano" and "Singing" at once.
//!
//! The model was trained on AudioSet's 10 s clips, and its time positions run
//! out just past 10 s, so a recording is read in 10 s windows: one after
//! another, the last ending where the recording ends (overlapping the one
//! before, so no window is padded). A recording shorter than a window is read
//! whole, at its own length, which the model was built to take: padding a
//! short sound with silence to 10 s pulls in "Silence" and "Sound effect" and
//! weakens what is there (a 1 s tone heard alone is "Sine wave" 0.70, padded
//! 0.07). Only a sound shorter than one patch of the model's input (0.15 s)
//! is padded, to that.
//!
//! The overall scores are the windows' mean, label by label: what holds
//! across the recording, which is what the model's configuration asks of a
//! long clip (`eval_avg: mean`). The maximum would let one window's stray
//! label stand for the whole; a label that matters in one stretch only shows
//! in that stretch's segment.

use std::ops::Range;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};

use crate::decode;

/// The rate the model hears at.
pub const SAMPLE_RATE: u32 = 16_000;
/// The clip length it was trained on.
pub const WINDOW_SECONDS: u32 = 10;
/// The model's file in its directory.
pub const MODEL_FILE: &str = "model.onnx";
/// The file holding the label names (`id2label`).
pub const LABELS_FILE: &str = "config.json";

const WINDOW: usize = (SAMPLE_RATE * WINDOW_SECONDS) as usize;
/// The STFT's hop, and the patch the encoder reads: 16 frames by 16 bands.
const HOP: usize = 160;
const PATCH_FRAMES: usize = 16;
/// The shortest input the graph takes: one patch of frames (a centred STFT
/// gives `samples / HOP + 1` frames).
const SHORTEST: usize = (PATCH_FRAMES - 1) * HOP;
/// Windows run together, to keep the encoder busy on a long recording.
const BATCH: usize = 4;

/// What [`tag`] reports.
#[derive(Debug, Clone, Copy)]
pub struct TagOptions {
    /// Labels for the whole recording.
    pub top: usize,
    /// Labels for each 10 s window; 0 leaves the windows out.
    pub segment_top: usize,
}

impl Default for TagOptions {
    fn default() -> Self {
        Self {
            top: 10,
            segment_top: 3,
        }
    }
}

/// A label and its score, 0 to 1.
#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    pub name: String,
    pub score: f32,
}

/// One window of the recording and its best labels.
#[derive(Debug, Clone)]
pub struct Segment {
    /// Seconds from the start.
    pub start: f32,
    pub end: f32,
    pub top: Vec<Label>,
}

/// A recording's labels.
#[derive(Debug, Clone)]
pub struct Tags {
    /// The recording's length, in seconds.
    pub seconds: f32,
    /// The best labels over the whole recording, by their mean score across
    /// the windows, best first.
    pub overall: Vec<Label>,
    /// Each window's best labels, in time order (empty when
    /// [`TagOptions::segment_top`] is 0).
    pub segments: Vec<Segment>,
}

impl Tags {
    /// The answer as JSON: scores to 3 decimals, times to 2.
    pub fn to_json(&self) -> Value {
        let labels = |labels: &[Label]| -> Vec<Value> {
            labels
                .iter()
                .map(|l| json!({"label": l.name, "score": round(l.score, 3)}))
                .collect()
        };
        let mut answer = json!({
            "seconds": round(self.seconds, 2),
            "labels": labels(&self.overall),
        });
        if !self.segments.is_empty() {
            answer["segments"] = self
                .segments
                .iter()
                .map(|s| {
                    json!({
                        "start": round(s.start, 2),
                        "end": round(s.end, 2),
                        "labels": labels(&s.top),
                    })
                })
                .collect();
        }
        answer
    }
}

/// Tag the recording at `input` with the model installed in `model_dir`.
/// Loads the model each time; [`Tagger`] keeps it.
pub fn tag(model_dir: &Path, input: &Path, options: &TagOptions) -> Result<Tags> {
    Tagger::load(model_dir)?.tag_file(input, options)
}

/// The loaded model and its label names.
pub struct Tagger {
    session: ort::session::Session,
    labels: Vec<String>,
}

impl Tagger {
    /// Load the model installed in `model_dir`.
    pub fn load(model_dir: &Path) -> Result<Self> {
        let config = model_dir.join(LABELS_FILE);
        let text = std::fs::read_to_string(&config)
            .with_context(|| format!("reading {}", config.display()))?;
        let labels = label_names(&text)
            .with_context(|| format!("reading the labels in {}", config.display()))?;
        let model = model_dir.join(MODEL_FILE);
        let session = ort::session::Session::builder()
            .and_then(|builder| builder.commit_from_file(&model))
            .with_context(|| format!("loading {}", model.display()))?;
        Ok(Self { session, labels })
    }

    /// The label names, in the model's output order.
    pub fn labels(&self) -> &[String] {
        &self.labels
    }

    /// Tag an audio file.
    pub fn tag_file(&mut self, path: &Path, options: &TagOptions) -> Result<Tags> {
        let audio = decode::read(path)?;
        let mono = decode::resample(&audio.mono(), audio.sample_rate, SAMPLE_RATE)
            .with_context(|| format!("resampling {}", path.display()))?;
        self.tag_samples(&mono, options)
            .with_context(|| format!("tagging {}", path.display()))
    }

    /// Tag 16 kHz mono samples.
    pub fn tag_samples(&mut self, samples: &[f32], options: &TagOptions) -> Result<Tags> {
        let ranges = windows(samples.len());
        let mut scores = Vec::with_capacity(ranges.len());
        // Every window but a lone short one is WINDOW long, so they batch.
        for group in ranges.chunks(BATCH) {
            let inputs: Vec<&[f32]> = group.iter().map(|r| &samples[r.clone()]).collect();
            scores.extend(self.run(&inputs)?);
        }
        let rate = SAMPLE_RATE as f32;
        let segments = if options.segment_top == 0 {
            Vec::new()
        } else {
            ranges
                .iter()
                .zip(&scores)
                .map(|(range, row)| Segment {
                    start: range.start as f32 / rate,
                    end: range.end as f32 / rate,
                    top: top(row, &self.labels, options.segment_top),
                })
                .collect()
        };
        Ok(Tags {
            seconds: samples.len() as f32 / rate,
            overall: top(&mean(&scores), &self.labels, options.top),
            segments,
        })
    }

    /// The scores of one stretch of 16 kHz mono, read whole (zero-padded
    /// to the shortest input the model takes). Longer than a window, the
    /// model refuses it: [`Self::tag_samples`] reads a recording in windows.
    pub fn scores(&mut self, samples: &[f32]) -> Result<Vec<f32>> {
        let mut rows = self.run(&[samples])?;
        rows.pop()
            .ok_or_else(|| anyhow!("the model gave no scores"))
    }

    /// Run windows of equal length as one batch: a row of scores each.
    fn run(&mut self, windows: &[&[f32]]) -> Result<Vec<Vec<f32>>> {
        let Some(first) = windows.first() else {
            return Ok(Vec::new());
        };
        let length = first.len().max(SHORTEST);
        if windows.iter().any(|w| w.len().max(SHORTEST) != length) {
            bail!("the windows of one batch differ in length");
        }
        let mut input = Vec::with_capacity(windows.len() * length);
        for window in windows {
            input.extend_from_slice(window);
            input.resize(input.len() + length - window.len(), 0.0);
        }
        let tensor = ort::value::Tensor::from_array(([windows.len(), length], input))
            .context("making the model's input")?;
        let outputs = self
            .session
            .run(ort::inputs!["waveform" => tensor])
            .context("running the model")?;
        let (_, flat) = outputs["logits"]
            .try_extract_tensor::<f32>()
            .context("reading the model's scores")?;
        let classes = self.labels.len();
        if flat.len() != windows.len() * classes {
            bail!(
                "the model gave {} scores for {} windows of {classes} labels",
                flat.len(),
                windows.len()
            );
        }
        Ok(flat.chunks_exact(classes).map(<[f32]>::to_vec).collect())
    }
}

/// The label names in `config.json`'s `id2label`, in index order.
fn label_names(config: &str) -> Result<Vec<String>> {
    let config: Value = serde_json::from_str(config).context("not JSON")?;
    let map = config
        .get("id2label")
        .and_then(Value::as_object)
        .filter(|map| !map.is_empty())
        .ok_or_else(|| anyhow!("no id2label"))?;
    let mut names = vec![None; map.len()];
    for (index, name) in map {
        let i: usize = index
            .parse()
            .with_context(|| format!("id2label has a key `{index}`"))?;
        let slot = names
            .get_mut(i)
            .ok_or_else(|| anyhow!("id2label skips an index below {i}"))?;
        *slot = Some(
            name.as_str()
                .ok_or_else(|| anyhow!("label {i} is not a string"))?
                .to_string(),
        );
    }
    names
        .into_iter()
        .enumerate()
        .map(|(i, name)| name.ok_or_else(|| anyhow!("id2label has no label {i}")))
        .collect()
}

/// The windows a recording of `len` samples is read in. Up to a window long
/// it is read whole; longer, in whole windows one after another, the last
/// ending at the recording's end.
fn windows(len: usize) -> Vec<Range<usize>> {
    if len <= WINDOW {
        return std::iter::once(0..len).collect();
    }
    let mut out: Vec<Range<usize>> = (0..len / WINDOW)
        .map(|i| i * WINDOW..(i + 1) * WINDOW)
        .collect();
    if !len.is_multiple_of(WINDOW) {
        out.push(len - WINDOW..len);
    }
    out
}

/// The mean of the rows, label by label.
fn mean(rows: &[Vec<f32>]) -> Vec<f32> {
    let Some(first) = rows.first() else {
        return Vec::new();
    };
    let mut sum = vec![0.0f32; first.len()];
    for row in rows {
        for (s, v) in sum.iter_mut().zip(row) {
            *s += v;
        }
    }
    let n = rows.len() as f32;
    sum.iter().map(|s| s / n).collect()
}

/// The `n` best-scoring labels, best first.
fn top(scores: &[f32], labels: &[String], n: usize) -> Vec<Label> {
    let mut order: Vec<usize> = (0..scores.len().min(labels.len())).collect();
    order.sort_by(|&a, &b| scores[b].total_cmp(&scores[a]));
    order
        .into_iter()
        .take(n)
        .map(|i| Label {
            name: labels[i].clone(),
            score: scores[i],
        })
        .collect()
}

fn round(value: f32, decimals: i32) -> f64 {
    let scale = 10f64.powi(decimals);
    (value as f64 * scale).round() / scale
}

#[cfg(feature = "mcp")]
pub use tool::TagsTool;

#[cfg(feature = "mcp")]
mod tool {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use anyhow::{Result, anyhow};
    use async_trait::async_trait;
    use localgpt_world_agent::mcp::{Tool, ToolSchema};
    use serde_json::{Value, json};

    use super::{TagOptions, Tagger};
    use crate::tools::{ToolContext, blocking};

    const NAME: &str = "audio_tags";

    /// `audio_tags`: the model as an agent's tool. The model is loaded on the
    /// first call and kept.
    pub struct TagsTool {
        context: ToolContext,
        tagger: Arc<Mutex<Option<Tagger>>>,
    }

    impl TagsTool {
        pub fn new(context: &ToolContext) -> Self {
            Self {
                context: context.clone(),
                tagger: Arc::default(),
            }
        }
    }

    #[async_trait]
    impl Tool for TagsTool {
        fn name(&self) -> &str {
            NAME
        }

        fn schema(&self) -> ToolSchema {
            ToolSchema {
                name: NAME.to_string(),
                description: "Tell what a sound or a recording contains — instruments, voices, \
                     animals, weather, places, machines, everyday sounds — as scores for \
                     AudioSet's fixed 527 labels (\"Music\", \"Piano\", \"Speech\", \"Dog\", \
                     \"Rain\", \"Engine\", \"Silence\", …), from a model on this machine. Use it \
                     to check that a sound placed in a world is what it should be, or to choose \
                     where a recording belongs. Each score is 0 to 1 and independent of the \
                     others (a song can be Music, Guitar and Singing at once), for the whole \
                     file (the mean over its 10 s windows) and, unless `segments` is false, the \
                     best 3 of each 10 s window. It cannot judge a free-text description of a \
                     sound; that is audio_match, when installed. Reads WAV, FLAC, Ogg, MP3, \
                     AAC/M4A, ALAC and AIFF, up to 20 minutes."
                    .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "file": {
                            "type": "string",
                            "description": "The recording: absolute, or relative to the world's folder (the working directory outside a world)"
                        },
                        "top": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 527,
                            "description": "How many labels for the whole file, best first (default 10)"
                        },
                        "segments": {
                            "type": "boolean",
                            "description": "Also the best 3 labels of each 10 s window, in time order (default true)"
                        }
                    },
                    "required": ["file"]
                }),
            }
        }

        fn annotations(&self) -> Option<Value> {
            Some(json!({
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false,
            }))
        }

        async fn execute(&self, arguments: &str) -> Result<String> {
            let args: Value = serde_json::from_str(arguments)
                .map_err(|e| anyhow!("the arguments aren't JSON: {e}"))?;
            let file = args
                .get("file")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("`file` is the recording's path"))?;
            let top = match args.get("top") {
                None | Some(Value::Null) => 10,
                Some(top) => top
                    .as_u64()
                    .filter(|n| (1..=527).contains(n))
                    .ok_or_else(|| anyhow!("`top` is a whole number from 1 to 527"))?
                    as usize,
            };
            let segments = match args.get("segments") {
                None | Some(Value::Null) => true,
                Some(segments) => segments
                    .as_bool()
                    .ok_or_else(|| anyhow!("`segments` is true or false"))?,
            };
            let options = TagOptions {
                top,
                segment_top: if segments { 3 } else { 0 },
            };
            let input = self.context.input(file);
            let home: PathBuf = self.context.models.join(crate::catalog::TAGS.id);
            let tagger = self.tagger.clone();
            let tags = blocking(move || {
                // A panic mid-run leaves nothing half-made in the model.
                let mut slot = tagger.lock().unwrap_or_else(|e| e.into_inner());
                let tagger = match &mut *slot {
                    Some(tagger) => tagger,
                    empty @ None => empty.insert(Tagger::load(&home)?),
                };
                tagger.tag_file(&input, &options)
            })
            .await?;
            let mut answer = tags.to_json();
            answer["file"] = json!(file);
            Ok(serde_json::to_string(&answer)?)
        }
    }

    #[cfg(test)]
    mod tests {
        use std::path::Path;

        use super::*;
        use crate::catalog::TAGS;

        fn context(dir: &Path) -> ToolContext {
            ToolContext {
                models: dir.to_path_buf(),
                base: dir.to_path_buf(),
                out: dir.to_path_buf(),
            }
        }

        #[test]
        fn the_tool_is_offered_once_the_model_is_installed() {
            let dir = tempfile::tempdir().unwrap();
            let offered = |dir: &Path| {
                crate::tools::installed(&context(dir))
                    .iter()
                    .any(|tool| tool.name() == NAME)
            };
            assert!(!offered(dir.path()));
            let home = dir.path().join(TAGS.id);
            std::fs::create_dir_all(&home).unwrap();
            for file in TAGS.files {
                let made = std::fs::File::create(home.join(file.name)).unwrap();
                made.set_len(file.bytes).unwrap();
            }
            assert!(offered(dir.path()));
        }

        #[tokio::test]
        async fn arguments_are_checked_and_a_missing_model_is_an_error() {
            let dir = tempfile::tempdir().unwrap();
            let tool = TagsTool::new(&context(dir.path()));
            for (arguments, wanted) in [
                ("not json", "JSON"),
                ("{}", "`file`"),
                (r#"{"file": "a.wav", "top": 0}"#, "`top`"),
                (r#"{"file": "a.wav", "top": 2.5}"#, "`top`"),
                (r#"{"file": "a.wav", "segments": "yes"}"#, "`segments`"),
            ] {
                let error = tool.execute(arguments).await.unwrap_err().to_string();
                assert!(error.contains(wanted), "{arguments}: {error}");
            }
            let error = tool.execute(r#"{"file": "a.wav"}"#).await.unwrap_err();
            assert!(
                format!("{error:#}").contains(super::super::LABELS_FILE),
                "{error:#}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn short_recordings_are_read_whole() {
        for len in [0, SHORTEST - 1, WINDOW / 3, WINDOW] {
            let ranges = windows(len);
            assert_eq!(ranges.len(), 1, "{len}");
            assert_eq!(ranges[0], 0..len);
        }
    }

    #[test]
    fn long_recordings_are_read_in_whole_windows_ending_at_the_end() {
        assert_eq!(windows(2 * WINDOW), [0..WINDOW, WINDOW..2 * WINDOW]);
        let len = 2 * WINDOW + 3 * SAMPLE_RATE as usize;
        assert_eq!(
            windows(len),
            [0..WINDOW, WINDOW..2 * WINDOW, len - WINDOW..len]
        );
        assert_eq!(windows(WINDOW + 1), [0..WINDOW, 1..WINDOW + 1]);
        for len in [WINDOW + 1, 3 * WINDOW - 7, 18 * WINDOW + 12_345] {
            let ranges = windows(len);
            assert!(ranges.iter().all(|r| r.len() == WINDOW), "{len}");
            assert_eq!(ranges.last().map(|r| r.end), Some(len));
            assert!(ranges.windows(2).all(|p| p[1].start <= p[0].end), "no gap");
        }
    }

    #[test]
    fn overall_is_the_windows_mean() {
        let rows = vec![
            vec![1.0, 0.0, 0.5],
            vec![0.0, 0.0, 0.5],
            vec![0.5, 0.3, 0.5],
        ];
        assert_eq!(mean(&rows), vec![0.5, 0.1, 0.5]);
        assert!(mean(&[]).is_empty());
    }

    #[test]
    fn top_is_best_first_and_survives_nan() {
        let labels: Vec<String> = ["a", "b", "c", "d"].map(String::from).to_vec();
        let best = top(&[0.1, 0.9, f32::NAN, 0.4], &labels, 2);
        let names: Vec<&str> = best.iter().map(|l| l.name.as_str()).collect();
        // NaN sorts above every number under total_cmp; the model never
        // gives one, and a panic would be worse than an odd label.
        assert_eq!(names, ["c", "b"]);
        assert_eq!(top(&[0.1, 0.9], &labels, 10).len(), 2);
    }

    #[test]
    fn labels_come_in_index_order() {
        let config = r#"{"id2label": {"1": "Music", "0": "Speech", "2": "Piano"}}"#;
        assert_eq!(label_names(config).unwrap(), ["Speech", "Music", "Piano"]);
        assert!(label_names(r#"{"id2label": {"0": "a", "2": "c"}}"#).is_err());
        assert!(label_names(r#"{"labels": []}"#).is_err());
        assert!(label_names(r#"{"id2label": {}}"#).is_err(), "no labels");
    }

    #[test]
    fn the_answer_rounds_scores_to_three_decimals() {
        let tags = Tags {
            seconds: 12.345_678,
            overall: vec![Label {
                name: "Music".into(),
                score: 0.912_345,
            }],
            segments: vec![Segment {
                start: 0.0,
                end: 10.0,
                top: vec![Label {
                    name: "Piano".into(),
                    score: 0.000_49,
                }],
            }],
        };
        let json = tags.to_json();
        assert_eq!(json["seconds"], json!(12.35));
        assert_eq!(json["labels"][0], json!({"label": "Music", "score": 0.912}));
        assert_eq!(json["segments"][0]["labels"][0]["score"], json!(0.0));
        assert_eq!(json["segments"][0]["end"], json!(10.0));
    }

    fn names(labels: &[Label]) -> String {
        labels
            .iter()
            .map(|l| format!("{} {:.3}", l.name, l.score))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// The installed model on synthesized sounds and the CC0 tracks of a
    /// `localgpt-world-assets` checkout (`LOCALGPT_WORLD_ASSETS_SRC`, else
    /// beside a directory above this crate). Install it first, e.g.
    /// `LOCALGPT_AUDIO_MODELS=/tmp/m cargo run -p localgpt -- audio install
    /// tags`, then `LOCALGPT_AUDIO_MODELS=/tmp/m cargo test --release -p
    /// localgpt-audio-models --all-features -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs the installed model"]
    fn the_installed_model_hears_tones_noise_silence_and_music() {
        let home = crate::models_dir()
            .expect("a models directory")
            .join(crate::catalog::TAGS.id);
        let mut tagger = Tagger::load(&home).expect("`localgpt audio install tags` first");
        assert_eq!(tagger.labels().len(), 527);
        let dir = tempfile::tempdir().unwrap();
        // Written at 44.1 kHz, so the resampling is heard too.
        let rate = 44_100u32;
        let seconds = |s: f32| (s * rate as f32) as usize;
        let mut hear = |name: &str, samples: Vec<f32>| -> Tags {
            let path = dir.path().join(format!("{name}.wav"));
            decode::write_wav(&path, &[samples], rate).unwrap();
            let tags = tagger.tag_file(&path, &TagOptions::default()).unwrap();
            println!("{name}: {}", names(&tags.overall[..5]));
            tags
        };

        let sine = (0..seconds(3.0))
            .map(|i| (i as f32 / rate as f32 * 440.0 * std::f32::consts::TAU).sin() * 0.5)
            .collect();
        assert_eq!(hear("sine", sine).overall[0].name, "Sine wave");

        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let noise = (0..seconds(3.0))
            .map(|_| {
                // xorshift64: uniform in [-0.3, 0.3)
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                ((state >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0) * 0.3
            })
            .collect();
        let noise = hear("noise", noise);
        assert!(
            noise.overall[..3].iter().any(|l| l.name == "White noise"),
            "{}",
            names(&noise.overall)
        );

        let silence = hear("silence", vec![0.0; seconds(2.0)]);
        assert_eq!(silence.overall[0].name, "Silence");
        // Shorter than one patch of the model's input: padded, not refused.
        let click = hear("click", vec![0.5; seconds(0.01)]);
        assert_eq!(click.overall.len(), 10);
        assert_eq!(click.segments.len(), 1);

        let Some(music) = std::env::var_os("LOCALGPT_WORLD_ASSETS_SRC")
            .map(PathBuf::from)
            .into_iter()
            .chain(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .ancestors()
                    .map(|dir| dir.join("localgpt-world-assets")),
            )
            .map(|dir| dir.join("music"))
            .find(|dir| dir.is_dir())
        else {
            println!("no localgpt-world-assets checkout: the tracks are not heard");
            return;
        };
        const INSTRUMENTS: &[&str] = &[
            "Musical instrument",
            "Synthesizer",
            "Keyboard (musical)",
            "Piano",
            "Electric piano",
            "Drum machine",
            "Drum",
            "Guitar",
            "Bell",
        ];
        for track in ["tidewater", "emberfall", "nightglass", "amber-drift"] {
            let path = music.join(format!("{track}.mp3"));
            let start = std::time::Instant::now();
            let tags = tagger.tag_file(&path, &TagOptions::default()).unwrap();
            println!(
                "{track} ({:.1} s, tagged in {:.2?}): {}",
                tags.seconds,
                start.elapsed(),
                names(&tags.overall)
            );
            for segment in &tags.segments {
                println!(
                    "  {:5.1}–{:5.1}  {}",
                    segment.start,
                    segment.end,
                    names(&segment.top)
                );
            }
            assert_eq!(tags.overall[0].name, "Music", "{track}");
            assert!(
                tags.overall[1..]
                    .iter()
                    .any(|l| INSTRUMENTS.contains(&l.name.as_str())),
                "{track}: no instrument"
            );
            assert_eq!(
                tags.segments.len(),
                (tags.seconds / WINDOW_SECONDS as f32).ceil() as usize
            );
        }
    }

    #[test]
    fn the_catalog_pins_the_files_this_reads() {
        let names: Vec<&str> = crate::catalog::TAGS.files.iter().map(|f| f.name).collect();
        assert!(names.contains(&MODEL_FILE) && names.contains(&LABELS_FILE));
    }
}
