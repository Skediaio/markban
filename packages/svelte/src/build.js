/**
 * Loads a board directory into the data `<Markban>` renders: the board view plus, for every card,
 * its description as sanitised HTML. Runs at build time in Node; the browser never turns card
 * markdown into HTML. The sanitiser is the one of remark-markban (`remark-markban/html`).
 * @module
 */
import { openBoard } from '@markban/core';
import { esc, renderMarkdown } from 'remark-markban/html';
import remarkGfm from 'remark-gfm';
import remarkParse from 'remark-parse';
import { unified } from 'unified';

/** @typedef {import('@markban/core').BoardView} BoardView */

/**
 * @typedef {object} CardData
 * @property {string} title
 * @property {string} column Name of the column the card is in, `Unsorted` for a card without one.
 * @property {string} html Sanitised HTML of the description, `''` when there is none.
 */

/**
 * What `import board from 'markban:...'` gives.
 * @typedef {object} BoardData
 * @property {BoardView} view
 * @property {Record<string, CardData>} cards By card id.
 * @property {string} descriptionHtml Sanitised HTML of the board description.
 * @property {import('@markban/core').BoardFile[]} files The board's raw files, so the component
 *   can run the full-text `search` in the browser.
 */

const processor = unified().use(remarkParse).use(remarkGfm);

/**
 * @param {string} markdown
 * @returns {string}
 */
function toHtml(markdown) {
  if (markdown.trim() === '') return '';
  try {
    return renderMarkdown(/** @type {any} */ (processor.parse(markdown)));
  } catch {
    return `<p>${esc(markdown)}</p>`;
  }
}

/**
 * @param {string} dir A board directory.
 * @returns {BoardData}
 * @throws {Error} `dir` is not a directory.
 */
export function buildBoardModule(dir) {
  const { view, card, files } = openBoard(dir);
  /** @type {Record<string, CardData>} */
  const cards = {};
  /**
   * @param {string} column
   * @param {{ id: string, title: string }[]} list
   */
  const add = (column, list) => {
    for (const { id, title } of list) {
      cards[id] = { title, column, html: toHtml(card(id)?.description ?? '') };
    }
  };
  for (const column of view.columns) add(column.name, column.cards);
  add('Unsorted', view.unsorted);
  // Archived cards sit in no column, but search can surface them; their column is their own
  // frontmatter's, resolved to a name.
  const nameOf = (/** @type {string | null | undefined} */ id) =>
    id == null ? 'Unsorted' : view.columns.find((c) => c.id === id)?.name ?? 'Unsorted';
  for (const { id } of view.archived) {
    const detail = card(id);
    if (detail) {
      cards[id] = { title: detail.title, column: nameOf(detail.column), html: toHtml(detail.description) };
    }
  }
  return { view, cards, descriptionHtml: toHtml(view.description), files };
}
