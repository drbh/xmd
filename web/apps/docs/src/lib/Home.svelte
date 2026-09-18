<script>
  // The start screen: templates and recent documents, like a document app's home.
  import Icon from "./Icon.svelte";
  import { TEMPLATES, relativeTime } from "./store.js";
  let { documents, engine, notice, theme, account = null, cloud = null, localCount = 0, onMoveLocal, onToggleTheme, onBook, onOpen, onNew, onImport, onRename, onDuplicate, onDownload, onDelete } = $props();
  let query = $state(""), menu = $state(null);
  const shown = $derived([...documents].sort((a, b) => b.updated - a.updated).filter(d => !query || d.name.toLowerCase().includes(query.toLowerCase()) || d.text.toLowerCase().includes(query.toLowerCase())));
  const preview = text => text.split("\n").filter(l => l.trim() && !/^#/.test(l)).slice(0, 6).join("\n");
  function act(fn, d) { menu = null; fn(d); }
</script>

<svelte:window onmousedown={e => { if (menu && !e.target.closest?.(".doc-menu")) menu = null; }} onkeydown={e => { if (e.key === "Escape") menu = null; }} />

<div class="home">
  <header class="home-bar">
    <div class="brand"><span class="logo"><Icon name="doc" size={22} /></span> WTF Docs</div>
    <label class="search"><Icon name="search" /><input type="search" placeholder="Search documents" aria-label="Search documents" bind:value={query}></label>
    <button type="button" class="button" onclick={onBook}><Icon name="doc" /> The Book</button>
    <label class="button primary"><Icon name="upload" /> Import<input type="file" accept=".wtf,text/plain" multiple hidden onchange={onImport}></label>
    <button type="button" class="tool theme-toggle" title={theme === "dark" ? "Switch to light theme" : "Switch to dark theme"} aria-label="Toggle theme" onclick={onToggleTheme}><Icon name={theme === "dark" ? "sun" : "moon"} /></button>
    {#if account}
      <div class="account" title={account.email}><span class="avatar">{account.email[0].toUpperCase()}</span><span class="email">{account.email}</span><button type="button" class="link" onclick={() => cloud.signOut()}>Sign out</button></div>
    {:else if cloud?.signIn}
      <button type="button" class="button" onclick={() => cloud.signIn()}>Sign in</button>
    {/if}
  </header>
  {#if notice}<p class="notice" role="status">{notice}</p>{/if}
  {#if account && localCount}
    <p class="notice info" role="status">{localCount} document{localCount === 1 ? "" : "s"} saved in this browser before you signed in. <button type="button" class="link" onclick={onMoveLocal}>Move to your account</button></p>
  {/if}

  <section class="templates" aria-label="Start a new document">
    <div class="section-head"><h2>Start a new document</h2><button type="button" class="link" onclick={onBook}>New here? Read the book: every feature as a live note →</button></div>
    <div class="template-row">
      {#each TEMPLATES as t (t.id)}
        <button type="button" class="template" onclick={() => onNew(t)}>
          <span class="thumb" class:blank={t.id === "blank"}>{#if t.id === "blank"}<Icon name="plus" size={40} />{:else}{preview(t.text)}{/if}</span>
          <span class="label">{t.name}</span>
        </button>
      {/each}
    </div>
  </section>

  <section class="recent" aria-label="Recent documents">
    <div class="section-head"><h2>{query ? "Matching documents" : "Recent documents"}</h2><span class="muted">{shown.length} of {documents.length}</span></div>
    {#if shown.length}
      <div class="doc-list" role="list">
        {#each shown as d (d.id)}
          <div class="doc-row" role="listitem">
            <button type="button" class="open" onclick={() => onOpen(d.id)}>
              <span class="doc-icon"><Icon name="doc" /></span>
              <span class="doc-name">{d.name || "Untitled document"}</span>
              <span class="doc-when">Edited {relativeTime(d.updated)}</span>
            </button>
            <div class="doc-menu">
              <button type="button" class="tool" aria-label={`Actions for ${d.name}`} aria-haspopup="true" aria-expanded={menu === d.id} onclick={() => (menu = menu === d.id ? null : d.id)}><Icon name="more" /></button>
              {#if menu === d.id}
                <div class="dropdown right" role="menu">
                  <button type="button" role="menuitem" onclick={() => act(onRename, d)}><span class="mark"></span><span class="label">Rename</span></button>
                  <button type="button" role="menuitem" onclick={() => act(onDuplicate, d)}><span class="mark"></span><span class="label">Make a copy</span></button>
                  <button type="button" role="menuitem" onclick={() => act(onDownload, d)}><span class="mark"></span><span class="label">Download</span></button>
                  <hr>
                  <button type="button" role="menuitem" onclick={() => act(onDelete, d)}><span class="mark"></span><span class="label">Remove</span></button>
                </div>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    {:else}
      <p class="empty">No documents match “{query}”.</p>
    {/if}
  </section>
  <footer class="home-foot">{engine} · {account ? `Documents are saved to ${account.email}.` : "Documents are stored in this browser. Download a copy to keep a backup."}</footer>
</div>
