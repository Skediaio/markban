<script>
  import 'remark-markban/style.css';

  /** @type {{ children: import('svelte').Snippet }} */
  let { children } = $props();
</script>

<nav>
  <a href="/">Markdown page (remark-markban)</a>
  <a href="/svelte">Svelte page (@markban/svelte)</a>
</nav>

<main>
  {@render children()}
</main>

<style>
  :global(body) {
    margin: 0;
    font: 16px/1.5 system-ui, sans-serif;
    color: #1c2330;
    background: #fff;
  }
  @media (prefers-color-scheme: dark) {
    :global(body) {
      color: #e6eaf0;
      background: #0b0e13;
    }
    :global(a) {
      color: #6ea0ff;
    }
  }
  nav {
    display: flex;
    flex-wrap: wrap;
    gap: 16px;
    padding: 12px 16px;
    border-bottom: 1px solid #8884;
  }
  main {
    max-width: 72rem;
    margin: 0 auto;
    padding: 0 16px 32px;
  }
</style>
