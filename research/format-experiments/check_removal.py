#!/usr/bin/env python3
"""Usage: check_removal.py <boarddir>  -- verify "Legacy importer" was restored to the end of Backlog."""
import os
import sys

from common import build_model, parse, read_dir

ID = "legacy-importer"


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    fs = read_dir(sys.argv[1])
    m, o = parse("A", fs), build_model(legacy=True)
    card = {c["id"]: c for c in m["cards"]}
    backlog = [c["id"] for c in m["cards"] if c["column"] == "Backlog"]
    ocard = {c["id"]: c for c in o["cards"]}
    exp = [c["id"] for c in o["cards"] if c["id"] != ID]
    got = [c["id"] for c in m["cards"] if c["id"] != ID]
    rows = [
        ("R1 last card of Backlog", backlog[-1:] == [ID]),
        ("R2 original description", ID in card and card[ID]["desc"] == ocard[ID]["desc"].strip() and card[ID]["title"] == "Legacy importer"),
        ("R3 nothing else changed", got == exp and all(card[i]["desc"] == ocard[i]["desc"].strip() for i in exp if i in card)),
        ("R4 well-formed", not m["errors"]),
    ]
    for n, ok in rows:
        print(f"{'PASS' if ok else 'FAIL'}  {n}")
    print(f"INFO  archive copy removed: {not any(p.startswith('archive/') for p in fs)}")
    sys.exit(0 if all(r[1] for r in rows) else 1)


if __name__ == "__main__":
    main()
