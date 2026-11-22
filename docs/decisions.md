# Decisions

Decisions taken, newest first, each with its reason. Open decisions at the bottom are the developer's to settle.

## Taken

### 2026-11-22: Search is a linear scan in core, no index

`markban search` reads the parsed board and scans it: card ids, titles, descriptions, column ids and names,
and the board title and description. A board is bounded (a server refuses runs past 5000 cards or 256 KiB per
card) and typical boards are hundreds of cards, so the scan is milliseconds — an index would add a derived
store to keep fresh, startup cost and a second thing that can drift from the files, to win nothing at this
scale. The files stay the only truth. Terms are literal, case-insensitive substrings, all of them required
(AND); no regex or wildcards, because a pattern language buys little for an agent and costs crash-safety and
determinism. Archived cards are hits like any other, marked `(archived)`: search is how archived work stays
findable — that is why cards are archived rather than deleted. One implementation in `markban-core`, exposed
by the CLI (local scan), the server (`GET …/search?q=`) and the WASM package.

### 2026-11-22: Archiving is a status in the card's frontmatter, not a directory

Boards fill up with finished work; the archive is the optional key `archived: true` in `cards/<id>.md`.
Chosen over an `archive/` directory because format B's core property is that every card change touches one
card file: a move would break that, split the id namespace across two directories (a new card could take a
parked id) and lose the card's column and order. The removal experiment of 2026-11-21 measured restoring a
removed card, not a status, so its verdict stands: removal stays deletion. An archived card keeps a valid
`column` — no exception on E007 — so a 0.1 tool that ignores `archived` still sees a valid board and treats
the card as active, which is harmless; `column.remove` therefore counts archived cards and moves them with
`move_to`, refusing without it ("column done still has 3 cards (2 archived); pass move_to"), and an
unarchive to a removed column cannot happen. No date in the value: git history and the server log already
record when; any value other than `true` is `W003` and the card counts as active. The format version stays
0.1.

### 2026-11-21: The server has fixed resource limits and a bounded, clipped log

A security review found that one client could exhaust the server: an unbounded log read whole on every
`GET /log`, bodies read before authentication, waiting runs holding blocking threads, and boards without size
limits. The server now authenticates from the headers before the body, admits at most 64 requests at once
(503 beyond, at once rather than queued, so clients back off) with a 30 s request and 10 s header timeout,
waits for a board on the async side, and refuses runs past 5000 cards per board or 256 KiB per card file
(checked by `fs::run_guarded` before anything is written). All limits are flags with these defaults. The log
is an audit trail, not a backup: text fields over 2 KiB are clipped, the file rotates at 16 MiB keeping one
previous file, and reads go backwards from the end. Partial runs are logged too; a failed log write still
answers success with `"log":"failed"`, because the board has changed and a retry would apply twice. Removing a
board moves it to `removed/` instead of deleting it. `GET /healthz` is unauthenticated for health checks.

### 2026-11-21: The server API lives in its own crate, `markban-server`, served by `markban serve`

The API is a small axum router in `markban-server` (the axum 0.8 that Dioxus fullstack already pulls in), and
the `markban` binary serves it. Putting it inside `markban-app` would make the CLI depend on the Dioxus viewer
(the wrong direction) and would need `dx` and a WASM build to test the access rules; as a plain library the
CLI's remote-mode tests run a real server in-process on an ephemeral port. Milestone 9 mounts the same router
in the viewer, which already uses the crate for its `Host` check. Admin commands edit `access.toml` on disk
and never go over HTTP; the `admin` flag is stored for the browser administration of milestone 9.

### 2026-11-21: A markban server for live, shared boards without git

Agents struggle with abstractions such as git workflows and external trackers. `markban serve` hosts several
boards; agents and people use the same CLI commands against it by setting `MARKBAN_URL` and `MARKBAN_TOKEN`,
and the browser UI sends the same commands. The board's Markdown files stay the database, on the server.
Conflicts reduce to clear errors because the server runs commands one at a time with the existing lock and
compare-and-swap, and commands carry intent ("move card X to Done") rather than diffs. Each user or agent has
a token that lists the boards it may see. An append-only log per board records who ran which command. Local
mode (a board directory in a repository, edited with git) stays fully supported. This revises "the viewer is
read-only": the viewer gains editing, through commands only.

### 2026-11-21: Format B, per-card frontmatter carries column and order

Measured in `research/format-experiments` against an index in `board.md` (A) and a single file (C). With
several writers, A and C lose updates because every change touches `board.md` (40% and 60% lost with four
naive writers in a shared directory; 98% and 97% of four-branch git merges conflict, and `merge=union` leaves
92% and 88% of boards invalid). B lost 0.3% and had no git conflicts at all, because every card change touches
one card file. Haiku agents solved all seven tasks correctly in every format with similar token use; reading
the whole board is cheapest in A, so `markban ls` gives agents that compact listing on top of B.

### 2026-11-21: `order` is a decimal number with an id-derived offset

Agents can compute a number between two neighbours by hand (1000 and 2000 give 1500; decimals are allowed, so
no renumbering). The residual loss in B came from two writers inserting into the same gap and picking the same
number; tools now add a small offset derived from the card id, deterministic and distinct per card. Chosen over
string keys (fractional indexing) because those are hard for an agent to compute by hand.

### 2026-11-21: Card ids are title slugs, created with create-new semantics

A counter (`0042-...`) needs a scan for the highest number and collides when two agents create at once. A slug
of the title, with `-2` on collision and a create-new write, fails loudly instead.

### 2026-11-21: Removing a card deletes its file

Git is the undo. In the experiment, restoring a deleted card from git cost an agent less than restoring one
from `archive/` (8.6k against 9.6k tokens of context, 5 against 8 tool calls). `archive/` stays reserved.

### 2026-11-21: Both collaboration modes are supported

One shared directory (the viewer watches it) and git branches per agent both work with format B. The CLI writes
atomically and re-reads before replacing a file (compare and swap).

### 2026-11-21: Agents are the only writers in iteration 1; the viewer is read-only

Agents create and manage cards and columns. People watch the board in a viewer and decide. Whether the viewer
gets editing later is a separate decision; an editor would send the same commands as the CLI.

### 2026-11-21: Viewer is Dioxus fullstack in the browser

The board must be viewable on any device, later including phones. A browser page served next to the board
directory does that with nothing to install. This deliberately departs from 3DCraft's "native window, no
webview" rule: all code stays Rust (server plus WASM), but rendering is DOM/CSS. Dioxus desktop was rejected
(a system webview, and it needs `webkit2gtk-4.1`). Pinned at Dioxus 0.7.10, matching the installed `dx`.

### 2026-11-21: The Markdown files are the database

The developer confirmed: the board's Markdown files are the database. There is no separate store of record.

A read-only viewer of one board needs none: parsing a board takes microseconds and an in-memory snapshot
rebuilt on each file change is correct by construction. The requested combination of sqlx and Turso does not
exist today: sqlx speaks C SQLite only, Turso (`turso` crate) has its own async API, and the only adapter
(`sqlx-turso` 0.1.0-alpha.1) is an unmaintained-looking alpha. When a database earns its place (search across
boards, users, an activity log), it is a derived, disposable index; the choice between `turso` directly and
`sqlx` with SQLite is made then.

### 2026-11-21: Adopt the 3DCraft principles

Built with and for AI; everything is a command; the files are the truth; never crash; tests are the gate;
determinism; platform, not applications. Adopted as principles, without 3DCraft's process weight. See
`AGENTS.md` § 3.

## Open

- None for iteration 1.
