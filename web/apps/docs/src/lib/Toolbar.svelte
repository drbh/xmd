<script>
  import Icon from "./Icon.svelte";
  import { shortcutLabel } from "./commands.js";
  let { commands, style = { kind: "text" }, zoom = 100, onZoom, outline = true } = $props();
  const tip = c => c.shortcut ? `${c.label} (${shortcutLabel(c.shortcut)})` : c.label;
  const styleValue = $derived(style.kind === "heading" && style.level <= 3 ? `h${style.level}` : "normal");
  const ZOOMS = [50, 75, 90, 100, 125, 150, 200];
</script>

{#snippet button(id, active = false)}
  {@const c = commands[id]}
  <button type="button" class="tool" class:active title={tip(c)} aria-label={c.label} aria-pressed={active} onmousedown={e => e.preventDefault()} onclick={c.run}><Icon name={c.icon} /></button>
{/snippet}

<div class="toolbar" role="toolbar" aria-label="Formatting">
  {@render button("outline", outline)}
  <span class="divider"></span>
  {@render button("undo")}
  {@render button("redo")}
  {@render button("print")}
  <span class="divider"></span>
  <select class="select zoom" aria-label="Zoom" value={String(zoom)} onchange={e => onZoom(Number(e.target.value))}>
    {#if !ZOOMS.includes(zoom)}<option value={String(zoom)}>{zoom}%</option>{/if}
    {#each ZOOMS as z}<option value={String(z)}>{z}%</option>{/each}
  </select>
  <span class="divider"></span>
  <select class="select style" aria-label="Text style" value={styleValue} onmousedown={e => e.stopPropagation()} onchange={e => commands[e.target.value === "normal" ? "normal" : e.target.value].run()}>
    <option value="normal">Normal text</option>
    <option value="h1">Heading 1</option>
    <option value="h2">Heading 2</option>
    <option value="h3">Heading 3</option>
  </select>
  <span class="divider"></span>
  {@render button("bold")}
  {@render button("italic")}
  {@render button("strike")}
  {@render button("code")}
  {@render button("insertLink")}
  <span class="divider"></span>
  {@render button("checklist", style.kind === "task")}
  {@render button("bulletList", style.kind === "bullet")}
  {@render button("numberList", style.kind === "number")}
  {@render button("outdent")}
  {@render button("indent")}
  <span class="divider"></span>
  {@render button("insertTable")}
  {@render button("insertTimer")}
  <span class="gap"></span>
  {@render button("find")}
</div>
