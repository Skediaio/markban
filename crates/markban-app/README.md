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

## Hosting

To share one board over the network, run the viewer with one shared password, optionally serving a board
that lives in a git repository. The root `Dockerfile` builds an image with the server, the web bundle and
`git`; it runs as a non-root user, listens on `0.0.0.0:8080` and checks itself with `server --healthcheck`.

```sh
docker build -t markban-viewer .
docker run -d --name markban-viewer -p 127.0.0.1:8080:8080 \
  -e MARKBAN_PASSWORD='a long shared password' \
  -e MARKBAN_ALLOWED_HOSTS=board.example.com \
  -e MARKBAN_TRUST_PROXY=1 \
  -e MARKBAN_GIT_URL=https://git.example.com/team/project.git \
  -e MARKBAN_GIT_TOKEN='read-only token' \
  -e MARKBAN_GIT_SUBDIR=board \
  -v markban-viewer:/data \
  markban-viewer
```

Put a TLS-terminating reverse proxy in front: the session cookie is `Secure`, so browsers only send it over
HTTPS (and to `localhost`).

| Variable | Default | Meaning |
|---|---|---|
| `MARKBAN_PASSWORD` | unset | The shared password. Unset or empty: no login (local use). |
| `MARKBAN_TRUST_PROXY` | unset | `1`: count failed logins per client by the last `X-Forwarded-For` entry. Otherwise all clients share one counter. Set it only behind a proxy that sets that header. |
| `MARKBAN_GIT_URL` | unset | Repository to serve the board from. Unset: `MARKBAN_BOARD` is used. |
| `MARKBAN_GIT_TOKEN` | unset | Access token for a private repository (HTTPS). |
| `MARKBAN_GIT_BRANCH` | `main` | Branch to follow. |
| `MARKBAN_GIT_SUBDIR` | `.` | Directory in the repository that holds `board.md`. |
| `MARKBAN_GIT_INTERVAL` | `30` | Seconds between fetches. |
| `MARKBAN_GIT_DIR` | `<temp>/markban-git` (`/data/repo` in the image) | Where the clone lives. |

**Password.** With `MARKBAN_PASSWORD` set, every page, asset and server function needs a session; without
one a page shows a login form with one password field and anything else gets `401`. `/healthz` stays open
and answers only `ok`. The password is checked only on the server, in constant time. A login sets a random
32-byte session id in an `HttpOnly; Secure; SameSite=Strict` cookie, valid 30 days; sessions live in memory,
so a restart logs everyone out. "Log out" ends the session. After 5 failed logins within a minute the login
answers `429` until the minute is over.

**Git.** At start the viewer clones the branch shallowly, then every interval runs `git fetch --depth 1`
and `git reset --hard` to the remote branch; the viewer never writes, so nothing is lost, and the open page
refreshes as soon as the files change. The token reaches git as an HTTP `Authorization` header (Basic,
user `x-access-token`) through the `GIT_CONFIG_*` environment variables: never in the URL, the process
arguments or the logs, and git messages are redacted before they are logged. A failed fetch is logged and
retried; the last good board keeps being served.

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
