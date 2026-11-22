import { describe, expect, inject, test } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import Markban from '../../src/Markban.svelte';

// Built in Node by test/boards.setup.js: the browser build of @markban/core cannot read files.
const { demo, hostile, messy } = inject('boards');

/** @param {string} name */
const cardButton = (name) => screen.getByRole('button', { name });

describe('<Markban>', () => {
  test('renders the columns with counts and the cards', () => {
    const { container } = render(Markban, { board: demo });
    expect(container.querySelector('.markban-title')?.textContent).toBe('Markban roadmap');
    const columns = [...container.querySelectorAll('.markban-column')];
    expect(columns.map((c) => c.getAttribute('data-column'))).toEqual(['backlog', 'doing', 'done']);
    for (const column of columns) {
      const n = column.querySelectorAll('.markban-card').length;
      expect(column.querySelector('.markban-count')?.textContent).toBe(String(n));
    }
    expect(screen.getAllByRole('button').length).toBeGreaterThan(0);
  });

  test('a card opens a modal dialog with title, column and description', async () => {
    const { container } = render(Markban, { board: demo });
    const dialog = /** @type {HTMLDialogElement} */ (container.querySelector('dialog'));
    expect(dialog.open).toBe(false);
    const [id, card] = Object.entries(demo.cards).find(([, c]) => c.html !== '') ?? [];
    expect(id).toBeDefined();
    await fireEvent.click(cardButton(card.title));
    expect(dialog.open).toBe(true);
    expect(dialog.getAttribute('aria-labelledby')).toBeTruthy();
    const scope = within(dialog);
    expect(scope.getByRole('heading', { name: card.title })).toBeTruthy();
    expect(dialog.textContent).toContain(card.column);
    expect(dialog.querySelector('.markban-dialog-body')?.innerHTML).toBe(card.html);
  });

  test('closing the dialog returns focus to the card', async () => {
    const { container } = render(Markban, { board: demo });
    const dialog = /** @type {HTMLDialogElement} */ (container.querySelector('dialog'));
    const title = Object.values(demo.cards)[0].title;
    const opener = cardButton(title);
    opener.focus();
    await fireEvent.click(opener);
    expect(dialog.open).toBe(true);
    await fireEvent.keyDown(dialog, { key: 'Escape' });
    expect(dialog.open).toBe(false);
    expect(document.activeElement).toBe(opener);
  });

  test('the close button closes the dialog', async () => {
    const { container } = render(Markban, { board: demo });
    const dialog = /** @type {HTMLDialogElement} */ (container.querySelector('dialog'));
    await fireEvent.click(cardButton(Object.values(demo.cards)[0].title));
    await fireEvent.click(within(dialog).getByRole('button', { name: 'Close' }));
    expect(dialog.open).toBe(false);
  });

  test('a card without a description says so', async () => {
    const { container } = render(Markban, { board: hostile });
    const dialog = /** @type {HTMLDialogElement} */ (container.querySelector('dialog'));
    await fireEvent.click(cardButton('No description'));
    expect(dialog.textContent).toContain('No description');
    expect(dialog.querySelector('.markban-dialog-body')).toBeNull();
  });

  test('hostile board content renders inert', async () => {
    const { container } = render(Markban, { board: hostile });
    expect(container.querySelector('script')).toBeNull();
    expect(container.querySelector('[onerror]')).toBeNull();
    await fireEvent.click(screen.getAllByRole('button').find((b) => b.textContent?.includes('img src=x')));
    const dialog = /** @type {HTMLDialogElement} */ (container.querySelector('dialog'));
    expect(dialog.querySelector('script')).toBeNull();
    expect(dialog.querySelector('[onclick]')).toBeNull();
    expect(dialog.querySelector('img')).toBeNull();
    for (const a of dialog.querySelectorAll('a')) {
      expect(a.getAttribute('href')).not.toMatch(/^\s*(javascript|data):/i);
    }
    expect(container.innerHTML).not.toContain('<img');
  });

  test('shows the diagnostics notice and the Unsorted column', () => {
    const { container } = render(Markban, { board: messy });
    expect(container.querySelector('.markban-notice')).not.toBeNull();
    expect(container.querySelector('.markban-notice li code')).not.toBeNull();
    const view = {
      title: 'T',
      description: '',
      columns: [{ id: 'a', name: 'A', cards: [] }],
      unsorted: [{ id: '0001-x', title: 'Lost' }],
      diagnostics: [],
    };
    const second = render(Markban, { board: view });
    expect(second.container.querySelector('.markban-unsorted')).not.toBeNull();
    expect(second.container.querySelector('.markban-empty')?.textContent).toBe('No cards');
  });

  test('a plain board view without card data still renders', () => {
    const { container } = render(Markban, { board: demo.view });
    expect(container.querySelectorAll('.markban-card').length).toBeGreaterThan(0);
  });

  test('an unusable board shows only the notice', () => {
    const view = {
      title: '',
      description: '',
      columns: [],
      unsorted: [],
      diagnostics: [{ code: 'E001', severity: 'error', file: 'board.md', line: null, message: 'missing' }],
    };
    const { container } = render(Markban, { board: view });
    expect(container.querySelector('.markban-notice-error')).not.toBeNull();
    expect(container.querySelector('.markban-board')).toBeNull();
  });
});

