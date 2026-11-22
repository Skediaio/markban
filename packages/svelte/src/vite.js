/**
 * The `markban()` Vite plugin: `import board from 'markban:boards/roadmap'` loads a board
 * directory at build time. The module exports `{ view, cards, descriptionHtml, files }` (see
 * `build.js`) as default and as named exports. In dev, an edit of `board.md` or of a file
 * directly in `cards/` reloads the page, so an agent's change shows up live.
 * @module
 */
import { join, dirname, resolve } from 'node:path';
import { locate } from 'remark-markban/locate';
import { buildBoardModule } from './build.js';

const PREFIX = 'markban:';
const VIRTUAL = '\0markban:';

/**
 * @typedef {object} Options
 * @property {string} [root] Directory that boards must live in and that board paths are relative
 *   to. Default: the Vite root.
 */

/**
 * @param {Options} [options]
 * @returns {import('vite').Plugin}
 */
export function markban(options = {}) {
  let viteRoot = process.cwd();
  /** @type {Set<string>} Board directories handed out so far (real paths). */
  const boards = new Set();
  /** @type {import('vite').ViteDevServer|undefined} */
  let devServer;

  /** Makes the dev server watch a board; boards outside the Vite root are not watched by default. @param {string} dir */
  const watch = (dir) => devServer?.watcher.add([join(dir, 'board.md'), join(dir, 'cards')]);

  const boundary = () => (options.root === undefined ? viteRoot : resolve(viteRoot, options.root));

  /**
   * @param {string} file
   * @returns {string|null} The board directory the file belongs to, or `null`.
   */
  const boardOf = (file) => {
    for (const dir of boards) {
      if (file === join(dir, 'board.md') || dirname(file) === join(dir, 'cards')) return dir;
    }
    return null;
  };

  return {
    name: 'markban',
    enforce: 'pre',

    configResolved(config) {
      viteRoot = config.root;
    },

    resolveId(source) {
      if (!source.startsWith(PREFIX)) return null;
      const given = source.slice(PREFIX.length).trim();
      const base = boundary();
      const found = locate(given, { base, boundary: base });
      if ('problem' in found) {
        return this.error(`markban: ${found.problem}`);
      }
      boards.add(found.dir);
      watch(found.dir);
      return VIRTUAL + found.dir;
    },

    load(id) {
      if (!id.startsWith(VIRTUAL)) return null;
      const dir = id.slice(VIRTUAL.length);
      let data;
      try {
        data = buildBoardModule(dir);
      } catch (error) {
        return this.error(`markban: cannot read the board in '${dir}': ${error instanceof Error ? error.message : error}`);
      }
      this.addWatchFile(join(dir, 'board.md'));
      for (const { id: cardId } of [...data.view.columns.flatMap((c) => c.cards), ...data.view.unsorted]) {
        this.addWatchFile(join(dir, 'cards', `${cardId}.md`));
      }
      const json = JSON.stringify(data);
      return `const board = ${json};\nexport default board;\nexport const { view, cards, descriptionHtml, files } = board;\n`;
    },

    configureServer(server) {
      devServer = server;
      /** @param {string} file */
      const onChange = (file) => {
        const dir = boardOf(resolve(file));
        if (dir === null) return;
        const id = VIRTUAL + dir;
        for (const environment of Object.values(server.environments)) {
          const mod = environment.moduleGraph.getModuleById(id);
          if (mod !== undefined) environment.moduleGraph.invalidateModule(mod);
        }
        server.ws.send({ type: 'full-reload' });
      };
      for (const event of ['add', 'change', 'unlink']) server.watcher.on(event, onChange);
      for (const dir of boards) watch(dir);
    },
  };
}

export default markban;
