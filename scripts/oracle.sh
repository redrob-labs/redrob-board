#!/usr/bin/env bash
# Run the stage-1 oracle: export every fixture to KiCad, run kicad-cli DRC, compare counts.
# usage: scripts/oracle.sh [outdir]   (needs cargo and kicad-cli on PATH)
set -euo pipefail
cd "$(dirname "$0")/.."
OUT="${1:-${KIROCREW_SCRATCH:-target}/oracle}"
rm -rf "$OUT"
cargo run -q -p board-oracle --bin board-fixtures -- "$OUT"
for board in "$OUT"/*.board.json; do
  name="$(basename "$board" .board.json)"
  cargo run -q -p board-kicad --bin board-kicad-export -- "$board" "$OUT/$name.netlist.json" "$OUT/$name" "$OUT/rules.json"
  (cd "$OUT/$name" && kicad-cli pcb drc --format json --severity-all --units mm -o drc.json board.kicad_pcb >/dev/null)
done
python3 scripts/oracle_compare.py "$OUT"
