# AGENTS.md: guide for AI agents and contributors

Markban is a plain-Markdown kanban format built with and for AI agents, plus a Rust engine (`markban-core`),
a command line (`markban`) and a read-only viewer (`markban-app`, Dioxus fullstack). Read this file first, then
`spec/SPEC.md`, `docs/roadmap.md` and `docs/decisions.md`.

**Language.** The developer prompts in Dutch (Flemish). Answer the developer in Flemish, but write everything
that lands in the repository in English: code, comments, identifiers, docs, commit messages, test names and
error messages.

## 1. Orientation

| Read | Why |
|---|---|
| `spec/SPEC.md` | The format. Source of truth: a format change is proposed there first. |
| `docs/roadmap.md` | Milestones, current focus, done and open. |
| `docs/decisions.md` | Decisions taken with their reasons, and open ones only the developer settles. |
| `research/` | Experiments behind the decisions (format, collaboration, removal). |

## 2. Layout

Dependencies point downwards only: `markban-app` → `markban-cli` → `markban-core`. The core has no UI, no
network and no file watching. The viewer never writes a board.

## 3. Golden rules

1. **Everything is a command.** A change to a board is a command with JSON parameters in `markban-core`, with
   tests. The CLI (and later a JSON-lines server and MCP) dispatch commands; none has logic of its own. A
   feature that can only be done by hand-editing is not finished.
2. **The files are the truth.** Never store state that cannot be rebuilt from the board's files. A database,
   cache or viewer state is derived and disposable.
3. **Lossless.** Parsing and writing a board round-trips byte for byte. Unknown keys, prose and formatting
   are preserved; a command changes only the lines it must.
4. **Never crash.** Non-test code never panics: no `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!`,
   `unreachable!` or slice indexing on input-derived values. Return an error the agent can act on (what is
   wrong, where, what is valid). Enforced by workspace clippy lints; `unsafe_code = "forbid"`. A panic-hunt
   test runs every command with adversarial parameters.
5. **Tests are the gate.** Every change comes with tests: write the test, run it, read the result, fix, until
   green. Never weaken or delete a failing test to get green. `examples/` boards always pass `markban check`.
6. **Determinism.** The same input gives byte-identical output: no `HashMap` order in anything written.
7. **Token-cheap for agents.** Every format and CLI choice is weighed by what an agent must read and write.
   Success output is minimal; errors are one actionable line.
8. **Platform, not applications.** Markban is the format and its engine. A workflow for one kind of team is a
   separate project on top of the commands.

## 4. Editing a board by hand

Follow "Rules for writers" in `spec/SPEC.md`. Never rename a card id. Preserve unknown keys and content. Run
`markban check <board>` afterwards once the CLI exists.

## 5. Before you finish a task

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p <crate you touched>
```

`cargo xtask ci` runs the full gate (fmt, clippy, tests, `markban check` on every example) once per finished
piece of work, before `main` moves. Then update `docs/roadmap.md` (done with its date, what is still open).
