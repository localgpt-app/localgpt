# Egui Web UI Visual Layout

This document describes the visual layout of the egui web UI PoC.

## Layout Structure

```
┌─────────────────────────────────────────────────────────────────┐
│ LocalGPT  │  ● Model: claude-cli/opus  │  [New Session]  Session│
│ (Toolbar)                                            ID: abc12..│
├─────────────────────────────────────────────────────────────────┤
│                                                                   │
│                      Welcome to LocalGPT                          │
│                                                                   │
│              This is a Proof of Concept egui web UI               │
│                                                                   │
│            Type a message below to start chatting                 │
│                                                                   │
│      🚧 Note: This is a static demo without backend connection    │
│                                                                   │
│                                                                   │
│                                                                   │
│                                                                   │
│                     (scrollable chat area)                        │
│                                                                   │
│                                                                   │
├─────────────────────────────────────────────────────────────────┤
│ [Type a message...                                      ] [Send] │
└─────────────────────────────────────────────────────────────────┘
```

## After Sending Messages

```
┌─────────────────────────────────────────────────────────────────┐
│ LocalGPT  │  ● Model: claude-cli/opus  │  [New Session]  Session│
│ (Toolbar)                                            ID: abc12..│
├─────────────────────────────────────────────────────────────────┤
│                                                                   │
│ ┌─────────────────────────────────────────────────────────────┐ │
│ │ user                                                          │ │
│ │ Hello, can you help me understand egui?                      │ │
│ └─────────────────────────────────────────────────────────────┘ │
│                                                                   │
│ ┌─────────────────────────────────────────────────────────────┐ │
│ │ assistant                                                     │ │
│ │ This is a PoC demo. Your message was: "Hello, can you help  │ │
│ │ me understand egui?"                                         │ │
│ │                                                               │ │
│ │ In the full implementation, this would connect to the        │ │
│ │ LocalGPT backend via WebSocket or HTTP API to send your      │ │
│ │ message and stream the response.                             │ │
│ └─────────────────────────────────────────────────────────────┘ │
│                                                                   │
│                     (scrollable chat area)                        │
│                                                                   │
├─────────────────────────────────────────────────────────────────┤
│ [Type a message...                                      ] [Send] │
└─────────────────────────────────────────────────────────────────┘
```

## Color Scheme

The UI uses a dark theme consistent with the desktop version:

- **Background**: Dark gray (#1a1a1a)
- **User messages**: Dark blue-gray background (#28283c)
- **Assistant messages**: Dark green-gray background (#1e3228)
- **User label**: Light blue
- **Assistant label**: Light green
- **Status indicator**: Green dot (●) when connected
- **Toolbar background**: Slightly lighter than main background

## Features Demonstrated

1. **Top Toolbar**:
   - App title ("LocalGPT")
   - Connection status indicator (green dot)
   - Model name display
   - "New Session" button
   - Session ID (truncated)

2. **Chat Area**:
   - Scrollable message history
   - Role-based message styling (user vs assistant)
   - Rounded corners on message bubbles
   - Proper spacing between messages
   - Empty state with welcome message

3. **Input Area**:
   - Multiline text input
   - "Send" button
   - Enter key support (without Shift)
   - Input automatically clears after sending

## Comparison with Desktop UI

The web UI mirrors the desktop implementation with:
- Same panel layout (top toolbar, central chat, bottom input)
- Same color scheme and styling
- Same interaction patterns
- Same egui widgets and components

The main difference is that the desktop version has:
- Additional panels for Sessions and Status (accessible via navigation)
- Direct agent integration (no HTTP roundtrip)
- Native file system access
- Better performance (no WASM overhead)

## Technical Notes

- Rendered entirely on HTML5 Canvas via WebGL
- No DOM manipulation (immediate mode GUI)
- All UI state managed in Rust
- Responsive to window resize
- ~2-3 MB initial download (WASM + JS)
