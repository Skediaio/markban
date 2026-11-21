# Markban format — v0.1

Markban is a plain-Markdown format for kanban boards. Its goals, in order:

1. **Agents work well with it**: one small file per change, few tokens to read or write, no hidden state.
2. **Code parses it deterministically**: a strict, line-based grammar; no LLM and no full YAML parser needed.
3. **Several writers can work at once**, in one shared directory or on git branches, without conflicts.
4. **People can view it** like a Trello board.

The choices below were measured; see `research/format-experiments` and `docs/decisions.md`.

## 1. Layout

A board is a directory:

```
<board>/
  board.md          # board title, columns, optional description
  cards/
    <id>.md         # one file per card: title, column, order, description
  .markban.lock     # optional, tool-owned: advisory lock file, never part of the board
```

`.markban.lock` is created by tools that write the board, to take turns (see § 5). It is empty, may be left
in place, should be git-ignored, and tools never report it. Symbolic links are never followed: a link in
`cards/` is ignored (W001), a `cards/` directory that is itself a link is ignored (W001) and tools refuse to
write into it, and a `board.md` that is a link counts as missing.

Each card carries its own column and position, so moving, adding, editing or removing a card changes exactly
one file. No change to a card touches `board.md`.

## 2. Frontmatter subset

Both files start with a frontmatter block: a line `---`, then key lines, then a line `---`. Markban reads a
strict subset of YAML:

- A key line is `key: value`. Keys match `[a-z][a-z0-9_]*`.
- A value is either **plain** (the rest of the line, trimmed) or **double-quoted** (`"..."` with the escapes
  `\"` and `\\`). Writers quote a value when it is empty, starts or ends with a space, starts with one of
  `` "'[]{}&*!|>%@`#,?: ``, starts with `-` and is not a decimal number, or contains `: ` or ` #`.
- `board.md` has one block key, `columns:`, followed by list items (see § 3).
- An empty value counts as missing. If a key appears twice, the first one counts.
- Any other line inside the block, and any key markban does not know, is preserved byte for byte.

## 3. `board.md`

```markdown
---
markban: 0.1
title: Acme sprint board
columns:
  - id: backlog
    name: Backlog
  - id: doing
    name: Doing
  - id: done
    name: Done
---

Optional free Markdown describing the board.
```

| Key | Required | Notes |
|---|---|---|
| `markban` | yes | Format version. This document is `0.1`. |
| `title` | yes | Board title. |
| `columns` | yes | List of columns. List order is display order. |
| `columns[].id` | yes | `[a-z0-9]+(-[a-z0-9]+)*`, unique. Cards refer to it. Never renamed. |
| `columns[].name` | yes | Display name. Rename a column by changing this. |

A column item starts with `  - id: <id>`; its other keys follow on lines indented by four spaces. Unknown item
keys are preserved. Everything after the closing `---` is the board description.

## 4. Card files (`cards/<id>.md`)

The filename without `.md` is the card id: `[a-z0-9]+(-[a-z0-9]+)*`, at most 64 characters. The id is the slug
of the title at creation (lowercase ASCII letters and digits kept, every run of other characters becomes `-`,
trimmed of `-`, `card` if nothing is left); on collision
append `-2`, `-3`, …. The id never changes, even when the title does.

```markdown
---
title: Fix login timeout
column: doing
order: 2000
---

Sessions expire after 5 minutes instead of 30.
```

| Key | Required | Notes |
|---|---|---|
| `title` | yes | One line. |
| `column` | yes | A `columns[].id` of `board.md`. |
| `order` | no | Decimal number matching `-?[0-9]+(\.[0-9]+)?` (`-12`, `1500`, `1500.25`). Sort key within the column, ascending. |

Everything after the closing `---` is the description, free Markdown. Unknown keys are preserved.

**Sorting a column**: cards with an `order` ascending, ties broken by id; then cards without `order`, by id.
A card whose `order` is invalid (E008) counts as a card without `order`.

A tool that places a card at the end of a column that has cards without `order` leaves `order` out of the
card (it would otherwise sort before those cards); the card then sorts by id among the unordered ones.

## 5. Rules for writers

These rules hold for agents editing by hand and for tools.

- **Read the board**: `markban ls` prints it in one compact listing. By hand: read `board.md`, then the
  frontmatter of each card (`grep -H '^\(title\|column\|order\):' cards/*.md`).
- **Add a card**: create `cards/<id>.md`. Never overwrite an existing file; if the id is taken, use the next
  suffix. Leave out `order` to put the card at the end of its column.
