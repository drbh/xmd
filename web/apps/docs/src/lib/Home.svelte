<script>
  // The start screen: templates and recent documents, like a document app's home.
  import Icon from "./Icon.svelte";
  import { TEMPLATES, relativeTime, colorFor } from "./store.js";
  let { documents, engine, notice, theme, account = null, cloud = null, localCount = 0, onMoveLocal, onToggleTheme, onBook, onOpen, onNew, onImport, onRename, onDuplicate, onDownload, onDelete } = $props();
  let query = $state(""), menu = $state(null);
  const matches = d => !query || d.name.toLowerCase().includes(query.toLowerCase()) || d.text.toLowerCase().includes(query.toLowerCase());
  const sorted = $derived([...documents].sort((a, b) => b.updated - a.updated).filter(matches));
  const mine = $derived(sorted.filter(d => d.role !== "editor" && d.role !== "viewer"));
  const shared = $derived(sorted.filter(d => d.role === "editor" || d.role === "viewer"));
  let accountMenu = $state(false);
  const preview = text => text.split("\n").filter(l => l.trim() && !/^#/.test(l)).slice(0, 6).join("\n");
  function act(fn, d) { menu = null; fn(d); }
</script>

<svelte:window onmousedown={e => { if (menu && !e.target.closest?.(".doc-menu")) menu = null; if (accountMenu && !e.target.closest?.(".account-menu")) accountMenu = false; }} onkeydown={e => { if (e.key === "Escape") { menu = null; accountMenu = false; } }} />

{#snippet row(d)}
  <div class="doc-row" role="listitem">
    <button type="button" class="open" onclick={() => onOpen(d.id)}>
      <span class="doc-icon"><Icon name="doc" /></span>
      <span class="doc-name">{d.name || "Untitled document"}{#if d.role === "viewer"}<span class="doc-tag">View only</span>{/if}</span>
      <span class="doc-by">{#if d.owner && d.role !== "owner"}<span class="avatar tiny" style={`background:${colorFor(d.owner)}`}>{d.owner[0].toUpperCase()}</span>{d.owner}{/if}</span>
      <span class="doc-when">Edited {relativeTime(d.updated)}</span>
    </button>
    <div class="doc-menu">
      <button type="button" class="tool" aria-label={`Actions for ${d.name}`} aria-haspopup="true" aria-expanded={menu === d.id} onclick={() => (menu = menu === d.id ? null : d.id)}><Icon name="more" /></button>
      {#if menu === d.id}
        <div class="dropdown right" role="menu">
          {#if d.role !== "viewer"}<button type="button" role="menuitem" onclick={() => act(onRename, d)}><span class="mark"></span><span class="label">Rename</span></button>{/if}
          <button type="button" role="menuitem" onclick={() => act(onDuplicate, d)}><span class="mark"></span><span class="label">Make a copy</span></button>
          <button type="button" role="menuitem" onclick={() => act(onDownload, d)}><span class="mark"></span><span class="label">Download</span></button>
          {#if d.role === "owner" || !d.role}<hr><button type="button" role="menuitem" onclick={() => act(onDelete, d)}><span class="mark"></span><span class="label">Remove</span></button>{/if}
        </div>
      {/if}
    </div>
  </div>
{/snippet}

<div class="home">
  <header class="home-bar">
    <div class="brand"><span class="logo"><Icon name="doc" size={22} /></span> WTF Docs</div>
    <label class="search"><Icon name="search" /><input type="search" placeholder="Search documents" aria-label="Search documents" bind:value={query}></label>
    <button type="button" class="button" onclick={onBook}><Icon name="doc" /> The Book</button>
    <label class="button primary"><Icon name="upload" /> Import<input type="file" accept=".wtf,text/plain" multiple hidden onchange={onImport}></label>
    <button type="button" class="tool theme-toggle" title={theme === "dark" ? "Switch to light theme" : "Switch to dark theme"} aria-label="Toggle theme" onclick={onToggleTheme}><Icon name={theme === "dark" ? "sun" : "moon"} /></button>
    {#if account}
      <div class="account-menu">
        <button type="button" class="avatar-button" title={account.email} aria-label="Account" aria-haspopup="true" aria-expanded={accountMenu} onclick={() => (accountMenu = !accountMenu)}><span class="avatar" style={`background:${colorFor(account.email)}`}>{account.email[0].toUpperCase()}</span></button>
        {#if accountMenu}
          <div class="dropdown right account-dropdown" role="menu">
            <div class="account-card"><span class="avatar" style={`background:${colorFor(account.email)}`}>{account.email[0].toUpperCase()}</span><div><div class="account-name">{account.name || account.email.split("@")[0]}</div><div class="muted">{account.email}</div></div></div>
            <hr>
            <button type="button" role="menuitem" onclick={() => cloud.signOut()}><span class="mark"></span><span class="label">Sign out</span></button>
          </div>
        {/if}
      </div>
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

  <section class="recent" aria-label={account ? "My documents" : "Recent documents"}>
    <div class="section-head"><h2>{query ? "Matching documents" : account ? "My documents" : "Recent documents"}</h2><span class="muted">{mine.length}</span></div>
    {#if mine.length}<div class="doc-list" role="list">{#each mine as d (d.id)}{@render row(d)}{/each}</div>
    {:else}<p class="empty">{query ? `No documents match “${query}”.` : "Nothing yet. Start with a template above."}</p>{/if}
  </section>
  {#if account}
    <section class="recent shared" aria-label="Shared with me">
      <div class="section-head"><h2>Shared with me</h2><span class="muted">{shared.length}</span></div>
      {#if shared.length}<div class="doc-list" role="list">{#each shared as d (d.id)}{@render row(d)}{/each}</div>
      {:else}<p class="empty">Documents other people share with you appear here. Ask them to add <strong>{account.email}</strong> from a document's Share button.</p>{/if}
    </section>
  {/if}
  <footer class="home-foot">{engine} · {account ? `Documents are saved to ${account.email}.` : "Documents are stored in this browser. Download a copy to keep a backup."}</footer>
</div>
