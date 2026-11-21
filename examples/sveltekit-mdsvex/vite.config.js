import { fileURLToPath } from 'node:url';
import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';
import { markban } from '@markban/svelte/vite';
import { mdsvex } from 'mdsvex';
import remarkMarkban from 'remark-markban';
import { defineConfig } from 'vite';

/** Boards may live anywhere below `examples/`; both plugins refuse paths outside of it. */
const boardsRoot = fileURLToPath(new URL('..', import.meta.url));

export default defineConfig({
  plugins: [
    // `markban:demo-board` is looked up in `examples/`, next to this app.
    markban({ root: boardsRoot }),
    sveltekit({
      adapter: adapter({ pages: 'build', assets: 'build' }),
      extensions: ['.svelte', '.md'],
      preprocess: [
        vitePreprocess(),
        mdsvex({ extensions: ['.md'], remarkPlugins: [[remarkMarkban, { root: boardsRoot }]] }),
      ],
    }),
  ],
  // Boards live outside the app, but the dev server may serve and watch them.
  server: { fs: { allow: [boardsRoot] } },
});
