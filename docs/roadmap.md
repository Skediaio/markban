# Roadmap

## Current focus

Iteration 1: agents run a board through `markban`, people watch it in a read-only viewer.

| # | Milestone | Status |
|---|---|---|
| 0 | Principles, decisions, format experiments | Done 2026-11-21 |
| 1 | Spec v0.1 and example board | Done 2026-11-21 |
| 2 | Workspace, lints, `cargo xtask ci` | Done 2026-11-21 |
| 3 | Core: lossless parse and write, `check` | Done 2026-11-21 |
| 4 | Core: commands, atomic writes with compare and swap, panic hunt | Done 2026-11-21 |
| 5 | CLI: `ls`, `show`, sugar commands, `run`, `check` | Done 2026-11-21 |
| 6 | Viewer: Dioxus fullstack, columns and cards, live refresh, card detail | Done 2026-11-21 |

## Next

| # | Milestone | Status |
|---|---|---|
| 7 | Plugins: `@markban/core` (WASM), `remark-markban` | In progress |
| 8 | Server: `markban serve`, several boards, tokens with board access, command API, log; CLI remote mode | Done 2026-11-21 |
| 9 | Viewer: board list, login, editing in the browser (move, add, edit) | Open |
| 10 | `@markban/svelte`, Vite plugin, SvelteKit + mdsvex example | Open |
| 11 | Agent skill for local and server mode | Open |
| 12 | `undo`, format v0.2 (assignee, labels, extra keys shown as chips) | Open |

## Open

- Viewer: render task-list items as checkboxes; show the column name instead of its id in the card detail.
- Independent review of iteration 1.
- Server: token rotation (`server user token <name>`); HTTP admin endpoints for the `admin` flag (milestone 9);
  reading the log from its end instead of the whole file; request timeouts and rate limits (today: leave them
  to the reverse proxy).

## Out of scope for iteration 1

A database, MCP, archive, labels and other reserved card keys.
