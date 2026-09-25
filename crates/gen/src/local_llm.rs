//! In-process local LLM (`local-llm` feature): runs a GGUF model through
//! mistral.rs as the `gguf/<name>` provider, so Gen needs no model server,
//! API key or CLI backend.
//!
//! Models come from the directory LocalGPT's apps share
//! ([`localgpt_core::paths::shared_llm_dir`],
//! `~/.local/share/localgpt/models/llm`), where LocalGPT MD's and Verse's
//! `scripts/fetch-bonsai.sh` put Bonsai-8B, so a model fetched for either
//! app is used here without a second download. Any GGUF dropped there works:
//!
//! - `gguf/<name>` picks `<name>.gguf` (the model menu lists every file);
//! - `gguf/default` picks the first one;
//! - `gguf//abs/path/model.gguf` loads a file anywhere.
//!
//! A tokenizer is taken from `<name>.tokenizer.json`, then `tokenizer.json`,
//! beside the GGUF; without one, mistral.rs uses the tokenizer inside the
//! GGUF.
//!
//! The first prompt loads the model (seconds to a minute for ~5 GB); after
//! that it stays loaded and is shared by every agent in the process (the
//! main one and a hosted session's remote worker). Switching to another
//! `gguf/` model unloads the previous one first, so two never sit in memory.
//!
//! Constraints carried over from MD and Verse, which verified mistral.rs 0.8
//! on Apple Silicon: the ~5 GB Q4_K_M needs `local-llm-metal` beside the
//! renderer, and grammar-constrained generation hangs on GGUF, so nothing
//! here uses it. Tool calling uses the model's chat template, as Verse's
//! agent tier does.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use localgpt_core::agent::providers::{
    LLMProvider, LLMResponse, LLMResponseContent, Message, Role, ToolCall, ToolSchema, Usage,
    register_provider_factory,
};
use mistralrs::{
    CalledFunction, Function, GgufModelBuilder, RequestBuilder, TextMessageRole, Tool,
    ToolCallResponse, ToolCallType, ToolChoice, ToolType,
};
use tokio::sync::Mutex;

/// The model-string prefix: `gguf/<name>`.
pub const PREFIX: &str = "gguf";

/// Route `gguf/*` models to this provider. Call once at startup, before any
/// agent is created.
pub fn register() {
    register_provider_factory(
        PREFIX,
        Arc::new(|model_id, _config| {
            let files = resolve(model_id, shared_dir().as_deref())?;
            Ok(Box::new(LocalGgufProvider { files }))
        }),
    );
}

/// `gguf/<name>` for every model in the shared directory, for the model menu.
pub fn available_models() -> Vec<String> {
    shared_dir()
        .map(|dir| gguf_files(&dir))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|file| {
            file.strip_suffix(".gguf")
                .map(|name| format!("{PREFIX}/{name}"))
        })
        .collect()
}

fn shared_dir() -> Option<PathBuf> {
    localgpt_core::paths::shared_llm_dir()
}

/// A GGUF on disk and the tokenizer to load with it. Filenames are bare,
/// relative to `dir` — the form MD and Verse verified with mistral.rs.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ModelFiles {
    dir: PathBuf,
    gguf: String,
    tokenizer: Option<String>,
}

