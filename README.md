# Markban

**Markban is a plain-Markdown kanban format built with and for AI agents: every change is a command, and every
board is checked.**

*An agent works. A machine checks. You decide.*

## The idea

Teams now include agents. Tools like Trello were built for people clicking in a browser: an agent has to go
through an API, learn its abstractions and spend tokens on JSON it does not need. Markban turns that around.
A board is a folder of small Markdown files in your repository. An agent reads one file to see the board and
changes one line to move a card. Code parses it deterministically, without an LLM. People watch the board in
a viewer that runs in any browser, on any device.

1. **Agents run the board.** They create cards, move them and shape the columns, by editing the files or
   through the `markban` command line.
2. **A machine checks it.** `markban check` validates a board in CI or in an agent hook, with errors an agent
   can act on.
3. **People watch and decide.** The viewer shows the board live as agents work. In this first version it does
   not edit: whoever wants an editor with buttons builds one on top, sending the same commands.

## Principles

- **Everything is a command.** Every change to a board is a named command with JSON parameters in
  `markban-core`, reachable from the CLI (later also a JSON-lines server and MCP). Editing the Markdown by hand
  stays legal; commands are the checked, token-cheap path.
- **The files are the truth.** Anything else (an index, a database, viewer state) is derived and can be thrown
  away at any time.
- **Never crash.** A malformed board or bad parameters give an actionable error, never a panic.
- **Checked, not hoped.** Tests are the gate. Output is deterministic: the same input gives byte-identical files.
- **Platform, not applications.** Markban is the format and its engine. Specific workflows are separate
  projects on top.

## Where it stands

Version 0.0.0: format spec v0.1 is settled, chosen by measurement
([`research/format-experiments`](research/format-experiments)). The code is being built. What is planned and what is done:
[`docs/roadmap.md`](docs/roadmap.md); decisions and their reasons: [`docs/decisions.md`](docs/decisions.md).

- **Format spec**: [`spec/SPEC.md`](spec/SPEC.md)
- **Example board**: [`examples/demo-board`](examples/demo-board)
- **Rules for agents and contributors**: [`AGENTS.md`](AGENTS.md)

## Use it in your docs

Two ways to put a board in a documentation site, both static and both safe for untrusted card text:

- **Markdown (mdsvex, unified):** [`remark-markban`](packages/remark-markban) turns a fenced `markban` block that
  names a board directory into HTML.
- **Svelte 5 and Vite:** [`@markban/svelte`](packages/svelte) gives `<Markban {board} />` and a Vite plugin:
  `import board from 'markban:boards/roadmap'`. In dev, an edit of the board reloads the page.

A working SvelteKit app with both: [`examples/sveltekit-mdsvex`](examples/sveltekit-mdsvex).

## Planned layout

Dependencies point downwards only.

| Path | What it is |
|---|---|
| `crates/markban-app` | The viewer: Dioxus fullstack, read-only, live refresh. |
| `crates/markban-cli` | `markban`: `check`, commands, batch `run`. |
| `crates/markban-core` | Lossless parser and writer, checks, commands. No UI. |

## Licence

MIT, see [LICENSE](LICENSE).
