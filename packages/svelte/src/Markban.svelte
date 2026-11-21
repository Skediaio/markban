<script>
  import '../style.css';

  /**
   * @typedef {import('@markban/core').BoardView} BoardView
   * @typedef {import('./build.js').BoardData} BoardData
   */

  /**
   * `board` is what `import board from 'markban:...'` gives, or a plain `BoardView` (cards then
   * open without a description).
   * @type {{ board: BoardData | BoardView, headingLevel?: number }}
   */
  let { board, headingLevel = 2 } = $props();

  const uid = $props.id();

  const view = $derived('view' in board ? board.view : board);
  const cards = $derived('cards' in board ? board.cards : {});
  const descriptionHtml = $derived('descriptionHtml' in board ? board.descriptionHtml : '');

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
    </header>
    {@render notice()}
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
