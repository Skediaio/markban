import { sveltekit } from '@sveltejs/kit/vite';
import { markban } from '@markban/svelte/vite';
import { defineConfig } from 'vite';
import { boardsRoot } from './svelte.config.js';

export default defineConfig({
  // `markban:demo-board` is looked up in `examples/`, next to this app.
  plugins: [markban({ root: boardsRoot }), sveltekit()],
  // Boards live outside the app, but the dev server may serve and watch them.
  server: { fs: { allow: [boardsRoot] } },
});
