/**
 * The component's search calls `await init()` on `@markban/core`, which would load the `.wasm`
 * from a URL — something the simulated DOM cannot fetch. Loading it here first, from the bytes
 * of the file in the workspace's node_modules, makes that `init()` a no-op.
 */
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
import { init } from '@markban/core';

// The simulated DOM gives modules an http:// import.meta.url, which node's resolver refuses;
// any real file of this package works as the base instead.
const require = createRequire(pathToFileURL(join(process.cwd(), 'package.json')));
const manifest = require.resolve('@markban/core/package.json');
await init(readFileSync(join(dirname(manifest), 'wasm', 'markban_wasm_bg.wasm')));
