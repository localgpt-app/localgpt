# LocalGPT Rust AI Ecosystem Integration Spec

**Adopting Battle-Tested Crates to Close Gaps with OpenClaw**

schemars + genai + eventsource-stream + tiktoken-rs  |  Single Binary  |  Zero Copyleft

| Field | Value |
|-------|-------|
| Version | 1.0 Draft |
| Date | February 13, 2026 |
| Author | Yi / LocalGPT |
| Status | Archived 2026-11-01 — the provider layer, SSE parsing and token counting this would have refactored went with `localgpt-core`; nothing in the tree calls a model any more |
| Priority | P1 — Product Competitiveness |
| Depends On | LocalGPT Shell Sandbox Spec (P0) |

---

## 1. Executive Summary

LocalGPT hand-rolls its LLM provider integration, tool schema definitions, SSE parsing, and token counting. This was the right call at MVP — minimize dependencies, ship fast. But the Rust AI ecosystem has crossed a maturity threshold. Battle-tested crates now exist for each of these concerns, with permissive licenses, high download counts, and active maintenance.

This spec defines four tiers of crate integrations — from drop-in improvements (Tier 1, ~1 week) to strategic evaluations (Tier 4, ongoing). The goal is to reduce hand-maintained code, expand provider coverage from 3 to 14+, and close feature gaps with OpenClaw — all without breaking the 27MB single-binary target.

### 1.1 The OpenClaw Gap

OpenClaw ships with 500+ npm transitive dependencies but gets automatic multi-provider support, structured tool calling, streaming, and token management from that ecosystem. LocalGPT's ~30-crate dependency tree is a strength for deployment — but a weakness for feature velocity. This spec closes the gap by adding ~6 carefully vetted crates.

### 1.2 EdgeQuake Influence

EdgeQuake (Rust Graph-RAG engine by Raphael Mansuy) validates two patterns worth adopting: tuple-delimited LLM output parsing for resilience, and 11-crate workspace modularization for build performance. Its core Graph-RAG functionality is orthogonal to LocalGPT but its engineering patterns are directly applicable.

---

## 2. Crate Inventory and Vetting

Every crate recommended in this spec has been vetted against these criteria:

| Criterion | Requirement |
|-----------|-------------|
| License | MIT or Apache-2.0 (no copyleft) |
| Maintenance | Commit in last 90 days |
| Downloads | >50K total on crates.io |
| Binary impact | <2MB addition to final binary (except tiktoken-rs, see §4.3) |
| Tokio compat | Compatible with tokio 1.x async runtime |
| No native deps | No C/C++ build dependencies (exception: tiktoken-rs uses openssl optionally) |

### 2.1 Approved Crate Summary

| Crate | Version | License | Downloads | Binary Impact | Purpose |
|-------|---------|---------|-----------|---------------|---------|
| `schemars` | 0.8.x | MIT | 155M | ~92KB | JSON Schema from Rust types |
| `eventsource-stream` | 0.2.x | MIT | 2.8M | ~15KB | SSE parser for byte streams |
| `tiktoken-rs` | 0.6.x | MIT | 314K/mo | ~8.5MB ⚠️ | OpenAI-compatible tokenizer |
| `genai` | 0.5.x | MIT/Apache-2.0 | 116K | ~200KB | Multi-provider LLM abstraction |
| `async-stream` | 0.3.x | MIT | Very high | ~10KB | `stream!` macro for async iterators |
| `text-splitter` | 0.16.x | MIT | Moderate | ~50KB | Token-aware text chunking |

### 2.2 Rejected / Deferred Crates

| Crate | Reason |
|-------|--------|
| `llm-chain` | Abandoned (~2 years stale). Do not adopt. |
| `rig-core` | Good patterns but adopting full framework conflicts with LocalGPT's session model. Borrow trait patterns only. |
| `kalosm` | Impressive but LLVM/CUDA dependencies violate zero-native-deps constraint. |
| `mistral.rs` | Only relevant if embedding local inference (major architectural shift). Defer to Tier 4. |
| `ollama-rs` | Typed Ollama client — nice but genai covers Ollama already. Redundant. |

---

## 3. Tier 1 — Drop-In Improvements (Week 1)

**Estimated effort: 3-5 days total. No architectural changes.**

---

### 3.1 ACTION: Add `schemars` for Tool Schema Generation

