# Agent runs (Claude Haiku, 2026-11-21)

Each run: a fresh Haiku general-purpose agent, given the format's RULES file and TASKS.md (or TASKS-removal.md), no CLI. Context growth is the final context size minus a no-tool baseline run (34,317 tokens). All 8 runs passed every check (check.py / check_removal.py).

| run | context growth over baseline (tok) | output tok | tool calls | tools |
|---|---|---|---|---|
| A-1 | 15465 | 3028 | 4 | Read,Read,Bash,Bash |
| A-2 | 16002 | 3698 | 4 | Read,Read,Bash,Bash |
| B-1 | 15073 | 2973 | 5 | Read,Read,Bash,Bash,Bash |
| B-2 | 23157 | 3189 | 5 | Read,Read,Bash,Bash,Bash |
| C-1 | 14456 | 3753 | 4 | Read,Read,Bash,Bash |
| C-2 | 14354 | 4660 | 4 | Read,Read,Bash,Bash |
| rm-delete | 8639 | 1293 | 5 | Read,Read,Bash,Bash,Bash |
| rm-archive | 9600 | 1945 | 8 | Read,Read,Read,Bash,Bash,Bash,Edit,Bash |

Observations: agents solved most tasks with one or two scripted Bash calls rather than reading every file, so the format's read cost was largely hidden (differences are within run-to-run noise, see B-1 vs B-2). n=2 per format; treat as indicative. Restoring a deleted card via git was cheaper than restoring from archive/ (fewer tool calls, less context).
