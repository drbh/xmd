<script>
  // Document symbols from the engine, as a nested list; the caret's symbol is marked.
  let { symbols = [], line = -1, onJump } = $props();
  const within = s => s.range.start.line <= line && line <= s.range.end.line;
  const current = $derived.by(() => {
    let best = null;
    const walk = items => { for (const s of items) { if (within(s)) { best = s; walk(s.children || []); } } };
    walk(symbols);
    return best;
  });
</script>

{#snippet tree(items, depth)}
  {#each items as symbol}
    <button type="button" class="row" aria-current={symbol === current ? "location" : undefined} style={`padding-left:${12 + depth * 14}px`} onclick={() => onJump?.(symbol)}>
      <span class="name">{symbol.name}</span>
      {#if symbol.detail}<span class="detail">{symbol.detail}</span>{/if}
    </button>
    {#if symbol.children?.length}{@render tree(symbol.children, depth + 1)}{/if}
  {/each}
{/snippet}

<nav class="outline" aria-label="Outline">
  {#if symbols.length}{@render tree(symbols, 0)}{:else}<p class="empty">Headings, values, and tasks you add will appear here.</p>{/if}
</nav>