**Problem:** LocalGPT defines tool/function schemas as hand-written JSON strings or `serde_json::Value` literals. Every time a tool parameter changes, the schema must be manually updated. Mismatches between the Rust struct and the JSON schema cause silent tool-calling failures.

**Solution:** Derive `JsonSchema` on tool parameter structs. Generate schemas at compile time.

**Implementation Steps:**

1. Add to `Cargo.toml`:
   ```toml
   [dependencies]
   schemars = "0.8"
   ```

2. For each tool, define the parameter struct with both `serde` and `schemars` derives:
   ```rust
   use schemars::JsonSchema;
   use serde::{Deserialize, Serialize};

   #[derive(Debug, Serialize, Deserialize, JsonSchema)]
   pub struct BashToolParams {
       /// The shell command to execute
       pub command: String,
       /// Working directory (defaults to workspace root)
       #[serde(default)]
       pub cwd: Option<String>,
       /// Timeout in seconds (defaults to 30)
       #[serde(default = "default_timeout")]
       pub timeout_secs: Option<u64>,
   }

   fn default_timeout() -> Option<u64> { Some(30) }
   ```

3. Generate the schema for LLM function-calling:
   ```rust
   let schema = schemars::schema_for!(BashToolParams);
   let schema_json = serde_json::to_value(&schema).unwrap();
   // Insert into the tool definition sent to the LLM
   ```

4. Refactor all existing tool definitions to use this pattern:
   - `BashToolParams` — shell execution
   - `FileReadParams` — file reading
   - `FileWriteParams` — file writing
   - `FileEditParams` — file editing (search/replace)
   - `MemorySearchParams` — FTS5 + vector search
   - `MemoryAppendParams` — append to memory files
   - `WebFetchParams` — HTTP fetch

5. Add compile-time test that schemas are valid:
   ```rust
   #[cfg(test)]
   mod tests {
       use super::*;
       #[test]
       fn tool_schemas_are_valid_json_schema() {
           // This catches struct/schema drift at CI time
           let schema = schemars::schema_for!(BashToolParams);
           assert!(schema.schema.object.is_some());
       }
   }
   ```

**Acceptance Criteria:**
- [ ] All tool parameter structs derive `JsonSchema`
- [ ] No hand-written JSON schema strings remain in codebase
- [ ] `cargo test` validates all schemas
- [ ] Tool calling works identically with OpenAI, Anthropic, and Ollama

**Files to Modify:**
- `src/agent/tools.rs` (or equivalent tool definition module)
- `Cargo.toml`

**OpenClaw Comparison:** OpenClaw uses Zod schemas (TypeScript) for tool validation. `schemars` provides equivalent compile-time safety with zero runtime cost — a Rust structural advantage.

---

### 3.2 ACTION: Add `eventsource-stream` for SSE Parsing

**Problem:** LocalGPT's streaming responses either use full-response-only mode or hand-parse SSE byte streams from LLM providers. Hand-rolled SSE parsing misses edge cases: multi-line `data:` fields, `retry:` directives, event ID tracking, and reconnection logic.

**Solution:** Pipe reqwest streaming responses through `eventsource-stream` for spec-compliant SSE parsing.

**Implementation Steps:**

1. Add to `Cargo.toml`:
   ```toml
   [dependencies]
   eventsource-stream = "0.2"
   ```

2. Replace hand-rolled SSE parsing in the provider layer:
   ```rust
   use eventsource_stream::Eventsource;
   use futures_util::StreamExt;

   async fn stream_completion(
       client: &reqwest::Client,
       url: &str,
       body: serde_json::Value,
   ) -> impl Stream<Item = Result<String, Error>> {
       let response = client.post(url)
           .json(&body)
           .send()
           .await?;

       let stream = response
           .bytes_stream()
           .eventsource()  // <-- This is the magic line
           .map(|event| {
               match event {
                   Ok(ev) => {
                       if ev.data == "[DONE]" {
                           return Ok(None);
                       }
                       let chunk: ChatChunk = serde_json::from_str(&ev.data)?;
                       Ok(Some(chunk.delta_content()))
                   }
                   Err(e) => Err(e.into()),
               }
           })
           .take_while(|r| futures_util::future::ready(!matches!(r, Ok(None))))
           .filter_map(|r| futures_util::future::ready(r.transpose()));

       stream
   }
   ```

