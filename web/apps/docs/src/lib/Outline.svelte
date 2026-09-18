<script>
  // Document symbols from the engine, as a nested list.
  let { symbols = [], onJump } = $props();
  const kinds = { 3: "section", 17: "task", 13: "value", 14: "value", 23: "table", 8: "column", 24: "event" };
</script>

{#snippet tree(items, depth)}
  {#each items as symbol}
    <button class="row" style={`padding-left:${8 + depth * 12}px`} onclick={() => onJump?.(symbol)}>
      <span class="name">{symbol.name}</span>
      {#if symbol.detail}<span class="detail">{symbol.detail}</span>{/if}
    </button>
    {#if symbol.children?.length}{@render tree(symbol.children, depth + 1)}{/if}
  {/each}
{/snippet}

<nav class="outline" aria-label="Outline">
  {#if symbols.length}{@render tree(symbols, 0)}{:else}<p class="empty">Nothing to outline yet.</p>{/if}
</nav>
