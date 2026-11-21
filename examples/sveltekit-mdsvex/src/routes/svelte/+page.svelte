<script>
  import { Markban } from '@markban/svelte';
  import board from 'markban:demo-board';
</script>

<svelte:head>
  <title>Markban in Svelte</title>
</svelte:head>

<h1>A board in Svelte</h1>

<p>
  The board is imported with <code>markban:demo-board</code>. Select a card to read its description.
</p>

<Markban {board} />