- **Move a card**: change `column` and, if the position matters, `order`. Top of a column: lower than the
  lowest order there. Between two cards: any number strictly between theirs. Decimals are fine; never
  renumber other cards.
- **Edit a card**: change `title` or the description in its file.
- **Remove a card**: delete its file. Git history is the undo.
- **Add, rename or reorder a column**: edit `columns` in `board.md`. Keep every `id`.
- **Remove a column**: only when no card refers to it; move or remove its cards first.
- Preserve unknown keys, lines and content. Keep line endings as they are.
- Never write a file partially: write a temporary file next to it and rename it into place.
- Tools that write take an exclusive advisory lock on `.markban.lock` in the board directory for the whole
  read, change and write cycle, so two tools never interleave. By hand, there is nothing to lock: just
  follow the rules above.
- Write in an order that keeps the board valid at every step: when a change adds columns, write `board.md`
  first; when it removes columns, write it last, after the cards moved away; when it does both, write a
  `board.md` with the union of the old and new columns first, then the cards, then the final `board.md`.

## 6. Validity

`markban check <board>` reports diagnostics with file and line. Errors make a board invalid:

| Code | Meaning |
|---|---|
| `E001` | `board.md` missing, or its frontmatter is missing or unterminated. |
| `E002` | `markban` missing, or a version this tool does not support. |
| `E003` | `title` missing (board or card). |
| `E004` | `columns` block missing; column id missing, malformed or duplicated; column name missing. |
| `E005` | Card file name is not a valid id. |
| `E006` | Card frontmatter missing or unterminated. |
| `E007` | Card `column` missing, or not a column of `board.md`. |
| `E008` | Card `order` is not a decimal number. |
| `E009` | A file is not valid UTF-8, or a quoted value is malformed. |

Warnings do not make a board invalid:

| Code | Meaning |
|---|---|
| `W001` | A file or directory in `cards/` that is not `<id>.md`, or a symbolic link (`symbolic link ignored`). `.markban.lock` is not reported. |
| `W002` | Two cards in one column with the same `order` (ties sort by id). |

A viewer shows cards with an unknown column in an extra "Unsorted" column and still renders the rest.

## 7. Commands

Every change has a command in `markban-core`, reachable as `markban run` JSON lines (`{"cmd":"card.move",…}`)
and as CLI sugar. A command validates the board after the change and refuses one that would make it invalid.
Column parameters accept an id or a name (case-insensitive). Positions are anchors, never indexes.

| Command | Parameters |
|---|---|
| `board.init` | `title`, `columns` (names) |
| `card.add` | `title`, `column?` (default: first), `description?`, `id?`, position† |
| `card.move` | `id`, `column?`, position† |
| `card.edit` | `id`, `title?`, `description?`, `append?`, `if_unchanged?`‡ |
| `card.remove` | `id` |
| `column.add` | `name`, `id?`, `before?` (column; default: end) |
| `column.rename` | `column`, `name` |
| `column.move` | `column`, `before?` (column; default: end) |
| `column.remove` | `column`, `move_to?` (column) |

† Position: one of `top: true`, `before: <card id>`, `after: <card id>`; default the end of the column.

‡ `if_unchanged`: the SHA-256 (64 hex digits) of the card file as last read; `markban show --json` prints it as
`sha256`. When the card changed since, the edit is refused as contention (exit code 3) and nothing is written.

When a tool picks an `order` between two neighbours, it takes the midpoint and moves it by a small offset
derived from the card id, so two writers inserting into the same gap at once get different, deterministic
orders.

Orders written by hand may have any number of decimals. For the end and the top of a column a tool works from
the integer ceiling or floor of the extreme order. Between two cards it works at one decimal more than the
longest neighbour (at least 6, at most 18) and refuses beyond that.

## 8. Agent quick reference

```
markban init "Title" COL...      # create a board in the current directory
markban ls                      # whole board, one line per card
markban show <id>               # one card with its description
markban add "Title" [-c Doing] [-d "text"] [--top | --before <id> | --after <id>]
markban mv <id> [Column] [--top | --before <id> | --after <id>]
markban edit <id> [--title "..."] [-d "..." | --append "..."]
markban rm <id>
markban col add|rename|mv|rm ...
markban run < commands.jsonl    # many commands, all or nothing
markban check
```

Success prints the affected id only. An error is one line saying what is wrong and what is valid.
`markban ls` prints at most one line about problems (`! N problem(s); run markban check`); `markban check`
prints them all. Text output replaces control characters from board content (except newline and tab) with
U+FFFD, so a board cannot inject terminal escape sequences; `--json` output is exact. `-d -` and
`--append -` read the text from stdin.

