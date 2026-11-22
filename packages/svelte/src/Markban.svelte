<script>
  import '../style.css';
  import { init, search } from '@markban/core';

  /**
   * @typedef {import('@markban/core').BoardView} BoardView
   * @typedef {import('@markban/core').CardHit} CardHit
   * @typedef {import('@markban/core').SearchResults} SearchResults
   * @typedef {import('./build.js').BoardData} BoardData
   */

  /**
   * `board` is what `import board from 'markban:...'` gives, or a plain `BoardView` (cards then
   * open without a description, and the search box falls back to ids and titles).
   * @type {{ board: BoardData | BoardView, headingLevel?: number }}
   */
  let { board, headingLevel = 2 } = $props();

  const uid = $props.id();

  const view = $derived('view' in board ? board.view : board);
  const cards = $derived('cards' in board ? board.cards : {});
  const descriptionHtml = $derived('descriptionHtml' in board ? board.descriptionHtml : '');
  // Only a board that came with its raw files can run the engine's full-text search; a plain
  // view is filtered on ids and titles alone.
  const files = $derived('files' in board ? board.files : undefined);

  const level = $derived(Math.min(5, Math.max(1, Math.trunc(headingLevel) || 2)));
  const title = $derived(view.title === '' ? 'Board' : view.title);
  const usable = $derived(
    !(
      view.title === '' &&
      view.columns.length === 0 &&
      view.unsorted.length === 0 &&
      view.diagnostics.some((d) => d.severity === 'error')
    ),
  );
  const columns = $derived([
    ...view.columns.map((c) => ({ id: c.id, name: c.name, cards: c.cards, unsorted: false })),
    ...(view.unsorted.length > 0
      ? [{ id: '', name: 'Unsorted', cards: view.unsorted, unsorted: true }]
      : []),
  ]);
  const severity = $derived(view.diagnostics.some((d) => d.severity === 'error') ? 'error' : 'warning');

  /** @type {HTMLDialogElement | undefined} */
  let dialog = $state();
  /** @type {HTMLElement | null} */
  let opener = null;
  /** @type {{ id: string, title: string, column: string } | null} */
  let selected = $state(null);
  const detail = $derived(selected === null ? undefined : cards[selected.id]);

  /**
   * @param {{ id: string, title: string }} card
   * @param {string} column
   * @param {MouseEvent & { currentTarget: HTMLElement }} event
   */
  function open(card, column, event) {
    opener = event.currentTarget;
    selected = { id: card.id, title: card.title, column };
    dialog?.showModal();
  }

  function onclose() {
    selected = null;
    opener?.focus();
    opener = null;
  }

  /** @param {KeyboardEvent} event */
  function onkeydown(event) {
    // The browser closes a modal dialog on Escape itself; this keeps it working without that.
    if (event.key === 'Escape') dialog?.close();
  }

  /** @param {MouseEvent} event */
  function onbackdrop(event) {
    if (event.target === dialog) dialog?.close();
  }

  // ---- search ----

  /** The search box text. */
  let query = $state('');
  /** The hits for the settled query; `null` shows the board instead of the results. */
  let hits = $state(null);
  /** True while the results for the current query are still on their way. */
  let pending = $state(false);

  const terms = $derived(query.trim() === '' ? null : query.trim().split(/\s+/));
  const hitCount = $derived(
    hits === null ? 0 : hits.cards.length + hits.columns.length + (hits.board ? 1 : 0),
  );

  function clear() {
    query = '';
  }

  // Debounced: every keystroke restarts this, so only a query that settles for 200 ms runs.
  $effect(() => {
    if (terms === null) {
      hits = null;
      pending = false;
      return;
    }
    const current = terms;
    const boardFiles = files;
    pending = true;
    const timer = setTimeout(async () => {
      if (boardFiles === undefined) {
        hits = viewSearch(current);
      } else {
        await init(); // a no-op after the first call
        hits = search(boardFiles, current);
      }
      pending = false;
    }, 200);
    return () => clearTimeout(timer);
  });

  /**
   * Fallback without raw files: the engine's result shape, but only ids and titles are at hand.
   * @param {string[]} termList
   * @returns {SearchResults}
   */
  function viewSearch(termList) {
    const lower = termList.map((t) => t.toLowerCase());
    /** @param {string} haystack */
    const match = (haystack) => lower.every((t) => haystack.includes(t));
    /** @type {SearchResults['cards']} */
    const out = [];
    /**
     * @param {{ id: string, title: string }[]} list
     * @param {string | null} column
     * @param {boolean} archived
     */
    const add = (list, column, archived) => {
      for (const card of list) {
        if (match(`${card.id}\n${card.title}`.toLowerCase())) {
          out.push({ id: card.id, title: card.title, column, archived, snippet: null });
        }
      }
    };
    for (const column of view.columns) add(column.cards, column.id, false);
    add(view.unsorted, null, false);
    add(view.archived, null, true);
    return {
      cards: out,
      columns: view.columns
        .filter((c) => match(`${c.id}\n${c.name}`.toLowerCase()))
        .map((c) => ({ id: c.id, name: c.name })),
      board: match(`${view.title}\n${view.description}`.toLowerCase()),
    };
  }

  const columnNames = $derived(new Map(view.columns.map((c) => [c.id, c.name])));

  /**
   * A hit's `column` is an id; the panel shows the name, `Unsorted` without a valid column.
   * @param {string | null} id
   */
  function columnName(id) {
    return id === null || !columnNames.has(id) ? 'Unsorted' : /** @type {string} */ (columnNames.get(id));
  }

  /**
   * @param {CardHit} hit
   * @param {MouseEvent & { currentTarget: HTMLElement }} event
   */
  function openHit(hit, event) {
    open({ id: hit.id, title: hit.title }, columnName(hit.column), event);
  }
