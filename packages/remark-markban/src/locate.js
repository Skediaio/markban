/**
 * Resolving a board path against a boundary directory, refusing anything outside it (also through
 * symbolic links). Shared by remark-markban and the `@markban/svelte` Vite plugin.
 * @module
 */
import { realpathSync, statSync } from 'node:fs';
import { isAbsolute, relative, resolve, sep } from 'node:path';

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
export function locate(given, { base, boundary }) {
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
