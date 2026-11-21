#!/usr/bin/env python3
"""Usage: check.py <A|B|C> <boarddir>  -- verify the 7 tasks; exit 0 only if all rows pass."""
import os
import sys

from common import COLUMNS, FORMATS, build_model, parse, read_dir

TOUCHED = {"fix-login-timeout", "old-prototype", "audit-log", "export-to-csv"}
NEWDESC = "Let users download the current table as CSV."
BLOCK = "Blocked on provider sandbox access."


def by_col(m):
    idx = {c["id"]: i for i, c in enumerate(m["columns"])}
    out = [[] for _ in m["columns"]]
    for c in m["cards"]:
        if c["column"] in idx:
            out[idx[c["column"]]].append(c)
    return out


def run(fmt, d):
    fs = read_dir(d)
    m, o = parse(fmt, fs), build_model()
    bc, ob = by_col(m), by_col(o)
    ids = lambda lst: [c["id"] for c in lst]
    card = {c["id"]: c for c in m["cards"]}
    ocard = {c["id"]: c for c in o["cards"]}
    rows = []

    def add(name, ok, why=""):
        rows.append((name, bool(ok), "" if ok else why))

    ncol = len(bc) == 5
    col = lambda i: bc[i] if ncol else []
    ans = fs.get("answer.txt")
    want = [c["title"] for c in ob[2]]
    add("1 list Doing", ans is not None and [l.strip() for l in ans.strip().split("\n")] == want, "answer.txt missing or wrong")
    done = col(4)
    add("2 move to top of Done", ncol and ids(done)[:1] == ["fix-login-timeout"] and ids(done)[1:] == ids(ob[4])
        and "fix-login-timeout" not in ids(col(2)), "not first in Done / Done order changed")
    n = card.get("export-to-csv")
    add("3 add Export to CSV", ncol and n and n["title"] == "Export to CSV" and n["desc"] == NEWDESC
        and ids(col(0))[-1:] == ["export-to-csv"], "missing, wrong text or not last in Backlog")
    add("4 rename Review->QA", [c["name"] for c in m["columns"]] == ["Backlog", "Ready", "Doing", "QA", "Done"], "names differ")
    exp5 = [i for i in ids(ob[0]) if i not in ("old-prototype", "audit-log")]
    exp5.insert(exp5.index("rate-limiter"), "audit-log")
    add("5 Audit log above Rate limiter", ncol and [i for i in ids(col(0)) if i not in ("old-prototype", "export-to-csv")] == exp5, "backlog order wrong")
    gone = "old-prototype" not in card and not any("old-prototype" in p for p in fs)
    add("6 remove Old prototype", gone, "card or file still present")
    pw = card.get("payment-webhooks")
    add("7 append to Payment webhooks", pw and pw["desc"] == ocard["payment-webhooks"]["desc"].strip() + "\n" + BLOCK, "description differs")
    # unchanged: everything not touched by a task
    bad = []
    if m["title"] != o["title"]:
        bad.append("board title")
    if [c["id"] for c in m["columns"]] != ([c["id"] for c in o["columns"]] if fmt == "B" else [c["id"] for c in m["columns"]]):
        bad.append("column ids")
    for cid, oc in ocard.items():
        c = card.get(cid)
        if cid == "old-prototype":
            continue
        if c is None:
            bad.append(f"{cid} missing")
            continue
        if c["title"] != oc["title"]:
            bad.append(f"{cid} title")
        if cid != "payment-webhooks" and c["desc"] != oc["desc"].strip():
            bad.append(f"{cid} desc")
        if cid != "fix-login-timeout":
            oci = next(i for i, l in enumerate(ob) if cid in ids(l))
            if not ncol or cid not in ids(bc[oci]):
                bad.append(f"{cid} column")
    for i in range(len(ob)):
        if len(bc) == len(ob):
            got = [x for x in ids(bc[i]) if x not in TOUCHED]
            exp = [x for x in ids(ob[i]) if x not in TOUCHED]
            if got != exp:
                bad.append(f"order in column {i}")
    extra = set(card) - set(ocard) - {"export-to-csv"}
    if extra:
        bad.append(f"extra cards {sorted(extra)}")
    add("U nothing else changed", not bad, "; ".join(bad[:5]))
    add("W well-formed", not m["errors"], "; ".join(str(e) for e in m["errors"][:3]))
    return rows


def main():
    if len(sys.argv) != 3 or sys.argv[1] not in FORMATS:
        sys.exit(__doc__)
    rows = run(*sys.argv[1:3])
    for name, ok, why in rows:
        print(f"{'PASS' if ok else 'FAIL'}  {name}" + (f"  ({why})" if why else ""))
    sys.exit(0 if all(r[1] for r in rows) else 1)


if __name__ == "__main__":
    main()
