/**
 * remark-markban: replaces a fenced `markban` code block, whose content is the path of a board
 * directory, with the board as static, accessible HTML.
 *
 * The plugin emits one raw `html` node per board, so the pipeline must let raw HTML through:
 * `remark-rehype` and `rehype-stringify` with `allowDangerousHtml: true` (mdsvex does this itself).
 * Everything that comes from the board is escaped or filtered before it gets there; see `html.js`.
 * @module
 */
import { realpathSync, statSync } from 'node:fs';
import { dirname, isAbsolute, relative, resolve, sep } from 'node:path';
import { openBoard } from '@markban/core';
import { notice, renderBoard } from './board.js';

/**
 * @typedef {object} Options
 * @property {string} [root] Directory that boards must live in, and that relative board paths
 *   are resolved against. Without it, paths are resolved against the directory of the processed
 *   file and must stay inside the current working directory.
 * @property {number} [headingLevel] Level (1 to 5) of the board title heading; the column titles
 *   use the next one. Default 2.
 */

/** @typedef {import('./html.js').Node} Node */

const LANG = 'markban';

/**
 * @param {string} base
 * @param {string} path
 * @returns {boolean} Whether `path` is `base` or below it.
 */
function isInside(base, path) {
  const rel = relative(base, path);
  return rel === '' || (rel !== '..' && !rel.startsWith(`..${sep}`) && !isAbsolute(rel));
}

/**
 * Finds the board directory for a `given` path, refusing anything outside the boundary.
 * @param {string} given
 * @param {{ base: string, boundary: string }} where
 * @returns {{ dir: string } | { problem: string }}
 */
function locate(given, { base, boundary }) {
  const outside = {
    problem: `board path '${given}' is outside the allowed root; use a path inside it or set the \`root\` option`,
  };
  const wanted = resolve(base, given);
  if (!isInside(boundary, wanted)) return outside;
  let real;
  try {
    real = realpathSync(wanted);
  } catch {
    return { problem: `board '${given}' is not a directory; use the path of a directory that contains board.md` };
  }
  let realBoundary = boundary;
  try {
    realBoundary = realpathSync(boundary);
  } catch {
    // A boundary that does not exist cannot contain anything.
    return outside;
  }
  if (!isInside(realBoundary, real)) return outside;
  if (!statSync(real, { throwIfNoEntry: false })?.isDirectory()) {
    return { problem: `board '${given}' is not a directory; use the path of a directory that contains board.md` };
  }
  return { dir: real };
}

/**
 * The directory of the processed file. vfile has `dirname`; mdsvex 0.12 builds its files with a
 * non-standard `filename` property instead, so that is read too.
 * @param {any} file
 * @returns {string|undefined}
 */
function fileDirectory(file) {
  if (typeof file?.dirname === 'string' && file.dirname !== '') return file.dirname;
  if (typeof file?.path === 'string' && file.path !== '') return dirname(file.path);
  if (typeof file?.filename === 'string' && file.filename !== '') return dirname(file.filename);
  return undefined;
}

/**
 * @param {Node} tree
 * @param {(node: Node) => Node | null} replace Returns the node to put in place of `node`, or
 *   `null` to keep it.
 */
function replaceIn(tree, replace) {
  const kids = tree.children;
  if (!Array.isArray(kids)) return;
  for (let i = 0; i < kids.length; i += 1) {
    const node = /** @type {Node} */ (kids[i]);
    const replacement = replace(node);
    if (replacement === null) replaceIn(node, replace);
    else kids[i] = replacement;
  }
}

/**
 * @this {import('unified').Processor}
 * @param {Options} [options]
 * @returns {(tree: Node, file: import('vfile').VFile) => void}
 */
export default function remarkMarkban(options = {}) {
  const processor = this;
  const headingLevel = Number.isInteger(options.headingLevel)
    ? Math.min(5, Math.max(1, /** @type {number} */ (options.headingLevel)))
    : 2;
  const parse = (/** @type {string} */ markdown) => /** @type {Node} */ (/** @type {unknown} */ (processor.parse(markdown)));

  return function transformer(tree, file) {
    const cwd = resolve(file?.cwd || process.cwd());
    const boundary = options.root === undefined ? cwd : resolve(cwd, options.root);
    const dir = fileDirectory(file);
    const base = options.root === undefined && dir !== undefined ? resolve(cwd, dir) : boundary;

    replaceIn(tree, (node) => {
      if (node.type !== 'code' || String(node.lang ?? '').trim().toLowerCase() !== LANG) return null;
      return { type: 'html', value: render(String(node.value ?? ''), { base, boundary }, parse, headingLevel) };
    });
  };
}

/**
 * @param {string} content The text of the code block.
 * @param {{ base: string, boundary: string }} where
 * @param {(markdown: string) => Node} parse
 * @param {number} headingLevel
 * @returns {string}
 */
function render(content, where, parse, headingLevel) {
  const given = content.trim();
  if (given === '' || given.includes('\n')) {
    return notice('error', 'markban: the code block must contain one board directory path');
  }
  const found = locate(given, where);
  if ('problem' in found) return notice('error', `markban: ${found.problem}`);
  try {
    const { view, card } = openBoard(found.dir);
    return renderBoard({ view, card, parse, headingLevel });
  } catch {
    return notice('error', `markban: cannot read the board in '${given}'`);
  }
}
