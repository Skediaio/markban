import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

export const here = dirname(fileURLToPath(import.meta.url));
/** The repository root. */
export const repo = join(here, '../../..');
export const demoBoard = join(repo, 'examples/demo-board');
export const hostileBoard = join(repo, 'packages/remark-markban/test/fixtures/hostile-board');
export const messyBoard = join(repo, 'packages/core/test/fixtures/messy-board');
