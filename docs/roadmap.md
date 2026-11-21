# Roadmap

## Current focus

Iteration 1: agents run a board through `markban`, people watch it in a read-only viewer.

| # | Milestone | Status |
|---|---|---|
| 0 | Principles, decisions, format experiments | Done 2026-11-21 |
| 1 | Spec v0.1 and example board | Done 2026-11-21 |
| 2 | Workspace, lints, `cargo xtask ci` | Open |
| 3 | Core: lossless parse and write, `check` | Open |
| 4 | Core: commands, atomic writes with compare and swap, panic hunt | Open |
| 5 | CLI: `ls`, `show`, sugar commands, `run`, `check` | Open |
| 6 | Viewer: Dioxus fullstack, columns and cards, live refresh, card detail | Open |

## Out of scope for iteration 1

Editing in the viewer, drag and drop, a database, auth, several boards per viewer, a JSON-lines server and
MCP, archive, labels and other reserved card keys.
