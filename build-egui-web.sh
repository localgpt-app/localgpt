#!/usr/bin/env bash
# Build script for LocalGPT egui web UI
#
# This script compiles the server crate to WASM and generates the necessary
# JavaScript bindings for the egui web UI.
#
# Prerequisites:
#   - Rust with wasm32-unknown-unknown target
#   - wasm-bindgen-cli (installed automatically if missing)
#
# Output:
#   crates/server/ui/egui/localgpt_server_bg.wasm  (~2-3 MB)
#   crates/server/ui/egui/localgpt_server.js
#   crates/server/ui/egui/localgpt_server.d.ts (if TypeScript enabled)

set -e

# Check if wasm32-unknown-unknown target is installed
if ! rustup target list --installed | grep -q wasm32-unknown-unknown; then
    echo "Installing wasm32-unknown-unknown target..."
    rustup target add wasm32-unknown-unknown
fi

# Check if wasm-bindgen-cli is installed
if ! command -v wasm-bindgen &> /dev/null; then
    echo "wasm-bindgen-cli not found. Installing..."
    echo "Note: This may take a few minutes..."
    cargo install wasm-bindgen-cli
fi

echo "Building WASM..."
cd "$(dirname "$0")"

# Build the server crate for WASM with egui-web feature
# Note: --release is important for acceptable binary size
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
# --target web: Generate ES module that can be imported
# --no-typescript: Skip .d.ts generation (optional)
wasm-bindgen \
    --out-dir crates/server/ui/egui \
    --target web \
    --no-typescript \
    target/wasm32-unknown-unknown/release/localgpt_server.wasm

# Optional: Optimize WASM with wasm-opt (from binaryen)
if command -v wasm-opt &> /dev/null; then
    echo "Optimizing WASM with wasm-opt..."
    wasm-opt \
        -Oz \
        crates/server/ui/egui/localgpt_server_bg.wasm \
        -o crates/server/ui/egui/localgpt_server_bg.wasm
fi

echo ""
echo "✅ WASM build complete!"
echo ""
echo "Output files:"
ls -lh crates/server/ui/egui/
echo ""
echo "📦 The egui web UI can now be served at /egui endpoint"
echo ""
echo "Next steps:"
echo "  1. Build the server: cargo build --package localgpt --bin localgpt --release"
echo "  2. Start the daemon: localgpt daemon start (or target/release/localgpt daemon start)"
echo "  3. Open browser: http://localhost:31327/egui"
echo ""
echo "Note: The egui web UI is a PoC that reuses the desktop egui code."
echo "      It demonstrates code reuse between desktop and web platforms."
echo ""
