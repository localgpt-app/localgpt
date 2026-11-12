//! The notes played in a recording: Spotify's basic-pitch, on the CPU.
//!
//! Ported from spotify/basic-pitch, Apache-2.0 (`basic_pitch/inference.py`,
//! `note_creation.py` and `constants.py` at commit fa5997a): the windowing
//! of 22 050 Hz mono into overlapping two-second windows, the unwrapping of
//! the model's output into one posteriorgram, and the note creation of
//! `output_to_notes_polyphonic` — inferred onsets, peak picking, the energy
//! tolerance, the minimum note length and the "melodia trick" — with
//! basic-pitch's defaults, and its frame-to-time conversion
//! (`model_frames_to_time`). Pitch bends are not ported: a note is a MIDI
//! pitch, a start, an end and a velocity.
//!
//! The model is basic-pitch's ICASSP 2022 network (`nmp.onnx`, 230 444
//! bytes, converted by tf2onnx 1.15.1: IR version 8, opset 15). The
//! constant-Q transform is inside the graph, so it takes samples. Its graph,
//! as ONNX Runtime reads it:
//!
//! | | name | shape (f32) | |
//! |-|-|-|-|
//! | input | `serving_default_input_2:0` | `[batch, 43844, 1]` | 43 844 samples of 22 050 Hz mono (two seconds less one hop) |
//! | output | `StatefulPartitionedCall:1` | `[batch, 172, 88]` | note: whether each piano key, A0 to C8, sounds in each frame (86 a second) |
//! | output | `StatefulPartitionedCall:2` | `[batch, 172, 88]` | onset: whether a note starts there |
//! | output | `StatefulPartitionedCall:0` | `[batch, 172, 264]` | contour, three bins a semitone, for pitch bends (not read) |
//!
//! The batch is dynamic, so several windows run at once.

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail, ensure};

use crate::decode;

#[cfg(feature = "mcp")]
pub(crate) mod tool;

/// The model's file in its directory (`models_dir()/notes/`).
pub const MODEL_FILE: &str = "nmp.onnx";

/// The rate the model hears, in Hz (`AUDIO_SAMPLE_RATE`).
pub const SAMPLE_RATE: u32 = 22_050;
const FFT_HOP: usize = 256;
/// Frames a second, as basic-pitch counts them (`ANNOTATIONS_FPS`: an
/// integer division, 86).
const FRAMES_PER_SECOND: usize = SAMPLE_RATE as usize / FFT_HOP;
/// Frames in one window's output (`ANNOT_N_FRAMES`, 172).
const WINDOW_FRAMES: usize = FRAMES_PER_SECOND * 2;
/// Samples in one window (`AUDIO_N_SAMPLES`): two seconds less a hop.
const WINDOW_SAMPLES: usize = SAMPLE_RATE as usize * 2 - FFT_HOP;
/// Frames a window shares with its neighbours
/// (`DEFAULT_OVERLAPPING_FRAMES`); half of them are cut from each end of a
/// window's output.
const OVERLAP_FRAMES: usize = 30;
const OVERLAP_SAMPLES: usize = OVERLAP_FRAMES * FFT_HOP;
/// From one window's start to the next's (36 164 samples).
const HOP_SAMPLES: usize = WINDOW_SAMPLES - OVERLAP_SAMPLES;
/// A window's frames once its overlap is cut (142).
const KEPT_FRAMES: usize = WINDOW_FRAMES - OVERLAP_FRAMES;
/// Piano keys, A0 to C8.
const KEYS: usize = 88;
/// The MIDI pitch of the lowest key, A0 (`MIDI_OFFSET`).
const LOWEST_KEY: u8 = 21;
/// basic-pitch's measured timing correction, in seconds
/// (`MAGIC_ALIGNMENT_OFFSET`).
const MAGIC_ALIGNMENT_OFFSET: f64 = 0.0018;
/// Windows given to the model in one run.
const BATCH: usize = 16;

const INPUT: &str = "serving_default_input_2:0";
const NOTE_OUTPUT: &str = "StatefulPartitionedCall:1";
const ONSET_OUTPUT: &str = "StatefulPartitionedCall:2";

/// How notes are read from the model's output. The defaults are
/// basic-pitch's.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteOptions {
    /// How sure the model must be that a note starts, 0 to 1 (0.5). Lower
    /// finds more notes.
    pub onset_threshold: f32,
    /// How sure it must be that a note still sounds, 0 to 1 (0.3). Lower
    /// makes notes longer and finds quieter ones.
    pub frame_threshold: f32,
    /// The shortest note kept, in milliseconds (127.7).
    pub min_note_ms: f32,
    /// Also make notes of sustained energy that no onset started (the
    /// "melodia trick", on).
    pub melodia_trick: bool,
    /// Frames under the frame threshold a note may run through before it
    /// ends (11, about 128 ms).
    pub energy_tolerance: usize,
    /// Hear the recording from here, in seconds.
    pub start: f32,
    /// Up to here, in seconds; `None` is its end.
    pub end: Option<f32>,
}

