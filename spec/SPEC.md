# Markban format — v0.1 (draft)

Markban is a plain-Markdown format for kanban boards. It has three goals:

1. **Agents work well with it**: plain files, one concern per file, no IDs to invent by hand.
2. **Code parses it deterministically**: a strict, small grammar, so no LLM is needed to read or write it.
3. **Humans can view and edit it** like a Trello board, in a UI or in any text editor.

## 1. Layout

A board is a directory:

```
<board>/
  board.md          # board metadata + column order
  cards/
    <id>.md         # one file per card
  archive/          # optional: archived cards, same format
```

One file per card keeps diffs small and avoids merge conflicts when several agents or people edit the same
board at once.

## 2. `board.md`

YAML frontmatter followed by an optional Markdown description.

```markdown
---
markban: 0.1
title: Demo board
columns:
  - id: backlog
    name: Backlog
  - id: doing
    name: Doing
    wip: 3
  - id: done
    name: Done
labels:
  - id: bug
    color: red
  - id: feature
    color: blue
---

Free-form description of the board.
```

| Field          | Required | Type   | Notes                                              |
| -------------- | -------- | ------ | -------------------------------------------------- |
| `markban`      | yes      | string | Format version.                                    |
| `title`        | yes      | string |                                                    |
| `columns`      | yes      | list   | Order in the list is the display order.            |
| `columns[].id` | yes      | slug   | `[a-z0-9-]+`, unique within the board.             |
| `columns[].wip`| no       | int    | Work-in-progress limit (advisory).                 |
| `labels`       | no       | list   | `id` slug + optional `color` name.                 |

## 3. Card files (`cards/<id>.md`)

The filename (without `.md`) is the card id: `[a-z0-9-]+`. Recommended form: `<number>-<slug>`, for example
`0007-login-page.md`. Tools allocate the next free number.

```markdown
---
title: Login page
column: doing
order: 2000
labels: [feature]
assignee: maarten
due: 2026-12-01
created: 2026-11-21
---

Description in free Markdown.

## Checklist

- [x] Design form
- [ ] Wire up API

## Comments

### 2026-11-21 maarten

First comment.
```

### Frontmatter

| Field      | Required | Type        | Notes                                                        |
| ---------- | -------- | ----------- | ------------------------------------------------------------ |
| `title`    | yes      | string      |                                                              |
| `column`   | yes      | slug        | Must match a `columns[].id` in `board.md`.                   |
| `order`    | no       | number      | Sort key within a column, ascending. Default: end of column. |
| `labels`   | no       | list[slug]  | Should match `labels[].id`.                                  |
| `assignee` | no       | string      |                                                              |
| `due`      | no       | ISO date    | `YYYY-MM-DD`.                                                |
| `created`  | no       | ISO date    |                                                              |

Unknown frontmatter keys **must be preserved** by tools (forward compatibility, custom fields).

`order` uses sparse numbers (steps of 1000), so moving a card only rewrites that one file: a card dropped
between 1000 and 2000 gets 1500.

### Body sections

The body is free Markdown. Two H2 sections have a defined meaning:

- `## Checklist`: GitHub task-list items (`- [ ]` / `- [x]`). Nested items are allowed.
- `## Comments`: each comment is an H3 `### <ISO date> <author>` followed by its text, oldest first.

Everything else is the description. Tools must round-trip unknown content byte-for-byte.

## 4. Rules for writers (agents and tools)

- Move a card: change `column` (and optionally `order`). Do not move files between directories.
- Archive a card: move the file to `archive/`.
- Never renumber or rename existing card ids.
- Keep frontmatter keys in the order shown above. Write lists inline (`[a, b]`).
- A card with an unknown `column` is invalid. Validators report it; viewers show it in an "Unsorted" column.

## Open questions

- Single-file mode (whole board in one `.md`, H2 = column, list item = card) for small boards?
- Card relations (`blocks`, `parent`)?
- Attachments: relative links to an `attachments/` folder?
