<script>
  import { onMount } from "svelte";
  import { mountEditor } from "@wtf/web/contenteditable";
  let { workspace, uri, text, onSnapshot, onError, controller = $bindable() } = $props();
  let view, hover;
  onMount(() => {
    let stopped = false, mounted;
    controller = null;
    mountEditor(view, {
      workspace, uri, source: text, hover, layout: "document",
      onRender: snapshot => { if (!stopped) onSnapshot?.(snapshot, snapshot.diagnostics); },
      onError,
    }).then(result => {
      mounted = result;
      if (stopped) result.destroy(); else controller = result;
    }).catch(onError);
    return () => { stopped = true; mounted?.destroy(); controller = null; };
  });
</script>

<div class="page">
  <pre class="view" bind:this={view} aria-label="Document"></pre>
</div>
<div class="hover" bind:this={hover} hidden></div>
