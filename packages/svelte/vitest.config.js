import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { svelteTesting } from '@testing-library/svelte/vite';
import { fileURLToPath } from 'node:url';

// The simulated DOM gives modules an http:// import.meta.url, under which @markban/core's Node
// entry cannot read its .wasm; the browser entry, pre-loaded by test/dom/setup.js, can.
const coreBrowser = fileURLToPath(new URL('../core/src/index.js', import.meta.url));

export default defineConfig({
  test: {
    // At the root so it resolves without the dom project's alias: build.js needs the Node
    // entry of @markban/core (openBoard), while the dom project's components get the browser
    // entry.
    globalSetup: ['./test/boards.setup.js'],
    projects: [
      {
        // Components compiled for the browser, run in a simulated DOM. The setup file loads
        // the wasm from its bytes, because the DOM cannot fetch the .wasm asset the
        // component's `await init()` would ask for. The alias is vite-style: @markban/core
        // must be the browser entry here, since the Node entry reads the .wasm at import
        // time with an import.meta.url that happy-dom does not give a file: scheme.
        plugins: [svelte(), svelteTesting()],
        resolve: { alias: { '@markban/core': coreBrowser } },
        test: {
          name: 'dom',
          include: ['test/dom/**/*.test.js'],
          environment: 'happy-dom',
          setupFiles: ['./test/dom/setup.js'],
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
