#!/usr/bin/env bash
# Usage: make_runs.sh <workdir>  -- fresh fixtures A-1 A-2 B-1 B-2 C-1 C-2 removal-delete removal-archive
# plus <workdir>/_kit with the task and rules files to hand to the test agents.
set -euo pipefail
W=${1:?usage: make_runs.sh <workdir>}
H=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$W/_kit"
for f in A B C; do for n in 1 2; do rm -rf "${W:?}/$f-$n"; python3 "$H/gen.py" "$f" "$W/$f-$n"; done; done
for v in delete archive; do rm -rf "${W:?}/removal-$v"; python3 "$H/gen_removal.py" "$v" "$W/removal-$v"; done
cp "$H"/RULES-*.md "$H"/TASKS.md "$H"/TASKS-removal.md "$W/_kit/"
echo "fixtures in $W: $(ls "$W" | tr '\n' ' ')"
