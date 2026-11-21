# Addendum to RULES-A: archived cards
- Cards that were removed from the board but kept for reference live in `archive/<id>.md` (same format as `cards/<id>.md`).
  They have no line in `board.md`.
- Restore an archived card: move the file back to `cards/<id>.md` and add its line `- [Title](cards/<id>.md)` to the wanted column.
- Archive a card: move `cards/<id>.md` to `archive/<id>.md` and delete its line from `board.md`.
