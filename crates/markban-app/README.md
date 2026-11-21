# markban-app

Read-only viewer for a markban board: a Dioxus 0.7.10 fullstack app (server plus WASM). The server reads the
board directory and never writes to it. Edits made by agents or by hand show up in the open page within about
a second, without a reload.

## Run

Needs the Rust toolchain with the `wasm32-unknown-unknown` target and `dx` 0.7.10
(`cargo install dioxus-cli --version 0.7.10 --locked`).

```sh
# development, with hot reload
MARKBAN_BOARD=examples/demo-board dx serve -p markban-app

# release build, then run the server binary from its output directory
dx build -p markban-app --release --web
MARKBAN_BOARD=$PWD/examples/demo-board PORT=8080 IP=127.0.0.1 \
  target/dx/markban-app/release/web/server
```

Open <http://127.0.0.1:8080>.

| Variable | Default | Meaning |
|---|---|---|
| `MARKBAN_BOARD` | current directory | Board directory (contains `board.md`). |
| `IP` | `127.0.0.1` | Address to bind. Keep it on loopback. |
| `PORT` | `8080` | Port. |
| `MARKBAN_ALLOWED_HOSTS` | empty | Comma-separated extra host names the server accepts in the `Host` header (a name, or `name:port`). |

## Security

- **Host check (DNS rebinding).** Every request must carry a `Host` of `localhost`, `127.0.0.1` or `[::1]`
  (any port); anything else gets `403`. To serve the viewer under another name, for example behind a reverse
  proxy, list it: `MARKBAN_ALLOWED_HOSTS=board.example.com,board.lan:8080`.
- **Markdown is sanitised** (`ammonia`): no scripts, no event handlers, no `javascript:` links, and images with
  a remote `src` are removed so a card cannot make your browser call out. Task-list items show as disabled
  checkboxes; the viewer never writes.

## Server functions

- `get_board()`: the current `BoardView`.
- `get_card(id)`: title, column name and the description as sanitised HTML (`pulldown-cmark` then `ammonia`).
- `wait_for_change(since)`: long poll, returns `(revision, BoardView)` when the revision differs from `since`,
  or after about 25 seconds.

## Test

```sh
cargo test -p markban-app
cargo clippy -p markban-app --all-targets -- -D warnings
```