/// Resolve a `gguf/` model id to files (see the module docs for the forms).
fn resolve(model_id: &str, shared: Option<&Path>) -> Result<ModelFiles> {
    let as_path = Path::new(model_id);
    if as_path.is_absolute() {
        anyhow::ensure!(as_path.is_file(), "no GGUF model at {model_id}");
        let dir = as_path.parent().unwrap_or(Path::new("/")).to_path_buf();
        let gguf = as_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .context("GGUF path has no file name")?;
        let tokenizer = tokenizer_for(&dir, &gguf);
        return Ok(ModelFiles {
            dir,
            gguf,
            tokenizer,
        });
    }

    let dir = shared
        .context("can't find the shared model directory; set LOCALGPT_LLM_DIR")?
        .to_path_buf();
    let files = gguf_files(&dir);
    let wanted = model_id.trim();
    let gguf = if wanted.is_empty() || wanted == "default" {
        files.into_iter().next()
    } else {
        let with_ext = format!("{}.gguf", wanted.trim_end_matches(".gguf"));
        files.into_iter().find(|file| *file == with_ext)
    }
    .with_context(|| {
        format!(
            "no GGUF model '{wanted}' in {}. Put a .gguf there (LocalGPT MD's or Verse's \
             scripts/fetch-bonsai.sh fetches Bonsai-8B), or set LOCALGPT_LLM_DIR",
            dir.display()
        )
    })?;
    let tokenizer = tokenizer_for(&dir, &gguf);
    Ok(ModelFiles {
        dir,
        gguf,
        tokenizer,
    })
}

/// `.gguf` files in `dir`, sorted so "first" is stable.
fn gguf_files(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<String> = entries
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().to_string_lossy().into_owned();
            name.ends_with(".gguf").then_some(name)
        })
        .collect();
    files.sort();
    files
}

fn tokenizer_for(dir: &Path, gguf: &str) -> Option<String> {
    let own = format!("{}.tokenizer.json", gguf.trim_end_matches(".gguf"));
    [own, "tokenizer.json".to_string()]
        .into_iter()
        .find(|name| dir.join(name).is_file())
}

/// The loaded model and the files it came from.
type Loaded = Option<(ModelFiles, Arc<mistralrs::Model>)>;

/// The one loaded model, shared by every provider instance in the process.
fn loaded() -> &'static Mutex<Loaded> {
    static LOADED: OnceLock<Mutex<Loaded>> = OnceLock::new();
    LOADED.get_or_init(|| Mutex::new(None))
}

/// The model for `files`, loading it (and unloading any other) on first use.
/// Concurrent callers wait for one load instead of starting their own.
async fn model_for(files: &ModelFiles) -> Result<Arc<mistralrs::Model>> {
    let mut slot = loaded().lock().await;
    if let Some((current, model)) = slot.as_ref()
        && current == files
    {
        return Ok(model.clone());
    }
    // Free the previous model before loading the next: two ~5 GB models
    // don't fit beside the renderer.
    *slot = None;

    tracing::info!(
        "local LLM: loading {} from {}",
        files.gguf,
        files.dir.display()
    );
    let started = std::time::Instant::now();
    let mut builder = GgufModelBuilder::new(
        files.dir.to_string_lossy().into_owned(),
        vec![files.gguf.clone()],
    );
    if let Some(tokenizer) = &files.tokenizer {
        builder = builder.with_tokenizer_json(tokenizer.clone());
    }
    let model = Arc::new(
        builder
            .build()
            .await
            .with_context(|| format!("couldn't load {}", files.dir.join(&files.gguf).display()))?,
    );
    tracing::info!(
        "local LLM: {} ready in {:.1}s",
        files.gguf,
        started.elapsed().as_secs_f32()
    );
    *slot = Some((files.clone(), model.clone()));
    Ok(model)
}

/// A `gguf/<name>` model, run in-process.
pub struct LocalGgufProvider {
    files: ModelFiles,
}

#[async_trait]
impl LLMProvider for LocalGgufProvider {
    fn name(&self) -> String {
        PREFIX.to_string()
    }

