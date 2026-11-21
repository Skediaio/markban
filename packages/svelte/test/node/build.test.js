import { describe, expect, test } from 'vitest';
import { buildBoardModule } from '../../src/build.js';
import { demoBoard, hostileBoard } from '../helpers.js';

describe('buildBoardModule', () => {
  test('gives the view and sanitised html for every card', () => {
    const board = buildBoardModule(demoBoard);
    expect(board.view.title).toBe('Markban roadmap');
    const ids = [...board.view.columns.flatMap((c) => c.cards), ...board.view.unsorted].map((c) => c.id);
    expect(Object.keys(board.cards).sort()).toEqual(ids.sort());
    const first = board.cards[ids[0]];
    expect(first.title).toBeTruthy();
    expect(typeof first.html).toBe('string');
  });

  test('applies the remark-markban sanitiser rules', () => {
    const { cards } = buildBoardModule(hostileBoard);
    const all = Object.values(cards).map((c) => c.html).join('\n');
    expect(all).not.toMatch(/<script|<img|<div|onclick/i);
    expect(all).not.toMatch(/href="\s*javascript:/i);
    expect(all).toContain('href="https://example.com"');
    expect(all).toContain('&#123;alert(1)&#125;');
  });

  test('records the column name of a card', () => {
    const { cards } = buildBoardModule(hostileBoard);
    expect(Object.values(cards).find((c) => c.title === 'A fine card')?.column).toBe('Done');
  });
});
