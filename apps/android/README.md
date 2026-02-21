# LocalGPT Android App

This directory contains the Android integration for LocalGPT using Rust, UniFFI, and Jetpack Compose.

## Project Structure

- `scripts/build_android.sh`: Build script that compiles the Rust core for Android (ARM64 & x86_64) and generates UniFFI bindings.
- `LocalGPTLib/`: Android library module structure containing the compiled `.so` files and generated Kotlin interface.
- `LocalGPTApp/`: Jetpack Compose source files for the chat application.

## Prerequisites

1.  **Android NDK**: Ensure the NDK is installed.
2.  **cargo-ndk**: Install via `cargo install cargo-ndk`.
3.  **Rust Targets**:
    ```bash
    rustup target add aarch64-linux-android x86_64-linux-android
    ```

## Getting Started

### 1. Build the Rust Library
Run the build script from the repository root:
```bash
bash apps/android/scripts/build_android.sh
```
This will populate `LocalGPTLib/src/main/jniLibs` and `LocalGPTLib/src/main/java`.

### 2. Open in Android Studio
1.  Open Android Studio and create a new **Empty Compose Activity** project.
2.  Import `LocalGPTLib` as a module or copy the files into your project.
3.  Ensure you have the following dependencies in your `build.gradle`:
    *   `androidx.lifecycle:lifecycle-viewmodel-compose`
    *   `androidx.activity:activity-compose`
    *   `net.java.dev.jna:jna:5.13.0@aar` (UniFFI requires JNA on Android)
4.  Replace the default `MainActivity.kt` and add `ChatViewModel.kt`.

## Features
- **Native Performance**: Rust core compiled to machine code for ARM64 and x86_64.
- **Modern UI**: Built with Jetpack Compose.
- **Privacy**: LLM logic runs locally on your device.
- **UniFFI**: High-level Kotlin bindings for the Rust core.
