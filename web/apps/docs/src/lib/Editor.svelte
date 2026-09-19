<script>
  import { onMount, tick, untrack } from "svelte";
  import { mountEditor } from "@wtf/web/contenteditable";
  import { lineOf, rangeOf } from "./editing.js";
  import Icon from "./Icon.svelte";
  let { workspace, uri, text, readOnly = false, frame = "page", live = null, onSnapshot, onCaret, onError, controller = $bindable() } = $props();
  let view, hover, page;
  // Code lenses drawn as chips at the end of their line, positioned over the
  // page so the editable text itself is never modified.
  let lenses = $state.raw([]), versions = $state.raw({}), chips = $state.raw([]);
  // On a phone there is no room beside the text: only the caret's line shows its actions, under the line.
  const phone = matchMedia("(max-width: 640px)");
  let caretLine = -1;
  // Other people's carets and selections, drawn over the page from source offsets.
  let people = $state.raw([]), carets = $state.raw([]);
  const PALETTE = 8;
  function placePeople() {
    if (!view || !page) return;
    const origin = page.getBoundingClientRect();
    const next = [];
    const highlights = Array.from({ length: PALETTE }, () => []);
    for (const person of people) {
      if (person.head === null || person.head === undefined) continue;
      const range = rangeOf(view, person.head, person.head);
      if (!range) continue;
      const rect = range.getClientRects()[0] || range.getBoundingClientRect();
      const slot = person.clientId % PALETTE;
      next.push({ id: person.clientId, name: person.user.name, color: person.user.color, top: rect.top - origin.top, left: rect.left - origin.left, height: rect.height || 20 });
      if (person.anchor !== null && person.anchor !== person.head) { const sel = rangeOf(view, Math.min(person.anchor, person.head), Math.max(person.anchor, person.head)); if (sel) highlights[slot].push(sel); }
    }
    carets = next;
    if (globalThis.CSS?.highlights) for (let i = 0; i < PALETTE; i++) { if (highlights[i].length) CSS.highlights.set(`presence-${i}`, new Highlight(...highlights[i])); else CSS.highlights.delete(`presence-${i}`); }
  }
  function place() {
    if (!view || !page) return;
    const origin = page.getBoundingClientRect();
    const byLine = new Map();
    for (const lens of lenses) {
      if (phone.matches && lens.range.start.line !== caretLine) continue;
      const line = view.querySelector(`.line[data-line="${lens.range.start.line}"]`);
      if (!line) continue;
      const rects = line.getClientRects();
      const rect = rects[rects.length - 1] || line.getBoundingClientRect();
      const list = byLine.get(line) || [];
      list.push(lens);
      byLine.set(line, list);
      if (list.length === 1) list.at = phone.matches
        ? { top: rect.bottom - origin.top + 2, left: 0, right: 12, height: 24, below: true }
        : { top: rect.top - origin.top, left: rect.right - origin.left, height: rect.height };
    }
    // Chips sit after the line's text, but never past the page edge.
    const width = list => list.reduce((n, l) => n + 30 + l.command.title.length * 6.2, 0) + (list.length - 1) * 4;
    chips = [...byLine.values()].map(list => ({ ...list.at, left: list.at.below ? null : Math.max(8, Math.min(list.at.left, origin.width - width(list) - 12)), lenses: list }));
  }
  // A live session binds once both it and the editor exist.
  $effect(() => {
    const session = live, editor = controller;
    if (!session || !editor || editor.destroyed) return;
    // Only `live` and `controller` are dependencies; presence updates must not re-run this.
    const stop = untrack(() => { session.attach(editor); return session.onPresence(list => { people = list; placePeople(); }); });
    return () => { stop(); untrack(() => { people = []; placePeople(); }); };
  });
  function run(lens) { controller?.execute(lens.command, versions).catch(onError); }
  onMount(() => {
    let stopped = false, mounted;
    controller = null;
    mountEditor(view, {
      workspace, uri, source: text, hover, layout: "document", controls: false,
      onRender: snapshot => {
        if (stopped) return;
        lenses = snapshot.lenses || []; versions = snapshot.versions;
        onSnapshot?.(snapshot, snapshot.diagnostics);
        tick().then(() => { place(); placePeople(); });
      },
      onError,
    }).then(result => {
      mounted = result;
      if (stopped) result.destroy(); else { controller = result; if (readOnly) result.element.contentEditable = "false"; report(); }
    }).catch(onError);
    const report = () => {
      if (!mounted || mounted.destroyed) return;
      const selection = mounted.selection();
      const line = selection ? lineOf(mounted.getSource(), selection.focus) : -1;
      if (line !== caretLine) { caretLine = line; if (phone.matches) place(); }
      onCaret?.(line, selection);
    };
    const observer = new ResizeObserver(() => { place(); placePeople(); });
    observer.observe(view);
    document.fonts?.ready.then(place);
    document.addEventListener("selectionchange", report);
    return () => { stopped = true; observer.disconnect(); if (globalThis.CSS?.highlights) for (let i = 0; i < PALETTE; i++) CSS.highlights.delete(`presence-${i}`); document.removeEventListener("selectionchange", report); mounted?.destroy(); controller = null; };
  });
</script>

<div class={frame === "page" ? "page" : "block"} bind:this={page}>
  <pre class="view" bind:this={view} aria-label="Document"></pre>
  {#each carets as caret (caret.id)}
    <div class="presence-caret" style={`top:${caret.top}px;left:${caret.left}px;height:${caret.height}px;--presence:${caret.color}`}><span class="presence-name">{caret.name}</span></div>
  {/each}
  {#each chips as chip}
    <div class="lenses" class:below={chip.below} style={chip.below ? `top:${chip.top}px;right:${chip.right}px;height:${chip.height}px` : `top:${chip.top}px;left:${chip.left}px;height:${chip.height}px`}>
      {#each chip.lenses as lens}
        <button type="button" class="lens" onmousedown={e => e.preventDefault()} onclick={() => run(lens)}><Icon name="play" size={12} /> {lens.command.title}</button>
      {/each}
    </div>
  {/each}
</div>
<div class="hover" bind:this={hover} hidden></div>