impl Default for NoteOptions {
    fn default() -> Self {
        Self {
            onset_threshold: 0.5,
            frame_threshold: 0.3,
            min_note_ms: 127.7,
            melodia_trick: true,
            energy_tolerance: 11,
            start: 0.0,
            end: None,
        }
    }
}

impl NoteOptions {
    fn check(&self) -> Result<()> {
        ensure!(
            self.onset_threshold > 0.0 && self.onset_threshold <= 1.0,
            "the onset threshold must be above 0 and at most 1"
        );
        ensure!(
            self.frame_threshold > 0.0 && self.frame_threshold <= 1.0,
            "the frame threshold must be above 0 and at most 1"
        );
        ensure!(
            self.min_note_ms.is_finite() && self.min_note_ms >= 0.0,
            "the minimum note length must be zero or more milliseconds"
        );
        ensure!(
            self.start.is_finite() && self.start >= 0.0,
            "`start` must be zero or more seconds"
        );
        if let Some(end) = self.end {
            ensure!(
                end.is_finite() && end > self.start,
                "`end` must come after `start`"
            );
        }
        Ok(())
    }

    /// The minimum note length in frames, as basic-pitch rounds it (127.7 ms
    /// is 11 frames). A note must be longer than this.
    fn min_note_frames(&self) -> usize {
        (self.min_note_ms as f64 / 1000.0 * (SAMPLE_RATE as f64 / FFT_HOP as f64)).round() as usize
    }
}

/// One note heard.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Note {
    /// When it starts, in seconds from the start of the file.
    pub start: f32,
    /// When it ends, in seconds from the start of the file.
    pub end: f32,
    /// Its MIDI pitch: 60 is middle C, C4.
    pub pitch: u8,
    /// How strongly it sounds, 0 to 1: the model's mean confidence over the
    /// note.
    pub velocity: f32,
}

/// What a transcription heard.
#[derive(Debug, Clone)]
pub struct Transcription {
    /// The notes, by start, then pitch.
    pub notes: Vec<Note>,
    /// The span heard, in seconds from the start of the file.
    pub heard: (f32, f32),
}

/// A pitch's name in scientific notation, sharps for black keys: 60 is
/// `C4`, 61 `C#4`, 21 `A0`.
pub fn note_name(pitch: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let octave = i32::from(pitch) / 12 - 1;
    format!("{}{octave}", NAMES[usize::from(pitch % 12)])
}

/// A transcription as JSON, as `audio_notes` and `localgpt audio notes
/// --json` give it: a summary (how many notes, the lowest and highest, the
/// span heard and the span they sound in) and the notes, the earliest
/// `limit` of them, each with its name, MIDI pitch, start and end in seconds
/// and velocity.
pub fn report(heard: &Transcription, limit: Option<usize>) -> serde_json::Value {
    use serde_json::json;
    let round = |v: f32, places: i32| {
        let scale = 10f64.powi(places);
        (f64::from(v) * scale).round() / scale
    };
    let notes = &heard.notes;
    let listed = &notes[..limit.map_or(notes.len(), |limit| limit.min(notes.len()))];
    let mut summary = json!({
        "count": notes.len(),
        "heard": {"start": round(heard.heard.0, 3), "end": round(heard.heard.1, 3)},
        "listed": listed.len(),
        "cut": listed.len() < notes.len(),
    });
    let pitches = notes.iter().map(|n| n.pitch);
    if let (Some(low), Some(high)) = (pitches.clone().min(), pitches.max()) {
        summary["lowest"] = json!(note_name(low));
        summary["highest"] = json!(note_name(high));
        let first = notes.iter().map(|n| n.start).fold(f32::INFINITY, f32::min);
        let last = notes.iter().map(|n| n.end).fold(0.0, f32::max);
        summary["sounding"] = json!({"start": round(first, 3), "end": round(last, 3)});
    }
    if let Some(next) = notes.get(listed.len()) {
        summary["more"] = json!(format!(
            "{} later notes are not listed: ask again from {:.3} s (`start`), or for more (`max_notes`)",
            notes.len() - listed.len(),
            next.start
        ));
    }
    let listed: Vec<_> = listed
        .iter()
        .map(|n| {
            json!({
                "note": note_name(n.pitch),
                "pitch": n.pitch,
                "start": round(n.start, 3),
                "end": round(n.end, 3),
                "velocity": round(n.velocity, 2),
            })
        })
        .collect();
    json!({"summary": summary, "notes": listed})
}

/// Transcribe `input` with the model installed in `model_dir`: the notes,
/// by start.
pub fn transcribe(model_dir: &Path, input: &Path, options: &NoteOptions) -> Result<Vec<Note>> {
    Ok(Transcriber::load(model_dir)?
        .transcribe(input, options)?
        .notes)
}

/// basic-pitch loaded, to transcribe one recording after another.
pub struct Transcriber {
    session: ort::session::Session,
}