3. Update both the HTTP server's SSE endpoint and the CLI's streaming display to consume this stream.

4. Add integration test with a mock SSE server:
   ```rust
   #[tokio::test]
   async fn test_sse_multiline_data() {
       // Verify multi-line data: fields parse correctly
       // Verify [DONE] terminates the stream
       // Verify malformed events don't crash the parser
   }
   ```

**Acceptance Criteria:**
- [ ] SSE parsing handles multi-line `data:` fields correctly
- [ ] `[DONE]` sentinel terminates stream cleanly
- [ ] Malformed SSE events produce errors, not panics
- [ ] Streaming works for OpenAI, Anthropic, and Ollama SSE formats

**Files to Modify:**
- `src/agent/providers/openai.rs`
- `src/agent/providers/anthropic.rs`
- `src/agent/providers/ollama.rs`
- `src/server/` (SSE relay endpoint)
- `Cargo.toml`

**OpenClaw Comparison:** OpenClaw uses Node.js `EventSource` or `fetch` with manual parsing. The `eventsource-stream` crate is more robust than either approach and handles reconnection semantics per the W3C spec.

---

### 3.3 ACTION: Add Mock LLM Provider for Testing

**Problem:** CI/CD requires real API keys to test agent behavior. Tests are flaky, slow, and expensive. EdgeQuake solved this with a Mock provider.

**Solution:** Add a `MockProvider` that returns deterministic responses from fixtures.

**Implementation Steps:**

1. Create `src/agent/providers/mock.rs`:
   ```rust
   pub struct MockProvider {
       responses: Vec<String>,
       current: AtomicUsize,
   }

   impl MockProvider {
       pub fn from_fixtures(responses: Vec<String>) -> Self {
           Self { responses, current: AtomicUsize::new(0) }
       }

       pub fn with_tool_call(tool_name: &str, args: serde_json::Value) -> Self {
           // Returns a response that triggers a specific tool call
       }
   }

   #[async_trait]
   impl LlmProvider for MockProvider {
       async fn complete(&self, messages: &[Message]) -> Result<Response> {
           let idx = self.current.fetch_add(1, Ordering::SeqCst);
           let text = self.responses.get(idx % self.responses.len())
               .cloned()
               .unwrap_or_default();
           Ok(Response::text(text))
       }

       async fn stream(&self, messages: &[Message]) -> Result<impl Stream<Item = Result<String>>> {
           // Emit response character-by-character with small delays
       }
   }
   ```

2. Add fixture files in `tests/fixtures/`:
   ```
   tests/fixtures/
   ├── simple_response.json
   ├── tool_call_bash.json
   ├── tool_call_file_write.json
   ├── multi_turn_conversation.json
   └── streaming_chunks.jsonl
   ```

3. Wire `mock` as a provider option in config:
   ```toml
   [llm]
   provider = "mock"  # Only in test/dev
   ```

**Acceptance Criteria:**
- [ ] All agent integration tests use `MockProvider` (no real API calls in CI)
- [ ] Mock supports both complete and streaming modes
- [ ] Mock can simulate tool-calling responses
- [ ] Test suite runs in <10 seconds without network access

**Files to Create:**
- `src/agent/providers/mock.rs`
- `tests/fixtures/*.json`

---

## 4. Tier 2 — Strategic Integrations (Weeks 2-3)

**Estimated effort: 5-8 days total. Moderate refactoring required.**

---

### 4.1 ACTION: Adopt `genai` for Multi-Provider Abstraction

**Problem:** LocalGPT manually implements HTTP request construction, response parsing, and error handling for each LLM provider (OpenAI, Anthropic, Ollama). Adding a new provider (Gemini, Groq, DeepSeek) requires writing and maintaining ~200-400 lines of provider-specific code.

**Solution:** Replace the per-provider HTTP layer with `genai`, which normalizes 14+ providers behind a single `Client` interface.

**Implementation Steps:**

1. Add to `Cargo.toml`:
   ```toml
   [dependencies]
   genai = "0.5"
   ```

