#!/usr/bin/env bash
# Fetch the CLAP ONNX models for the `ml` feature (PLAN.md M5).
#
# Model: LAION CLAP (larger_clap_music_and_speech), ONNX export by Xenova.
#   https://huggingface.co/Xenova/larger_clap_music_and_speech
# License: model card lists cc-by-nc-4.0 for LAION CLAP checkpoints —
# see PLAN.md §4 ("CLAP checkpoint license") and verify before shipping.
# The app runs fine without these files (falls back to the rule mapper).
#
# Runtime needs the audio model (moods + track embeddings) and, since the
# M5→M6 asset-selection hook landed, the text model + tokenizer too — the
# text tower runs at runtime on a background thread to embed the asset
# manifest for track-driven placement ranking.
set -euo pipefail
cd "$(dirname "$0")/.."

BASE="https://huggingface.co/Xenova/larger_clap_music_and_speech/resolve/main"
OUT="assets/ml"
mkdir -p "$OUT"

fetch() {
  local path="$1" dest="$2"
  if [ -s "$dest" ]; then
    echo "  have $dest"
  else
    echo "  fetching $path"
    curl -sL --fail -o "$dest" "$BASE/$path"
  fi
}

fetch "onnx/audio_model_quantized.onnx" "$OUT/audio_model_quantized.onnx"
fetch "onnx/text_model_quantized.onnx"  "$OUT/text_model_quantized.onnx"
fetch "tokenizer.json"                  "$OUT/tokenizer.json"

echo "done -> $OUT"