describe('<Markban> search box', () => {
  test('full-text search replaces the columns with the hits', async () => {
    const { container } = render(Markban, { board: demo });
    await fireEvent.input(screen.getByRole('searchbox'), { target: { value: 'experiments' } });
    await waitFor(() => expect(screen.getByText('2 hit(s)')).toBeTruthy());
    expect(container.querySelector('.markban-columns')).toBeNull();
    const found = [...container.querySelectorAll('.markban-search-cards .markban-card')];
    expect(found.map((li) => li.getAttribute('data-card'))).toEqual([
      '0001-spec-draft',
      '0004-research',
    ]);
    expect(found[1].textContent).toContain('Done', 'the muted column name');
    expect(found[1].textContent).toContain('(archived)');
  });

  test('a hit opens the card dialog, an archived one included', async () => {
    const { container } = render(Markban, { board: demo });
    const dialog = /** @type {HTMLDialogElement} */ (container.querySelector('dialog'));
    await fireEvent.input(screen.getByRole('searchbox'), { target: { value: 'experiments' } });
    await waitFor(() => expect(screen.getByText('2 hit(s)')).toBeTruthy());
    await fireEvent.click(screen.getByRole('button', { name: /Format experiments/ }));
    expect(dialog.open).toBe(true);
    expect(dialog.textContent).toContain('kept for reference', 'the archived card keeps its body');
  });

  test('column and board hits are lines, and a miss says 0', async () => {
    const { container } = render(Markban, { board: demo });
    const box = screen.getByRole('searchbox');
    await fireEvent.input(box, { target: { value: 'backlog' } });
    await waitFor(() => expect(container.textContent).toContain('Column: Backlog'));
    expect(container.querySelectorAll('.markban-card')).toHaveLength(0);
    await fireEvent.input(box, { target: { value: 'roadmap' } });
    await waitFor(() => expect(container.textContent).toContain('Board: Markban roadmap'));
    await fireEvent.input(box, { target: { value: 'zzz' } });
    await waitFor(() => expect(screen.getByText('0 hit(s)')).toBeTruthy());
  });

  test('clearing with the × button or Escape brings the board back', async () => {
    const { container } = render(Markban, { board: demo });
    const box = /** @type {HTMLInputElement} */ (screen.getByRole('searchbox'));
    await fireEvent.input(box, { target: { value: 'experiments' } });
    await waitFor(() => expect(screen.getByText('2 hit(s)')).toBeTruthy());
    await fireEvent.click(screen.getByRole('button', { name: 'Clear search' }));
    await waitFor(() => expect(container.querySelector('.markban-columns')).not.toBeNull());
    expect(box.value).toBe('');
    await fireEvent.input(box, { target: { value: 'zzz' } });
    await waitFor(() => expect(screen.getByText('0 hit(s)')).toBeTruthy());
    await fireEvent.keyDown(box, { key: 'Escape' });
    await waitFor(() => expect(container.querySelector('.markban-columns')).not.toBeNull());
  });

  test('a plain board view without files searches ids and titles only', async () => {
    const { container } = render(Markban, { board: demo.view });
    await fireEvent.input(screen.getByRole('searchbox'), { target: { value: 'viewer' } });
    await waitFor(() => expect(screen.getByText('1 hit(s)')).toBeTruthy());
    const hit = container.querySelector('.markban-search-cards .markban-card');
    expect(hit?.getAttribute('data-card')).toBe('0003-board-ui');
    expect(hit?.textContent).toContain('Done');
  });
});
