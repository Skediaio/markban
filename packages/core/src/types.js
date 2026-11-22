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
 * @property {boolean} [archived] Present (true) only on archived cards of a view that includes them.
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
 * @property {CardSummary[]} archived Archived cards, sorted by id; never in `columns` or `unsorted`.
 * @property {Diagnostic[]} diagnostics
 */

/**
 * What `markban show --json` prints.
 * @typedef {object} CardDetail
 * @property {string} id
 * @property {string} title
 * @property {string|null} column
 * @property {string} description Markdown after the frontmatter.
 * @property {boolean} [archived] Present, and `true`, only when the card is archived.
 */

/**
 * One matching description line of a card hit.
 * @typedef {object} Snippet
 * @property {number} line 1-based line within the description.
 * @property {string} text The line, trimmed and capped at 120 characters with a trailing `…`.
 */

/**
 * A card whose id, title or description contains every term.
 * @typedef {object} CardHit
 * @property {string} id
 * @property {string} title
 * @property {string|null} column The card's column id; `null` when its `column` key is absent,
 *   empty or not a column of the board.
 * @property {boolean} archived
 * @property {Snippet|null} snippet The first description line containing a term, when any term
 *   matches the description.
 */

/**
 * A column whose id and name contain every term.
 * @typedef {object} ColumnHit
 * @property {string} id
 * @property {string} name
 */

/**
 * The hits of one search.
 * @typedef {object} SearchResults
 * @property {CardHit[]} cards Matching cards: active ones first in `ls` order, then archived
 *   ones by id.
 * @property {ColumnHit[]} columns Matching columns, in board order.
 * @property {boolean} board Whether the board title and description contain every term.
 */

export {};
