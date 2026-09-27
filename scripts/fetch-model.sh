#!/usr/bin/env bash
# Fetch the local LLM that MD, Verse and Gen share (their `llm` features).
#
# Default model: Bonsai-8B Q4_K_M (~5.2 GB, Apache-2.0). Runtime-verified on
# Apple Silicon via the `llm-metal` feature (~25 tok/s plain generation).
#
# Notes from that verification (2026-09):
#   - The 1-bit Q1_0 quant from prism-ml/Bonsai-8B-gguf does NOT parse in
#     mistral.rs 0.8 ("Critical failure loading model part 0") — don't fetch it.
#   - The 5 GB Q4_K_M does not fit the CPU device map alongside the world
#     renderer (~7.6 GB free seen); build with `--features llm-metal` on Macs.
#   - mistral.rs 0.8's grammar-constrained `generate_structured` hangs on GGUF
#     (even a two-field schema) — src/llm.rs deliberately uses plain
#     instructed-JSON generation instead.
#
# The tokenizer comes from prism-ml/Bonsai-8B-unpacked (the GGUF repos ship
# none, and mistral.rs's GgufModelBuilder requires one).
#
# Any standard GGUF (e.g. Qwen2.5-7B-Instruct) + its matching tokenizer.json
# dropped into assets/llm/ is picked up the same way — src/llm.rs loads the
# first .gguf it finds, so no code change is needed.
#
# Shared model directory: the model goes to ~/.local/share/localgpt/models/llm
# ($LOCALGPT_LLM_DIR overrides; $XDG_DATA_HOME moves the base), the directory
# MD, Verse and Gen all read, so one ~5 GB download serves every app. Keep in
# step with localgpt-world-agent's `paths::shared_llm_dir`.
# A copy already in this repo's assets/llm/ (where this script used to put it)
# is moved there instead of being downloaded again.
set -euo pipefail
cd "$(dirname "$0")/.."

# Keep in step with localgpt-world-agent's `paths::shared_llm_dir`.
OUT="${LOCALGPT_LLM_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/localgpt/models/llm}"
LEGACY="crates/verse/assets/llm"
mkdir -p "$OUT"

# Defaults; override with BONSAI_REPO / BONSAI_FILE / BONSAI_TOKENIZER_REPO.
GGUF_REPO="${BONSAI_REPO:-bartowski/prism-ml_Bonsai-8B-unpacked-GGUF}"
GGUF_FILE="${BONSAI_FILE:-prism-ml_Bonsai-8B-unpacked-Q4_K_M.gguf}"
TOK_REPO="${BONSAI_TOKENIZER_REPO:-prism-ml/Bonsai-8B-unpacked}"

fetch() { # <url> <dest>
  local url="$1" dest="$2"
  local legacy="$LEGACY/$(basename "$dest")"
  if [ -s "$dest" ]; then
    echo "  have $dest"
  elif [ -s "$legacy" ]; then
    echo "moving $legacy -> $dest (shared with the other LocalGPT apps)"
    mv "$legacy" "$dest"
  else
    echo "fetching $url (~5.2 GB; resumable — re-run if interrupted)"
    # -C - resumes a partial download; -L follows HF redirects.
    curl -L --fail -C - -o "$dest" "$url"
  fi
}

fetch "https://huggingface.co/${GGUF_REPO}/resolve/main/${GGUF_FILE}" \
  "$OUT/$(basename "$GGUF_FILE")"
fetch "https://huggingface.co/${TOK_REPO}/resolve/main/tokenizer.json" \
  "$OUT/tokenizer.json"

echo "done -> $OUT"
echo "license: Bonsai-8B weights + tokenizer are Apache-2.0 (prism-ml /"
echo "        bartowski's quant); still verify NOTICE.txt before distribution."
echo "run with: cargo run -p localgpt-verse --features llm-metal   # macOS GPU path"
echo "          cargo run -p localgpt-md    --features llm-metal -- samples/hello.md"
