# Decisions

Decisions taken, newest first, each with its reason. Open decisions at the bottom are the developer's to settle.

## Taken

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
