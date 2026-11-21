/**
 * Shared JSDoc types of `@markban/core`. This file has no runtime code.
 * @module
 */

/**
 * One file of a board, with its UTF-8 text. `path` is relative to the board directory with `/`
 * separators: `board.md` or `cards/<id>.md`. Below `cards/`, a path with a further `/` (such as
 * `cards/sub/` or `cards/sub/x.md`) stands for a subdirectory, which is reported as not a card.
 * @typedef {object} BoardFile
 * @property {string} path
 * @property {string} content
 */

/**
 * @typedef {object} Diagnostic
 * @property {'E001'|'E002'|'E003'|'E004'|'E005'|'E006'|'E007'|'E008'|'E009'|'W001'|'W002'} code
 * @property {'error'|'warning'} severity
 * @property {string} file Relative to the board directory.
 * @property {number|null} line 1-based, or null.
 * @property {string} message
 */

/**
 * @typedef {object} CardSummary
 * @property {string} id
 * @property {string} title
 */

/**
 * @typedef {object} ColumnView
 * @property {string} id
 * @property {string} name
 * @property {CardSummary[]} cards In board order.
 */

/**
 * What `markban ls --json` prints.
 * @typedef {object} BoardView
 * @property {string} title
 * @property {string} description
 * @property {ColumnView[]} columns
 * @property {CardSummary[]} unsorted Cards whose column is missing or unknown, sorted by id.
 * @property {Diagnostic[]} diagnostics
 */

/**
 * What `markban show --json` prints.
 * @typedef {object} CardDetail
 * @property {string} id
 * @property {string} title
 * @property {string|null} column
 * @property {string} description Markdown after the frontmatter.
 */

export {};
