# markban

A Markdown standard for kanban boards that agents can read and write easily, that plain code can parse
without an LLM, and that people can view and edit like a Trello board.

- **Format spec**: [`spec/SPEC.md`](spec/SPEC.md) (v0.1 draft)
- **Example board**: [`examples/demo-board`](examples/demo-board)

## Planned pieces

1. **Spec**: the format itself, versioned.
2. **Parser/serializer library**: Markdown ↔ typed board model, lossless round-trip.
3. **Validator CLI**: `markban check <board>` for CI and agent hooks.
4. **Board UI**: Trello-style view and editor (drag cards, edit, comment) that writes back to the files.

Status: idea and spec draft. No code yet.