2. Create adapter layer `src/agent/providers/genai_adapter.rs`:
   ```rust
   use genai::client::Client;
   use genai::chat::{ChatMessage, ChatRequest, ChatResponse};

   pub struct GenaiAdapter {
       client: Client,
       model: String,
   }

   impl GenaiAdapter {
       pub fn new(model: &str) -> Result<Self> {
           // genai auto-detects API keys from environment:
           // OPENAI_API_KEY, ANTHROPIC_API_KEY, GEMINI_API_KEY, etc.
           let client = Client::default();
           Ok(Self { client, model: model.to_string() })
       }
   }

   #[async_trait]
   impl LlmProvider for GenaiAdapter {
       async fn complete(&self, messages: &[Message]) -> Result<Response> {
           let genai_messages: Vec<ChatMessage> = messages
               .iter()
               .map(|m| convert_message(m))
               .collect();

           let request = ChatRequest::default()
               .with_messages(genai_messages);

           let response = self.client
               .exec_chat(&self.model, request, None)
               .await?;

           Ok(convert_response(response))
       }

       async fn stream(&self, messages: &[Message]) -> Result<impl Stream<Item = Result<String>>> {
           let request = ChatRequest::default()
               .with_messages(/* ... */);

           let stream = self.client
               .exec_chat_stream(&self.model, request, None)
               .await?;

           // Convert genai stream events to LocalGPT's stream format
           Ok(stream.map(|event| convert_stream_event(event)))
       }
   }
   ```

3. Maintain existing provider implementations as fallbacks:
   ```rust
   pub fn create_provider(config: &LlmConfig) -> Result<Box<dyn LlmProvider>> {
       match config.provider.as_str() {
           // genai-backed providers (new)
           "openai" | "anthropic" | "gemini" | "groq"
           | "deepseek" | "cohere" | "xai" => {
               Ok(Box::new(GenaiAdapter::new(&config.model)?))
           }
           // Direct providers (kept for special cases)
           "ollama" => Ok(Box::new(OllamaProvider::new(config)?)),
           "claude-cli" => Ok(Box::new(ClaudeCliProvider::new(config)?)),
           "mock" => Ok(Box::new(MockProvider::default())),
           _ => Err(anyhow!("Unknown provider: {}", config.provider)),
       }
   }
   ```

4. Update config.toml documentation with new provider options:
   ```toml
   [llm]
   # Supported providers (via genai):
   #   openai, anthropic, gemini, groq, deepseek, cohere, xai,
   #   ollama, together, jina, sambanova
   # Direct providers:
   #   claude-cli, mock
   provider = "anthropic"
   model = "claude-sonnet-4-20250514"
   ```

5. **Migration path:** Swap one provider at a time, starting with OpenAI (most standardized API). Keep direct Anthropic provider until genai's tool_use support matches LocalGPT's needs. Keep direct Ollama for local-first users who may not want genai's overhead.

**Acceptance Criteria:**
- [ ] `localgpt config set llm.provider gemini` works out of the box
- [ ] All 14 genai-supported providers are available
- [ ] Existing OpenAI/Anthropic/Ollama configs continue working
- [ ] Streaming works through genai for all providers
- [ ] Tool calling works through genai (or falls back to direct provider)
- [ ] Binary size increase < 300KB

**Files to Create/Modify:**
- `src/agent/providers/genai_adapter.rs` (new)
- `src/agent/providers/mod.rs` (add genai_adapter)
- `src/config.rs` (new provider names)

**OpenClaw Comparison:** OpenClaw supports ~10 providers via separate npm packages (one per provider). genai provides 14+ in a single crate with zero per-provider dependencies — cleaner and more maintainable.

---

### 4.2 ACTION: Implement Tuple-Delimited Parsing Fallback (EdgeQuake Pattern)

**Problem:** When LLMs generate tool-calling JSON, parse failures (missing brackets, truncated output, extra text before/after JSON) cause the entire response to be lost. The agent retries from scratch, wasting tokens and latency.

**Solution:** Implement EdgeQuake's tuple-delimited format as a fallback parser. When JSON parsing fails, re-prompt with a simpler format that allows partial recovery.

**Implementation Steps:**

