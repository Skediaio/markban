# @markban/core

The markban engine (parse, view, check) compiled to WebAssembly.

```js
import { loadBoard, openBoard } from '@markban/core'; // Node >= 20
loadBoard('examples/demo-board'); // what `markban ls --json` prints
```

```js
import { init, parseBoard, card, check, search } from '@markban/core'; // browsers and bundlers
await init();
const view = parseBoard([{ path: 'board.md', content }, { path: 'cards/a.md', content: a }]);
search([{ path: 'board.md', content }], ['roadmap', 'sync']); // hits, AND over the terms
```

Build the wasm first with `npm run build` at the repository root. With Vite, exclude the package from
dependency pre-bundling in dev (`optimizeDeps: { exclude: ['@markban/core'] }`) so the `.wasm` URL resolves.