Exit codes:

| Code | Meaning |
|---|---|
| 0 | Success (`check`: no errors; warnings are fine). |
| 1 | Invalid input or the command was refused: fix the input. `check` also uses 1 when the board has errors. |
| 2 | Usage error (unknown option, missing argument). |
| 3 | Contention: another writer changed the board while the command ran, the board lock could not be taken within the timeout, or `if_unchanged` did not match. Nothing was written; retry (after reading the card again). |
| 4 | Partially applied: some files were written before a failure. Run `markban check` and inspect before retrying. |

## 9. Server mode

`markban serve` hosts several boards for people and agents without git. The board files stay the database;
the server runs every change as a command, one at a time per board, with the same lock and compare and swap.

**Data directory** (`markban serve --data <DIR>`, default `$MARKBAN_DATA`, else the current directory):

```
access.toml          users, their token hashes and boards
boards/<board-id>/   one ordinary board each (sections 1 to 8 apply unchanged)
log/<board-id>.jsonl append-only log, outside the board so it never shows up as a card
```

A board id is `[a-z0-9]+(-[a-z0-9]+)*`, at most 64 characters. A symbolic link in `boards/` is not a board.

**Access.** `access.toml` lists users:

```toml
[users.maarten]
token_sha256 = "<64 hex digits>"
boards = ["*"]          # every board
admin = true            # reserved for administration from the browser
[users.designer-anna]
token_sha256 = "<64 hex digits>"
boards = ["kuuno"]
```

A token is 32 random bytes (`mb_` and 64 hex digits), shown once and stored only as its SHA-256; hashes are
compared in constant time. The server reads `access.toml` on every request, so changes apply at once; a file it
cannot parse refuses every request. A board the user may not use answers exactly like a board that does not
exist.

**Administration** runs on the server with direct file access, never over HTTP:

```
markban server init <DIR> [--admin NAME]          # layout plus a first user with "*"; prints its token
markban server user add <name> --boards a,b [--admin]   # prints the new token once
markban server user remove <name>
markban server user boards <name> a,b             # or * for all boards
markban server board add <id> "Title" COL...      # board.init in boards/<id>
markban server board list
```

The `server` commands take `--data <DIR>` (default `$MARKBAN_DATA`, else the current directory) and edit
`access.toml` atomically, keeping comments and unknown keys.

**API.** Every request needs `Authorization: Bearer <token>`; a missing or wrong token is 401. The server binds
`127.0.0.1:7171` by default (`--listen`) and answers 403 to a `Host` header that is not loopback or listed in
`MARKBAN_ALLOWED_HOSTS` (comma separated; set it behind a reverse proxy, which also terminates TLS).

| Request | Answer |
|---|---|
| `GET /api/boards` | `[{"id","title"}]`, the boards the token may use |
| `GET /api/boards/<id>` | the board view, as `markban ls --json` |
| `GET /api/boards/<id>/cards/<card>` | the card, as `markban show --json` |
| `POST /api/boards/<id>/run` | body: a JSON array of commands, all or nothing; `{"outcomes":[{"id"}]}` |
| `GET /api/boards/<id>/log?limit=N` | the last N log entries (default 50, at most 1000), newest last |

An error is `{"error": "<one line>", "code": <exit code>, "index": <failing command>?}` with HTTP status 400
(code 1), 409 (code 3), 500 (code 4 or a server fault), 401, 404 (unknown board, card or endpoint) or 413. A
request body may hold at most 1 MiB and 1000 commands.

**Log.** Each successful command appends one line: `{"ts":"<RFC 3339 UTC>","user":"<name>","cmd":{…},"outcome":"<id>"}`.

**CLI remote mode.** With `MARKBAN_URL` (or `--url`) set, every board command (`ls`, `show`, `add`, `mv`,
`edit`, `rm`, `col`, `init`, `run`, `check`) sends the same commands to the server and prints the same output
with the same exit codes as locally. The token comes from `MARKBAN_TOKEN` (`--token` works but shows in the
process list); the board from `-b <id>` or `MARKBAN_BOARD`, which is a board id in remote mode and a
directory locally. `-C` is refused in remote mode. Two commands only work remotely: `markban boards [--json]`
and `markban log [-n N]` (JSON lines). A network failure is one line with exit code 1.

## Reserved for later versions

Labels, assignees, due dates, WIP limits, checklists, comments, archive, attachments, card relations. Tools
preserve such keys and content today; they get a meaning in a later version.
