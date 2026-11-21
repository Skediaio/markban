#!/usr/bin/env python3
"""Usage: gen.py <A|B|C> <outdir>  -- write the Acme board in that format and git-init it."""
import os
import sys

from common import FORMATS, build_model, init_repo, render, write_dir


def main():
    if len(sys.argv) != 3 or sys.argv[1] not in FORMATS:
        sys.exit(__doc__)
    fmt, out = sys.argv[1:3]
    if os.path.isdir(out) and os.listdir(out):
        sys.exit(f"{out} is not empty")
    os.makedirs(out, exist_ok=True)
    write_dir(out, render(fmt, build_model()))
    init_repo(out)


if __name__ == "__main__":
    main()
