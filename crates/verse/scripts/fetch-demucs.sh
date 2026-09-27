#!/usr/bin/env bash
# Demucs stem-separation model for the `ml` feature (PLAN.md M7, src/demucs.rs).
#
# NOTE: NO MANUAL FETCH IS NEEDED. `stem-splitter-core` (MIT OR Apache-2.0)
# downloads + caches the htdemucs ONNX itself on first use, via its internal
# `ensure_model`. This script exists only to (a) pre-warm the cache so the first
# track doesn't pay the download cost, and (b) document the model + license.
#
# Model: HT-Demucs (htdemucs), the standard 4-stem v4 model
#   (drums / bass / vocals / other). MIT-licensed — commercial-friendly, unlike
#   CLAP's CC-BY-NC. The `ml` feature's CLAP tier stays opt-in/non-commercial;
#   this Demucs tier is the commercial-clean stem path.
#
# This is informational only — running it is optional. To pre-warm the cache,
# build with `cargo build --features ml` and run the app once on any track; the
# first separation triggers the download (see stem-splitter-core's cache dir,
# typically under the user data dir). Subsequent runs reuse the cached model.
set -euo pipefail

echo "fetch-demucs.sh — informational only."
echo ""
echo "stem-splitter-core fetches the htdemucs model automatically on first use."
echo "No manual download is required. The model is MIT-licensed (shippable)."
echo ""
echo "To pre-warm: run the app once with --features ml and play any track."
