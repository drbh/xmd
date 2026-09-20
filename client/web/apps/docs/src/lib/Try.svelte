<script>
  // One widget behind every "try" in the reference: a live block seeded with
  // the snippet, a reset that puts the snippet back, and the block's own
  // diagnostics. A collection's snippet is a query, not a note, so it runs
  // against the sample note and shows the rows it returns.
  import { onMount } from "svelte";
  import Editor from "./Editor.svelte";
  import { SAMPLE_URI } from "./reference.js";
  let { workspace, uri, text, kind = "note", onError } = $props();
  let ready = $state(false), attempt = $state(0), problems = $state.raw([]), rows = $state(""), failed = $state("");
  async function seed() {
    failed = "";
    if (kind === "query") {
      try { const result = await workspace.query(SAMPLE_URI, "query", { query: text }); rows = JSON.stringify(result?.rows ?? [], null, 2); }
      catch (error) { failed = error.message; }
    } else {
      await workspace.setDocument(uri, text);
    }
    ready = true;
  }
  onMount(() => {
    seed().catch(onError);
    // The try notes are scratch: they leave the workspace with the widget.
    return () => { if (kind !== "query") Promise.resolve(workspace.removeDocument(uri)).catch(() => {}); };
  });
  function reset() { ready = false; problems = []; attempt += 1; seed().catch(onError); }
</script>

<figure class="wtf-block try-widget">
  <figcaption>
    <span>{kind === "query" ? "query · the sample note" : text.split("\n")[0].slice(0, 40)}</span>
    <button type="button" class="try-reset" onclick={reset}>reset</button>
  </figcaption>
  {#if kind === "query"}
    <pre class="try-query">{text}</pre>
    {#if failed}<p class="try-failed">{failed}</p>{:else if ready}<pre class="try-rows">{rows}</pre>{/if}
  {:else if ready}
    {#key attempt}
      <Editor {workspace} {uri} {text} frame="block" onSnapshot={(snapshot, list) => (problems = list)} {onError} />
    {/key}
    {#if problems.length}
      <ul class="problems">{#each problems as d}<li class={d.severity === 2 ? "warn" : "error"}>Line {d.range.start.line + 1}: {d.message}</li>{/each}</ul>
    {/if}
  {/if}
</figure>