impl Transcriber {
    /// Load the model from `model_dir`.
    pub fn load(model_dir: &Path) -> Result<Self> {
        let path = model_dir.join(MODEL_FILE);
        if !path.is_file() {
            bail!(
                "basic-pitch is not installed ({} is missing): run `localgpt audio install notes`",
                path.display()
            );
        }
        let session = ort::session::Session::builder()
            .and_then(|builder| builder.commit_from_file(&path))
            .with_context(|| format!("loading {}", path.display()))?;
        let has = |outlets: &[ort::value::Outlet], name: &str| {
            outlets.iter().any(|outlet| outlet.name() == name)
        };
        ensure!(
            has(session.inputs(), INPUT)
                && has(session.outputs(), NOTE_OUTPUT)
                && has(session.outputs(), ONSET_OUTPUT),
            "{} is not basic-pitch's graph (no `{INPUT}`, `{NOTE_OUTPUT}` or `{ONSET_OUTPUT}`)",
            path.display()
        );
        Ok(Self { session })
    }

    /// Transcribe `input` (any format [`decode::read`] reads).
    pub fn transcribe(&mut self, input: &Path, options: &NoteOptions) -> Result<Transcription> {
        options.check()?;
        let audio = decode::read(input)?;
        let rate = audio.sample_rate.max(1) as f32;
        let seconds = audio.seconds();
        if options.start >= seconds {
            bail!(
                "{} is {seconds:.2} s long; `start` is after its end",
                input.display()
            );
        }
        let end = options.end.map_or(seconds, |end| end.min(seconds));
        let mono = audio.mono();
        let from = ((options.start * rate) as usize).min(mono.len());
        let to = ((end * rate).ceil() as usize).clamp(from, mono.len());
        let mono = decode::resample(&mono[from..to], audio.sample_rate, SAMPLE_RATE)
            .with_context(|| format!("resampling {}", input.display()))?;
        let posteriors = self
            .posteriors(&mono)
            .with_context(|| format!("running basic-pitch on {}", input.display()))?;
        let offset = from as f64 / f64::from(audio.sample_rate.max(1));
        let notes = posteriors
            .notes(options)
            .into_iter()
            .map(|n| Note {
                start: (offset + frame_time(n.start)) as f32,
                end: (offset + frame_time(n.end)) as f32,
                pitch: n.pitch,
                velocity: n.amplitude,
            })
            .collect();
        Ok(Transcription {
            notes,
            heard: (options.start, end),
        })
    }

    /// The model's output over `audio` (22 050 Hz mono), windowed and
    /// unwrapped as basic-pitch's `run_inference` does.
    pub fn posteriors(&mut self, audio: &[f32]) -> Result<Posteriors> {
        let frames = expected_frames(audio.len());
        let mut out = Posteriors::default();
        if frames == 0 {
            return Ok(out);
        }
        let windows = window_count(audio.len());
        out.note.reserve(windows * KEPT_FRAMES * KEYS);
        out.onset.reserve(windows * KEPT_FRAMES * KEYS);
        let mut first = 0;
        while first < windows && out.frames < frames {
            let batch = BATCH.min(windows - first);
            let mut input = Vec::with_capacity(batch * WINDOW_SAMPLES);
            for w in first..first + batch {
                input.extend(window(audio, w));
            }
            let tensor = ort::value::Tensor::from_array(([batch, WINDOW_SAMPLES, 1], input))?;
            let outputs = self.session.run(ort::inputs![INPUT => tensor])?;
            let read = |name: &str| -> Result<&[f32]> {
                let value = outputs
                    .get(name)
                    .ok_or_else(|| anyhow!("the model gave no `{name}`"))?;
                let (shape, data) = value.try_extract_tensor::<f32>()?;
                ensure!(
                    **shape == [batch as i64, WINDOW_FRAMES as i64, KEYS as i64],
                    "`{name}` is {shape}, not [{batch}, {WINDOW_FRAMES}, {KEYS}]"
                );
                Ok(data)
            };
            let (note, onset) = (read(NOTE_OUTPUT)?, read(ONSET_OUTPUT)?);
            for b in 0..batch {
                let kept = keep(b);
                out.note.extend(note[kept.clone()].iter().map(|v| sane(*v)));
                out.onset.extend(onset[kept].iter().map(|v| sane(*v)));
                out.frames += KEPT_FRAMES;
            }
            first += batch;
        }
        // Trim to the frames the audio fills (`unwrap_output`).
        out.frames = out.frames.min(frames);
        out.note.truncate(out.frames * KEYS);
        out.onset.truncate(out.frames * KEYS);
        Ok(out)
    }
}

/// A model output that is not a number counts as silence.
fn sane(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}

/// The part of window `b`'s output kept: its frames less half the overlap
/// at each end, as a range into the batch's flat output.
fn keep(b: usize) -> std::ops::Range<usize> {
    let base = b * WINDOW_FRAMES * KEYS;
    base + OVERLAP_FRAMES / 2 * KEYS..base + (WINDOW_FRAMES - OVERLAP_FRAMES / 2) * KEYS
}

