#!/usr/bin/env python3
"""Compare board-sim node voltages with ngspice's for every simulation fixture.

usage: sim_oracle.py <dir>

Expects, per fixture NAME: NAME.cir (deck), NAME.ours.json (our dc and ac numbers) and
NAME.ngspice.log (ngspice -b output of the deck). Every node ngspice prints must be within 1% of
ours (or 1 mV when the value is near zero). Exit 0 when all agree, 1 otherwise.
"""

import json
import re
import sys
from pathlib import Path

TOL_REL = 0.01
TOL_ABS = 1e-3

# ngspice prints "v(out) = 2.500000e+00" after `print`, and "vm(out) = ..." after an ac print.
LINE = re.compile(r"^\s*(vm?)\((\S+?)\)\s*=\s*([-+0-9.eE]+)")


def parse_log(text):
    dc, ac = {}, []
    block = None
    for line in text.splitlines():
        m = LINE.match(line)
        if not m:
            continue
        kind, node, val = m.group(1), m.group(2).upper(), float(m.group(3))
        if kind == "v":
            dc[node] = val
        else:
            if block is None or node in block:
                block = {}
                ac.append(block)
            block[node] = val
    return dc, ac


def close(a, b):
    return abs(a - b) <= max(TOL_ABS, TOL_REL * max(abs(a), abs(b)))


def main(root):
    ok = True
    for ours_path in sorted(root.glob("*.ours.json")):
        name = ours_path.name[: -len(".ours.json")]
        log_path = root / f"{name}.ngspice.log"
        if not log_path.exists():
            print(f"{name}: ngspice log missing")
            ok = False
            continue
        ours = json.loads(ours_path.read_text())
        dc, ac = parse_log(log_path.read_text())
        if not dc:
            print(f"{name}: ngspice printed no node voltages")
            ok = False
            continue
        bad = []
        for node, theirs in dc.items():
            mine = ours["dc"].get(node)
            if mine is None or not close(mine, theirs):
                bad.append(f"dc {node}: ours={mine} ngspice={theirs}")
        freqs = sorted(ours.get("ac", {}).keys(), key=float)  # the deck prints them in rising order
        for block, f in zip(ac, freqs):
            for node, theirs in block.items():
                mine = ours["ac"][f].get(node)
                if mine is None or not close(mine, theirs):
                    bad.append(f"ac@{f} {node}: ours={mine} ngspice={theirs}")
        if len(ac) != len(freqs):
            bad.append(f"ac blocks: ours {len(freqs)} ngspice {len(ac)}")
        if bad:
            ok = False
            print(f"{name}: MISMATCH")
            for b in bad:
                print(f"  {b}")
        else:
            print(f"{name}: match ({len(dc)} dc nodes, {len(ac)} ac points)")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(Path(sys.argv[1])))
