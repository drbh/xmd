<script>
  // The start screen: templates, folders, your documents, and what others
  // shared with you. A folder opens in place, with a breadcrumb back.
  import Icon from "./Icon.svelte";
  import { TEMPLATES, relativeTime, colorFor } from "./store.js";
  let { documents, folders = [], thumbs = {}, engine, notice, onDismiss, trash = null, onRestore, theme, account = null, cloud = null, localCount = 0, canShare = false,
    onMoveLocal, onToggleTheme, onBook, onKeys, onOpen, onNew, onImport, onRename, onDuplicate, onDownload, onDelete,
    onNewFolder, onRenameFolder, onDeleteFolder, onShareFolder, onMove, current = $bindable(null) } = $props();
  let query = $state(""), menu = $state(null), accountMenu = $state(false);
  // The trash is loaded when opened; restoring or purging refreshes it.
  let trashOpen = $state(false), trashed = $state(null);
  async function loadTrash() { try { trashed = await trash.list(); } catch { trashed = []; } }
  $effect(() => { if (trashOpen && trash) loadTrash(); });
  $effect(() => { documents.length; if (trashOpen && trash) loadTrash(); });
  async function purge(d) { if (!confirm(`Delete “${d.name}” forever?`)) return; await trash.purge(d.id); await loadTrash(); }
  const folder = $derived(folders.find(f => f.id === current) ?? null);
  const matches = d => !query || d.name.toLowerCase().includes(query.toLowerCase()) || d.text.toLowerCase().includes(query.toLowerCase());
  const sorted = $derived([...documents].sort((a, b) => b.updated - a.updated).filter(matches));
  const isShared = x => x.role === "editor" || x.role === "viewer";
  const ownFolders = $derived(folders.filter(f => !isShared(f)));
  const sharedFolders = $derived(folders.filter(isShared));
  const inFolder = $derived(folder ? sorted.filter(d => d.folder === folder.id) : []);
  const mine = $derived(sorted.filter(d => !isShared(d) && (query || !d.folder || !folders.some(f => f.id === d.folder))));
  const shared = $derived(sorted.filter(d => isShared(d) && (query || !sharedFolders.some(f => f.id === d.folder))));
  const count = id => documents.filter(d => d.folder === id).length;
  function act(fn, x) { menu = null; fn(x); }
  function open(f) { current = f.id; query = ""; }
</script>

<svelte:window onmousedown={e => { if (menu && !e.target.closest?.(".doc-menu")) menu = null; if (accountMenu && !e.target.closest?.(".account-menu")) accountMenu = false; }} onkeydown={e => { if (e.key === "Escape") { menu = null; accountMenu = false; } }} />