/// How many windows cover `len` samples once half an overlap of silence is
/// put before them (`get_audio_input`, `window_audio_file`).
fn window_count(len: usize) -> usize {
    (len + OVERLAP_SAMPLES / 2).div_ceil(HOP_SAMPLES)
}

/// Window `w` of `audio`: the padded audio from `w` hops in, zero-filled
/// past the end.
fn window(audio: &[f32], w: usize) -> Vec<f32> {
    let mut out = vec![0.0; WINDOW_SAMPLES];
    // In the padded signal the window starts `w` hops in, and the audio half
    // an overlap in.
    let (start, pad) = (w * HOP_SAMPLES, OVERLAP_SAMPLES / 2);
    let skip = pad.saturating_sub(start);
    let from = start.saturating_sub(pad).min(audio.len());
    let to = (start + WINDOW_SAMPLES - pad).clamp(from, audio.len());
    out[skip..skip + (to - from)].copy_from_slice(&audio[from..to]);
    out
}

/// The frames of output `len` samples fill (`unwrap_output`'s trim).
fn expected_frames(len: usize) -> usize {
    (len as f64 / HOP_SAMPLES as f64 * KEPT_FRAMES as f64) as usize
}

/// When frame `frame` of the unwrapped output falls, in seconds
/// (`model_frames_to_time`): a hop a frame, less basic-pitch's correction
/// for each window passed. The correction is counted in windows of 172
/// frames, as basic-pitch counts it; its magic offset is what keeps that in
/// step with the 142 frames a window really keeps.
fn frame_time(frame: usize) -> f64 {
    let hop = FFT_HOP as f64 / f64::from(SAMPLE_RATE);
    let window_offset = hop * (WINDOW_FRAMES as f64 - WINDOW_SAMPLES as f64 / FFT_HOP as f64)
        + MAGIC_ALIGNMENT_OFFSET;
    frame as f64 * hop - window_offset * (frame / WINDOW_FRAMES) as f64
}

/// The model's output over a recording: a row a frame (86 a second), a
/// column a piano key (A0 to C8).
#[derive(Debug, Clone, Default)]
pub struct Posteriors {
    pub frames: usize,
    /// Whether each key sounds in each frame, frame-major.
    pub note: Vec<f32>,
    /// Whether a note starts there, frame-major.
    pub onset: Vec<f32>,
}

/// A note in frames: `[start, end)`, its MIDI pitch and its mean confidence.
#[derive(Debug, Clone, Copy, PartialEq)]
struct FrameNote {
    start: usize,
    end: usize,
    pitch: u8,
    amplitude: f32,
}

