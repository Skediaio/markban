import { fileURLToPath } from 'node:url';
import { build } from 'vite';
import { describe, expect, it } from 'vitest';

/**
 * The browser entry point must bundle with Vite and ship the .wasm as a hashed asset; the
 * `--target web` output locates it with `new URL(..., import.meta.url)`, which Vite understands.
 */
describe('Vite', () => {
  it('bundles the browser entry and emits the wasm asset', async () => {
    const root = fileURLToPath(new URL('./vite-entry/', import.meta.url));
    const result = await build({
      root,
      logLevel: 'silent',
      configFile: false,
      build: {
        write: false,
        target: 'esnext',
        rollupOptions: { input: 'main.js' },
      },
    });
    const outputs = (Array.isArray(result) ? result : [result]).flatMap((r) =>
      'output' in r ? r.output : [],
    );
    const names = outputs.map((o) => o.fileName);
    expect(names.some((n) => n.endsWith('.wasm'))).toBe(true);
    const entry = outputs.find((o) => o.type === 'chunk' && o.isEntry);
    expect(entry && 'code' in entry ? entry.code : '').not.toContain('node:fs');
  });
});
