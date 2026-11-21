/**
 * `@markban/core` for Node 20 or newer. The wasm module is loaded synchronously when this file is
 * imported, so `init()` is optional (it exists for API parity with the browser entry point).
 * @module
 */
import { readFileSync } from 'node:fs';
import { initSync } from '../wasm/markban_wasm.js';
import { markReady } from './api.js';

initSync({ module: readFileSync(new URL('../wasm/markban_wasm_bg.wasm', import.meta.url)) });
markReady();

export { parseBoard, card, check } from './api.js';
export { loadBoard, openBoard, readBoardFiles } from './load.js';

/**
 * Does nothing: the wasm module is already loaded in Node.
 * @returns {Promise<void>}
 */
export function init() {
  return Promise.resolve();
}