1. Create `src/agent/parsing.rs`:
   ```rust
   /// Delimiter used between fields in tuple format
   const TUPLE_DELIM: &str = "<|#|>";
   /// Marks the end of a complete record
   const TUPLE_END: &str = "<|COMPLETE|>";

   #[derive(Debug)]
   pub struct TupleRecord {
       pub fields: Vec<String>,
   }

   /// Parse tuple-delimited LLM output.
   /// Returns all successfully parsed records, skipping malformed lines.
   pub fn parse_tuple_output(raw: &str) -> (Vec<TupleRecord>, usize) {
       let mut records = Vec::new();
       let mut failures = 0;

       for line in raw.lines() {
           let line = line.trim();
           if line.is_empty() || !line.contains(TUPLE_DELIM) {
               continue;
           }

           // Strip COMPLETE marker if present
           let line = line.trim_end_matches(TUPLE_END).trim();

           let fields: Vec<String> = line
               .split(TUPLE_DELIM)
               .map(|f| f.trim().to_string())
               .collect();

           if fields.len() >= 2 {
               records.push(TupleRecord { fields });
           } else {
               failures += 1;
           }
       }

       (records, failures)
   }
   ```

2. Add fallback logic to tool-call parsing:
   ```rust
   pub async fn parse_tool_call(raw: &str) -> Result<ToolCall> {
       // Attempt 1: Standard JSON parsing
       if let Ok(call) = serde_json::from_str::<ToolCall>(raw) {
           return Ok(call);
       }

       // Attempt 2: Extract JSON from markdown code blocks
       if let Some(json_str) = extract_json_from_markdown(raw) {
           if let Ok(call) = serde_json::from_str::<ToolCall>(&json_str) {
               return Ok(call);
           }
       }

       // Attempt 3: Tuple-delimited fallback
       // (only for simple single-field tools like bash)
       let (records, _) = parse_tuple_output(raw);
       if let Some(record) = records.first() {
           return ToolCall::from_tuple_record(record);
       }

       Err(anyhow!("Failed to parse tool call from LLM output"))
   }
   ```

3. Add metrics for parse-path tracking:
   ```rust
   pub struct ParseMetrics {
       pub json_direct: AtomicU64,
       pub json_from_markdown: AtomicU64,
       pub tuple_fallback: AtomicU64,
       pub total_failures: AtomicU64,
   }
   ```

**Acceptance Criteria:**
- [ ] JSON parsing remains the primary path (no behavior change for well-formed output)
- [ ] Tuple fallback recovers at least partial data from malformed JSON responses
- [ ] Parse metrics are logged for monitoring (which path was used, failure rate)
- [ ] Unit tests cover: valid JSON, JSON in markdown, tuple format, garbage input

**Files to Create/Modify:**
- `src/agent/parsing.rs` (new)
- `src/agent/mod.rs` (use new parsing module)

---

### 4.3 ACTION: Add `tiktoken-rs` for Context Window Management

**Problem:** LocalGPT estimates token counts heuristically (~4 chars/token). This leads to context window overflows (truncated responses) or underutilization (wasted capacity). Context compaction triggers at the wrong time.

**Solution:** Use `tiktoken-rs` for accurate token counting per model.

**⚠️ Binary Size Warning:** `tiktoken-rs` embeds tokenizer data files, adding ~8.5MB to the binary. This would push LocalGPT from ~27MB to ~35MB. Two mitigation options exist:

**Option A — Accept the size increase (recommended):**
The 8.5MB buys accurate token counting for all OpenAI-compatible models. 35MB is still dramatically smaller than OpenClaw's ~100MB Node.js installation.

**Option B — Load tokenizer data from disk:**
Use `tokenizers` crate (HuggingFace) instead, which loads .json tokenizer files from `~/.localgpt/tokenizers/`. Smaller binary but requires first-run download.

**Implementation Steps (Option A):**

1. Add to `Cargo.toml`:
   ```toml
   [dependencies]
   tiktoken-rs = "0.6"
   ```

