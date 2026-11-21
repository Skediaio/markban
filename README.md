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

## Server mode

For live, shared boards without git, `markban serve` hosts several boards from one data directory. Each user or
agent gets a token that lists the boards it may use; every change is a command, run one at a time per board, and
logged. Agents keep using the same CLI by pointing it at the server:

```sh
markban server init /srv/markban            # prints the first admin token once
markban server --data /srv/markban board add kuuno "Kuuno" Todo Doing Done
markban server --data /srv/markban user add designer-anna --boards kuuno   # prints her token once
markban serve --data /srv/markban           # http://127.0.0.1:7171

export MARKBAN_URL=http://127.0.0.1:7171 MARKBAN_TOKEN=mb_... MARKBAN_BOARD=kuuno
markban ls
markban add "Draft the landing page" -c Doing
markban log -n 5
```

Behind a reverse proxy, keep `serve` on loopback, terminate TLS in the proxy and set
`MARKBAN_ALLOWED_HOSTS` to its host name; `--token-file` writes a new token to a 0600 file instead of stdout.
The CLI refuses plain `http://` to a non-loopback host unless `--allow-insecure-http`. The server limits
cards per board, card size, requests in flight and request time (flags on `serve`); `GET /healthz` answers `ok`.

See [`spec/SPEC.md`](spec/SPEC.md) section 9 for the data layout, the HTTP API, limits, the log and access rules.

## Planned layout

Dependencies point downwards only.

| Path | What it is |
|---|---|
| `crates/markban-app` | The viewer: Dioxus fullstack, read-only, live refresh. |
| `crates/markban-cli` | `markban`: `check`, commands, batch `run`, remote mode, `serve`. |
| `crates/markban-server` | The server: data directory, `access.toml`, HTTP API, log. |
| `crates/markban-core` | Lossless parser and writer, checks, commands. No UI. |

## Licence

MIT, see [LICENSE](LICENSE).
