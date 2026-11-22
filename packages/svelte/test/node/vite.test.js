import { EventEmitter } from 'node:events';
import { cpSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createServer } from 'vite';
import { afterEach, describe, expect, test, vi } from 'vitest';
import { markban } from '../../src/vite.js';
import { repo } from '../helpers.js';

/** A plugin context double: `this.error` throws like Rollup's. */
const ctx = {
  error(message) {
    throw new Error(String(message));
  },
  addWatchFile() {},
};

/** @param {string} root @param {object} [options] */
function setup(root, options) {
  const plugin = markban(options);
  plugin.configResolved?.({ root });
  return plugin;
}

/** A temporary Vite root that contains a copy of the demo board at boards/roadmap. */
function tempRoot() {
  const root = mkdtempSync(join(tmpdir(), 'markban-svelte-'));
  cpSync(join(repo, 'examples/demo-board'), join(root, 'boards/roadmap'), { recursive: true });
  cleanups.push(() => rmSync(root, { recursive: true, force: true }));
  return root;
}
/** @type {(() => unknown)[]} */
const cleanups = [];
afterEach(async () => {
  for (const fn of cleanups.splice(0)) await fn();
});

describe('markban() Vite plugin', () => {
  test('resolves markban: ids to a virtual module and loads the board', async () => {
    const root = tempRoot();
    const plugin = setup(root);
    const id = await plugin.resolveId.call(ctx, 'markban:boards/roadmap', undefined, {});
    expect(id).toMatch(/^\0markban:/);
    const code = await plugin.load.call(ctx, id);
    expect(code).toContain('export default');
    expect(code).toMatch(/export const \{[^}]*\bfiles\b[^}]*\} = board/);
    const data = JSON.parse(code.split('\n')[0].replace(/^const board = /, '').replace(/;$/, ''));
    expect(data.view.title).toBe('Markban roadmap');
    expect(Object.values(data.cards)[0]).toHaveProperty('html');
  });

  test('ignores ids that are not markban:', async () => {
    const plugin = setup(tempRoot());
    expect(await plugin.resolveId.call(ctx, './x.js', undefined, {})).toBeNull();
  });

  test.each(['markban:../outside', 'markban:/etc', 'markban:boards/../../x'])('refuses %s', async (spec) => {
    const plugin = setup(tempRoot());
    await expect(async () => plugin.resolveId.call(ctx, spec, undefined, {})).rejects.toThrow(/outside the allowed root/);
  });

  test('refuses a symlink that leaves the root', async () => {
    const { symlinkSync } = await import('node:fs');
    const root = tempRoot();
    symlinkSync(join(repo, 'examples/demo-board'), join(root, 'boards/link'));
    const plugin = setup(root);
    await expect(async () => plugin.resolveId.call(ctx, 'markban:boards/link', undefined, {})).rejects.toThrow(
      /outside the allowed root/,
    );
  });

  test('says so when the path is not a board directory', async () => {
    const plugin = setup(tempRoot());
    await expect(async () => plugin.resolveId.call(ctx, 'markban:nope', undefined, {})).rejects.toThrow(/not a directory/);
  });

  test('the root option sets where boards are looked up', async () => {
    const plugin = setup('/nonexistent', { root: join(repo, 'examples') });
    const id = await plugin.resolveId.call(ctx, 'markban:demo-board', undefined, {});
    expect(id).toMatch(/demo-board$/);
  });

  test('a change to board.md or a card invalidates the module and reloads', async () => {
    const root = tempRoot();
    const plugin = setup(root);
    const id = await plugin.resolveId.call(ctx, 'markban:boards/roadmap', undefined, {});
    await plugin.load.call(ctx, id);

    const watcher = Object.assign(new EventEmitter(), { add: vi.fn() });
    const mod = { id };
    const invalidated = [];
    const graph = {
      getModuleById: (x) => (x === id ? mod : undefined),
      invalidateModule: (m) => invalidated.push(m),
    };
    const send = vi.fn();
    plugin.configureServer({ watcher, environments: { client: { moduleGraph: graph }, ssr: { moduleGraph: graph } }, ws: { send } });
    expect(watcher.add).toHaveBeenCalled();

    const dir = join(root, 'boards/roadmap');
    watcher.emit('change', join(dir, 'cards/0001-spec-draft.md'));
    expect(invalidated.length).toBe(2);
    expect(send).toHaveBeenCalledWith({ type: 'full-reload' });

    watcher.emit('change', join(dir, 'board.md'));
    expect(send).toHaveBeenCalledTimes(2);

    watcher.emit('add', join(dir, 'cards/9999-new.md'));
    expect(send).toHaveBeenCalledTimes(3);
  });

  test('changes elsewhere are ignored (cards/ is not recursive)', async () => {
    const root = tempRoot();
    const plugin = setup(root);
    const id = await plugin.resolveId.call(ctx, 'markban:boards/roadmap', undefined, {});
    await plugin.load.call(ctx, id);
    const watcher = Object.assign(new EventEmitter(), { add: vi.fn() });
    const send = vi.fn();
    const graph = { getModuleById: () => ({}), invalidateModule: vi.fn() };
    plugin.configureServer({ watcher, environments: { client: { moduleGraph: graph } }, ws: { send } });
    const dir = join(root, 'boards/roadmap');
    watcher.emit('change', join(dir, 'cards/sub/deep.md'));
    watcher.emit('change', join(dir, 'README.md'));
    watcher.emit('change', join(root, 'src/App.svelte'));
    expect(send).not.toHaveBeenCalled();
    expect(graph.invalidateModule).not.toHaveBeenCalled();
  });

  test('a real dev server serves the board and sees a card edit', async () => {
    const root = tempRoot();
    const server = await createServer({
      root,
      configFile: false,
      logLevel: 'silent',
      appType: 'custom',
      plugins: [markban()],
      server: { middlewareMode: true, hmr: false, watch: { usePolling: true, interval: 50 } },
    });
    cleanups.push(() => server.close());
    const first = await server.ssrLoadModule('markban:boards/roadmap');
    expect(first.default.view.title).toBe('Markban roadmap');
    expect(first.view).toBe(first.default.view);
    expect(first.files).toBe(first.default.files);
    expect(first.files.length).toBeGreaterThan(0);

    const sent = [];
    const original = server.ws.send.bind(server.ws);
    server.ws.send = (payload) => {
      sent.push(payload);
      return original(payload);
    };
    // The watcher's first scan may not be finished yet, so keep editing until it notices.
    let n = 0;
    await vi.waitFor(
      () => {
        n += 1;
        writeFileSync(
          join(root, 'boards/roadmap/cards/0001-spec-draft.md'),
          `---\ntitle: Renamed live\ncolumn: done\norder: 1000\n---\n\nbody ${n}\n`,
        );
        expect(sent.some((p) => p.type === 'full-reload')).toBe(true);
      },
      { timeout: 15000, interval: 300 },
    );
    const second = await server.ssrLoadModule('markban:boards/roadmap');
    expect(Object.values(second.default.cards).map((c) => c.title)).toContain('Renamed live');
  }, 30000);
});
