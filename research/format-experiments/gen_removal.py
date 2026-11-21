#!/usr/bin/env python3
"""Usage: gen_removal.py <delete|archive> <outdir>
Format A board where card "Legacy importer" was removed in the last commit."""
import os
import sys

from common import build_model, git, init_repo, render, write_dir

ID = "legacy-importer"


def main():
    if len(sys.argv) != 3 or sys.argv[1] not in ("delete", "archive"):
        sys.exit(__doc__)
    variant, out = sys.argv[1:3]
    if os.path.isdir(out) and os.listdir(out):
        sys.exit(f"{out} is not empty")
    os.makedirs(out, exist_ok=True)
    m = build_model(legacy=True)
    write_dir(out, render("A", m))
    init_repo(out, "Initial board")
    m["cards"] = [c for c in m["cards"] if c["id"] != ID]
    write_dir(out, {"board.md": render("A", m)["board.md"]})
    if variant == "delete":
        git(out, "rm", "-q", f"cards/{ID}.md")
    else:
        os.makedirs(os.path.join(out, "archive"))
        git(out, "mv", f"cards/{ID}.md", f"archive/{ID}.md")
    git(out, "add", "-A")
    git(out, "commit", "-q", "-m", "Remove Legacy importer card" + (" (archived)" if variant == "archive" else ""))


if __name__ == "__main__":
    main()
