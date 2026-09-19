<script>
  import Icon from "./Icon.svelte";
  import { findMatches, reveal, highlightMatches, clearHighlights } from "./editing.js";
  let { controller, replace = false, onClose } = $props();
  let query = $state(""), replacement = $state(""), caseSensitive = $state(false), index = $state(-1), total = $state(0);
  // Where the writer was: the document selection moves into these inputs, so
  // closing hands this offset back to the editor.
  let caret = 0;
  let input;
  $effect(() => { caret = controller?.selection()?.focus ?? 0; input?.focus(); input?.select(); });
  function matches() { return controller ? findMatches(controller.getSource(), query, { caseSensitive }) : []; }
  function show(list, i) {
    total = list.length;
    if (!list.length) { index = -1; highlightMatches(controller?.element, [], -1); return; }
    index = (i + list.length) % list.length;
    caret = list[index].start;
    controller.select(list[index].start, list[index].end);
    reveal(controller);
    input?.focus({ preventScroll: true });
    highlightMatches(controller.element, list, index);
  }
  function search() {
    const list = matches();
    // Start from the caret so "find" walks forward from where the writer is.
    const at = caret;
    const i = list.findIndex(m => m.start >= at);
    show(list, i === -1 ? 0 : i);
  }
  const next = () => show(matches(), index + 1);
  const previous = () => show(matches(), index - 1);
  async function replaceOne() {
    const list = matches();
    if (!list.length || index < 0) return search();
    const m = list[index];
    caret = m.start + replacement.length;
    await controller.replaceRange(m.start, m.end, replacement, { anchor: caret });
    show(matches(), index);
  }
  async function replaceAll() {
    const list = matches();
    if (!list.length) return;
    const source = controller.getSource();
    let out = "", last = 0;
    for (const m of list) { out += source.slice(last, m.start) + replacement; last = m.end; }
    caret = out.length;
    out += source.slice(last);
    await controller.replaceRange(0, source.length, out, { anchor: caret });
    total = 0; index = -1;
  }
  function keydown(event) {
    if (event.key === "Escape") { event.preventDefault(); onClose(caret); }
    else if (event.key === "Enter") { event.preventDefault(); if (event.target === input) (event.shiftKey ? previous : next)(); else if (event.target.name === "replacement") replaceOne(); }
  }
  $effect(() => { query; caseSensitive; search(); });
  $effect(() => clearHighlights);
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="findbar" role="search" onkeydown={keydown}>
  <div class="row">
    <input bind:this={input} bind:value={query} type="search" placeholder="Find in document" aria-label="Find" autocomplete="off" spellcheck="false">
    <span class="count">{query ? (total ? `${index + 1} of ${total}` : "No results") : ""}</span>
    <button type="button" class="tool" title="Previous match (Shift+Enter)" aria-label="Previous match" onclick={previous}><Icon name="up" /></button>
    <button type="button" class="tool" title="Next match (Enter)" aria-label="Next match" onclick={next}><Icon name="down" /></button>
    <button type="button" class="tool" title="Close (Esc)" aria-label="Close find" onclick={() => onClose(caret)}><Icon name="close" /></button>
  </div>
  {#if replace}
    <div class="row">
      <input name="replacement" bind:value={replacement} type="text" placeholder="Replace with" aria-label="Replace with" autocomplete="off" spellcheck="false">
      <button type="button" class="text" onclick={replaceOne} disabled={!total}>Replace</button>
      <button type="button" class="text" onclick={replaceAll} disabled={!total}>Replace all</button>
    </div>
  {/if}
  <label class="option"><input type="checkbox" bind:checked={caseSensitive}> Match case</label>
  <button type="button" class="link" onclick={() => (replace = !replace)}>{replace ? "Hide replace" : "Replace…"}</button>
</div>
