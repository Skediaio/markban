# markban format C (single file) - quick reference
Board dir: one `board.md` containing everything.
- id = slug of the title: lowercase, runs of non-alphanumerics become `-`. "Fix login timeout" -> `fix-login-timeout`.
- Layout: frontmatter (`markban: 0.1`), `# Board title`, `## Column` sections, and inside each column one block per card:
  ```
  ### Card title
  <!-- id: card-title -->

  Description lines...
  ```
  The id comment sits directly under the `###` heading. Block order = card order (top of section = top of column).
- Read: `board.md` (all of it, or grep the `## ` / `### ` headings for the structure).
- Move to another column: cut the whole block (heading, id comment, description) and paste it into the target
  section (top = directly under the `## ` heading, end = before the next `## ` heading or end of file).
- Reorder inside a column: cut and paste the block.
- Add: write a new block at the end of the column section.
- Rename column: edit its `## ` heading.
- Remove: delete the whole block.
- Edit description: edit the lines under the card's id comment, before the next `###`/`##` heading.
- Invariants: every card has an id comment; ids unique in the file; description lines never start with `#`;
  never rename ids; preserve everything else.