{#snippet row(d)}
  <div class="doc-row" role="listitem">
    <button type="button" class="open" onclick={() => onOpen(d.id)}>
      <span class="doc-icon"><Icon name="doc" /></span>
      <span class="doc-name">{d.name || "Untitled document"}{#if d.file && d.file !== d.name}<span class="doc-file">{d.file}.wtf</span>{/if}{#if d.role === "viewer"}<span class="doc-tag">View only</span>{/if}{#if !folder && d.folder && folders.some(f => f.id === d.folder)}<span class="doc-tag">{folders.find(f => f.id === d.folder).name}</span>{/if}</span>
      <span class="doc-by">{#if d.owner && isShared(d)}<span class="avatar tiny" style={`background:${colorFor(d.owner)}`}>{d.owner[0].toUpperCase()}</span>{d.owner}{/if}</span>
      <span class="doc-when">Edited {relativeTime(d.updated)}</span>
    </button>
    <div class="doc-menu">
      <button type="button" class="tool" aria-label={`Actions for ${d.name}`} aria-haspopup="true" aria-expanded={menu === d.id} onclick={() => (menu = menu === d.id ? null : d.id)}><Icon name="more" /></button>
      {#if menu === d.id}
        <div class="dropdown right" role="menu">
          {#if d.role !== "viewer"}<button type="button" role="menuitem" onclick={() => act(onRename, d)}><span class="mark"></span><span class="label">Rename</span></button>{/if}
          <button type="button" role="menuitem" onclick={() => act(onDuplicate, d)}><span class="mark"></span><span class="label">Make a copy</span></button>
          <button type="button" role="menuitem" onclick={() => act(onDownload, d)}><span class="mark"></span><span class="label">Download</span></button>
          {#if !isShared(d) && ownFolders.length}
            <hr>
            <div class="menu-heading">Move to</div>
            {#each ownFolders as f (f.id)}<button type="button" role="menuitemradio" aria-checked={d.folder === f.id} onclick={() => act(() => onMove(d, f.id))}><span class="mark">{#if d.folder === f.id}✓{/if}</span><span class="label">{f.name}</span></button>{/each}
            <button type="button" role="menuitemradio" aria-checked={!d.folder} onclick={() => act(() => onMove(d, null))}><span class="mark">{#if !d.folder}✓{/if}</span><span class="label">No folder</span></button>
          {/if}
          {#if !isShared(d)}<hr><button type="button" role="menuitem" onclick={() => act(onDelete, d)}><span class="mark"></span><span class="label">Remove</span></button>{/if}
        </div>
      {/if}
    </div>
  </div>
{/snippet}

{#snippet folderCard(f)}
  <div class="folder-card" class:shared={isShared(f)}>
    <button type="button" class="folder-open" onclick={() => open(f)}>
      <span class="folder-icon"><Icon name="folder" size={22} /></span>
      <span class="folder-name">{f.name}</span>
      <span class="folder-meta">{count(f.id)} document{count(f.id) === 1 ? "" : "s"}{#if isShared(f)} · <span class="avatar tiny" style={`background:${colorFor(f.owner)}`}>{f.owner[0].toUpperCase()}</span>{f.owner}{/if}</span>
    </button>
    <div class="doc-menu">
      <button type="button" class="tool" aria-label={`Actions for folder ${f.name}`} aria-haspopup="true" aria-expanded={menu === f.id} onclick={() => (menu = menu === f.id ? null : f.id)}><Icon name="more" /></button>
      {#if menu === f.id}
        <div class="dropdown right" role="menu">
          {#if !isShared(f)}
            <button type="button" role="menuitem" onclick={() => act(onRenameFolder, f)}><span class="mark"></span><span class="label">Rename</span></button>
            {#if canShare}<button type="button" role="menuitem" onclick={() => act(onShareFolder, f)}><span class="mark"></span><span class="label">Share folder…</span></button>{/if}
            <hr>
            <button type="button" role="menuitem" onclick={() => act(onDeleteFolder, f)}><span class="mark"></span><span class="label">Delete folder</span></button>
          {:else}
            <button type="button" role="menuitem" onclick={() => act(onShareFolder, f)}><span class="mark"></span><span class="label">Who has access</span></button>
          {/if}
        </div>
      {/if}
    </div>
  </div>
{/snippet}

<div class="home">
  <header class="home-bar">
    <div class="brand"><span class="logo"><Icon name="doc" size={22} /></span> WTF Docs</div>
    <label class="search"><Icon name="search" /><input type="search" placeholder="Search documents" aria-label="Search documents" bind:value={query}></label>
    <button type="button" class="button" onclick={onBook} title="The Book"><Icon name="doc" /><span class="text">The Book</span></button>
    <label class="button primary" title="Import"><Icon name="upload" /><span class="text">Import</span><input type="file" accept=".wtf,text/plain" multiple hidden onchange={onImport}></label>
    <button type="button" class="tool theme-toggle" title={theme === "dark" ? "Switch to light theme" : "Switch to dark theme"} aria-label="Toggle theme" onclick={onToggleTheme}><Icon name={theme === "dark" ? "sun" : "moon"} /></button>
    {#if account}
      <div class="account-menu">
        <button type="button" class="avatar-button" title={account.email} aria-label="Account" aria-haspopup="true" aria-expanded={accountMenu} onclick={() => (accountMenu = !accountMenu)}><span class="avatar" style={`background:${colorFor(account.email)}`}>{account.email[0].toUpperCase()}</span></button>
        {#if accountMenu}
          <div class="dropdown right account-dropdown" role="menu">
            <div class="account-card"><span class="avatar" style={`background:${colorFor(account.email)}`}>{account.email[0].toUpperCase()}</span><div><div class="account-name">{account.name || account.email.split("@")[0]}</div><div class="muted">{account.email}</div></div></div>
            <hr>
            {#if onKeys}<button type="button" role="menuitem" onclick={() => { accountMenu = false; onKeys(); }}><span class="mark"></span><span class="label">API keys…</span></button>{/if}
            <button type="button" role="menuitem" onclick={() => cloud.signOut()}><span class="mark"></span><span class="label">Sign out</span></button>
          </div>
        {/if}
      </div>
    {:else if cloud?.signIn}
      <button type="button" class="button" onclick={() => cloud.signIn()}>Sign in</button>
    {/if}
  </header>
  {#if notice}<p class="notice" role="status">{notice}<button type="button" class="tool dismiss" aria-label="Dismiss" onclick={onDismiss}><Icon name="close" size={14} /></button></p>{/if}
  {#if account && cloud?.offline}<p class="notice" role="status">You're offline. Documents you opened before are available; changes are kept on this device and sent when the network returns.</p>{/if}
  {#if account && localCount}
    <p class="notice info" role="status">{localCount} document{localCount === 1 ? "" : "s"} saved in this browser before you signed in. <button type="button" class="link" onclick={onMoveLocal}>Move to your account</button></p>
  {/if}

  <section class="templates" aria-label="Start a new document">
    <div class="section-head"><h2>Start a new document{#if folder} in {folder.name}{/if}</h2><button type="button" class="link book-link" onclick={onBook}><span class="text">New here? Read the book: every feature as a live note</span><span class="short">Read the book</span> →</button></div>
    <div class="template-row">
      {#each TEMPLATES as t (t.id)}
        <button type="button" class="template" onclick={() => onNew(t, folder && !isShared(folder) ? folder.id : null)}>
          <span class="thumb" class:blank={t.id === "blank"}>{#if t.id === "blank"}<Icon name="plus" size={40} />{:else if thumbs[t.id]}<pre class="wtf" data-layout="document">{@html thumbs[t.id]}</pre>{/if}</span>
          <span class="label">{t.name}</span>
        </button>
      {/each}
    </div>
  </section>

  {#if folder}
    <section class="recent" aria-label={folder.name}>
      <div class="section-head">
        <h2 class="crumbs"><button type="button" class="link" onclick={() => (current = null)}>{isShared(folder) ? "Shared with me" : "My documents"}</button><span class="crumb-sep">›</span><Icon name="folder" /> {folder.name}{#if isShared(folder)}<span class="doc-tag">{folder.role === "viewer" ? "View only" : "Shared by " + folder.owner}</span>{/if}</h2>
        <span class="muted">{inFolder.length}</span>
      </div>
      {#if inFolder.length}<div class="doc-list" role="list">{#each inFolder as d (d.id)}{@render row(d)}{/each}</div>
      {:else}<p class="empty">This folder is empty. Start a document above or move one here from its ⋮ menu.</p>{/if}
    </section>
  {:else}
    {#if !query}
      <section class="recent" aria-label="Folders">
        <div class="section-head"><h2>Folders</h2><button type="button" class="link" onclick={onNewFolder}><Icon name="plus" size={14} /> New folder</button></div>
        {#if ownFolders.length}<div class="folder-grid">{#each ownFolders as f (f.id)}{@render folderCard(f)}{/each}</div>
        {:else}<p class="empty">Group documents into folders{canShare ? ", and share a whole folder at once" : ""}.</p>{/if}
      </section>
    {/if}
    <section class="recent" aria-label={account ? "My documents" : "Recent documents"}>
      <div class="section-head"><h2>{query ? "Matching documents" : account ? "My documents" : "Recent documents"}</h2><span class="muted">{mine.length}</span></div>
      {#if mine.length}<div class="doc-list" role="list">{#each mine as d (d.id)}{@render row(d)}{/each}</div>
      {:else}<p class="empty">{query ? `No documents match “${query}”.` : "Nothing yet. Start with a template above."}</p>{/if}
    </section>
    {#if account}
      <section class="recent shared" aria-label="Shared with me">
        <div class="section-head"><h2>Shared with me</h2><span class="muted">{sharedFolders.length + shared.length}</span></div>
        {#if sharedFolders.length && !query}<div class="folder-grid">{#each sharedFolders as f (f.id)}{@render folderCard(f)}{/each}</div>{/if}
        {#if shared.length}<div class="doc-list" role="list">{#each shared as d (d.id)}{@render row(d)}{/each}</div>
        {:else if !sharedFolders.length}<p class="empty">Documents and folders other people share with you appear here. Ask them to add <strong>{account.email}</strong> from a Share button.</p>{/if}
      </section>
    {/if}
  {/if}
  {#if trash && !query && !folder}
    <section class="recent trash-section" aria-label="Trash">
      <div class="section-head"><h2><button type="button" class="link" onclick={() => (trashOpen = !trashOpen)}><Icon name="expand" size={14} /> Trash</button></h2>{#if trashOpen && trashed}<span class="muted">{trashed.length}</span>{/if}</div>
      {#if trashOpen}
        {#if trashed === null}<p class="empty">Loading…</p>
        {:else if trashed.length}
          <div class="doc-list" role="list">
            {#each trashed as d (d.id)}
              <div class="doc-row" role="listitem">
                <div class="open static"><span class="doc-icon"><Icon name="doc" /></span><span class="doc-name">{d.name}{#if d.file && d.file !== d.name}<span class="doc-file">{d.file}.wtf</span>{/if}</span><span class="doc-by"></span><span class="doc-when">Removed {relativeTime(d.deleted)}</span></div>
                <button type="button" class="link" onclick={() => onRestore(d)}>Restore</button>
                <button type="button" class="link danger" onclick={() => purge(d)}>Delete forever</button>
              </div>
            {/each}
          </div>
        {:else}<p class="empty">The trash is empty. Removed documents wait here until you delete them for good.</p>{/if}
      {/if}
    </section>
  {/if}
  <footer class="home-foot">{engine} · {account ? `Documents are saved to ${account.email}.` : "Documents are stored in this browser. Download a copy to keep a backup."}</footer>
</div>
