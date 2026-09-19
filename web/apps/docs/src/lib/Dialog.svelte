<script>
  // A modal panel with a title; Escape and the backdrop close it.
  import Icon from "./Icon.svelte";
  let { title, onClose, children, wide = false } = $props();
  let dialog;
  $effect(() => { dialog?.showModal(); });
</script>

<dialog bind:this={dialog} class="dialog" class:wide onclose={onClose} onclick={e => { if (e.target === dialog) onClose(); }}>
  <div class="body">
    <header><h2>{title}</h2><button type="button" class="tool" aria-label="Close" onclick={onClose}><Icon name="close" /></button></header>
    <div class="content">{@render children()}</div>
  </div>
</dialog>
