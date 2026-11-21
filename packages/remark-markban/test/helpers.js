import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { unified } from 'unified';
import remarkParse from 'remark-parse';
import remarkRehype from 'remark-rehype';
import rehypeStringify from 'rehype-stringify';
import { compile } from 'mdsvex';
import remarkMarkban from '../src/index.js';

export const here = dirname(fileURLToPath(import.meta.url));
/** The repository root. */
export const repo = join(here, '../../..');

/**
 * Renders markdown with the current unified (v11) ecosystem.
 * @param {string} markdown
 * @param {import('../src/index.js').Options} [options]
 * @param {string} [path] The path of the markdown file, for relative board paths.
 * @returns {Promise<string>}
 */
export async function renderUnified(markdown, options, path = join(here, 'page.md')) {
  const file = await unified()
    .use(remarkParse)
    .use(remarkMarkban, options)
    .use(remarkRehype, { allowDangerousHtml: true })
    .use(rehypeStringify, { allowDangerousHtml: true })
    .process({ value: markdown, path });
  return String(file);
}

/**
 * Renders markdown through mdsvex 0.12.x (its bundled, older remark pipeline).
 * @param {string} markdown
 * @param {import('../src/index.js').Options} [options]
 * @param {string} [filename]
 * @returns {Promise<string>} The generated Svelte component source.
 */
export async function renderMdsvex(markdown, options, filename = join(here, 'page.svx')) {
  const result = await compile(markdown, {
    filename,
    remarkPlugins: [[remarkMarkban, options ?? {}]],
  });
  return result?.code ?? '';
}

/**
 * The board fragment of rendered output, or '' when there is none.
 * @param {string} html
 * @returns {string}
 */
export function boardFragment(html) {
  const start = html.indexOf('<section class="markban-board');
  const end = html.lastIndexOf('</section>');
  return start < 0 || end < 0 ? '' : html.slice(start, end + '</section>'.length);
}