2. Create `src/agent/tokens.rs`:
   ```rust
   use tiktoken_rs::{get_bpe_from_model, CoreBPE};

   pub struct TokenCounter {
       bpe: CoreBPE,
       model_max_tokens: usize,
   }

   impl TokenCounter {
       pub fn for_model(model: &str) -> Result<Self> {
           let bpe = get_bpe_from_model(model)
               .unwrap_or_else(|_| {
                   // Fall back to cl100k_base for unknown models
                   tiktoken_rs::cl100k_base().unwrap()
               });

           let max_tokens = Self::lookup_max_tokens(model);
           Ok(Self { bpe, model_max_tokens: max_tokens })
       }

       pub fn count(&self, text: &str) -> usize {
           self.bpe.encode_ordinary(text).len()
       }

       pub fn count_messages(&self, messages: &[Message]) -> usize {
           messages.iter()
               .map(|m| {
                   // Per-message overhead: ~4 tokens for role/formatting
                   4 + self.count(&m.content)
               })
               .sum::<usize>() + 3 // Every reply is primed with 3 tokens
       }

       pub fn remaining_capacity(&self, messages: &[Message]) -> usize {
           let used = self.count_messages(messages);
           self.model_max_tokens.saturating_sub(used)
       }

       pub fn should_compact(&self, messages: &[Message]) -> bool {
           let remaining = self.remaining_capacity(messages);
           let threshold = self.model_max_tokens / 4; // Compact at 75% usage
           remaining < threshold
       }

       fn lookup_max_tokens(model: &str) -> usize {
           match model {
               m if m.contains("gpt-4o") => 128_000,
               m if m.contains("gpt-4-turbo") => 128_000,
               m if m.contains("gpt-4") => 8_192,
               m if m.contains("claude-3") => 200_000,
               m if m.contains("claude-sonnet-4") => 200_000,
               m if m.contains("claude-opus-4") => 200_000,
               _ => 8_192, // Conservative default
           }
       }
   }
   ```

3. Integrate into session management:
   ```rust
   // Before each LLM call:
   let counter = TokenCounter::for_model(&config.llm.model)?;
   if counter.should_compact(&session.messages) {
       session.compact(&counter).await?;
   }

   // In context compiler:
   let budget = counter.remaining_capacity(&base_messages);
   let memory_results = memory_search(query, budget / 2)?; // Use half for memory
   ```

**Acceptance Criteria:**
- [ ] Token counts are accurate within 5% for OpenAI and Anthropic models
- [ ] Context compaction triggers based on actual token usage, not message count
- [ ] Memory search results are truncated to fit remaining context budget
- [ ] Binary size increase is documented and accepted

**Files to Create/Modify:**
- `src/agent/tokens.rs` (new)
- `src/agent/session.rs` (use TokenCounter for compaction decisions)
- `src/agent/context.rs` (use TokenCounter for context budget allocation)
- `Cargo.toml`

**OpenClaw Comparison:** OpenClaw uses `tiktoken` (Python/JS bindings) for the same purpose. LocalGPT gets identical accuracy with a pure Rust implementation — no FFI, no Node native modules.

---

## 5. Tier 3 — Feature Parity Improvements (Weeks 3-5)

**Estimated effort: 5-10 days total. Product-level changes.**

---

### 5.1 ACTION: Implement Model Tiering (OpenClaw Pattern)

**Problem:** LocalGPT uses the same model for all operations — user chat, heartbeat tasks, memory summarization, and context compaction. This is wasteful: heartbeat cron checks don't need Claude Opus; compaction summaries don't need GPT-4o.

**Solution:** Define model tiers and assign operations to appropriate tiers.

**Implementation Steps:**

1. Add tiering config to `config.toml`:
   ```toml
   [llm.tiers]
   # Tier 1: User-facing chat — best available model
   primary = { provider = "anthropic", model = "claude-sonnet-4-20250514" }
   # Tier 2: Background tasks — fast and cheap
   background = { provider = "ollama", model = "llama3.2:3b" }
   # Tier 3: Internal operations (compaction, summarization) — cheapest
   internal = { provider = "ollama", model = "llama3.2:1b" }
   ```

2. Create `src/agent/tiering.rs`:
   ```rust
   pub enum ModelTier {
       Primary,    // User-facing chat
       Background, // Heartbeat, autonomous tasks
       Internal,   // Compaction, summarization, embedding queries
   }

   pub struct TieredProviderPool {
       providers: HashMap<ModelTier, Box<dyn LlmProvider>>,
   }

   impl TieredProviderPool {
       pub fn get(&self, tier: ModelTier) -> &dyn LlmProvider {
           self.providers.get(&tier)
               .or_else(|| self.providers.get(&ModelTier::Primary))
               .expect("Primary provider must always be configured")
       }
   }
   ```

3. Tag each agent operation with its tier:
   - `ModelTier::Primary` — interactive chat, skill execution
   - `ModelTier::Background` — heartbeat task evaluation, scheduled prompts
   - `ModelTier::Internal` — context compaction summaries, memory consolidation, search query reformulation

