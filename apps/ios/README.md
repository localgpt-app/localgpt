# LocalGPT iOS App

This directory contains the iOS integration for LocalGPT using Rust, UniFFI, and SwiftUI.

## Project Structure

- `scripts/build_ios.sh`: Build script that compiles the Rust core for iOS (Device & Simulator) and generates UniFFI bindings.
- `LocalGPTWrapper/`: A Swift Package that wraps the Rust binary (as an XCFramework) and provides the generated Swift interface.
- `LocalGPTApp/`: SwiftUI source files for the chat application.

## Getting Started

### 1. Build the Rust Library
Ensure you have the iOS targets installed:
```bash
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
```

Run the build script from the repository root:
```bash
bash apps/ios/scripts/build_ios.sh
```
This will create `LocalGPTWrapper/LocalGPTCore.xcframework`.

### 2. Open in Xcode
1.  Open Xcode and create a new **iOS App** project named `LocalGPT`.
2.  Choose **SwiftUI** for Interface and **Swift** for Language.
3.  Drag the `LocalGPTWrapper` folder into your Xcode project (select "Copy items if needed" and "Create groups").
4.  Alternatively, use **File > Add Packages... > Add Local...** and select the `LocalGPTWrapper` folder.
5.  In your App target's **General** settings, ensure `LocalGPTWrapper` is listed under **Frameworks, Libraries, and Embedded Content**.
6.  Replace the default `ContentView.swift` and `LocalGPTApp.swift` with the files in `LocalGPTApp/`.

## Features
- **Local-first**: Agent logic runs entirely on-device.
- **Async**: UI remains responsive while the Rust core is thinking.
- **UniFFI**: Modern type-safe bindings between Swift and Rust.
- **XCFramework**: Easy distribution and integration into Xcode.
"#;