impl Posteriors {
    /// The notes in these posteriors (`output_to_notes_polyphonic`, with
    /// inferred onsets and no frequency limits), by start, then pitch.
    fn notes(&self, options: &NoteOptions) -> Vec<FrameNote> {
        let n = self.frames;
        if n < 2 || self.note.len() != n * KEYS || self.onset.len() != n * KEYS {
            return Vec::new();
        }
        let frames = &self.note;
        let at = |t: usize, f: usize| t * KEYS + f;
        let threshold = options.frame_threshold;
        let tolerance = options.energy_tolerance;
        let min_len = options.min_note_frames();
        let onsets = inferred_onsets(&self.onset, frames, n);

        // Onsets: peaks in time at or above the onset threshold, walked
        // backwards in time (and down in pitch within a frame).
        let mut starts = Vec::new();
        for t in 1..n - 1 {
            for f in 0..KEYS {
                let v = onsets[at(t, f)];
                if v >= options.onset_threshold
                    && v > onsets[at(t - 1, f)]
                    && v > onsets[at(t + 1, f)]
                {
                    starts.push((t, f));
                }
            }
        }

        let mut remaining = frames.clone();
        // A note claims its key and the keys beside it.
        let clear = |remaining: &mut [f32], t: usize, f: usize| {
            remaining[at(t, f)] = 0.0;
            if f + 1 < KEYS {
                remaining[at(t, f + 1)] = 0.0;
            }
            if f > 0 {
                remaining[at(t, f - 1)] = 0.0;
            }
        };
        let mean = |from: usize, to: usize, f: usize| -> f32 {
            (from..to).map(|t| frames[at(t, f)]).sum::<f32>() / (to - from).max(1) as f32
        };
        let mut notes = Vec::new();
        for &(start, f) in starts.iter().rev() {
            if start >= n - 1 {
                continue;
            }
            // Run on until the key has been quiet for `tolerance` frames.
            let mut i = start + 1;
            let mut k = 0;
            while i < n - 1 && k < tolerance {
                if remaining[at(i, f)] < threshold {
                    k += 1;
                } else {
                    k = 0;
                }
                i += 1;
            }
            i -= k;
            if i - start <= min_len {
                continue;
            }
            for t in start..i {
                clear(&mut remaining, t, f);
            }
            notes.push(FrameNote {
                start,
                end: i,
                pitch: LOWEST_KEY + f as u8,
                amplitude: mean(start, i, f),
            });
        }

        if options.melodia_trick {
            // What energy is left, loudest first: a lazy heap stands in for
            // basic-pitch's repeated argmax, the same order (ties to the
            // earliest frame, then the lowest key) since a cell only ever
            // drops to zero.
            let mut heap: BinaryHeap<Cell> = remaining
                .iter()
                .enumerate()
                .filter(|(_, v)| **v > threshold)
                .map(|(index, value)| Cell {
                    value: *value,
                    index,
                })
                .collect();
            while let Some(cell) = heap.pop() {
                if remaining[cell.index] <= threshold {
                    continue;
                }
                let (mid, f) = (cell.index / KEYS, cell.index % KEYS);
                remaining[cell.index] = 0.0;

                let mut i = mid + 1;
                let mut k = 0;
                while i < n - 1 && k < tolerance {
                    if remaining[at(i, f)] < threshold {
                        k += 1;
                    } else {
                        k = 0;
                    }
                    clear(&mut remaining, i, f);
                    i += 1;
                }
                let end = i - 1 - k;

                let mut i = mid as isize - 1;
                let mut k = 0;
                while i > 0 && k < tolerance {
                    if remaining[at(i as usize, f)] < threshold {
                        k += 1;
                    } else {
                        k = 0;
                    }
                    clear(&mut remaining, i as usize, f);
                    i -= 1;
                }
                let start = (i + 1 + k as isize) as usize;

                if end - start <= min_len {
                    continue;
                }
                notes.push(FrameNote {
                    start,
                    end,
                    pitch: LOWEST_KEY + f as u8,
                    amplitude: mean(start, end, f),
                });
            }
        }

        notes.sort_by(|a, b| a.start.cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
        notes
    }
}

/// The model's onsets, raised where a key's confidence jumps
/// (`get_infered_onsets`): the smaller of its rise over one and over two
/// frames, scaled so the largest rise matches the largest onset.
fn inferred_onsets(onsets: &[f32], frames: &[f32], n: usize) -> Vec<f32> {
    const N_DIFF: usize = 2;
    let mut rise = vec![0.0f32; n * KEYS];
    for t in N_DIFF..n {
        for f in 0..KEYS {
            let now = frames[t * KEYS + f];
            let d = (1..=N_DIFF)
                .map(|m| now - frames[(t - m) * KEYS + f])
                .fold(f32::INFINITY, f32::min);
            rise[t * KEYS + f] = d.max(0.0);
        }
    }
    let most_rise = rise.iter().copied().fold(0.0f32, f32::max);
    let most_onset = onsets.iter().copied().fold(0.0f32, f32::max);
    if most_rise <= 0.0 {
        // basic-pitch divides by zero here and finds no onsets at all; the
        // model's own onsets stand.
        return onsets.to_vec();
    }
    onsets
        .iter()
        .zip(rise)
        .map(|(onset, rise)| onset.max(most_onset * rise / most_rise))
        .collect()
}

/// A cell of the posteriorgram, ordered loudest first, then earliest in
/// row-major order (numpy's `argmax`).
#[derive(Debug, Clone, Copy)]
struct Cell {
    value: f32,
    index: usize,
}

impl PartialEq for Cell {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Cell {}

impl PartialOrd for Cell {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Cell {
    fn cmp(&self, other: &Self) -> Ordering {
        self.value
            .total_cmp(&other.value)
            .then(other.index.cmp(&self.index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_constants_are_basic_pitchs() {
        assert_eq!(FRAMES_PER_SECOND, 86);
        assert_eq!(WINDOW_FRAMES, 172);
        assert_eq!(WINDOW_SAMPLES, 43_844);
        assert_eq!(HOP_SAMPLES, 36_164);
        assert_eq!(KEPT_FRAMES, 142);
        assert_eq!(NoteOptions::default().min_note_frames(), 11);
    }

    #[test]
    fn note_names_are_scientific_pitch() {
        assert_eq!(note_name(60), "C4");
        assert_eq!(note_name(61), "C#4");
        assert_eq!(note_name(21), "A0");
        assert_eq!(note_name(108), "C8");
        assert_eq!(note_name(0), "C-1");
        assert_eq!(note_name(127), "G9");
    }

    #[test]
    fn frames_become_times_with_the_window_correction() {
        let hop = 256.0 / 22_050.0;
        assert_eq!(frame_time(0), 0.0);
        assert!((frame_time(1) - hop).abs() < 1e-12);
        assert!((frame_time(171) - 171.0 * hop).abs() < 1e-12);
        // From frame 172 each window takes off 188 samples' worth and 1.8 ms.
        let offset = 188.0 / 22_050.0 + 0.0018;
        assert!((frame_time(172) - (172.0 * hop - offset)).abs() < 1e-12);
        assert!((frame_time(344) - (344.0 * hop - 2.0 * offset)).abs() < 1e-12);
        // Over a long recording that stays within a frame of where the
        // windows really put a frame: 142 frames and 188 samples a window.
        for frame in (0..200_000).step_by(997) {
            let truth = frame as f64 * hop - (frame / 142) as f64 * 188.0 / 22_050.0;
            assert!(
                (frame_time(frame) - truth).abs() < hop,
                "frame {frame}: {} against {truth}",
                frame_time(frame)
            );
        }
    }

    #[test]
    fn windows_overlap_and_trim_as_basic_pitchs() {
        assert_eq!(window_count(0), 1);
        assert_eq!(expected_frames(0), 0);
        assert_eq!(expected_frames(22_050), 86);
        assert_eq!(window_count(22_050), 1);
        assert_eq!(window_count(HOP_SAMPLES - OVERLAP_SAMPLES / 2), 1);
        assert_eq!(window_count(HOP_SAMPLES - OVERLAP_SAMPLES / 2 + 1), 2);
        // Every window's kept frames cover what the trim keeps.
        for len in [1, 1000, 22_050, 36_164, 100_000, 1_323_000] {
            assert!(window_count(len) * KEPT_FRAMES >= expected_frames(len));
        }

        let audio: Vec<f32> = (0..100_000).map(|i| i as f32).collect();
        let first = window(&audio, 0);
        assert_eq!(first.len(), WINDOW_SAMPLES);
        assert!(
            first[..3840].iter().all(|s| *s == 0.0),
            "half an overlap of silence"
        );
        assert_eq!(first[3840], 0.0);
        assert_eq!(first[3841], 1.0);
        let second = window(&audio, 1);
        assert_eq!(second[0], (HOP_SAMPLES - 3840) as f32);
        let last = window(&audio, window_count(audio.len()) - 1);
        assert_eq!(*last.last().unwrap(), 0.0, "zero-filled past the end");
    }

    /// Posteriors of `frames` frames, every key silent.
    fn silent(frames: usize) -> Posteriors {
        Posteriors {
            frames,
            note: vec![0.0; frames * KEYS],
            onset: vec![0.0; frames * KEYS],
        }
    }

    /// Sound `key` from `from` to `to` at `level`, with an onset at `from`.
    fn play(p: &mut Posteriors, key: usize, from: usize, to: usize, level: f32, onset: f32) {
        for t in from..to {
            p.note[t * KEYS + key] = level;
        }
        p.onset[from * KEYS + key] = onset;
    }

    #[test]
    fn an_onset_and_its_energy_make_a_note() {
        let mut p = silent(100);
        play(&mut p, 39, 10, 40, 0.8, 0.9); // C4
        let notes = p.notes(&NoteOptions::default());
        assert_eq!(notes.len(), 1, "{notes:?}");
        let note = notes[0];
        assert_eq!(note.pitch, 60);
        assert_eq!((note.start, note.end), (10, 40));
        assert!((note.amplitude - 0.8).abs() < 1e-6);
    }

    #[test]
    fn short_notes_are_dropped() {
        let options = NoteOptions::default();
        // 11 frames is not longer than the minimum; 12 is.
        let mut p = silent(100);
        play(&mut p, 39, 10, 21, 0.8, 0.9);
        play(&mut p, 50, 50, 62, 0.8, 0.9);
        let notes = p.notes(&options);
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert_eq!(notes[0].pitch, 71);
        assert_eq!((notes[0].start, notes[0].end), (50, 62));
    }

    #[test]
    fn a_note_runs_through_a_dip_shorter_than_the_tolerance() {
        // Ten frames under the threshold, then back, too gently for an
        // inferred onset.
        let mut p = silent(120);
        play(&mut p, 39, 10, 30, 0.8, 0.9);
        play(&mut p, 39, 30, 40, 0.25, 0.0);
        play(&mut p, 39, 40, 60, 0.4, 0.0);
        let notes = p.notes(&NoteOptions::default());
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert_eq!((notes[0].start, notes[0].end), (10, 60));

        // A gap of eleven frames ends it; the rest, with no onset, is found by
        // the melodia trick, or not at all without it. (A note the trick finds
        // ends at its last frame above the threshold, not after it, as in
        // basic-pitch.)
        let mut p = silent(120);
        play(&mut p, 39, 10, 30, 0.8, 0.9);
        play(&mut p, 39, 41, 61, 0.35, 0.0);
        let with = p.notes(&NoteOptions::default());
        assert_eq!(with.len(), 2, "{with:?}");
        assert_eq!((with[0].start, with[0].end), (10, 30));
        assert_eq!((with[1].start, with[1].end), (41, 60));
        let without = p.notes(&NoteOptions {
            melodia_trick: false,
            ..NoteOptions::default()
        });
        assert_eq!(without.len(), 1, "{without:?}");
    }

    #[test]
    fn a_weak_onset_is_raised_by_a_jump_in_energy() {
        // The onset output alone is under the threshold, but the key's
        // confidence jumps there; the inferred onset starts the note, where
        // the melodia trick alone would not (it is off here).
        let mut p = silent(100);
        play(&mut p, 39, 20, 50, 0.9, 0.2);
        // Another note's strong onset sets the scale.
        play(&mut p, 60, 60, 90, 0.4, 0.9);
        let notes = p.notes(&NoteOptions {
            melodia_trick: false,
            ..NoteOptions::default()
        });
        let pitches: Vec<u8> = notes.iter().map(|n| n.pitch).collect();
        assert_eq!(pitches, [60, 81], "{notes:?}");
        assert_eq!(notes[0].start, 20);
    }

    #[test]
    fn chords_are_polyphonic_and_neighbours_are_claimed() {
        let mut p = silent(100);
        // C4, E4, G4 together.
        for key in [39, 43, 46] {
            play(&mut p, key, 10, 50, 0.7, 0.8);
        }
        // Energy leaking into the key above C4 belongs to C4.
        for t in 10..50 {
            p.note[t * KEYS + 40] = 0.35;
        }
        let notes = p.notes(&NoteOptions::default());
        let pitches: Vec<u8> = notes.iter().map(|n| n.pitch).collect();
        assert_eq!(pitches, [60, 64, 67], "{notes:?}");
        assert!(notes.iter().all(|n| (n.start, n.end) == (10, 50)));
    }

    #[test]
    fn melodia_picks_the_loudest_first() {
        let mut p = silent(100);
        // Two sustained keys a semitone apart with no onsets: the louder one
        // is taken and claims its neighbour.
        for t in 20..60 {
            p.note[t * KEYS + 30] = 0.6;
            p.note[t * KEYS + 31] = 0.5;
        }
        let notes = p.notes(&NoteOptions::default());
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert_eq!(notes[0].pitch, LOWEST_KEY + 30);
        assert_eq!((notes[0].start, notes[0].end), (20, 59));
    }

    #[test]
    fn nothing_or_almost_nothing_makes_no_notes() {
        let options = NoteOptions::default();
        assert!(silent(0).notes(&options).is_empty());
        assert!(silent(1).notes(&options).is_empty());
        assert!(silent(500).notes(&options).is_empty());
        let mut p = silent(3);
        p.note.fill(1.0);
        p.onset.fill(1.0);
        assert!(p.notes(&options).is_empty());
        let mut p = silent(20);
        p.note.fill(1.0);
        p.onset.fill(1.0);
        let notes = p.notes(&options);
        assert!(notes.iter().all(|n| n.end <= 19), "{notes:?}");
    }

    #[test]
    fn options_are_checked() {
        let ok = NoteOptions::default();
        assert!(ok.check().is_ok());
        for bad in [
            NoteOptions {
                onset_threshold: 0.0,
                ..ok.clone()
            },
            NoteOptions {
                frame_threshold: 1.5,
                ..ok.clone()
            },
            NoteOptions {
                min_note_ms: f32::NAN,
                ..ok.clone()
            },
            NoteOptions {
                start: 5.0,
                end: Some(4.0),
                ..ok.clone()
            },
        ] {
            assert!(bad.check().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_report_summarizes_and_says_when_it_is_cut() {
        let heard = Transcription {
            notes: vec![
                Note {
                    start: 0.5,
                    end: 1.0,
                    pitch: 60,
                    velocity: 0.734,
                },
                Note {
                    start: 1.0004,
                    end: 1.5,
                    pitch: 73,
                    velocity: 0.5,
                },
                Note {
                    start: 2.0,
                    end: 2.25,
                    pitch: 48,
                    velocity: 0.25,
                },
            ],
            heard: (0.0, 3.0),
        };
        let all = report(&heard, None);
        let summary = &all["summary"];
        assert_eq!(summary["count"], 3);
        assert_eq!(summary["lowest"], "C3");
        assert_eq!(summary["highest"], "C#5");
        assert_eq!(summary["sounding"]["end"], 2.25);
        assert_eq!(summary["heard"]["end"], 3.0);
        assert_eq!(summary["cut"], false);
        assert!(summary.get("more").is_none());
        assert_eq!(all["notes"][0]["note"], "C4");
        assert_eq!(all["notes"][0]["velocity"], 0.73);
        assert_eq!(all["notes"][1]["start"], 1.0);
        assert_eq!(all["notes"][1]["pitch"], 73);

        let cut = report(&heard, Some(2));
        assert_eq!(cut["summary"]["count"], 3);
        assert_eq!(cut["summary"]["listed"], 2);
        assert_eq!(cut["summary"]["cut"], true);
        assert_eq!(cut["notes"].as_array().map(Vec::len), Some(2));
        let more = cut["summary"]["more"].as_str().unwrap_or_default();
        assert!(more.contains("from 2.000 s"), "{more}");

        let none = report(
            &Transcription {
                notes: Vec::new(),
                heard: (0.0, 1.0),
            },
            None,
        );
        assert_eq!(none["summary"]["count"], 0);
        assert!(none["summary"].get("lowest").is_none());
    }

    /// Where basic-pitch is installed for the tests that need it:
    /// `LOCALGPT_AUDIO_MODELS=… cargo run -p localgpt -- audio install notes`.
    fn installed() -> Option<std::path::PathBuf> {
        let dir = crate::models_dir()?.join("notes");
        dir.join(MODEL_FILE).is_file().then_some(dir)
    }

    /// A tone with a few harmonics and a short attack and release, as a
    /// plucked or bowed instrument might sound.
    fn harmonic_tone(out: &mut [f32], rate: u32, at: f32, seconds: f32, pitch: u8) {
        let hz = 440.0 * 2f32.powf((f32::from(pitch) - 69.0) / 12.0);
        let from = (at * rate as f32) as usize;
        let len = (seconds * rate as f32) as usize;
        for i in 0..len {
            let Some(slot) = out.get_mut(from + i) else {
                break;
            };
            let t = i as f32 / rate as f32;
            let envelope = (t / 0.01).min(1.0) * ((seconds - t) / 0.05).clamp(0.0, 1.0);
            let sound: f32 = [1.0, 0.5, 0.25, 0.125]
                .iter()
                .enumerate()
                .map(|(h, a)| a * (t * hz * (h + 1) as f32 * std::f32::consts::TAU).sin())
                .sum();
            *slot += 0.3 * envelope * sound;
        }
    }

    /// That `notes` holds `melody` — each note once, at its pitch, starting
    /// within 50 ms of when it was played and lasting most of its half
    /// second — and otherwise only overtones: a quieter note an octave above
    /// a melody note, starting with it. basic-pitch hears a strong second
    /// harmonic that way (its own Python does the same on this melody).
    fn check_melody(notes: &[Note], melody: &[(u8, f32)]) {
        let played = |n: &Note| {
            melody
                .iter()
                .any(|(pitch, at)| n.pitch == *pitch && (n.start - at).abs() < 0.05)
        };
        for (pitch, at) in melody {
            let found: Vec<&Note> = notes
                .iter()
                .filter(|n| n.pitch == *pitch && (n.start - at).abs() < 0.05)
                .collect();
            assert_eq!(found.len(), 1, "{} at {at} s: {notes:?}", note_name(*pitch));
            assert!(found[0].end > found[0].start + 0.3, "{:?}", found[0]);
        }
        for n in notes.iter().filter(|n| !played(n)) {
            let under = notes
                .iter()
                .find(|m| played(m) && m.pitch + 12 == n.pitch && (m.start - n.start).abs() < 0.05);
            assert!(
                under.is_some_and(|m| m.velocity > n.velocity),
                "{n:?} is neither the melody nor an overtone of it: {notes:?}"
            );
        }
    }

    #[test]
    #[ignore = "needs basic-pitch installed (localgpt audio install notes)"]
    fn a_synthesized_melody_comes_back() {
        let Some(dir) = installed() else {
            panic!("install the model first: localgpt audio install notes");
        };
        let rate = 44_100;
        // C4, E4, G4, C5, a note every 0.6 s, the last running past a window
        // boundary (2.6 s in).
        let melody = [(60u8, 0.5f32), (64, 1.1), (67, 1.7), (72, 2.3)];
        let mut audio = vec![0.0f32; rate as usize * 4];
        for (pitch, at) in melody {
            harmonic_tone(&mut audio, rate, at, 0.5, pitch);
        }
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("melody.wav");
        decode::write_wav(&path, &[audio], rate).unwrap();

        let notes = transcribe(&dir, &path, &NoteOptions::default()).unwrap();
        for n in &notes {
            eprintln!(
                "{:>4} {:>3}  {:.3}–{:.3} s  velocity {:.2}",
                note_name(n.pitch),
                n.pitch,
                n.start,
                n.end,
                n.velocity
            );
        }
        check_melody(&notes, &melody);

        // A span of the file: only what is in it, at the file's times.
        let later = Transcriber::load(&dir)
            .unwrap()
            .transcribe(
                &path,
                &NoteOptions {
                    start: 1.5,
                    end: Some(3.0),
                    ..NoteOptions::default()
                },
            )
            .unwrap();
        check_melody(&later.notes, &melody[2..]);
        assert_eq!(later.heard, (1.5, 3.0));

        // Silence, and a recording shorter than a note, hear nothing.
        let silence = tmp.path().join("silence.wav");
        decode::write_wav(&silence, &[vec![0.0; 22_050 * 3]], 22_050).unwrap();
        assert!(
            transcribe(&dir, &silence, &NoteOptions::default())
                .unwrap()
                .is_empty()
        );
        let blip = tmp.path().join("blip.wav");
        decode::write_wav(&blip, &[vec![0.1; 100]], 22_050).unwrap();
        assert!(
            transcribe(&dir, &blip, &NoteOptions::default())
                .unwrap()
                .is_empty()
        );
    }
}
