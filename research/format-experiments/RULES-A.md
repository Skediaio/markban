# markban format A (index) - quick reference
Board dir: `board.md` (index) + `cards/<id>.md` (description only, plain Markdown, no frontmatter).
- id = slug of the title: lowercase, runs of non-alphanumerics become `-`. "Fix login timeout" -> `fix-login-timeout`.
- `board.md`: frontmatter (`markban: 0.1`), `# Board title`, then one `## Column` section per column.
  Each card is one line `- [Title](cards/<id>.md)`. List order = card order (top of list = top of column).
- Read: `board.md` gives columns, cards and order; `cards/<id>.md` gives a description.
- Move to another column: delete the card's line, insert it in the target section (top = directly under the
  heading, end = after the last card line). The card file is untouched.
- Reorder inside a column: move the line.
- Add: create `cards/<id>.md` with the description AND add the line to the column.
- Rename column: edit its `## ` heading (nothing else refers to the name).
- Remove: delete the line AND delete `cards/<id>.md`.
- Edit description: edit `cards/<id>.md` only.
- Invariants: every line under a column is a card line; every card file has exactly one line in the index;
  never rename ids; preserve everything else byte for byte.