    async fn chat(
        &self,
        messages: &[Message],
        tools: Option<&[ToolSchema]>,
    ) -> Result<LLMResponse> {
        let model = model_for(&self.files).await?;
        let response = model
            // NOTE: no ToolCallsKeyFix here — see build_request's docs. The
            // request goes out with mistral.rs 0.8's upstream rendering:
            // assistant tool calls hidden, results as fresh user turns.
            .send_chat_request(build_request(messages, tools))
            .await
            .map_err(|e| anyhow::anyhow!("{}: {e}", self.files.gguf))?;
        let usage = Usage {
            input_tokens: response.usage.prompt_tokens as u64,
            output_tokens: response.usage.completion_tokens as u64,
            ..Usage::default()
        };
        let message = response
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message)
            .context("the local model returned no reply")?;
        let content = into_content(message.content, message.tool_calls);
        match &content {
            LLMResponseContent::ToolCalls { calls, .. } => tracing::debug!(
                "local LLM: {} tokens in, {} out; calls {}",
                usage.input_tokens,
                usage.output_tokens,
                calls
                    .iter()
                    .map(|call| format!(
                        "{} {}",
                        call.name,
                        localgpt_core::text::ellipsize_chars(&call.arguments, 200)
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            LLMResponseContent::Text(text) => tracing::debug!(
                "local LLM: {} tokens in, {} out; replied {}",
                usage.input_tokens,
                usage.output_tokens,
                localgpt_core::text::ellipsize_chars(text, 200)
            ),
        }
        Ok(LLMResponse {
            content,
            usage: Some(usage),
        })
    }

    async fn summarize(&self, text: &str) -> Result<String> {
        let messages = [
            Message {
                role: Role::System,
                content: "Summarize the following conversation concisely, keeping every fact, \
                          decision and open task."
                    .to_string(),
                tool_calls: None,
                tool_call_id: None,
                images: Vec::new(),
            },
            Message {
                role: Role::User,
                content: text.to_string(),
                tool_calls: None,
                tool_call_id: None,
                images: Vec::new(),
            },
        ];
        match self.chat(&messages, None).await?.content {
            LLMResponseContent::Text(summary) => Ok(summary),
            LLMResponseContent::ToolCalls { text, .. } => Ok(text.unwrap_or_default()),
        }
    }
}

/// Translate the agent's conversation into a mistral.rs request.
///
/// Assistant tool calls go through mistral.rs's own
/// `add_message_with_tool_call` — which in 0.8.1 files them under a
/// `function` key the chat templates never read, so the model sees tool
/// *results* (as user turns) but not its own previous calls. That is what
/// LocalGPT Verse's agent tier verified in production, so it stays.
/// (Renaming the key to `tool_calls` renders the calls but made repetition
/// worse, not better.)
///
/// Sampling is set explicitly: mistral.rs's `RequestBuilder` defaults to
/// greedy decoding (top_k = 1) and never applies the model's generation
/// defaults (verified in 0.8.1), and a small greedy agent loops the same
/// tool call verbatim until its turn budget runs out (seen with Bonsai-8B
/// repeating `memory_search` ~20 times). Temperature/top-p plus a light
/// repetition penalty — llama.cpp's defaults — break the exact loops.
///
/// Runs of user (or system) messages are merged: the agent sometimes adds
/// two in a row, and several GGUF chat templates reject anything but strict
/// alternation. Images are dropped with a note: this path is text-only.
fn build_request(messages: &[Message], tools: Option<&[ToolSchema]>) -> RequestBuilder {
    let mut request = RequestBuilder::new().set_sampling(mistralrs::SamplingParams {
        temperature: Some(0.8),
        top_p: Some(0.95),
        repetition_penalty: Some(1.1),
        ..mistralrs::SamplingParams::neutral()
    });
    let mut pending: Option<(Role, String)> = None;
    let flush = |request: RequestBuilder, pending: &mut Option<(Role, String)>| match pending.take()
    {
        Some((Role::System, text)) => request.add_message(TextMessageRole::System, text),
        Some((_, text)) => request.add_message(TextMessageRole::User, text),
        None => request,
    };

    for message in messages {
        match message.role {
            Role::System | Role::User => {
                let mut text = message.content.clone();
                if !message.images.is_empty() {
                    text.push_str(&format!(
                        "\n\n[{} image(s) omitted: the local model reads text only]",
                        message.images.len()
                    ));
                }
                match &mut pending {
                    Some((role, joined)) if *role == message.role => {
                        joined.push_str("\n\n");
                        joined.push_str(&text);
                    }
                    _ => {
                        request = flush(request, &mut pending);
                        pending = Some((message.role, text));
                    }
                }
            }
            Role::Assistant => {
                request = flush(request, &mut pending);
                request = match message.tool_calls.as_deref() {
                    Some(calls) if !calls.is_empty() => request.add_message_with_tool_call(
                        TextMessageRole::Assistant,
                        message.content.clone(),
                        calls
                            .iter()
                            .enumerate()
                            .map(|(index, call)| ToolCallResponse {
                                index,
                                id: call.id.clone(),
                                tp: ToolCallType::Function,
                                function: CalledFunction {
                                    name: call.name.clone(),
                                    arguments: call.arguments.clone(),
                                },
                            })
                            .collect(),
                    ),
                    _ => request.add_message(TextMessageRole::Assistant, message.content.clone()),
                };
            }
            Role::Tool => {
                request = flush(request, &mut pending);
                request = request.add_tool_message(
                    message.content.clone(),
                    message.tool_call_id.clone().unwrap_or_default(),
                );
            }
        }
    }
    request = flush(request, &mut pending);

    match tools {
        Some(tools) if !tools.is_empty() => request
            .set_tools(tools.iter().map(to_mistral_tool).collect())
            .set_tool_choice(ToolChoice::Auto),
        _ => request,
    }
}

fn to_mistral_tool(schema: &ToolSchema) -> Tool {
    Tool {
        tp: ToolType::Function,
        function: Function {
            description: Some(schema.description.clone()),
            name: schema.name.clone(),
            parameters: serde_json::from_value::<HashMap<String, serde_json::Value>>(
                schema.parameters.clone(),
            )
            .ok(),
        },
    }
}

/// The reply as the agent expects it. Reasoning a model leaves inline
/// (`<think>…</think>`) is dropped; mistral.rs already separates it for
/// templates it knows.
fn into_content(
    content: Option<String>,
    tool_calls: Option<Vec<ToolCallResponse>>,
) -> LLMResponseContent {
    let text = content
        .map(|text| strip_thinking(&text).trim().to_string())
        .filter(|text| !text.is_empty());
    match tool_calls {
        Some(calls) if !calls.is_empty() => LLMResponseContent::ToolCalls {
            calls: calls
                .into_iter()
                .map(|call| ToolCall {
                    id: call.id,
                    name: call.function.name,
                    arguments: call.function.arguments,
                })
                .collect(),
            text,
        },
        _ => LLMResponseContent::Text(text.unwrap_or_default()),
    }
}

fn strip_thinking(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<think>") {
        out.push_str(&rest[..start]);
        match rest[start..].find("</think>") {
            Some(end) => rest = &rest[start + end + "</think>".len()..],
            // Unclosed: the model ran out of tokens mid-thought.
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "localgpt-gen-llm-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn message(role: Role, content: &str) -> Message {
        Message {
            role,
            content: content.to_string(),
            tool_calls: None,
            tool_call_id: None,
            images: Vec::new(),
        }
    }

    #[test]
    fn resolves_named_default_and_absolute_models() {
        let dir = temp_dir("resolve");
        std::fs::write(dir.join("b-model.gguf"), b"gguf").unwrap();
        std::fs::write(dir.join("a-model.gguf"), b"gguf").unwrap();
        std::fs::write(dir.join("a-model.tokenizer.json"), b"{}").unwrap();
        std::fs::write(dir.join("tokenizer.json"), b"{}").unwrap();

        let named = resolve("b-model", Some(&dir)).unwrap();
        assert_eq!(named.gguf, "b-model.gguf");
        assert_eq!(named.tokenizer.as_deref(), Some("tokenizer.json"));

        let default = resolve("default", Some(&dir)).unwrap();
        assert_eq!(default.gguf, "a-model.gguf");
        assert_eq!(
            default.tokenizer.as_deref(),
            Some("a-model.tokenizer.json"),
            "a model's own tokenizer wins over the shared one"
        );

        let absolute = dir.join("b-model.gguf");
        let by_path = resolve(absolute.to_str().unwrap(), None).unwrap();
        assert_eq!(by_path.dir, dir);
        assert_eq!(by_path.gguf, "b-model.gguf");

        let missing = resolve("nope", Some(&dir)).unwrap_err().to_string();
        assert!(missing.contains("no GGUF model 'nope'"), "{missing}");
    }

    #[test]
    fn a_gguf_without_a_tokenizer_uses_the_embedded_one() {
        let dir = temp_dir("no-tok");
        std::fs::write(dir.join("m.gguf"), b"gguf").unwrap();
        assert_eq!(resolve("m", Some(&dir)).unwrap().tokenizer, None);
    }

    #[test]
    fn replies_become_text_or_tool_calls() {
        match into_content(Some("<think>plan</think>\nDone.".into()), None) {
            LLMResponseContent::Text(text) => assert_eq!(text, "Done."),
            _ => panic!("expected text"),
        }
        let call = ToolCallResponse {
            index: 0,
            id: "call-1".into(),
            tp: ToolCallType::Function,
            function: CalledFunction {
                name: "gen_spawn_primitive".into(),
                arguments: "{\"shape\":\"cube\"}".into(),
            },
        };
        match into_content(Some(String::new()), Some(vec![call])) {
            LLMResponseContent::ToolCalls { calls, text } => {
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].name, "gen_spawn_primitive");
                assert_eq!(calls[0].arguments, "{\"shape\":\"cube\"}");
                assert_eq!(text, None);
            }
            _ => panic!("expected tool calls"),
        }
    }

