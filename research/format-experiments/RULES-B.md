# markban format B (frontmatter per card) - quick reference
Board dir: `board.md` (columns only, no cards) + `cards/<id>.md` (one file per card, carries its own position).
- id = slug of the title: lowercase, runs of non-alphanumerics become `-`. "Fix login timeout" -> `fix-login-timeout`.
- `board.md` frontmatter: `markban: 0.1`, `title`, `columns:` list of `{id: <slug>, name: <Name>}`.
- `cards/<id>.md`: frontmatter `title`, `column` (a column id), `order` (integer, sparse, step 1000), then the description.
- Read: a column = all cards with that `column`, sorted by `order` ascending (ties: by id).
  Tip: `grep -r '^column: doing' cards/` finds the cards of a column without reading them all.
- Move to another column: change `column` (and `order`) in that card's frontmatter only.
- Top of a column: `order` = lowest order in the column minus 1000 (negative or zero is fine).
- End of a column: `order` = highest order in the column plus 1000.
- Between two cards: any integer strictly between their orders (e.g. the midpoint); if no integer fits, renumber the column in steps of 1000.
- Add: create `cards/<id>.md` with frontmatter and description. No other file changes.
- Rename column: change `name` in `board.md`; keep the `id` (cards refer to it).
- Remove: delete `cards/<id>.md`.
- Edit description: edit the body of `cards/<id>.md`.
- Invariants: `column` must be an existing column id; never rename ids; preserve unknown keys and other content.
