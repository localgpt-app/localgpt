//! `audio_notes`: basic-pitch as an agent's tool.

use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use localgpt_world_agent::mcp::{Tool, ToolSchema};
use serde_json::{Value, json};

use super::{NoteOptions, Transcriber, report};
use crate::catalog::NOTES;
use crate::tools::{ToolContext, blocking};

/// The notes an answer lists unless asked for more.
const MAX_NOTES: usize = 2000;

pub(crate) struct NotesTool {
    context: ToolContext,
    /// The model, loaded on the first call and kept for the next.
    model: Arc<Mutex<Option<Transcriber>>>,
}

impl NotesTool {
    pub(crate) fn new(context: ToolContext) -> Self {
        Self {
            context,
            model: Arc::default(),
        }
    }
}

/// An optional number argument.
fn number(args: &Value, key: &str) -> Result<Option<f32>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_f64()
            .map(|v| Some(v as f32))
            .ok_or_else(|| anyhow!("`{key}` must be a number")),
    }
}

#[async_trait]
impl Tool for NotesTool {
    fn name(&self) -> &str {
        "audio_notes"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "audio_notes".to_string(),
            description: "Hear the notes played in a recording (basic-pitch, on this machine): \
                 each note's name (\"C#4\"), MIDI pitch, start and end in seconds and velocity \
                 (0 to 1), and a summary — how many, the pitch range, the span heard. It hears \
                 pitched instruments and voice, chords included, best one at a time: on a band \
                 mix the parts blur together and drums add stray notes, so run it on a stem \
                 (`audio_stems`, when installed). Use them to read a melody — its key, range \
                 and rhythm — or to tune what a world synthesizes to the song, a `Hum` \
                 emitter's `frequency` on its root note, say. They are data, not sound: no \
                 renderer plays the format's `Abc` source yet. Reads WAV, FLAC, MP3, Ogg, \
                 AAC/M4A, ALAC and AIFF; a long recording answers sooner as a span \
                 (`start`, `end`)."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "file": {
                        "type": "string",
                        "description": "The recording: an absolute path, or one relative to the world's folder"
                    },
                    "start": {
                        "type": "number",
                        "description": "Hear from here, in seconds (default 0). Times in the answer are still from the start of the file."
                    },
                    "end": {
                        "type": "number",
                        "description": "Hear up to here, in seconds (default: the end)"
                    },
                    "onset_threshold": {
                        "type": "number",
                        "description": "How sure the model must be that a note starts, above 0 and at most 1 (default 0.5); lower finds more notes"
                    },
                    "frame_threshold": {
                        "type": "number",
                        "description": "How sure it must be that a note still sounds, above 0 and at most 1 (default 0.3); lower gives longer and quieter notes"
                    },
                    "min_note_ms": {
                        "type": "number",
                        "description": "The shortest note kept, in milliseconds (default 127.7)"
                    },
                    "max_notes": {
                        "type": "integer",
                        "description": "List at most this many notes, the earliest (default 2000); the summary says when the list was cut"
                    }
                },
                "required": ["file"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)
            .map_err(|e| anyhow!("the arguments aren't JSON: {e}"))?;
        let file = args
            .get("file")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("`file` must be the path of a recording"))?
            .to_string();
        let defaults = NoteOptions::default();
        let options = NoteOptions {
            onset_threshold: number(&args, "onset_threshold")?.unwrap_or(defaults.onset_threshold),
            frame_threshold: number(&args, "frame_threshold")?.unwrap_or(defaults.frame_threshold),
            min_note_ms: number(&args, "min_note_ms")?.unwrap_or(defaults.min_note_ms),
            start: number(&args, "start")?.unwrap_or(0.0),
            end: number(&args, "end")?,
            ..defaults
        };
        let max_notes = match args.get("max_notes") {
            None | Some(Value::Null) => MAX_NOTES,
            Some(value) => value
                .as_u64()
                .filter(|n| *n > 0)
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(|| anyhow!("`max_notes` must be a whole number above 0"))?,
        };
        let input = self.context.input(&file);
        let dir = self.context.models.join(NOTES.id);
        let model = self.model.clone();
        let heard = blocking(move || {
            // A panic in an earlier call leaves the session as it was.
            let mut slot = model.lock().unwrap_or_else(|e| e.into_inner());
            let mut transcriber = match slot.take() {
                Some(transcriber) => transcriber,
                None => Transcriber::load(&dir)?,
            };
            let heard = transcriber.transcribe(&input, &options);
            *slot = Some(transcriber);
            heard
        })
        .await?;
        let mut answer = report(&heard, Some(max_notes));
        answer["summary"]["file"] = json!(file);
        Ok(answer.to_string())
    }

    fn annotations(&self) -> Option<Value> {
        Some(json!({
            "readOnlyHint": true,
            "idempotentHint": true,
            "openWorldHint": false
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bad_arguments_are_refused_before_the_model_loads() {
        let dir = tempfile::tempdir().unwrap();
        let tool = NotesTool::new(ToolContext {
            models: dir.path().to_path_buf(),
            base: dir.path().to_path_buf(),
            out: dir.path().to_path_buf(),
        });
        for args in [
            "not json",
            r#"{}"#,
            r#"{"file": 3}"#,
            r#"{"file": "a.wav", "start": "soon"}"#,
            r#"{"file": "a.wav", "max_notes": 0}"#,
            r#"{"file": "a.wav", "max_notes": 2.5}"#,
        ] {
            assert!(tool.execute(args).await.is_err(), "{args}");
        }
        // Good arguments get as far as the model, which is not installed here.
        let err = tool.execute(r#"{"file": "a.wav"}"#).await.unwrap_err();
        assert!(
            err.to_string().contains("localgpt audio install notes"),
            "{err}"
        );
    }
}