**Acceptance Criteria:**
- [ ] Heartbeat tasks use `background` tier model
- [ ] Compaction uses `internal` tier model
- [ ] User chat uses `primary` tier model
- [ ] Graceful fallback: if `background` or `internal` not configured, use `primary`
- [ ] Cost reduction measurable: >50% fewer tokens on expensive models for typical daily usage

**OpenClaw Comparison:** OpenClaw implements this via per-agent model configuration. LocalGPT's approach is more systematic with explicit tier definitions rather than per-feature overrides.

---

### 5.2 ACTION: Add `text-splitter` for RAG Document Ingestion

**Problem:** When ingesting large documents into memory, LocalGPT splits on arbitrary byte boundaries. This produces chunks that break mid-sentence, mid-word, or mid-code-block — degrading retrieval quality.

**Solution:** Use `text-splitter` for semantic, token-aware chunking.

**Implementation Steps:**

1. Add to `Cargo.toml`:
   ```toml
   [dependencies]
   text-splitter = { version = "0.16", features = ["tiktoken-rs", "markdown"] }
   ```

2. Create `src/memory/chunker.rs`:
   ```rust
   use text_splitter::{MarkdownSplitter, TextSplitter};

   pub fn chunk_document(
       content: &str,
       max_tokens: usize,
       is_markdown: bool,
   ) -> Vec<String> {
       if is_markdown {
           let splitter = MarkdownSplitter::new(max_tokens);
           splitter.chunks(content)
               .map(|c| c.to_string())
               .collect()
       } else {
           let splitter = TextSplitter::new(max_tokens);
           splitter.chunks(content)
               .map(|c| c.to_string())
               .collect()
       }
   }
   ```

3. Integrate into memory indexing pipeline:
   ```rust
   // When indexing a large file for vector search:
   let chunks = chunk_document(&content, 512, path.ends_with(".md"));
   for (i, chunk) in chunks.iter().enumerate() {
       let embedding = embed(&chunk)?;
       db.insert_chunk(path, i, chunk, &embedding)?;
   }
   ```

**Acceptance Criteria:**
- [ ] Chunks respect sentence boundaries
- [ ] Markdown headers are preserved as chunk boundaries
- [ ] Code blocks are not split mid-block
- [ ] Chunk sizes are within 10% of target token count

---

### 5.3 ACTION: Adopt `async-stream` for Cleaner Stream Implementations

**Problem:** Manual `Stream` implementations in the WebSocket/SSE server layer require implementing `Pin`, `Poll`, and `Context` by hand — error-prone and hard to read.

**Solution:** Use `async-stream`'s `stream!` macro for generator-style stream creation.

**Implementation Steps:**

1. Add to `Cargo.toml`:
   ```toml
   [dependencies]
   async-stream = "0.3"
   ```

2. Refactor streaming endpoints:
   ```rust
   use async_stream::stream;

   fn agent_response_stream(
       agent: &Agent,
       messages: Vec<Message>,
   ) -> impl Stream<Item = Result<ServerSentEvent>> {
       stream! {
           let mut llm_stream = agent.stream(&messages).await?;

           while let Some(chunk) = llm_stream.next().await {
               match chunk {
                   Ok(text) => yield Ok(ServerSentEvent::default().data(text)),
                   Err(e) => {
                       yield Ok(ServerSentEvent::default()
                           .event("error")
                           .data(e.to_string()));
                       break;
                   }
               }
           }

           yield Ok(ServerSentEvent::default().event("done").data(""));
       }
   }
   ```

**Acceptance Criteria:**
- [ ] All manual `Stream` implementations replaced with `stream!` macro
- [ ] WebSocket and SSE endpoints use consistent stream patterns
- [ ] Error handling is explicit in stream bodies

---

## 6. Tier 4 — Strategic Evaluations (Ongoing)

These items require architectural decisions. Evaluate, prototype, but don't commit without further spec work.

---

### 6.1 EVALUATE: Workspace Modularization (EdgeQuake Pattern)

EdgeQuake's 11-crate workspace separates `edgequake-core`, `edgequake-llm`, `edgequake-storage`, etc. This enables parallel compilation and clear API boundaries.

**Evaluation criteria:**
- Does LocalGPT's compile time justify the refactoring cost?
- Can `localgpt-memory`, `localgpt-sandbox`, `localgpt-llm` be extracted as internal crates?
- Would this enable community contributions to individual subsystems?

**Action:** Measure current incremental compile times. If >30s for typical changes, prototype a workspace split. If <15s, defer.

