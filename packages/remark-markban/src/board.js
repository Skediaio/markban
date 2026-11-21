/**
 * Renders a loaded board to static, accessible HTML. Class names are the stable styling API; see
 * `style.css`.
 * @module
 */
import { esc, renderMarkdown } from './html.js';

/** @typedef {import('@markban/core').BoardView} BoardView */
/** @typedef {import('@markban/core').Diagnostic} Diagnostic */
/** @typedef {import('./html.js').Node} Node */

/**
 * @typedef {object} Source
 * @property {BoardView} view
 * @property {(id: string) => import('@markban/core').CardDetail | null} card
 * @property {(markdown: string) => Node} parse The processor's own markdown parser.
 * @property {number} headingLevel Level of the board title; columns use the next one.
 */

/**
 * A notice box. `lines` are plain text.
 * @param {'error'|'warning'} severity
 * @param {string} title
 * @param {string[]} [lines]
 * @returns {string}
 */
export function notice(severity, title, lines = []) {
  const list = lines.length > 0 ? `<ul>${lines.map((l) => `<li>${l}</li>`).join('')}</ul>` : '';
  return (
    `<aside class="markban-notice markban-notice-${severity}" role="note">` +
    `<p class="markban-notice-title">${esc(title)}</p>${list}</aside>`
  );
}

/**
 * @param {Diagnostic[]} diagnostics
 * @returns {string}
 */
function diagnosticsNotice(diagnostics) {
  if (diagnostics.length === 0) return '';
  const severity = diagnostics.some((d) => d.severity === 'error') ? 'error' : 'warning';
  const count = diagnostics.length;
  const lines = diagnostics.map((d) => {
    const where = d.line === null ? d.file : `${d.file}:${d.line}`;
    return `<code>${esc(where)}</code> ${esc(d.code)} ${esc(d.message)}`;
  });
  return notice(severity, `This board has ${count} ${count === 1 ? 'problem' : 'problems'}`, lines);
}

/**
 * @param {string} markdown
 * @param {Source} source
 * @returns {string}
 */
function markdownHtml(markdown, source) {
  try {
    return renderMarkdown(source.parse(markdown));
  } catch {
    return `<p>${esc(markdown)}</p>`;
  }
}

/**
 * @param {{ id: string, title: string }} card
 * @param {Source} source
 * @returns {string}
 */
function cardHtml(card, source) {
  const detail = source.card(card.id);
  const body = detail === null || detail.description.trim() === '' ? '' : markdownHtml(detail.description, source);
  const id = esc(card.id);
  if (body === '') {
    return `<li class="markban-card" data-card="${id}"><div class="markban-card-title">${esc(card.title)}</div></li>`;
  }
  return (
    `<li class="markban-card" data-card="${id}"><details class="markban-card-details">` +
    `<summary class="markban-card-title">${esc(card.title)}</summary>` +
    `<div class="markban-card-body">${body}</div></details></li>`
  );
}

/**
 * @param {{ classes: string, attrs: string, name: string, cards: { id: string, title: string }[] }} column
 * @param {Source} source
 * @param {number} level
 * @returns {string}
 */
function columnHtml(column, source, level) {
  const n = column.cards.length;
  const count = `<span class="markban-count" aria-label="${n} ${n === 1 ? 'card' : 'cards'}">${n}</span>`;
  const heading =
    `<h${level} class="markban-column-title"><span class="markban-column-name">${esc(column.name)}</span> ${count}</h${level}>`;
  const cards =
    n === 0
      ? '<p class="markban-empty">No cards</p>'
      : `<ul class="markban-cards">${column.cards.map((c) => cardHtml(c, source)).join('\n')}</ul>`;
  return `<section class="${column.classes}"${column.attrs} aria-label="${esc(column.name)}">\n${heading}\n${cards}\n</section>`;
}

/**
 * Whether the view is the empty one `parseBoard` returns for a missing or unusable board.md.
 * @param {BoardView} view
 * @returns {boolean}
 */
function isUnusable(view) {
  return (
    view.title === '' &&
    view.columns.length === 0 &&
    view.unsorted.length === 0 &&
    view.diagnostics.some((d) => d.severity === 'error')
  );
}

/**
 * @param {Source} source
 * @returns {string}
 */
export function renderBoard(source) {
  const { view, headingLevel } = source;
  const problems = diagnosticsNotice(view.diagnostics);
  if (isUnusable(view)) return problems;

  const title = view.title === '' ? 'Board' : view.title;
  const description =
    view.description === '' ? '' : `\n<div class="markban-description">${markdownHtml(view.description, source)}</div>`;
  const columns = view.columns.map((c) =>
    columnHtml(
      { classes: 'markban-column', attrs: ` data-column="${esc(c.id)}"`, name: c.name, cards: c.cards },
      source,
      headingLevel + 1,
    ),
  );
  if (view.unsorted.length > 0) {
    columns.push(
      columnHtml(
        { classes: 'markban-column markban-unsorted', attrs: '', name: 'Unsorted', cards: view.unsorted },
        source,
        headingLevel + 1,
      ),
    );
  }
  return (
    `<section class="markban-board" aria-label="${esc(`Board: ${title}`)}">\n` +
    `<header class="markban-header">\n<h${headingLevel} class="markban-title">${esc(title)}</h${headingLevel}>${description}\n</header>\n` +
    (problems === '' ? '' : `${problems}\n`) +
    `<div class="markban-columns">\n${columns.join('\n')}\n</div>\n</section>`
  );
}
