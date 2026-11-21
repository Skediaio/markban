/**
 * Turns the card and board markdown (an mdast tree, produced by the processor's own parser) into
 * HTML through an allowlist. Nothing the card author writes is trusted:
 *
 * - raw HTML nodes are dropped, every text value is escaped;
 * - links keep their target only for http, https, mailto, tel and relative URLs;
 * - images become their alt text;
 * - `{`, `}` and the backtick are written as entities, so Svelte (mdsvex) sees no expressions;
 * - node types outside the allowlist fall back to their escaped text.
 * @module
 */

/** @typedef {{ type: string, children?: Node[], value?: string, [key: string]: any }} Node */

const ENTITIES = /** @type {Record<string, string>} */ ({
  '&': '&amp;',
  '<': '&lt;',
  '>': '&gt;',
  '"': '&quot;',
  "'": '&#39;',
  '{': '&#123;',
  '}': '&#125;',
  '`': '&#96;',
});

/**
 * Escapes text for HTML text and attribute values, and for Svelte templates.
 * @param {unknown} text
 * @returns {string}
 */
export function esc(text) {
  return String(text).replace(/[&<>"'{}`]/g, (c) => ENTITIES[c] ?? c);
}

const SAFE_SCHEMES = new Set(['http', 'https', 'mailto', 'tel']);
const MAX_DEPTH = 32;

/**
 * The URL when it is safe to put in an `href`, else `null`. Browsers ignore whitespace and
 * control characters inside a scheme (`java\tscript:`), so those are removed before looking.
 * @param {unknown} url
 * @returns {string|null}
 */
export function safeUrl(url) {
  const text = String(url ?? '');
  const compact = text.replace(/[\u0000-\u0020\u007f-\u009f\u00ad\u200b-\u200f\u2028\u2029\ufeff]/g, '');
  const scheme = /^([a-z][a-z0-9+.-]*):/i.exec(compact);
  if (scheme === null) return text.trim();
  return SAFE_SCHEMES.has((scheme[1] ?? '').toLowerCase()) ? text.trim() : null;
}

/**
 * @typedef {object} Context
 * @property {Map<string, string>} definitions Link reference definitions by identifier.
 */

/**
 * @param {Node} tree
 * @param {Map<string, string>} out
 */
function collectDefinitions(tree, out) {
  if (tree.type === 'definition' && typeof tree.identifier === 'string') {
    if (!out.has(tree.identifier)) out.set(tree.identifier, String(tree.url ?? ''));
  }
  for (const child of tree.children ?? []) collectDefinitions(child, out);
}

/**
 * Renders a markdown tree to safe HTML.
 * @param {Node} tree
 * @returns {string}
 */
export function renderMarkdown(tree) {
  /** @type {Context} */
  const ctx = { definitions: new Map() };
  collectDefinitions(tree, ctx.definitions);
  return blocks(tree.children ?? [], ctx, 0);
}

/**
 * @param {Node[]} nodes
 * @param {Context} ctx
 * @param {number} depth
 * @returns {string}
 */
function blocks(nodes, ctx, depth) {
  return nodes.map((node) => block(node, ctx, depth + 1)).join('');
}

/**
 * @param {Node[]} nodes
 * @param {Context} ctx
 * @param {number} depth
 * @returns {string}
 */
function inlines(nodes, ctx, depth) {
  return nodes.map((node) => inline(node, ctx, depth + 1)).join('');
}

/**
 * @param {Node} node
 * @returns {string}
 */
function plainText(node) {
  if (typeof node.value === 'string') return node.value;
  if (node.type === 'image' || node.type === 'imageReference') return String(node.alt ?? '');
  return (node.children ?? []).map(plainText).join('');
}

/**
 * @param {Node} node
 * @param {Context} ctx
 * @param {number} depth
 * @returns {string}
 */
function block(node, ctx, depth) {
  if (depth > MAX_DEPTH) return '';
  const kids = node.children ?? [];
  switch (node.type) {
    case 'paragraph':
      return `<p>${inlines(kids, ctx, depth)}</p>`;
    case 'heading':
      return `<p class="markban-heading"><strong>${inlines(kids, ctx, depth)}</strong></p>`;
    case 'blockquote':
      return `<blockquote>${blocks(kids, ctx, depth)}</blockquote>`;
    case 'code':
      return `<pre><code>${esc(node.value ?? '')}</code></pre>`;
    case 'thematicBreak':
      return '<hr>';
    case 'list': {
      const tag = node.ordered ? 'ol' : 'ul';
      const start = node.ordered && Number.isInteger(node.start) && node.start !== 1 ? ` start="${node.start}"` : '';
      return `<${tag}${start}>${blocks(kids, ctx, depth)}</${tag}>`;
    }
    case 'listItem': {
      const task = node.checked === true ? '[x] ' : node.checked === false ? '[ ] ' : '';
      const body = kids
        .map((child) =>
          child.type === 'paragraph' && node.spread !== true
            ? inlines(child.children ?? [], ctx, depth + 1)
            : block(child, ctx, depth + 1),
        )
        .join('');
      return `<li>${task}${body}</li>`;
    }
    case 'table':
      return table(kids, ctx, depth);
    case 'html':
    case 'yaml':
    case 'toml':
    case 'definition':
    case 'footnoteDefinition':
      return '';
    default:
      if (kids.length > 0) return blocks(kids, ctx, depth);
      return typeof node.value === 'string' && node.value !== '' ? `<p>${esc(node.value)}</p>` : '';
  }
}

/**
 * @param {Node[]} rows
 * @param {Context} ctx
 * @param {number} depth
 * @returns {string}
 */
function table(rows, ctx, depth) {
  const row = (/** @type {Node} */ r, /** @type {string} */ cell) =>
    `<tr>${(r.children ?? []).map((c) => `<${cell}>${inlines(c.children ?? [], ctx, depth + 2)}</${cell}>`).join('')}</tr>`;
  const [head, ...body] = rows;
  if (head === undefined) return '';
  const bodyHtml = body.length > 0 ? `<tbody>${body.map((r) => row(r, 'td')).join('')}</tbody>` : '';
  return `<table><thead>${row(head, 'th')}</thead>${bodyHtml}</table>`;
}

/**
 * @param {string} href
 * @param {string} inner
 * @returns {string}
 */
function anchor(href, inner) {
  const url = safeUrl(href);
  if (url === null) return `<span class="markban-unsafe">${inner}</span>`;
  const external = /^[a-z][a-z0-9+.-]*:/i.test(url) || url.startsWith('//');
  const rel = external ? ' rel="nofollow noopener noreferrer"' : '';
  return `<a href="${esc(url)}"${rel}>${inner}</a>`;
}

/**
 * @param {Node} node
 * @param {Context} ctx
 * @param {number} depth
 * @returns {string}
 */
function inline(node, ctx, depth) {
  if (depth > MAX_DEPTH) return '';
  const kids = node.children ?? [];
  switch (node.type) {
    case 'text':
      return esc(node.value ?? '');
    case 'emphasis':
      return `<em>${inlines(kids, ctx, depth)}</em>`;
    case 'strong':
      return `<strong>${inlines(kids, ctx, depth)}</strong>`;
    case 'delete':
      return `<del>${inlines(kids, ctx, depth)}</del>`;
    case 'inlineCode':
      return `<code>${esc(node.value ?? '')}</code>`;
    case 'break':
      return '<br>';
    case 'link':
      return anchor(String(node.url ?? ''), inlines(kids, ctx, depth));
    case 'linkReference': {
      const url = ctx.definitions.get(String(node.identifier ?? ''));
      const inner = inlines(kids, ctx, depth);
      return url === undefined ? inner : anchor(url, inner);
    }
    case 'image':
    case 'imageReference':
      return `<span class="markban-image">${esc(plainText(node))}</span>`;
    case 'html':
    case 'footnoteReference':
      return '';
    default:
      if (kids.length > 0) return inlines(kids, ctx, depth);
      return typeof node.value === 'string' ? esc(node.value) : '';
  }
}
