# LocalGPT Core

The fundamental building blocks of the LocalGPT system. This crate provides the essential logic for agent reasoning, memory management, configuration handling, and security modules.

## Features

- **Agent Framework**: Trait-based tool definitions and task orchestration.
- **Memory System**: Persistent knowledge storage using SQLite with FTS5 and semantic search.
- **Security**: OS-level sandbox integration, prompt policy injection with sanitization, and protected-files deny lists.
- **Configuration**: Robust handling of JSON5/TOML configuration with environment variable overrides.
- **Embeddings**: Support for local ONNX embeddings via `fastembed`, OpenAI API, and GGUF models.
