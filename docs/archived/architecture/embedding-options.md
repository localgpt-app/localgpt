# Embedding Model Options for LocalGPT

This document compares embedding model formats and providers for semantic search in LocalGPT.

## Quick Start

```bash
# Default build (ONNX/FastEmbed, no C++ required)
cargo build --release

# With GGUF support (requires C++ compiler)
cargo build --release --features gguf
```

## Providers

| Provider | Format | Build Requirement | Feature Flag |
|----------|--------|------------------|--------------|
| `local` | ONNX | None | default |
| `gguf` | GGUF | C++ compiler | `--features gguf` |
| `openai` | API | None | default |
| `none` | - | None | default |

## Configuration

```toml
[memory]
# Provider: "local" (FastEmbed/ONNX), "gguf", "openai", or "none"
embedding_provider = "local"

# Model depends on provider (see below)
embedding_model = "all-MiniLM-L6-v2"

# Cache directory for downloaded models
embedding_cache_dir = "~/.cache/localgpt/models"
```

## Available Models

### ONNX Models (FastEmbed) - Default

| Model | Size | Dimensions | Languages |
|-------|------|------------|-----------|
| all-MiniLM-L6-v2 | ~80MB | 384 | English (default) |
| bge-base-en-v1.5 | ~430MB | 768 | English |
| bge-small-zh-v1.5 | ~95MB | 512 | Chinese |
| multilingual-e5-small | ~470MB | 384 | 100+ langs |
| multilingual-e5-base | ~1.1GB | 768 | 100+ langs |
| bge-m3 | ~2.2GB | 1024 | 100+ langs |

### GGUF Models (llama.cpp) - Requires `--features gguf`

| Model | Size | Dimensions | Languages |
|-------|------|------------|-----------|
| embeddinggemma-300M-Q8_0 | ~320MB | 1024 | Multilingual |
| nomic-embed-text-v1.5.Q8_0 | ~270MB | 768 | English |
| mxbai-embed-large-v1-q8_0 | ~670MB | 1024 | English |

**Note:** GGUF models must be downloaded manually and the full path specified:

```toml
[memory]
embedding_provider = "gguf"
embedding_model = "/path/to/embeddinggemma-300M-Q8_0.gguf"
```

Download from:
- [embeddinggemma-300M-GGUF](https://huggingface.co/ggml-org/embeddinggemma-300M-GGUF)
- [nomic-embed-text-v1.5-GGUF](https://huggingface.co/nomic-ai/nomic-embed-text-v1.5-GGUF)

## Build Size Impact

| Build | Binary Size | Notes |
|-------|-------------|-------|
| Default (ONNX) | ~27 MB | No C++ required |
| With GGUF | ~45-50 MB | Includes llama.cpp |

## Comparison with OpenClaw

| Aspect | OpenClaw | LocalGPT |
|--------|----------|----------|
| Runtime | node-llama-cpp | FastEmbed (default) or llama-cpp-2 |
| Default Model | embeddinggemma-300M-GGUF | all-MiniLM-L6-v2 |
| Default Format | GGUF | ONNX |
| GGUF Support | Built-in | Optional (`--features gguf`) |

## Recommendations

### For Most Users
Use the default **FastEmbed/ONNX** provider:
- No C++ compiler needed
- Models auto-download on first use
- Good quality with `all-MiniLM-L6-v2` (English) or `multilingual-e5-base` (multilingual)

### For OpenClaw Compatibility
Build with `--features gguf` and use embeddinggemma-300M:
```bash
cargo build --release --features gguf
```
```toml
[memory]
embedding_provider = "gguf"
embedding_model = "~/.cache/localgpt/models/embeddinggemma-300M-Q8_0.gguf"
```

### For Minimal Build
Use default build without GGUF feature for smallest binary and simplest compilation.
