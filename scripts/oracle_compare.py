#!/usr/bin/env python3
"""Compare Redrob Board DRC counts with KiCad's for every fixture.

usage: oracle_compare.py <fixtures dir>

Expects, per fixture NAME: NAME.drc.json (ours) and NAME/drc.json (kicad-cli pcb drc --format json).
Exit 0 when every mapped category agrees, 1 otherwise. Only the oracle is run here; this script
contains no design-rule logic of its own.
"""

import json
import sys
from collections import Counter
from pathlib import Path

# Our rule -> the oracle's violation type. Rules with no oracle counterpart are not compared.
MAPPING = {
    "Clearance": "clearance",
    "Short": "shorting_items",
    "TrackWidth": "track_width",
    "AnnularRing": "annular_width",
    "DrillSize": "drill_out_of_range",
    "EdgeClearance": "copper_edge_clearance",
    "CourtyardOverlap": "courtyards_overlap",
}
# Oracle categories we deliberately do not mirror: warnings about silkscreen, missing libraries,
# dangling track ends (our router never leaves them), and via_diameter (implied by our ring check).
IGNORED = {"silk_over_copper", "lib_footprint_issues", "track_dangling", "via_diameter", "footprint_type_mismatch"}


def main(root: Path) -> int:
    ok = True
    for ours_path in sorted(root.glob("*.drc.json")):
        name = ours_path.name[: -len(".drc.json")]
        theirs_path = root / name / "drc.json"
        if not theirs_path.exists():
            print(f"{name}: oracle report missing at {theirs_path}")
            ok = False
            continue
        ours = json.loads(ours_path.read_text())
        theirs = json.loads(theirs_path.read_text())
        errors = Counter(v["type"] for v in theirs["violations"] if v["severity"] == "error")
        unconnected = len(theirs.get("unconnected_items", []))
        rows = []
        for rule, kind in MAPPING.items():
            a, b = ours.get(rule, 0), errors.get(kind, 0)
            rows.append((rule, a, b))
        rows.append(("Unrouted", ours.get("Unrouted", 0), unconnected))
        unexpected = {k: n for k, n in errors.items() if k not in MAPPING.values() and k not in IGNORED}
        bad = [r for r in rows if r[1] != r[2]]
        status = "match" if not bad and not unexpected else "MISMATCH"
        if status != "match":
            ok = False
        print(f"{name}: {status}")
        for rule, a, b in rows:
            if a or b:
                print(f"  {rule:16} ours={a:3} oracle={b:3}{'' if a == b else '  <--'}")
        if unexpected:
            print(f"  oracle-only errors: {unexpected}")
    return 0 if ok else 1


if __name__ == "__main__":
    if len(sys.argv) != 2:
        print(__doc__)
        sys.exit(2)
    sys.exit(main(Path(sys.argv[1])))
