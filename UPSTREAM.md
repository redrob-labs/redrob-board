# Upstream

This repository is **not a fork**. Every line is written here, from scratch, in Rust.

The projects below are read for their algorithms, file formats and test methodology. None of
their code is copied, vendored or linked. Some are run in CI as oracles: a stage is done only when
our tool's result matches the oracle's (DRC violation count, rendered Gerber, simulated voltages).
The oracles are installed on the CI runner and never ship in a release.

| Area | Reference | Role here |
|---|---|---|
| Board model, integer-nanometre coordinates, DRC rules | KiCad (pcbnew) | design reference, CI oracle (DRC count, Gerber render) |
| Footprint and symbol formats | KiCad `.kicad_mod` / `.kicad_sym` | import format (library data under its own licence, not bundled yet) |
| Circuit-as-code | atopile, tscircuit | design reference |
| Push-and-shove routing | KiCad PNS, Freerouting | design reference |
| Polygon clipping | Clipper2 (algorithm papers) | design reference |
| Gerber RS-274X, Excellon | Ucamco format specification | format specification |
| Circuit simulation | ngspice | CI oracle (node voltages) |
| Board houses | JLCPCB, PCBWay (public file requirements and APIs) | output targets |

Trademarks of the projects above belong to their owners and appear here only to name them.
