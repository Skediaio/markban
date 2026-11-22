import { describe, expect, test } from 'vitest';
import { buildBoardModule } from '../../src/build.js';
import { demoBoard, hostileBoard } from '../helpers.js';

describe('buildBoardModule', () => {
  test('gives the view, the raw files and sanitised html for every card', () => {
    const board = buildBoardModule(demoBoard);
    expect(board.view.title).toBe('Markban roadmap');
    expect(board.files.map((f) => f.path)).toContain('board.md');
    const ids = [
      ...board.view.columns.flatMap((c) => c.cards),
      ...board.view.unsorted,
      ...board.view.archived,
    ].map((c) => c.id);
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

  test('an archived card keeps its column name and description', () => {
    const { cards } = buildBoardModule(demoBoard);
    expect(cards['0004-research']).toMatchObject({ title: 'Format experiments', column: 'Done' });
    expect(cards['0004-research'].html).toContain('kept for reference');
  });
});
