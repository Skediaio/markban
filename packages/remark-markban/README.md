# remark-markban

A remark plugin: a fenced code block with the language `markban` whose content is the path of a board
directory becomes the board as static, accessible HTML (title, columns with counts, cards; each card's
description sits in a `<details>`).

````md
```markban
examples/demo-board
```
````

```js
import { unified } from 'unified';
import remarkParse from 'remark-parse';
import remarkRehype from 'remark-rehype';
import rehypeStringify from 'rehype-stringify';
import remarkMarkban from 'remark-markban';

const file = await unified()
  .use(remarkParse)
  .use(remarkMarkban, { root: process.cwd() })
  .use(remarkRehype, { allowDangerousHtml: true }) // the board is one raw html node
  .use(rehypeStringify, { allowDangerousHtml: true })
  .process({ path: 'docs/page.md', value: markdown });
```

With mdsvex 0.12, add it to `remarkPlugins`; nothing else is needed.

- Options: `root` (boards must live inside it; relative paths resolve against it; default: paths resolve
  against the file's directory and must stay inside the working directory) and `headingLevel` (default 2).
- Card and board text is escaped and filtered: raw HTML is dropped, only http, https, mailto, tel and
  relative links keep their target, images become their alt text, and `{`, `}` and backticks are written as
  entities so Svelte sees no expressions.
- Problems (missing path, a path outside `root`, board diagnostics) render as a `markban-notice`, never as a
  build error.
- Optional styling: `import 'remark-markban/style.css'`. Class names: `markban-board`, `markban-column`,
  `markban-card`, `markban-card-details`, `markban-count`, `markban-notice`, ...

Internal building blocks, also used by `@markban/svelte`: `remark-markban/html` (the sanitiser, `renderMarkdown` on an
mdast tree) and `remark-markban/locate` (resolve a board path inside a root, refusing escapes and symlinks that leave it).
