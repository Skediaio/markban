/**
 * Reading a board directory with `node:fs`. Mirrors `markban_core::fs::load`: symbolic links are
 * never followed (a link in `cards/` is reported as W001, a `board.md` or `cards/` that is a link
 * counts as missing), a card that cannot be read is E009, other entries of `cards/` are ignored
 * (W001) and `.markban.lock` is not part of the board.
 * @module
 */
import { constants, closeSync, openSync, readFileSync, readdirSync, statSync, lstatSync } from 'node:fs';
import { join } from 'node:path';
import { parseBoard, card } from './api.js';

/** @typedef {import('./types.js').BoardFile} BoardFile */
/** @typedef {import('./types.js').BoardView} BoardView */
/** @typedef {import('./types.js').CardDetail} CardDetail */
/** @typedef {import('./types.js').Diagnostic} Diagnostic */

const decoder = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true });

/**
 * @param {Diagnostic['code']} code
 * @param {string} file
 * @param {string} message
 * @returns {Diagnostic}
 */
function diagnostic(code, file, message) {
  return { code, severity: code.startsWith('W') ? 'warning' : 'error', file, line: null, message };
}

/**
 * @param {string} file
 * @returns {Diagnostic}
 */
function symlinkDiagnostic(file) {
  return diagnostic(
    'W001',
    file,
    'symbolic link ignored; markban never follows links, replace it with a regular file',
  );
}

/**
 * Reads one regular file without following a symbolic link at the last path component.
 * @param {string} path
 * @returns {string|null} The text, or `null` when the file vanished or is a link.
 * @throws {Error & {code?: string}} Anything else, including `ERR_ENCODING_INVALID_ENCODED_DATA`.
 */
function readText(path) {
  let fd;
  try {
    fd = openSync(path, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
  } catch (error) {
    const code = /** @type {{code?: string}} */ (error).code;
    if (code === 'ENOENT' || code === 'ELOOP') return null;
    throw error;
  }
  try {
    return decoder.decode(readFileSync(fd));
  } finally {
    closeSync(fd);
  }
}

/**
 * @param {Diagnostic} a
 * @param {Diagnostic} b
 * @returns {number}
 */
function compareDiagnostics(a, b) {
  const text = (/** @type {string} */ x, /** @type {string} */ y) => (x < y ? -1 : x > y ? 1 : 0);
  return (
    text(a.file, b.file) ||
    (a.line ?? 0) - (b.line ?? 0) ||
    text(a.code, b.code) ||
    text(a.message, b.message)
  );
}

/**
 * Reads `board.md` and `cards/*` of a board directory.
 * @param {string} dir
 * @returns {{ files: BoardFile[], diagnostics: Diagnostic[] }} The files for `parseBoard` and the
 *   diagnostics only the file system can know about (links, unreadable files).
 * @throws {Error} `dir` is not a directory, or an entry cannot be listed.
 */
export function readBoardFiles(dir) {
  if (!statSync(dir, { throwIfNoEntry: false })?.isDirectory()) {
    throw new Error(`'${dir}' is not a directory; pass the board directory that contains board.md`);
  }
  /** @type {BoardFile[]} */
  const files = [];
  /** @type {Diagnostic[]} */
  const diagnostics = [];

  const boardPath = join(dir, 'board.md');
  const boardStat = lstatSync(boardPath, { throwIfNoEntry: false });
  if (boardStat?.isSymbolicLink()) {
    diagnostics.push(symlinkDiagnostic('board.md'));
  } else if (boardStat?.isFile()) {
    try {
      const content = readText(boardPath);
      if (content !== null) files.push({ path: 'board.md', content });
    } catch (error) {
      diagnostics.push(unreadable('board.md', error));
    }
  }

  const cardsDir = join(dir, 'cards');
  const cardsStat = lstatSync(cardsDir, { throwIfNoEntry: false });
  if (cardsStat?.isSymbolicLink()) {
    diagnostics.push(symlinkDiagnostic('cards'));
  } else if (cardsStat?.isDirectory()) {
    const entries = readdirSync(cardsDir, { withFileTypes: true }).sort((a, b) =>
      a.name < b.name ? -1 : a.name > b.name ? 1 : 0,
    );
    for (const entry of entries) {
      const name = entry.name;
      if (entry.isSymbolicLink()) {
        diagnostics.push(symlinkDiagnostic(`cards/${name}`));
      } else if (name.endsWith('.md') && entry.isFile()) {
        try {
          const content = readText(join(cardsDir, name));
          if (content !== null) files.push({ path: `cards/${name}`, content });
        } catch (error) {
          diagnostics.push(unreadable(`cards/${name}`, error));
        }
      } else if (entry.isDirectory()) {
        files.push({ path: `cards/${name}/`, content: '' });
      } else {
        files.push({ path: `cards/${name}`, content: '' });
      }
    }
  }
  return { files, diagnostics };
}

/**
 * @param {string} file
 * @param {unknown} error
 * @returns {Diagnostic}
 */
function unreadable(file, error) {
  const code = /** @type {{code?: string}} */ (error).code;
  if (code === 'ERR_ENCODING_INVALID_ENCODED_DATA') {
    return diagnostic('E009', file, 'file is not valid UTF-8; re-save it as UTF-8');
  }
  const message = error instanceof Error ? error.message : String(error);
  return diagnostic('E009', file, `cannot read file: ${message}`);
}

/**
 * Loads a board directory once and gives access to its view, its cards and its raw files (for
 * `search`).
 * @param {string} dir
 * @returns {{ view: BoardView, card: (id: string) => CardDetail | null, files: BoardFile[] }}
 * @throws {Error} `dir` is not a directory.
 */
export function openBoard(dir) {
  const { files, diagnostics } = readBoardFiles(dir);
  const view = parseBoard(files);
  view.diagnostics = [...view.diagnostics, ...diagnostics].sort(compareDiagnostics);
  return { view, card: (id) => card(files, id), files };
}

/**
 * Loads a board directory and returns what `markban ls --json` prints for it. Problems in the
 * board are diagnostics in the result, never exceptions.
 * @param {string} dir
 * @returns {BoardView}
 * @throws {Error} `dir` is not a directory.
 */
export function loadBoard(dir) {
  return openBoard(dir).view;
}
