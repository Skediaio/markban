# @markban/svelte

A Svelte 5 component for a markban board and a Vite plugin that loads the board at build time.

```js
// vite.config.js
import { sveltekit } from '@sveltejs/kit/vite';
import { markban } from '@markban/svelte/vite';

export default { plugins: [markban(), sveltekit()] };
```

```svelte
<script>
  import { Markban } from '@markban/svelte';
  import board from 'markban:boards/roadmap'; // path relative to the Vite root
</script>

<Markban {board} />
```

## `<Markban {board} headingLevel={2} />`

- `board`: what the `markban:` import gives (`{ view, cards, descriptionHtml, files }`). A plain `BoardView` (from
  `@markban/core`) also works; its cards then open without a description, and the search box filters ids and titles
  only (no raw files for a full-text search).
- Renders the title, a notice for diagnostics, the columns with counts and an `Unsorted` column when a card has no
  valid column. Empty columns say so. Columns scroll and snap sideways on narrow screens. Light and dark follow
  `prefers-color-scheme`.
- A search box in the header runs the engine's full-text search (`@markban/core` `search`) over the board's files,
  debounced; while a query is active the columns are replaced by the hits — card buttons (with the column name and
  an `(archived)` marker), small lines for column and board hits, and the `N hit(s)` count. Escape or the × button
  clears the query and brings the board back. In the browser this loads the `@markban/core` wasm (`await init()`),
  so a bundler must be able to resolve its `.wasm` asset (with Vite: `optimizeDeps: { exclude: ['@markban/core'] }`).
- A card is a button. It opens a native modal `<dialog>` with the card title, its column and its description.
  Escape, the Close button and a click on the backdrop close it; focus returns to the card.
- Styling reuses `remark-markban/style.css` (imported through `@markban/svelte/style.css`, which the component loads
  itself). Retheme with the `--markban-*` custom properties.
- Server rendering gives the whole board as static HTML. The dialog needs JavaScript, so without it cards are inert.

## `markban()` Vite plugin (`@markban/svelte/vite`)

- `import board from 'markban:<path>'` resolves `<path>` against the Vite root (option `root` changes both the
  base and the boundary). A path that leaves the root, also through a symbolic link, fails the build with one line.
- The virtual module exports `{ view, cards, descriptionHtml, files }` as default and by name. `cards` maps a card
  id to `{ title, column, html }`: `column` is the column name, `html` the description as HTML (archived cards
  included). `files` are the board's raw files, for the component's full-text search. It is built with
  `loadBoard`/`openBoard` of `@markban/core`.
- `html` goes through the sanitiser of `remark-markban`: raw HTML dropped, only http, https, mailto, tel and relative
  links keep their target, images become their alt text, `{`, `}` and backticks are entities. The component only
  puts that string in the page, so card markdown is never turned into HTML in the browser.
- Dev: the plugin watches `board.md` and the files directly in `cards/` (not subdirectories) of every board that was
  imported, invalidates the virtual module and sends a full reload, so an agent's edit shows up live. A production
  build reads the board once.

Boards with problems are not build errors: the diagnostics are part of `view` and the component shows them.

Example: [`examples/sveltekit-mdsvex`](../../examples/sveltekit-mdsvex).
