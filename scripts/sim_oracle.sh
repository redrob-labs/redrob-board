#!/usr/bin/env bash
# Simulation gate against the oracle: write every fixture's SPICE deck and our numbers, run ngspice
# on the deck, and require every node voltage (and AC magnitude) to agree within 1%.
# usage: scripts/sim_oracle.sh [outdir]   (needs cargo and ngspice on PATH)
set -euo pipefail
cd "$(dirname "$0")/.."
OUT="${1:-target/sim-oracle}"
rm -rf "$OUT"
cargo run -q -p board-sim --bin board-sim-fixtures -- "$OUT"
for deck in "$OUT"/*.cir; do
  name="$(basename "$deck" .cir)"
  # -b: batch; the deck's .control block prints the nodes and quits.
  ngspice -b "$deck" >"$OUT/$name.ngspice.log" 2>&1 || true
done
python3 scripts/sim_oracle.py "$OUT"
