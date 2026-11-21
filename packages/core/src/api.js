import * as wasm from '../wasm/markban_wasm.js';

/** @typedef {import('./types.js').BoardFile} BoardFile */
/** @typedef {import('./types.js').BoardView} BoardView */
/** @typedef {import('./types.js').CardDetail} CardDetail */
/** @typedef {import('./types.js').Diagnostic} Diagnostic */

let ready = false;

/** Marks the wasm module as instantiated. Internal: called by the entry points. */
export function markReady() {
  ready = true;
}

/** @returns {boolean} */
export function isReady() {
  return ready;
}

function assertReady() {
  if (!ready) {
    throw new Error('@markban/core: the wasm module is not loaded; call `await init()` first');
  }
}

/**
 * Parses a board held in memory. A board without a usable `board.md` gives an empty view that
 * carries the diagnostics. Never throws on board content.
 * @param {BoardFile[]} files
 * @returns {BoardView}
 */
export function parseBoard(files) {
  assertReady();
  return wasm.parseBoard(files);
}

/**
 * One card in full, or `null` when there is no such card.
 * @param {BoardFile[]} files
 * @param {string} id The file name without `.md`.
 * @returns {CardDetail|null}
 */
export function card(files, id) {
  assertReady();
  return wasm.card(files, id);
}

/**
 * All diagnostics of a board, sorted by file, line, code and message.
 * @param {BoardFile[]} files
 * @returns {Diagnostic[]}
 */
export function check(files) {
  assertReady();
  return wasm.check(files);
}
