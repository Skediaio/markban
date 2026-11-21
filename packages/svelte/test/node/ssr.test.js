import { describe, expect, test } from 'vitest';
import { render } from 'svelte/server';
import Markban from '../../src/Markban.svelte';
import { buildBoardModule } from '../../src/build.js';
import { demoBoard, hostileBoard } from '../helpers.js';

describe('<Markban> on the server', () => {
  test('puts the board in the static HTML', () => {
    const { body } = render(Markban, { props: { board: buildBoardModule(demoBoard) } });
    expect(body).toContain('class="markban-board"');
    expect(body).toContain('Markban roadmap');
    expect(body).toContain('markban-column-title');
    expect(body).toContain('<dialog');
  });

  test('hostile content is inert in the static HTML', () => {
    const { body } = render(Markban, { props: { board: buildBoardModule(hostileBoard) } });
    expect(body).not.toMatch(/<script/i);
    expect(body).not.toMatch(/<img/i);
    expect(body).not.toMatch(/<[^>]*\son(error|click)=/i);
    expect(body).not.toMatch(/href="\s*javascript:/i);
  });
});
