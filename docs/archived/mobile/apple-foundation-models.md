# Apple Foundation Models Integration

**Status:** superseded 2026-10-31. The iOS app this plans for left the
repository with `mobile-ffi` ([the-harness-goes.md](../../rfcs/authoring/the-harness-goes.md));
a phone is now a remote for the desktop app
([mobile-remote.md](../../rfcs/authoring/mobile-remote.md)), and on-device
authoring, if it is wanted, is worldwalk's question.

## Overview

Integrate Apple's on-device Foundation Models framework into LocalGPT for iOS, providing a privacy-preserving, zero-cost LLM option that works offline.

## Requirements

- **OS**: iOS 26.0+ / iPadOS 26.0+ / macOS 26.0+
- **Hardware**: Apple Silicon Macs, iPhone 15 Pro+ (A17 Pro), iPhone 16 series, iPad with M1+
- **User Setting**: Apple Intelligence must be enabled in System Settings
- **Xcode**: Xcode 26+ with iOS 26 SDK

## Architecture

### Approach: Swift-to-Rust FFI Bridge

Since Foundation Models is a Swift-only framework with no C API, we need a bridge layer:

```
┌─────────────────────────────────────────────────────────┐
│                    iOS App (Swift)                       │
│  ┌─────────────────┐      ┌─────────────────────────┐   │
│  │  ChatViewModel   │──────│ AppleFoundationModels   │   │
│  │  (uses LocalGpt  │      │ (Swift wrapper)         │   │
│  │   Client FFI)    │      └──────────┬──────────────┘   │
│  └─────────────────┘                 │                  │
└──────────────────────────────────────│──────────────────┘
                                       │ Swift FFI
                                       ▼
┌─────────────────────────────────────────────────────────┐
│                  Rust Core (localgpt-core)               │
│  ┌─────────────────────────────────────────────────┐    │
│  │           AppleFoundationProvider                │    │
│  │  - Implements LLMProvider trait                  │    │
│  │  - Calls Swift via FFI for chat()                │    │
│  │  - Conditional compilation: #[cfg(target_vendor  │    │
│  │    = "apple")]                                   │    │
│  └─────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────┘
```

### Alternative: Direct Swift Implementation

For simplicity, the iOS app could bypass Rust for the LLM call:

```swift
// ChatViewModel.swift
import FoundationModels

func send(text: String) async {
    // Check availability
    guard SystemLanguageModel.default.isAvailable else {
        // Fallback to Rust API providers (Anthropic, etc.)
        await sendViaRustClient(text)
        return
    }

    let session = LanguageModelSession {
        """
        You are LocalGPT, a helpful AI assistant.
        \(systemPrompt)
        """
    }

    do {
        // Stream response for better UX
        let stream = try await session.streamResponse(to: text)
        var fullResponse = ""
        for try await partial in stream {
            fullResponse = partial
            // Update UI with partial response
            await MainActor.run {
                self.currentResponse = partial
            }
        }
        // Save to message history
    } catch {
        // Fallback to Rust client on error
        await sendViaRustClient(text)
    }
}
```

## Implementation Plan

### Phase 1: Swift-Only Integration (Recommended First Step)

1. **Add Foundation Models to iOS App**
   - Add `NSAppleIntelligenceUsageDescription` to Info.plist
   - Create `AppleFoundationModelsService.swift` wrapper
   - Integrate into `ChatViewModel` with fallback to Rust client

2. **Benefits**
   - No Rust FFI complexity
   - Immediate access to streaming, tool calling
   - Easier to iterate on prompts

### Phase 2: Rust Provider (Optional, for consistency)

1. **Create FFI Bridge**
   - Create Swift static library that exposes C-compatible functions
   - Build with SwiftPM or xcodebuild
   - Link into `localgpt-mobile-ffi` crate

2. **Implement Provider in Rust**
   ```rust
   // crates/core/src/agent/providers.rs

   #[cfg(target_vendor = "apple")]
   pub struct AppleFoundationProvider {
       // FFI bridge handle
   }

   #[cfg(target_vendor = "apple")]
   impl LLMProvider for AppleFoundationProvider {
       fn complete(&self, request: ChatRequest) -> Result<ChatResponse> {
           // Call Swift FFI
           apple_foundation_complete(&request)
       }
   }
   ```

3. **Conditional Compilation**
   - Add `apple-foundation` feature flag
   - Only compile for `aarch64-apple-ios` and `aarch64-apple-ios-sim`

## Swift API Reference

### Basic Usage

```swift
import FoundationModels

// Check availability
guard SystemLanguageModel.default.isAvailable else {
    print("Apple Intelligence not available")
    return
}

// Create session with system instructions
let session = LanguageModelSession {
    "You are LocalGPT, a helpful AI assistant with persistent memory."
}

// Simple response
let response = try await session.respond(to: "Hello!")
print(response.content)

// Streaming response
let stream = try await session.streamResponse(to: "Tell me a story")
for try await partial in stream {
    print(partial) // Incremental text
}

// Structured output
@Generable
struct Task {
    let title: String
    let priority: Int
    let dueDate: String?
}

let task = try await session.respond(
    to: "Create a task for buying groceries",
    generating: Task.self
)
print(task.content.title)

// Tool calling
final class SearchMemoryTool: Tool {
    struct Args: Generable {
        let query: String
    }

    func call(arguments: Args) async throws -> ToolOutput {
        let results = await searchMemory(arguments.query)
        return ToolOutput(results)
    }
}

let session = LanguageModelSession(tools: [SearchMemoryTool()])
```

### Prewarming for Performance

```swift
// Call during app launch for faster first response
session.prewarm()
```

## Info.plist Requirements

```xml
<key>NSAppleIntelligenceUsageDescription</key>
<string>LocalGPT uses Apple Intelligence for on-device AI assistance.</string>
```

## Fallback Strategy

1. **Primary**: Apple Foundation Models (if available)
2. **Fallback**: Rust LocalGptClient with API provider (Anthropic, OpenAI, etc.)
3. **Display**: Show indicator when using cloud vs. on-device

## References

- [Apple Foundation Models Documentation](https://developer.apple.com/documentation/FoundationModels)
- [Apple Intelligence Developer Portal](https://developer.apple.com/apple-intelligence/)
- WWDC 2025 Session: "Get started with Foundation Models"

## Status

- [x] Research completed
- [ ] Phase 1: Swift-only integration
- [ ] Phase 2: Rust FFI provider (optional)
