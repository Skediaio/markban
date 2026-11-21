import { fileURLToPath } from 'node:url';
import adapter from '@sveltejs/adapter-static';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';
import { mdsvex } from 'mdsvex';
import remarkMarkban from 'remark-markban';

/** Boards may live anywhere below `examples/`; both plugins refuse paths outside of it. */
export const boardsRoot = fileURLToPath(new URL('..', import.meta.url));

/** @type {import('@sveltejs/kit').Config} */
const config = {
  extensions: ['.svelte', '.md'],
  preprocess: [
    vitePreprocess(),
    mdsvex({ extensions: ['.md'], remarkPlugins: [[remarkMarkban, { root: boardsRoot }]] }),
  ],
  kit: {
    adapter: adapter({ pages: 'build', assets: 'build' }),
  },
};

export default config;
