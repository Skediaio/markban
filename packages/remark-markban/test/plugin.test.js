import { mkdtempSync, mkdirSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';
import { boardFragment, here, renderMdsvex, renderUnified, repo } from './helpers.js';

const demo = '```markban\nexamples/demo-board\n```\n';
const hostile = '```markban\nfixtures/hostile-board\n```\n';
const root = { root: repo };

describe('demo board under unified v11', () => {
  it('renders the static board', async () => {
    const html = await renderUnified(demo, root);
    await expect(html + '\n').toMatchFileSnapshot('__snapshots__/demo.unified.html');
  });

  it('has the expected structure', async () => {
    const html = await renderUnified(demo, root);
    expect(html).toContain('<section class="markban-board"');
    expect(html).toContain('Markban roadmap');
    expect(html.match(/class="markban-column"/g)).toHaveLength(3);
    expect(html.match(/<details class="markban-card-details"/g)).toHaveLength(3);
    expect(html).toContain('<span class="markban-count"');
    expect(html).not.toContain('```');
  });
});

describe('demo board through mdsvex 0.12.x', () => {
  it('renders the same board fragment as unified', async () => {
    const svelte = await renderMdsvex(demo, root);
    const html = await renderUnified(demo, root);
    expect(boardFragment(svelte)).toBe(boardFragment(html));
    await expect(svelte + '\n').toMatchFileSnapshot('__snapshots__/demo.mdsvex.svelte');
  });

  it('resolves a relative path against the file given to compile()', async () => {
    const svelte = await renderMdsvex('```markban\nfixtures/hostile-board\n```\n');
    expect(svelte).toContain('markban-board');
  });
});

describe('sanitisation', () => {
  /** @type {Record<string, () => Promise<string>>} */
  const renderers = {
    unified: () => renderUnified(hostile),
    mdsvex: () => renderMdsvex(hostile),
  };

  for (const [name, render] of Object.entries(renderers)) {
    it(`never emits active content (${name})`, async () => {
      const out = boardFragment(await render());
      expect(out).toContain('markban-board');
      expect(out).not.toMatch(/<script/i);
      expect(out).not.toMatch(/<img/i);
      expect(out).not.toMatch(/<div onclick/i);
      expect(out).not.toMatch(/<iframe/i);
      expect(out).not.toMatch(/javascript:/i);
      expect(out).not.toMatch(/data:text/i);
      const openTags = [...out.matchAll(/<[a-z][^>]*>/gi)].map((m) => m[0]);
      expect(openTags.filter((t) => /\son[a-z]+\s*=/i.test(t))).toEqual([]);
      // Every `<` in the output opens a tag from the allowlist.
      const tags = [...out.matchAll(/<\/?([a-z0-9]+)/gi)].map((m) => m[1]);
      const allowed = new Set([
        'section', 'header', 'h2', 'h3', 'div', 'p', 'span', 'ul', 'ol', 'li', 'details', 'summary',
        'a', 'em', 'strong', 'del', 'code', 'pre', 'blockquote', 'br', 'hr', 'aside', 'table',
        'thead', 'tbody', 'tr', 'th', 'td',
      ]);
      expect(tags.filter((t) => !allowed.has(t))).toEqual([]);
    });

    it(`escapes braces so Svelte sees no expressions (${name})`, async () => {
      const out = boardFragment(await render());
      expect(out).not.toMatch(/[{}`]/);
      expect(out).toContain('&#123;alert(1)&#125;');
      expect(out).toContain('&#123;block&#125;');
    });

    it(`shows hostile text as text (${name})`, async () => {
      const out = boardFragment(await render());
      expect(out).toContain('&lt;img src=x onerror=alert(1)&gt;');
      // mdsvex's older remark-parse keeps the backslash of `\<`; either way it is escaped text.
      expect(out).toMatch(/\\?&lt;script&gt;alert\(3\)\\?&lt;\/script&gt;/);
      expect(out).toContain('&lt;b&gt;not bold&lt;/b&gt;');
    });

    it(`keeps safe links and drops unsafe ones (${name})`, async () => {
      const out = boardFragment(await render());
      expect(out).toContain('href="https://example.com"');
      expect(out).toContain('href="docs/page.md"');
      expect(out).toContain('href="mailto:a@b.c"');
      expect(out).toContain('>me</span>');
    });
  }
});

describe('diagnostics', () => {
  it('renders them as a notice', async () => {
    const html = await renderUnified(
      '```markban\npackages/core/test/fixtures/messy-board\n```\n',
      root,
    );
    expect(html).toContain('<aside class="markban-notice markban-notice-error"');
    expect(html).toContain('E007');
    expect(html).toContain('cards/0002-lost.md:3');
    expect(html).toContain('Messy board');
    expect(html).toContain('markban-unsorted');
  });
});

describe('failures are notices, not crashes', () => {
  it('missing board path', async () => {
    const html = await renderUnified('```markban\nfixtures/nope\n```\n');
    expect(html).toContain('class="markban-notice markban-notice-error"');
    expect(html).toContain('not a directory');
    expect(html).not.toContain('markban-board');
  });

  it('empty block', async () => {
    const html = await renderUnified('```markban\n\n```\n');
    expect(html).toContain('markban-notice');
  });

  it('a path escaping the root is refused', async () => {
    const html = await renderUnified('```markban\n../../etc\n```\n', { root: join(here, 'fixtures') });
    expect(html).toContain('markban-notice');
    expect(html).toContain('outside');
    expect(html).not.toContain('markban-board');
  });

  it('an absolute path outside the root is refused', async () => {
    const html = await renderUnified('```markban\n/etc\n```\n', { root: join(here, 'fixtures') });
    expect(html).toContain('outside');
  });

  it('refuses the same without a root option (the boundary is the working directory)', async () => {
    const html = await renderUnified('```markban\n../../../../../../../../etc\n```\n');
    expect(html).toContain('outside');
  });

  describe('symbolic link to outside', () => {
    const dir = mkdtempSync(join(tmpdir(), 'remark-markban-'));
    afterAll(() => rmSync(dir, { recursive: true, force: true }));

    it('is refused', async () => {
      const inside = join(dir, 'root');
      mkdirSync(inside);
      symlinkSync(join(here, 'fixtures/hostile-board'), join(inside, 'link'));
      writeFileSync(join(inside, 'x.md'), '');
      const html = await renderUnified('```markban\nlink\n```\n', { root: inside });
      expect(html).toContain('outside');
      expect(html).not.toContain('markban-board');
    });
  });

  it('under mdsvex too', async () => {
    const svelte = await renderMdsvex('```markban\n../../etc\n```\n', { root: join(here, 'fixtures') });
    expect(svelte).toContain('markban-notice');
    expect(svelte).not.toContain('markban-board');
  });
});

describe('other code blocks', () => {
  it('are left alone', async () => {
    const html = await renderUnified('```js\nconst a = 1;\n```\n\n```markdown\nx\n```\n');
    expect(html).toContain('<code class="language-js">');
    expect(html).not.toContain('markban');
  });

  it('boards inside a blockquote and a list are replaced', async () => {
    const html = await renderUnified(`> ${demo.split('\n').join('\n> ')}\n\n- item\n\n  ${demo.split('\n').join('\n  ')}\n`, root);
    expect(html.match(/<section class="markban-board"/g)).toHaveLength(2);
  });

  it('headingLevel moves the headings', async () => {
    const html = await renderUnified(demo, { ...root, headingLevel: 3 });
    expect(html).toContain('<h3 class="markban-title">');
    expect(html).toContain('<h4 class="markban-column-title">');
  });
});
