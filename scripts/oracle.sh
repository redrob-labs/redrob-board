#!/usr/bin/env bash
# Stage gates against the oracle: export every fixture to KiCad, run its DRC and compare counts
# (stage 1), then plot its Gerbers and drill and require pixel/hole identity with ours (stage 4).
# usage: scripts/oracle.sh [outdir]   (needs cargo and kicad-cli on PATH)
set -euo pipefail
cd "$(dirname "$0")/.."
OUT="${1:-target/oracle}"
rm -rf "$OUT"
cargo build -q -p board-oracle -p board-kicad -p board-gerber
BIN=target/debug
"$BIN/board-fixtures" "$OUT"
for board in "$OUT"/*.board.json; do
  name="$(basename "$board" .board.json)"
  "$BIN/board-kicad-export" "$board" "$OUT/$name.netlist.json" "$OUT/$name" "$OUT/rules.json"
  (cd "$OUT/$name" && kicad-cli pcb drc --format json --severity-all --units mm -o drc.json board.kicad_pcb >/dev/null)
  mkdir -p "$OUT/$name/fab"
  kicad-cli pcb export gerbers --layers F.Cu,In1.Cu,In2.Cu,B.Cu,Edge.Cuts,F.Mask,B.Mask,F.Paste,B.Paste,F.Silkscreen,B.Silkscreen --no-x2 --no-netlist -o "$OUT/$name/fab/" "$OUT/$name/fills/board.kicad_pcb" >/dev/null
  kicad-cli pcb export drill --format excellon --excellon-units mm --excellon-zeros-format decimal -o "$OUT/$name/fab/" "$OUT/$name/board.kicad_pcb" >/dev/null
done
python3 scripts/oracle_compare.py "$OUT"
status=0
for fab in "$OUT"/*.fab; do
  name="$(basename "$fab" .fab)"
  for f in "$fab"/*; do
    "$BIN/board-gerber-compare" "$f" "$OUT/$name/fab/$(basename "$f")" || status=1
  done
done
exit $status
