# egui Mobile Platform Support

**Status:** superseded. The app's interface is a web page, not egui
([the-page.md](../../rfcs/authoring/the-page.md)), and a phone gets the same page
([mobile-remote.md](../../rfcs/authoring/mobile-remote.md)).

This document summarizes egui's mobile platform support status as of February 2026, relevant to LocalGPT's desktop GUI implementation.

## Current Desktop Implementation

LocalGPT's desktop app uses eframe 0.30 with the `glow` (OpenGL) backend:

```toml
eframe = { version = "0.30", default-features = false, features = [
    "default_fonts",
    "glow",
    "persistence",
]}
```

## Platform Support Overview

| Platform | Status | Notes |
|----------|--------|-------|
| Windows | Stable | Full support |
| macOS | Stable | Full support |
| Linux | Stable | Full support |
| Web/WASM | Stable | WebGL/WebGPU backends |
| Android | Supported | Official since Sep 2024, rough edges |
| iOS | Experimental | Keyboard input is problematic |

## Android Support

### What Works
- Officially supported via eframe as of September 2024
- CI job verifies compilation
- Community examples and boilerplates available
- Release APK size ~5 MB (stripped)

### Limitations
- Keyboard integration has known issues
- Development setup is complex (requires Android SDK, NDK)
- Doesn't auto-handle status bar/navigation bar spacing
- Debug builds are large (~151 MB before stripping)

### Required Changes for LocalGPT
1. Switch from `glow` to `wgpu` backend for better compatibility
2. Add Android-specific Cargo configuration
3. Handle soft keyboard lifecycle
4. Adjust UI for touch input and mobile screen sizes

## iOS Support

### What Works
- Basic rendering and touch input
- Apps can run on device
- Community forks with improvements exist

### Blockers
- **Keyboard input**: Primary blocker - `winit` lacks native iOS keyboard support
- Visual artifacts (black bar at bottom) require `Info.plist` configuration
- No first-class official support yet

### Required Changes for LocalGPT
1. Use `wgpu` backend (Metal support required)
2. Implement keyboard workarounds or wait for upstream fixes
3. Add iOS-specific configuration (`Info.plist`)
4. Handle safe areas for notch/home indicator

## Alternative Approaches

### 1. Web/WASM with Mobile WebView
egui compiles to WebAssembly and runs in browsers. Could wrap in:
- **iOS**: WKWebView
- **Android**: WebView or Capacitor

Pros: Single codebase, keyboard works via browser
Cons: Performance overhead, less native feel

### 2. Flutter Frontend
The monorepo already includes Flutter (GlobeGoFlutter). Could create a Flutter UI that calls Rust core via FFI.

Pros: Mature mobile support, native keyboard handling
Cons: Separate UI codebase, FFI complexity

### 3. Native Swift/Kotlin with Rust Core
Build native UIs that communicate with a Rust backend via:
- FFI bindings (uniffi, cbindgen)
- HTTP API (LocalGPT's existing server)

Pros: Best native experience
Cons: Two separate UI codebases to maintain

## Recommendations

1. **Short-term**: Focus on desktop (stable) and web/WASM (good support)
2. **Medium-term**: Monitor egui iOS keyboard support progress
3. **Long-term**: Consider Flutter or native UI if mobile becomes a priority

## References

- [egui Android/iOS Support Discussion](https://github.com/emilk/egui/issues/2066)
- [iOS Native Target Issue](https://github.com/emilk/egui/issues/3117)
- [eframe documentation](https://docs.rs/eframe/latest/eframe/)
- [egui official site](https://www.egui.rs/)
