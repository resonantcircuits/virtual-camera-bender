#!/bin/zsh
# Build the A520 validation sheet: our renders of the reference sky through the
# clean, HPF, DIV and INV presets on the top row, the reference quadrants below.
#
# Needs validation/a520-sky-input.png and validation/a520-ref-{hpf,div,inv}.png,
# which are cropped from the local reference composite (see docs/PLAN.md, stage 4).
# Usage: scripts/validate-a520.sh [output.png] [extra vcb sheet flags...]
set -eu
cd "$(dirname "$0")/.."
out=${1:-validation/a520-comparison.png}
shift $(( $# > 0 ? 1 : 0 ))
cargo build --release -q
./target/release/vcb sheet "$out" \
  --input validation/a520-sky-input.png \
  --preset presets/a520-clean.toml \
  --preset presets/a520-hpf.toml \
  --preset presets/a520-div.toml \
  --preset presets/a520-inv.toml \
  --image validation/a520-sky-input.png \
  --image validation/a520-ref-hpf.png \
  --image validation/a520-ref-div.png \
  --image validation/a520-ref-inv.png \
  --cols 4 "$@"