    #[test]
    fn unclosed_thinking_is_dropped() {
        assert_eq!(strip_thinking("a<think>b</think>c<think>d"), "ac");
    }

    #[test]
    fn consecutive_user_messages_are_merged() {
        let messages = [
            message(Role::System, "sys"),
            message(Role::User, "one"),
            message(Role::User, "two"),
            Message {
                tool_calls: Some(vec![ToolCall {
                    id: "c1".into(),
                    name: "gen_scene_info".into(),
                    arguments: "{}".into(),
                }]),
                ..message(Role::Assistant, "")
            },
            Message {
                tool_call_id: Some("c1".into()),
                ..message(Role::Tool, "ok")
            },
        ];
        let tools = [ToolSchema {
            name: "gen_scene_info".into(),
            description: "Describe the scene".into(),
            parameters: serde_json::json!({"type": "object", "properties": {}}),
        }];
        use mistralrs::RequestLike as _;

        let mut request = build_request(&messages, Some(&tools));
        let sent = request.messages_ref();
        assert_eq!(sent.len(), 4, "system, merged user, assistant, tool");
        assert_eq!(
            format!("{:?}", sent[1]["content"]),
            format!("{:?}", mistralrs::MessageContent::Left("one\n\ntwo".into()))
        );
        assert!(sent[3].contains_key("tool_call_id"));
        let (sent_tools, _) = request.take_tools().expect("tools attached");
        assert_eq!(sent_tools[0].function.name, "gen_scene_info");

        // The assistant's calls keep mistral.rs's upstream `function` key
        // (hidden from the chat template) — see build_request's docs.
        let mut request = build_request(&messages, None);
        let mistralrs::RequestMessage::Chat { messages: sent, .. } = request.take_messages() else {
            panic!("expected a text chat request");
        };
        assert!(sent[2].contains_key("function"));
        assert!(!sent[2].contains_key("tool_calls"));
    }
}
