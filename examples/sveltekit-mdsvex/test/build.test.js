import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { beforeAll, describe, expect, test } from 'vitest';

const app = join(dirname(fileURLToPath(import.meta.url)), '..');
const page = (name) => readFileSync(join(app, 'build', name), 'utf8');

describe('static build of the example', () => {
  beforeAll(() => {
    execFileSync('npx', ['vite', 'build'], { cwd: app, stdio: 'pipe' });
  }, 120_000);

  test('/ has the board from the markdown page as static HTML', () => {
    const html = page('index.html');
    expect(html).toContain('class="markban-board"');
    expect(html).toContain('Markban roadmap');
    expect(html).toContain('markban-card-details');
    expect(html).not.toMatch(/<script>alert/i);
  });

  test('/svelte has the board from the component as static HTML', () => {
    const html = page('svelte.html');
    expect(html).toContain('class="markban-board"');
    expect(html).toContain('Markban roadmap');
    expect(html).toContain('markban-card-button');
    expect(html).toContain('<dialog');
  });
});
