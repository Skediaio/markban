# SvelteKit + mdsvex example

A minimal SvelteKit 3 app (Svelte 5, plain JavaScript with JSDoc, `adapter-static`, everything prerendered) that
shows a markban board two ways. It uses the workspace packages of this repository.

| Route | How | What it shows |
|---|---|---|
| `/` | `src/routes/+page.md`, mdsvex with `remark-markban` | A ```` ```markban ```` block that becomes static HTML, no JavaScript needed. |
| `/svelte` | `src/routes/svelte/+page.svelte`, `import board from 'markban:demo-board'` | `<Markban {board} />`, cards open in an accessible dialog. |

Both read `examples/demo-board`. Boards are looked up in `examples/` (the `root` option of both plugins, see
`vite.config.js`; Kit 3 takes its configuration there).

From the repository root:

```sh
npm ci
npm run build                                # the wasm engine (needs Rust and wasm-bindgen)
npm run build -w examples/sveltekit-mdsvex   # output in examples/sveltekit-mdsvex/build
npm run dev -w examples/sveltekit-mdsvex     # edit examples/demo-board and the page reloads
```

This directory is an app, not a board, so `cargo xtask ci` does not run `markban check` on it.
