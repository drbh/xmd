<script>
  // Google-Docs-style menu bar: click opens, hovering across switches menus,
  // arrows move within a menu, Escape closes and returns focus to the document.
  import { shortcutLabel } from "./commands.js";
  let { menus, onRun } = $props();
  let open = $state(null);
  let bar;
  function toggle(name) { open = open === name ? null : name; }
  function hover(name) { if (open && open !== name) open = name; }
  function run(item) { open = null; item.run?.(); onRun?.(item); }
  function keydown(event) {
    if (!open) return;
    const items = [...bar.querySelectorAll(`[data-menu="${open}"] [role="menuitem"], [data-menu="${open}"] [role="menuitemcheckbox"]`)];
    const index = items.indexOf(document.activeElement);
    const names = menus.map(m => m.name);
    if (event.key === "Escape") { open = null; event.preventDefault(); }
    else if (event.key === "ArrowDown") { items[(index + 1) % items.length]?.focus(); event.preventDefault(); }
    else if (event.key === "ArrowUp") { items[(index - 1 + items.length) % items.length]?.focus(); event.preventDefault(); }
    else if (event.key === "ArrowRight") { open = names[(names.indexOf(open) + 1) % names.length]; event.preventDefault(); }
    else if (event.key === "ArrowLeft") { open = names[(names.indexOf(open) - 1 + names.length) % names.length]; event.preventDefault(); }
  }
  function outside(event) { if (open && !bar.contains(event.target)) open = null; }
  // Escape closes an open menu wherever focus is: on a phone a tap on the menu
  // button need not focus it, so the bar's own keydown would never see the key.
  function escape(event) { if (open && event.key === "Escape" && !bar.contains(event.target)) { open = null; event.preventDefault(); } }
</script>

<svelte:window onmousedown={outside} onkeydown={escape} />

<div class="menubar" role="menubar" tabindex="-1" bind:this={bar} onkeydown={keydown}>
  {#each menus as menu (menu.name)}
    <div class="menu" data-menu={menu.name}>
      <button type="button" role="menuitem" aria-haspopup="true" aria-expanded={open === menu.name} class:open={open === menu.name}
        onclick={() => toggle(menu.name)} onmouseenter={() => hover(menu.name)}>{menu.name}</button>
      {#if open === menu.name}
        <div class="dropdown" role="menu" aria-label={menu.name}>
          {#each menu.items as item (item.id)}
            <button type="button" role={item.checked ? "menuitemcheckbox" : "menuitem"} aria-checked={item.checked ? item.checked() : undefined} onclick={() => run(item)}>
              <span class="mark">{#if item.checked?.()}✓{/if}</span>
              <span class="label">{item.label}</span>
              {#if item.hint}<span class="hint">{item.hint}</span>{/if}
              {#if item.shortcut}<span class="shortcut">{shortcutLabel(item.shortcut)}</span>{/if}
            </button>
            {#if item.separator}<hr>{/if}
          {/each}
        </div>
      {/if}
    </div>
  {/each}
</div>
