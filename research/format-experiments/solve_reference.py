#!/usr/bin/env python3
"""Usage: solve_reference.py <A|B|C|removal-delete|removal-archive> <dir>
Performs the 7 tasks (or the removal restore) programmatically using the shared text ops."""
import os
import shutil
import sys

import ops
from common import FORMATS, build_model, git, read_dir, sync_dir

LEGACY_ID = "legacy-importer"


def solve_tasks(fmt, d):
    model = build_model()
    before = read_dir(d)
    fs = dict(before)
    for n, op in sorted(ops.task_ops(model).items()):
        ops.run_op(fmt, fs, op)
    fs["answer.txt"] = "\n".join(c["title"] for c in model["cards"] if c["column"] == "doing") + "\n"
    sync_dir(d, before, fs)


def solve_removal(variant, d):
    fs = read_dir(d)
    cp = f"cards/{LEGACY_ID}.md"
    if variant == "delete":
        desc = git(d, "show", f"HEAD~1:{cp}").stdout
    else:
        os.makedirs(os.path.join(d, "cards"), exist_ok=True)
        shutil.move(os.path.join(d, f"archive/{LEGACY_ID}.md"), os.path.join(d, cp))
        desc = open(os.path.join(d, cp), encoding="utf-8").read()
        fs = read_dir(d)
    before = dict(fs)
    fs.pop(f"archive/{LEGACY_ID}.md", None)
    fs[cp] = desc
    ops.run_op("A", fs, {"op": "add", "id": LEGACY_ID, "title": "Legacy importer", "desc": desc.rstrip("\n"), "col": 0})
    fs[cp] = desc
    sync_dir(d, {p: t for p, t in before.items() if p != f"archive/{LEGACY_ID}.md" or variant == "delete"}, fs)


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    what, d = sys.argv[1:3]
    if what in FORMATS:
        solve_tasks(what, d)
    elif what.startswith("removal-"):
        solve_removal(what.split("-", 1)[1], d)
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
