<script>
  import { onMount, tick } from "svelte";
  import { mountEditor } from "@wtf/web/contenteditable";
  import { lineOf } from "./editing.js";
  import Icon from "./Icon.svelte";
  let { workspace, uri, text, readOnly = false, onSnapshot, onCaret, onError, controller = $bindable() } = $props();
  let view, hover, page;
  // Code lenses drawn as chips at the end of their line, positioned over the
  // page so the editable text itself is never modified.
  let lenses = $state.raw([]), versions = $state.raw({}), chips = $state.raw([]);
  function place() {
    if (!view || !page) return;
    const origin = page.getBoundingClientRect();
    const byLine = new Map();
    for (const lens of lenses) {
      const line = view.querySelector(`.line[data-line="${lens.range.start.line}"]`);
      if (!line) continue;
      const rects = line.getClientRects();
      const rect = rects[rects.length - 1] || line.getBoundingClientRect();
      const list = byLine.get(line) || [];
      list.push(lens);
      byLine.set(line, list);
      if (list.length === 1) list.at = { top: rect.top - origin.top, left: rect.right - origin.left, height: rect.height };
    }
    chips = [...byLine.values()].map(list => ({ ...list.at, lenses: list }));
  }
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
        tick().then(place);
      },
      onError,
    }).then(result => {
      mounted = result;
      if (stopped) result.destroy(); else { controller = result; if (readOnly) result.element.contentEditable = "false"; report(); }
    }).catch(onError);
    const report = () => {
      if (!mounted || mounted.destroyed) return;
      const selection = mounted.selection();
      onCaret?.(selection ? lineOf(mounted.getSource(), selection.focus) : -1, selection);
    };
    const observer = new ResizeObserver(place);
    observer.observe(view);
    document.fonts?.ready.then(place);
    document.addEventListener("selectionchange", report);
    return () => { stopped = true; observer.disconnect(); document.removeEventListener("selectionchange", report); mounted?.destroy(); controller = null; };
  });
</script>

<div class="page" bind:this={page}>
  <pre class="view" bind:this={view} aria-label="Document"></pre>
  {#each chips as chip}
    <div class="lenses" style={`top:${chip.top}px;left:${chip.left}px;height:${chip.height}px`}>
      {#each chip.lenses as lens}
        <button type="button" class="lens" onmousedown={e => e.preventDefault()} onclick={() => run(lens)}><Icon name="play" size={12} /> {lens.command.title}</button>
      {/each}
    </div>
  {/each}
</div>
<div class="hover" bind:this={hover} hidden></div>
