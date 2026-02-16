#!/bin/bash
# Build script for LocalGPT egui web UI
#
# This script compiles the server crate to WASM and generates the necessary
# JavaScript bindings for the egui web UI.

set -e

# Check if wasm32-unknown-unknown target is installed
if ! rustup target list --installed | grep -q wasm32-unknown-unknown; then
    echo "Installing wasm32-unknown-unknown target..."
    rustup target add wasm32-unknown-unknown
fi

# Check if wasm-bindgen-cli is installed
if ! command -v wasm-bindgen &> /dev/null; then
    echo "wasm-bindgen-cli not found. Installing..."
    cargo install wasm-bindgen-cli
fi

echo "Building WASM..."
cd "$(dirname "$0")"

# Build the server crate for WASM with egui-web feature
cargo build \
    --package localgpt-server \
    --lib \
    --target wasm32-unknown-unknown \
    --features egui-web \
    --release

echo "Generating JavaScript bindings..."
# Create output directory for WASM artifacts
mkdir -p crates/server/ui/egui

# Run wasm-bindgen to generate JS bindings
wasm-bindgen \
    --out-dir crates/server/ui/egui \
    --target web \
    --no-typescript \
    target/wasm32-unknown-unknown/release/localgpt_server.wasm

echo "WASM build complete!"
echo "Output files:"
ls -lh crates/server/ui/egui/
echo ""
echo "The egui web UI can now be served at /egui endpoint"
