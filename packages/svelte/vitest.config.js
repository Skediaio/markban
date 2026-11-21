import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { svelteTesting } from '@testing-library/svelte/vite';

export default defineConfig({
  test: {
    projects: [
      {
        // Components compiled for the browser, run in a simulated DOM.
        plugins: [svelte(), svelteTesting()],
        test: {
          name: 'dom',
          include: ['test/dom/**/*.test.js'],
          environment: 'happy-dom',
          globalSetup: ['./test/boards.setup.js'],
        },
      },
      {
        // Server rendering and the Vite plugin, in plain Node.
        plugins: [svelte()],
        test: { name: 'node', include: ['test/node/**/*.test.js'], environment: 'node' },
      },
    ],
  },
});
