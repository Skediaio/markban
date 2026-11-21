# markban format experiments

Empirical harness for choosing markban's file format, collaboration model and card-removal policy.
Python 3 stdlib + git only. Nothing here touches the rest of the repo.

## Formats (same board: "Acme sprint board", 5 columns, 40 cards)
- **A index**: `board.md` lists cards as `- [Title](cards/<id>.md)` per column; card files hold the description only.
- **B frontmatter per card**: `board.md` holds columns; each card file has `title`, `column`, `order` (step 1000).
- **C single file**: everything in `board.md`, `### Title` + `<!-- id: slug -->` per card.
Ids are title slugs. `RULES-{A,B,C}.md` are the agent quick references (they count as cost).

## Files
- `common.py` model, renderers, tolerant parsers (shared). `ops.py` per-format ops as minimal text edits.
- `gen.py <A|B|C> <dir>` fixture + git init. `TASKS.md` the 7 tasks. `check.py <A|B|C> <dir>` PASS/FAIL table.
- `solve_reference.py <A|B|C|removal-delete|removal-archive> <dir>` programmatic solver (validates the checkers).
- `static_cost.py` -> `results/static.md` (min read/write bytes and tokens per task and format).
- `collab_sim.py` -> `results/collab.md` (S1 shared directory: naive / CAS / string-replace writers;
  S2 git branch per agent, default vs `merge=union`, 200 trials per cell).
- `gen_removal.py <delete|archive> <dir>`, `TASKS-removal.md`, `RULES-A-archive.md`, `check_removal.py <dir>`.
- `make_runs.sh <workdir>` fresh A-1 A-2 B-1 B-2 C-1 C-2 removal-delete removal-archive + `_kit/` (rules and tasks).

## Run
    ./make_runs.sh /path/to/work                 # fixtures for test agents
    python3 check.py A /path/to/work/A-1         # after an agent ran (exit 0 = all pass)
    python3 check_removal.py /path/to/work/removal-delete
    python3 static_cost.py                       # fast
    TMPDIR=/some/scratch python3 collab_sim.py   # a few minutes; --s1-trials N --s2-trials N --only s1|s2

## Caveats
- Writers own disjoint cards so lost updates are well defined; real overlap would add semantic conflicts.
- Static grep costs are oracle lower bounds (agent knows the slug rule and exact anchor lines).
- check.py rows: 1-7 the tasks, U nothing else changed, W board well-formed.