---

### 6.2 EVALUATE: Embedded Local Inference via `mistral.rs`

`mistral.rs` could replace Ollama as LocalGPT's local inference backend — eliminating the need for a separate Ollama installation. However, this would increase binary size by 50-100MB+ and add CUDA/Metal build complexity.

**Evaluation criteria:**
- Is "zero external dependencies including Ollama" a compelling value proposition?
- Can it be a compile-time feature flag (`--features local-inference`)?
- What's the performance delta vs. Ollama for equivalent models?

**Action:** Prototype with `mistral.rs` as an optional feature. Benchmark against Ollama for Llama 3.2 3B on CPU.

---

### 6.3 EVALUATE: `rig-core` Tool-Calling Trait Pattern

Rig's `Tool` trait with associated types (`Args: JsonSchema`, `Output: Serialize`) and `definition()` → `ToolDefinition` is the cleanest Rust tool-calling abstraction. Even without adopting rig-core as a dependency, LocalGPT should consider borrowing this trait design.

```rust
// Rig pattern (for reference, not direct adoption):
#[async_trait]
pub trait Tool: Send + Sync {
    type Args: for<'a> Deserialize<'a> + JsonSchema + Send;
    type Output: Serialize;
    type Error: std::error::Error + Send + Sync;

    fn definition(&self, _prompt: String) -> ToolDefinition;
    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error>;
}
```

**Action:** After implementing §3.1 (`schemars`), evaluate whether refactoring LocalGPT's tool trait to match rig's pattern improves ergonomics enough to justify the migration.

---

### 6.4 EVALUATE: Graph-RAG for Memory (EdgeQuake Integration)

EdgeQuake could potentially serve as a library for enhancing LocalGPT's memory system with knowledge-graph-based retrieval. Instead of flat FTS5 + vector search, entity-relationship graphs could enable multi-hop reasoning over accumulated knowledge.

**Evaluation criteria:**
- Can EdgeQuake's core be used as a library (vs. standalone server)?
- Does Graph-RAG meaningfully improve retrieval quality for personal assistant use cases?
- What's the storage and indexing overhead?

**Action:** Defer until LocalGPT's basic memory system is stable. Revisit when the user base reports retrieval quality issues.

---

## 7. Implementation Timeline

| Week | Actions | Dependencies |
|------|---------|-------------|
| 1 | §3.1 schemars, §3.2 eventsource-stream, §3.3 Mock provider | None |
| 2 | §4.1 genai adapter, §4.2 tuple parsing fallback | §3.1 (schemars for tool schemas) |
| 3 | §4.3 tiktoken-rs token counting | §4.1 (provider layer stable) |
| 4 | §5.1 model tiering, §5.2 text-splitter | §4.1 (multi-provider), §4.3 (token counting) |
| 5 | §5.3 async-stream refactor, testing/polish | All Tier 1-3 complete |
| Ongoing | §6.x evaluations | As capacity permits |

---

## 8. Success Criteria

1. **Provider coverage:** 14+ LLM providers available without per-provider maintenance burden
2. **Test reliability:** CI runs without API keys (MockProvider), <30s test suite
3. **Parse resilience:** <2% total tool-call parse failures (down from estimated 5-15%)
4. **Context accuracy:** Token counting within 5% of actual for OpenAI/Anthropic models
5. **Binary size:** <40MB final binary (accepts tiktoken-rs overhead)
6. **Zero copyleft:** All new dependencies MIT or Apache-2.0

---

## 9. Appendix: Crate Provenance

| Crate | Repository | Last Commit | Key Maintainer | Notes |
|-------|-----------|-------------|----------------|-------|
| `schemars` | github.com/GREsau/schemars | Active | Graham Esau | Used by AWS SDK |
| `eventsource-stream` | github.com/jpopesculian/eventsource-stream | Active | Julian | Core SSE crate |
| `tiktoken-rs` | github.com/zurawiki/tiktoken-rs | Active | Zurawiki | Pure Rust port |
| `genai` | github.com/jeremychone/rust-genai | Very active | Jeremy Chone | 14+ providers |
| `async-stream` | github.com/tokio-rs/async-stream | Active | Tokio team | Official tokio ecosystem |
| `text-splitter` | github.com/benbrandt/text-splitter | Active | Ben Brandt | tiktoken integration |