</script>

{#snippet notice()}
  {#if view.diagnostics.length > 0}
    <aside class="markban-notice markban-notice-{severity}" role="note">
      <p class="markban-notice-title">
        This board has {view.diagnostics.length}
        {view.diagnostics.length === 1 ? 'problem' : 'problems'}
      </p>
      <ul>
        {#each view.diagnostics as d, i (i)}
          <li><code>{d.line === null ? d.file : `${d.file}:${d.line}`}</code> {d.code} {d.message}</li>
        {/each}
      </ul>
    </aside>
  {/if}
{/snippet}

{#if usable}
  <section class="markban-board" aria-label="Board: {title}">
    <header class="markban-header">
      <svelte:element this={`h${level}`} class="markban-title">{title}</svelte:element>
      {#if descriptionHtml !== ''}
        <div class="markban-description">{@html descriptionHtml}</div>
      {/if}
      <div class="markban-search">
        <input
          class="markban-search-input"
          type="search"
          placeholder="Search…"
          aria-label="Search the board"
          value={query}
          oninput={(event) =>
            (query = /** @type {HTMLInputElement} */ (event.currentTarget).value)}
          onkeydown={(event) => {
            if (event.key === 'Escape') clear();
          }}
        />
        {#if query !== ''}
          <button type="button" class="markban-search-clear" aria-label="Clear search" onclick={clear}
            >×</button
          >
        {/if}
      </div>
    </header>
    {@render notice()}
    {#if terms === null}
      <div class="markban-columns">
        {#each columns as column (column.unsorted ? '\0unsorted' : column.id)}
          <section
            class={['markban-column', column.unsorted && 'markban-unsorted']}
            data-column={column.unsorted ? undefined : column.id}
            aria-label={column.name}
          >
            <svelte:element this={`h${level + 1}`} class="markban-column-title">
              <span class="markban-column-name">{column.name}</span>
              <span
                class="markban-count"
                aria-label="{column.cards.length} {column.cards.length === 1 ? 'card' : 'cards'}"
                >{column.cards.length}</span
              >
            </svelte:element>
            {#if column.cards.length === 0}
              <p class="markban-empty">No cards</p>
            {:else}
              <ul class="markban-cards">
                {#each column.cards as card (card.id)}
                  <li class="markban-card" data-card={card.id}>
                    <button
                      type="button"
                      class="markban-card-title markban-card-button"
                      aria-haspopup="dialog"
                      onclick={(event) => open(card, column.name, event)}>{card.title}</button
                    >
                  </li>
                {/each}
              </ul>
            {/if}
          </section>
        {/each}
      </div>
    {:else}
      <div class="markban-search-results" aria-label="Search results">
        <p class="markban-search-count">{#if pending}Searching…{:else}{hitCount} hit(s){/if}</p>
        {#if hits !== null}
          {#if hits.cards.length > 0}
            <ul class="markban-cards markban-search-cards">
              {#each hits.cards as hit (hit.id)}
                <li class="markban-card" data-card={hit.id}>
                  <button
                    type="button"
                    class="markban-card-title markban-card-button"
                    aria-haspopup="dialog"
                    onclick={(event) => openHit(hit, event)}
                  >
                    <span class="markban-hit-id">{hit.id}</span> {hit.title}
                    <span class="markban-hit-column">{columnName(hit.column)}</span>
                    {#if hit.archived}<span class="markban-hit-archived">(archived)</span>{/if}
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
          {#each hits.columns as column (column.id)}
            <p class="markban-search-extra">Column: {column.name}</p>
          {/each}
          {#if hits.board}
            <p class="markban-search-extra">Board: {title}</p>
          {/if}
        {/if}
      </div>
    {/if}

    <dialog
      class="markban-dialog"
      bind:this={dialog}
      {onclose}
      {onkeydown}
      onclick={onbackdrop}
      aria-labelledby="{uid}-title"
    >
      {#if selected !== null}
        <div class="markban-dialog-header">
          <h3 class="markban-dialog-title" id="{uid}-title">{selected.title}</h3>
          <button type="button" class="markban-dialog-close" onclick={() => dialog?.close()}>Close</button>
        </div>
        <p class="markban-dialog-column">{selected.column}</p>
        {#if detail !== undefined && detail.html !== ''}
          <div class="markban-dialog-body">{@html detail.html}</div>
        {:else}
          <p class="markban-dialog-empty">No description</p>
        {/if}
      {/if}
    </dialog>
  </section>
{:else}
  {@render notice()}
{/if}
