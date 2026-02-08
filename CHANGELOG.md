# Changelog

## [Unreleased]

### Fixed

#### OpenAI-compatible provider: tool calls silently dropped during streaming

**Problem**

When using the OpenAI provider with a local llama-server backend (or any
OpenAI-compatible endpoint), tool calls are never executed. The model
reports its available tools correctly, but when asked to use one it emits
raw XML-like text instead of producing structured tool calls:

```
LocalGPT: <tool_call>
<bash>
pwd
</tool_call>
</tool_call>
```

The tools are never executed. The session transcript confirms the
response arrives as plain text content, not as a `tool_calls` array:

```json
{
  "content": [
    {
      "text": "<tool_call>\n<bash>\npwd\n</tool_call>\n</tool_call>",
      "type": "text"
    }
  ],
  "role": "assistant"
}
```

However, hitting llama-server directly via curl with the same tool
schemas returns a correctly structured response with
`"finish_reason": "tool_calls"` and a valid `tool_calls` array.

**Diagnosis**

The `OpenAIProvider` implements `chat()` and `summarize()` but does not
implement `chat_stream()`. The interactive chat CLI uses the streaming
path (`agent.chat_stream_with_images()`), which falls through to the
default `chat_stream` implementation on the `LLMProvider` trait.

Two bugs in the default fallback (`src/agent/providers.rs`, lines
152-173):

1. **Tools are dropped.** The fallback calls `self.chat(messages, None)`
   — passing `None` for the tools parameter instead of forwarding the
   tools it received. The model never sees tool schemas in the API
   request, so it cannot produce structured `tool_calls` responses. It
   falls back to emitting its training-time tool format as raw text.

2. **ToolCalls response is treated as an error.** If `chat()` were to
   return `LLMResponseContent::ToolCalls`, the fallback returns
   `Err("Tool calls not supported in streaming")` instead of converting
   the tool calls into a `StreamChunk` with the `tool_calls` field
   populated.

The combination means: the model never receives tool schemas (bug 1),
and even if it did, the response would be discarded (bug 2).

**Impact**

Any provider that relies on the default `chat_stream` fallback — which
currently includes `OpenAIProvider` — cannot execute tools when used via
the interactive chat CLI. This affects all OpenAI-compatible local
backends (llama-server, vLLM, LM Studio, etc.) and the OpenAI API
itself.

The Anthropic and Ollama providers are unaffected because they implement
their own `chat_stream`. (Ollama separately drops tools intentionally.)

**Fix**

1. Forward the `tools` parameter in the default `chat_stream` fallback.
2. Convert `ToolCalls` responses into a `StreamChunk` with `tool_calls`
   populated instead of returning an error.
