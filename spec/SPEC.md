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
`cards/` is ignored (W001) and a `board.md` that is a link counts as missing.

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
  first; when it removes columns, write it last, after the cards moved away.

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
| `card.edit` | `id`, `title?`, `description?`, `append?` |
| `card.remove` | `id` |
| `column.add` | `name`, `id?`, `before?` (column; default: end) |
| `column.rename` | `column`, `name` |
| `column.move` | `column`, `before?` (column; default: end) |
| `column.remove` | `column`, `move_to?` (column) |

† Position: one of `top: true`, `before: <card id>`, `after: <card id>`; default the end of the column.

When a tool picks an `order` between two neighbours, it takes the midpoint and moves it by a small offset
derived from the card id, so two writers inserting into the same gap at once get different, deterministic
orders.

Orders written by hand may have any number of decimals. For the end and the top of a column a tool works from
the integer ceiling or floor of the extreme order. Between two cards it works at one decimal more than the
longest neighbour (at least 6, at most 18) and refuses beyond that.

## 8. Agent quick reference

```
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
| 3 | Contention: another writer changed the board while the command ran. Nothing was written; retry. |
| 4 | Partially applied: some files were written before a failure. Run `markban check` and inspect before retrying. |

## Reserved for later versions

Labels, assignees, due dates, WIP limits, checklists, comments, archive, attachments, card relations. Tools
preserve such keys and content today; they get a meaning in a later version.
