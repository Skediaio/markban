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

## Server functions

- `get_board()`: the current `BoardView`.
- `get_card(id)`: title, column and the description as sanitised HTML (`pulldown-cmark` then `ammonia`).
- `wait_for_change(since)`: long poll, returns `(revision, BoardView)` when the revision differs from `since`,
  or after about 25 seconds.

## Test

```sh
cargo test -p markban-app
cargo clippy -p markban-app --all-targets -- -D warnings
```
