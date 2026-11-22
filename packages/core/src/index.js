/**
 * `@markban/core` for browsers and bundlers (Vite, webpack, ...). Call `await init()` once before
 * `parseBoard`, `card`, `check` or `search`. The `.wasm` file is located relative to this package
 * with `new URL(..., import.meta.url)`, which bundlers turn into an asset.
 * @module
 */
import initWasm from '../wasm/markban_wasm.js';
import { markReady, isReady } from './api.js';

export { parseBoard, card, check, search } from './api.js';

/** @type {Promise<void>|null} */
let loading = null;

/**
 * Loads and instantiates the wasm module. Safe to call more than once.
 * @param {RequestInfo | URL | Response | BufferSource | WebAssembly.Module} [wasm] Where to get
 *   the module from; defaults to the file shipped with this package.
 * @returns {Promise<void>}
 */
export function init(wasm) {
  if (isReady()) return Promise.resolve();
  loading ??= initWasm(wasm === undefined ? undefined : { module_or_path: wasm }).then(
    () => markReady(),
    (error) => {
      loading = null;
      throw error;
    },
  );
  return loading;
}
