<script>
  import { onMount, untrack } from "svelte";
  import { createWorkspace, EXTENSION, noteFile, noteStem } from "@xmd/web";
  import { titleOf, uriOf as documentUri, createDocument, createFolder, fileNameFor, uniqueFile, loadPrefs, savePrefs, relativeTime, colorFor, TEMPLATES } from "./lib/store.js";
  import { resolveBackend } from "./lib/backend.js";
  import { createCommands, matches, shortcutLabel, isMac } from "./lib/commands.js";
  import { lineStyle, reveal, stats } from "./lib/editing.js";
  import Editor from "./lib/Editor.svelte";
  import Outline from "./lib/Outline.svelte";
  import MenuBar from "./lib/MenuBar.svelte";
  import Toolbar from "./lib/Toolbar.svelte";
  import FindBar from "./lib/FindBar.svelte";
  import Dialog from "./lib/Dialog.svelte";
  import Home from "./lib/Home.svelte";
  import Icon from "./lib/Icon.svelte";
  import Console from "./lib/Console.svelte";
  import Share from "./lib/Share.svelte";
  import Keys from "./lib/Keys.svelte";

  let documents = $state([]);
  let folders = $state([]);
  // A document's address in the engine follows its folder and file name.
  const uriOf = d => documentUri(d, folders);
  const byUri = uri => documents.find(d => uriOf(d) === uri);
  let thumbs = $state.raw({});
  let shareFolder = $state(null);
  let keysOpen = $state(false);
  let homeFolder = $state(null);
  let backends = $state.raw(null);
  const backend = $derived(backends?.backend);
  const account = $derived(backends?.cloud?.account ?? null);
  let localCount = $state(0);
  let prefs = $state(loadPrefs());
  let activeId = $state(null);
  let view = $state("home"); // "home" | "doc"
  let engine = $state("Starting the engine…");
  let engineVersion = $state("");
  let saved = $state("");
  let notice = $state("");
  let symbols = $state([]);
  let problems = $state([]);
  let caretLine = $state(-1);
  let style = $state({ kind: "text" });
  let find = $state(null);
  let dialog = $state(null);
  let ready = $state(false), controller = $state(null);
  // Live editing: one session per open cloud document, from the backend when it offers it.
  let live = $state.raw(null), liveStatus = $state(""), people = $state.raw([]);
  $effect(() => {
    // A room needs the document to exist on the server: wait for the first save (version) before joining.
    const id = activeId, persisted = active?.version !== undefined;
    const session = untrack(() => backend?.collaborate && active && persisted && active.role !== "link" ? backend.collaborate({ id, role: active.role }) : null);
    if (!session) { live = null; liveStatus = ""; people = []; return; }
    let current = null, cancelled = false, stops = [];
    session.then(s => {
      if (cancelled) { s.destroy(); return; }
      current = s; live = s;
      stops.push(s.onStatus(status => (liveStatus = status)), s.onPresence(list => (people = list)));
    }).catch(error);
    return () => { cancelled = true; for (const stop of stops) stop(); current?.destroy(); live = null; liveStatus = ""; people = []; };
  });
  let now = $state(Date.now());
  let titleInput = $state(null), sidebarOpenMobile = $state(false), accountMenu = $state(false);
  const active = $derived(documents.find(d => d.id === activeId));
  const counts = $derived(active ? stats(active.text) : null);
  const readOnly = $derived(active?.role === "viewer" || active?.role === "link" || liveStatus === "locked");
  let online = $state(navigator.onLine);
  let update = $state(null);
  const symbolNames = $derived.by(() => { const out = new Set(); const walk = list => { for (const s of list) { if (/^[A-Za-z_]\w*$/.test(s.name)) out.add(s.name); walk(s.children || []); } }; walk(symbols); return [...out]; });
  const systemDark = matchMedia("(prefers-color-scheme: dark)");
  let dark = $state(systemDark.matches);
  const theme = $derived(prefs.theme === "system" ? (dark ? "dark" : "light") : prefs.theme);
  $effect(() => { document.documentElement.dataset.theme = theme; document.documentElement.classList.toggle("xmd-light", theme === "light"); });
  $effect(() => { savePrefs($state.snapshot(prefs)); });
  $effect(() => { document.title = active ? `${active.name || "Untitled document"} – XMD Docs` : "XMD Docs"; });

  const workspace = createWorkspace({ onError: e => { engine = `Engine failed: ${e.message}`; } });
  const rpc = workspace.request;
  onMount(() => {
    const unsubscribe = workspace.onChange(persist);
    let unsubscribeBackend = () => {};
    const onScheme = e => (dark = e.matches);
    systemDark.addEventListener("change", onScheme);
    const tick = setInterval(() => (now = Date.now()), 30_000);
    (async () => {
      backends = await resolveBackend();
      if (backends.cloud?.offline) online = false;
      unsubscribeBackend = backend.subscribe(event => {
        if (event.type === "paused") { saved = event.message; notice = event.message; paused = true; }
        if (event.type === "online" || event.type === "offline") { online = event.type === "online"; saved = ""; }
      });
      documents = await backend.list();
      folders = await backend.listFolders();
      // Older documents have no file name yet; derive one, keeping names unique per folder.
      for (const d of documents) if (!d.file) d.file = uniqueFile(fileNameFor(d.name), d.folder, documents, d.id);
      // A file name follows the heading until someone renames the document on purpose.
      for (const d of documents) if (!d.named && d.role !== "viewer" && d.role !== "editor" && fileNameFor(d.name) !== d.file) {
        d.file = uniqueFile(fileNameFor(d.name), d.folder, documents, d.id);
        backend.file?.($state.snapshot(d)).catch(() => {});
      }
      if (backends.cloud?.account) localCount = (await backends.local.stored()).length;
      for (const d of documents) await workspace.setDocument(uriOf(d), d.text);
      // Template thumbnails are the templates themselves, resolved by the engine
      // before any editor mounts, so nothing else is repainting meanwhile.
      const rendered = {};
      for (const t of TEMPLATES) {
        const uri = `file:///workspace/templates/${noteFile(t.id)}`;
        await workspace.setDocument(uri, t.text);
        rendered[t.id] = (await workspace.analyze(uri, { force: true, editing: false }))?.html ?? "";
      }
      thumbs = rendered;
      route();
      ready = true;
      engine = "Rust / WebAssembly · runs in this tab";
    })().catch(e => { engine = `Engine failed: ${e.message}`; notice = e.message; });
    return () => { unsubscribe(); unsubscribeBackend(); systemDark.removeEventListener("change", onScheme); clearInterval(tick); clearTimeout(saveTimer); workspace.destroy(); };
  });

  // Routing: the home screen is "#/", a document is "#/d/<id>".
  function idFromHash() { const m = /^#\/d\/([\w-]+)/.exec(location.hash); return m && documents.some(d => d.id === m[1]) ? m[1] : null; }
  function route() {
    const link = /^#\/s\/([A-Za-z0-9_-]+)/.exec(location.hash);
    if (link) { openShared(link[1]); return; }
    activeId = idFromHash();
    view = activeId ? "doc" : "home";
  }
  $effect(() => {
    if (!ready) return;
    const hash = active?.role === "link" ? `#/s/${active.token}` : activeId ? `#/d/${activeId}` : "#/";
    if (location.hash !== hash) history.pushState(null, "", hash);
  });
  function onHashChange() { route(); }
  // Leaving a document is when its file name catches up with its heading.
  async function settleFile(d) {
    if (!d || d.named || (d.role && d.role !== "owner")) return;
    const wanted = fileNameFor(d.name);
    if (wanted === d.file) return;
    const before = uriOf(d);
    d.file = uniqueFile(wanted, d.folder, documents, d.id);
    await readdress(d, before);
    await backend.file?.($state.snapshot(d)).catch(error);
  }
  function open(id) {
    const d = documents.find(x => x.id === id);
    if (!d) return;
    if (active && active.id !== id) settleFile(active);
    d.opened = Date.now();
    activeId = id; view = "doc"; find = null; dialog = null; symbols = []; problems = []; caretLine = -1;
  }
  // A document shared by link: fetched without an account, shown read-only, never saved here.
  async function openShared(token) {
    try {
      const module = await import(/* @vite-ignore */ new URL("./backend.js", document.baseURI).href);
      const shared = await module.sharedDocument(token);
      const d = { ...shared, token, folder: null, updated: shared.updated ?? Date.now() };
      documents = [d, ...documents.filter(x => x.id !== d.id)];
      await workspace.setDocument(uriOf(d), d.text);
      activeId = d.id; view = "doc"; find = null; dialog = null; symbols = []; problems = []; caretLine = -1;
    } catch (e) { view = "home"; activeId = null; notice = e.message || "This link no longer works"; }
  }
  function home() { const leaving = active; activeId = null; view = "home"; find = null; dialog = null; settleFile(leaving); }

  // Saving: edits are debounced, then handed to the backend one document at a
  // time. A conflict or unavailable store pauses saving for that document and
  // says so; the writer's text is never overwritten from here.
  let saveTimer, paused = $state(false);
  const dirty = new Set();
  function persist({ uri, source }) {
    const document = byUri(uri);
    if (!document || document.text === source) return;
    document.text = source;
    document.name = titleOf(source, document.name);
    document.updated = Date.now();
    now = document.updated;
    // A live document is saved by its room, so there is nothing to schedule; a linked one is not ours.
    if ((live && live.id === document.id) || document.role === "link") return;
    schedule(document);
  }
  function schedule(document) {
    dirty.add(document.id);
    if (saved !== "Saving…" && saved !== backend?.label) notice = ""; // a new edit clears an old error
    saved = "Saving…";
    clearTimeout(saveTimer);
    saveTimer = setTimeout(flush, 300);
  }
  async function flush() {
    clearTimeout(saveTimer);
    if (!backend || paused) return;
    const ids = [...dirty]; dirty.clear();
    for (const id of ids) {
      const document = documents.find(d => d.id === id);
      if (!document || document.stale) continue;
      try {
        const result = await backend.save($state.snapshot(document));
        if (result?.version !== undefined) document.version = result.version;
        saved = backend.label;
      } catch (e) {
        if (e.code === "conflict") { document.stale = true; saved = "Changed elsewhere; download your edits, then reload to see the latest"; }
        else saved = e.message || "Saving paused; download your changes";
        notice = saved;
      }
    }
  }
  async function newDocument(template = TEMPLATES[0], folder = null) {
    const d = createDocument(template, template.id === "blank" ? "Untitled document" : undefined);
    d.folder = folder;
    d.file = uniqueFile(d.file, folder, documents);
    // A second "Trip budget" is "Trip budget 2" in its heading too, so lists tell them apart.
    if (d.file !== fileNameFor(d.name)) { d.text = d.text.replace(/^(#+\s+)(.*?)(\s+:\w+)?$/m, (m, hashes, _title, tag) => `${hashes}${d.file}${tag || ""}`); d.name = titleOf(d.text, d.file); }
    documents = [d, ...documents];
    await workspace.setDocument(uriOf(d), d.text);
    schedule(d);
    open(d.id);
    // Land the caret on the empty line after the heading so typing starts immediately.
    queueMicrotask(() => setTimeout(() => controller?.select(d.text.length), 50));
  }
  async function duplicate(d = active) {
    if (!d) return;
    const copy = { ...createDocument({ text: d.text }), name: `Copy of ${d.name}`, folder: d.role && d.role !== "owner" ? null : d.folder ?? null };
    copy.file = uniqueFile(`Copy of ${d.file ?? fileNameFor(d.name)}`, copy.folder, documents);
    documents = [copy, ...documents];
    await workspace.setDocument(uriOf(copy), copy.text);
    schedule(copy);
    open(copy.id);
  }
  async function remove(d = active) {
    if (!d || !confirm(`Move "${d.name}" to the trash?`)) return;
    documents = documents.filter(x => x.id !== d.id);
    if (activeId === d.id) home();
    dirty.delete(d.id);
    backend.delete(d.id).catch(error);
    try { await rpc("removeDocument", { uri: uriOf(d) }); } catch { /* the workspace may not know it yet */ }
  }
  // Re-address a document in the engine after its file name or folder changes.
  async function readdress(d, before) {
    const after = uriOf(d);
    if (after === before) return;
    try { await rpc("removeDocument", { uri: before }); } catch { /* not open yet */ }
    await workspace.setDocument(after, d.text);
  }
  function download(d = active) {
    if (!d) return;
    const blob = new Blob([d.text], { type: "text/plain" });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = noteFile(d.name.replace(/[^\w.-]+/g, "-") || "document");
    a.click();
    URL.revokeObjectURL(a.href);
  }
  async function importFiles(event) {
    let last;
    for (const file of event.target.files) {
      const text = await file.text();
      const d = createDocument({ text }, titleOf(text, noteStem(file.name)));
      documents = [d, ...documents];
      d.file = uniqueFile(fileNameFor(d.name), null, documents);
      await workspace.setDocument(uriOf(d), text);
      dirty.add(d.id);
      last = d.id;
    }
    flush();
    event.target.value = "";
    if (last) open(last);
  }
  let importInput;
  // Renaming edits the document's first heading when it has one, so the title and text stay in step.
  async function rename(d = active, name) {
    if (!d) return;
    if (name === undefined) {
      if (d.id === activeId && titleInput) { titleInput.focus(); titleInput.select(); return; }
      name = prompt("Rename document", d.name);
      if (name === null) return;
    }
    name = name.trim() || "Untitled document";
    const before = uriOf(d);
    const lines = d.text.split("\n");
    const at = lines.findIndex(l => /^#+\s+\S/.test(l));
    if (at !== -1) {
      const line = lines[at];
      const m = /^(#+\s+)(.*?)(\s+:\w+)?$/.exec(line);
      const next = `${m[1]}${name}${m[3] || ""}`;
      const start = lines.slice(0, at).reduce((n, l) => n + l.length + 1, 0);
      if (d.id === activeId && controller) await controller.replaceRange(start, start + line.length, next, { anchor: start + next.length }).catch(error);
      else { lines[at] = next; d.text = lines.join("\n"); await workspace.setDocument(before, d.text); }
    }
    // An explicit rename also renames the file, which is what imports refer to.
    const wanted = fileNameFor(name);
    d.name = name; d.file = uniqueFile(wanted, d.folder, documents, d.id); d.named = true; d.updated = Date.now();
    if (d.file !== wanted) notice = `Another document here is already called “${wanted}”, so this one's file is “${d.file}”.`;
    await readdress(d, before);
    dirty.add(d.id);
    await flush();
    if (backend.file) await backend.file($state.snapshot(d)).catch(error);
  }
  // Trash: a removed document comes back with a free file name, into its folder if that still exists.
  async function restore(d) {
    try {
      const result = await backend.trash.restore(d.id);
      const back = { ...d, folder: result.folder ?? null, file: result.file ?? d.file, deleted: undefined, role: d.role ?? "owner" };
      documents = [back, ...documents];
      await workspace.setDocument(uriOf(back), back.text);
      notice = `Restored “${back.name}”.`;
    } catch (e) { error(e); }
  }
  // Folders group documents; sharing a folder shares everything in it.
  async function newFolder(name = prompt("Folder name")) {
    name = (name ?? "").trim();
    if (!name) return;
    const f = createFolder(name);
    const saved = await backend.saveFolder(f).catch(error);
    if (saved) folders = [...folders, { ...f, ...saved }];
  }
  async function renameFolder(f, name = prompt("Rename folder", f.name)) {
    name = (name ?? "").trim();
    if (!name || name === f.name) return;
    const inside = documents.filter(d => d.folder === f.id).map(d => [d, uriOf(d)]);
    f.name = name; f.updated = Date.now();
    await backend.saveFolder($state.snapshot(f)).catch(error);
    for (const [d, before] of inside) await readdress(d, before);
  }
  async function deleteFolder(f) {
    if (!confirm(`Delete the folder "${f.name}"? Its documents stay and are just unfiled.`)) return;
    const inside = documents.filter(d => d.folder === f.id).map(d => [d, uriOf(d)]);
    folders = folders.filter(x => x.id !== f.id);
    for (const [d] of inside) { d.folder = null; d.file = uniqueFile(d.file, null, documents, d.id); }
    await backend.deleteFolder(f.id).catch(error);
    for (const [d, before] of inside) await readdress(d, before);
  }
  async function moveDocument(d, folder) {
    const before = uriOf(d);
    d.folder = folder;
    d.file = uniqueFile(d.file ?? fileNameFor(d.name), folder, documents, d.id);
    await readdress(d, before);
    await backend.file($state.snapshot(d)).catch(error);
  }
  // Documents saved in this browser before signing in can move to the account.
  async function moveLocal() {
    try {
      const local = await backends.local.stored();
      for (let d of local) {
        if (documents.some(x => x.id === d.id)) continue;
        d = { ...d, folder: null, version: undefined, file: uniqueFile(d.file ?? fileNameFor(d.name), null, documents) };
        // An id the account cannot write to (taken elsewhere) gets a fresh one.
        try { await backends.cloud.save(d); }
        catch { d = { ...d, id: crypto.randomUUID() }; await backends.cloud.save(d); }
        documents = [d, ...documents];
        await workspace.setDocument(uriOf(d), d.text);
      }
      await backends.local.clear();
      localCount = 0;
      notice = `${local.length} document${local.length === 1 ? "" : "s"} moved to your account.`;
    } catch (e) { error(e); }
  }
  function jump(symbol) {
    if (!controller) return;
    const lines = controller.getSource().split("\n");
    const offset = lines.slice(0, symbol.selectionRange.start.line).reduce((n, l) => n + l.length + 1, 0) + symbol.selectionRange.start.character;
    controller.select(offset);
    reveal(controller);
    sidebarOpenMobile = false;
  }
  function onCaret(line, selection) {
    caretLine = line;
    const source = controller?.getSource() ?? "";
    style = line >= 0 ? lineStyle(source.split("\n")[line] || "") : { kind: "text" };
  }
  const error = e => { notice = e.message; saved = e.message; };

  const commands = createCommands({
    editor: () => controller,
    error,
    prefs: () => prefs,
    toggle: key => (prefs[key] = !prefs[key]),
    theme: value => (prefs.theme = value),
    zoom: delta => (prefs.zoom = delta ? Math.max(50, Math.min(200, prefs.zoom + delta)) : 100),
    fullscreen: () => (document.fullscreenElement ? document.exitFullscreen() : document.documentElement.requestFullscreen?.()),
    newDocument: () => newDocument(),
    home, duplicate: () => duplicate(), remove: () => remove(), download: () => download(), rename: () => rename(),
    importFiles: () => importInput?.click(),
    print: () => print(),
    find: replace => (find = { replace }),
    dialog: name => (dialog = name),
    canShare: () => !!backend?.acl,
    readOnly: () => readOnly,
  });
  function keydown(event) {
    if (!active) return;
    if (event.target.closest?.(".findbar, dialog, .title-input, .console")) return;
    if ((isMac ? event.metaKey : event.ctrlKey) && event.code === "KeyS" && !event.shiftKey) { event.preventDefault(); flush(); return; }
    if (event.key === "Escape") { find = null; return; }
    for (const c of commands.list) if (c.shortcut && !c.native && matches(event, c.shortcut)) { event.preventDefault(); c.run(); return; }
  }
  const SYNTAX = [
    ["# Heading", "Headings structure the outline; `## Section :tag` names a section"],
    ["$1,234:rent", "A named value: money, dates, times, and durations are recognised"],
    ["total := rent + food", "A formula; its result appears inline and recalculates as you type"],
    ["[total]", "Show a value anywhere in prose"],
    ["- [ ] Task @due(2026-10-01)", "A task; click the box to complete it, attach a due date or @estimate(30m)"],
    ["focus := countdown(25m)", "A timer with controls in the Actions panel"],
    ["items := table", "Followed by a pipe table; use sum(items, quantity * price) over it"],
    ["**bold** _italic_ `code`", "Inline emphasis"],
    [`other := import("./${noteFile("other")}")`, "Reference another document's values as [other.total]"],
    ["<!-- note -->", "A comment that never renders a value"],
  ];
  if (new URLSearchParams(location.search).has("test")) {
    window.xmdDocs = { get documents() { return documents; }, get backend() { return backend; }, get account() { return account; }, get controller() { return controller; }, get active() { return active; }, rpc, workspace, newDocument, newFolder, moveDocument, get folders() { return folders; }, open, home, get live() { return live; }, get people() { return people; }, get ready() { return ready; } };
  }
</script>

<svelte:window onkeydown={keydown} onhashchange={onHashChange} ononline={() => (online = true)} onoffline={() => (online = false)} onxmd:update={e => (update = e.detail)} onmousedown={e => { if (accountMenu && !e.target.closest?.(".account-menu")) accountMenu = false; }} />
<input bind:this={importInput} type="file" accept=".{EXTENSION.split(".").pop()},text/markdown,text/plain" multiple hidden onchange={importFiles}>

{#if keysOpen && backend?.keys}
  <Keys keys={backend.keys} site={new URL("../", document.baseURI).href.replace(/\/$/, "")} onClose={() => (keysOpen = false)} />
{/if}
{#if shareFolder && backend?.folderAcl}
  <Share acl={backend.folderAcl(shareFolder.id)} name={shareFolder.name} kind="folder" onClose={() => (shareFolder = null)} />
{/if}
{#if !ready}
  <div class="splash"><p>{engine}</p></div>
{:else if !active}
  <Home {documents} {folders} {thumbs} {engine} {notice} bind:current={homeFolder} onDismiss={() => (notice = "")} trash={backend?.trash} onRestore={restore} {theme} {account} onKeys={backend?.keys ? () => (keysOpen = true) : null} onNewFolder={() => newFolder()} onRenameFolder={f => renameFolder(f)} onDeleteFolder={deleteFolder} onShareFolder={f => (shareFolder = f)} onMove={moveDocument} canShare={!!backend?.folderAcl} {localCount} cloud={backends?.cloud} onMoveLocal={moveLocal} onToggleTheme={() => (prefs.theme = theme === "dark" ? "light" : "dark")} onOpen={open} onNew={newDocument} onImport={importFiles} onRename={d => rename(d)} onDuplicate={duplicate} onDownload={download} onDelete={remove} />
{:else}
  <div class="app" class:pageless={prefs.pageless} class:no-outline={!prefs.outline} class:read-only={readOnly} style={`--zoom:${prefs.zoom / 100}`}>
    <header class="chrome">
      <div class="titlebar">
        <button type="button" class="logo" title="Back to documents" aria-label="Documents home" onclick={home}><Icon name="doc" size={26} /></button>
        <div class="title-block">
          <div class="title-row">
            {#if active.folder && folders.some(f => f.id === active.folder)}
              <button type="button" class="crumb" title="Open folder" onclick={() => { const f = active.folder; home(); homeFolder = f; }}><Icon name="folder" size={14} /> {folders.find(f => f.id === active.folder).name}<span class="crumb-sep">›</span></button>
            {/if}
            <input bind:this={titleInput} class="title-input" aria-label="Document title" value={active.name} spellcheck="false" readonly={readOnly}
              onchange={e => rename(active, e.target.value)} onkeydown={e => { if (e.key === "Enter") { e.preventDefault(); e.target.blur(); controller?.select(controller.selection()?.focus ?? 0); } }}>
            <span class="status" role="status" title={saved}>
              <Icon name={saved.startsWith("Saved") ? "cloud" : saved ? "warning" : "cloud"} size={16} />
              <span class="status-text">{liveStatus === "locked" ? "Open this document online once to edit it offline" : active.role === "link" ? `Shared by ${active.owner} · view only` : readOnly ? "View only" : saved || (ready ? backend?.label : engine)}</span>
            </span>
            {#if !online}
              <span class="live off" title="Working offline. Changes are kept on this device and sent when the network returns."><span class="dot"></span><span>Offline</span></span>
            {:else if live}
              <span class="live" class:off={liveStatus !== "connected"} title={liveStatus === "connected" ? "Edits sync in real time" : "Reconnecting…"}><span class="dot"></span><span>{liveStatus === "connected" ? "Live" : "Reconnecting…"}</span></span>
            {/if}
          </div>
          <MenuBar menus={commands.menus} />
        </div>
        <div class="title-actions">
          <span class="edited">Last edit {relativeTime(active.updated, now)}</span>
          {#if live}
            <div class="people-here" title={[...people.map(p => p.user.name), "you"].join(", ")} aria-label={`${people.length + 1} people in this document`}>
              {#each people.slice(0, 8) as p (p.clientId)}<span class="avatar small" style={`background:${p.user.color}`} title={p.user.email || p.user.name}>{(p.user.name || "?")[0].toUpperCase()}</span>{/each}
              {#if people.length > 8}<span class="avatar small more">+{people.length - 8}</span>{/if}
              <span class="avatar small you" style={`background:${colorFor(account?.email)}`} title="You">{(account?.email || "?")[0].toUpperCase()}</span>
            </div>
          {/if}
          {#if backend?.acl && !readOnly}
            <button type="button" class="button primary share-button" onclick={() => (dialog = "share")}><Icon name="link" /> Share</button>
          {:else}
            <button type="button" class="button" onclick={() => download()}><Icon name="upload" /> Download</button>
          {/if}
          <button type="button" class="tool theme-toggle" title={theme === "dark" ? "Switch to light theme" : "Switch to dark theme"} aria-label="Toggle theme" onclick={() => (prefs.theme = theme === "dark" ? "light" : "dark")}><Icon name={theme === "dark" ? "sun" : "moon"} /></button>
          {#if account}
            <div class="account-menu">
              <button type="button" class="avatar-button" title={account.email} aria-label="Account" aria-haspopup="true" aria-expanded={accountMenu} onclick={() => (accountMenu = !accountMenu)}><span class="avatar" style={`background:${colorFor(account.email)}`}>{account.email[0].toUpperCase()}</span></button>
              {#if accountMenu}
                <div class="dropdown right account-dropdown" role="menu">
                  <div class="account-card"><span class="avatar" style={`background:${colorFor(account.email)}`}>{account.email[0].toUpperCase()}</span><div><div class="account-name">{account.name || account.email.split("@")[0]}</div><div class="muted">{account.email}</div></div></div>
                  <hr>
                  <button type="button" role="menuitem" onclick={() => { accountMenu = false; home(); }}><span class="mark"></span><span class="label">All documents</span></button>
                  <button type="button" role="menuitem" onclick={() => { accountMenu = false; download(); }}><span class="mark"></span><span class="label">Download this document</span></button>
                  {#if backend?.keys}<button type="button" role="menuitem" onclick={() => { accountMenu = false; dialog = "keys"; }}><span class="mark"></span><span class="label">API keys…</span></button>{/if}
                  <button type="button" role="menuitem" onclick={() => backends.cloud.signOut()}><span class="mark"></span><span class="label">Sign out</span></button>
                </div>
              {/if}
            </div>
          {/if}
        </div>
      </div>
      <Toolbar commands={commands.byId} {style} zoom={prefs.zoom} outline={prefs.outline} onZoom={z => (prefs.zoom = z)} />
    </header>

    <div class="workspace">
      <aside class="sidebar" class:mobile-open={sidebarOpenMobile} aria-label="Document outline">
        <section>
          <h2>Outline</h2>
          <Outline {symbols} line={caretLine} onJump={jump} />
        </section>
        {#if problems.length}
          <section>
            <h2>Problems <span class="badge">{problems.length}</span></h2>
            <ul class="problems">
              {#each problems as p}<li><button type="button" class={p.severity === 2 ? "warn" : "error"} onclick={() => jump({ selectionRange: p.range })}><span>Line {p.range.start.line + 1}</span>{p.message}</button></li>{/each}
            </ul>
          </section>
        {/if}
      </aside>

      <!-- svelte-ignore a11y_no_noninteractive_element_interactions, a11y_click_events_have_key_events -->
      <main class="canvas" onclick={e => { if (e.target === e.currentTarget && controller) { controller.select(controller.getSource().length); } }}>
        {#if ready}
          {#key uriOf(active)}
            <Editor {workspace} uri={uriOf(active)} text={active.text} {readOnly} {live}
              onSnapshot={(snapshot, list) => { symbols = snapshot.symbols || []; problems = list; engineVersion = snapshot.engineVersion || ""; }}
              {onCaret} onError={error} bind:controller />
          {/key}
        {:else}
          <div class="page loading"><p>{engine}</p></div>
        {/if}
      </main>
    </div>

    {#if prefs.console}<Console {rpc} uri={uriOf(active)} names={symbolNames} bind:height={prefs.consoleHeight} onClose={() => { prefs.console = false; controller?.element.focus(); }} />{/if}
    {#if find}<FindBar {controller} replace={find.replace} onClose={at => { find = null; controller?.select(at); }} />{/if}
    {#if prefs.wordCount && counts}<button type="button" class="chip words" title="Word count" onclick={() => (dialog = "stats")}>{counts.words} words</button>{/if}
    {#if problems.length}<button type="button" class="chip problems-chip" onclick={() => (dialog = "problems")}><Icon name="warning" size={14} /> {problems.length} problem{problems.length === 1 ? "" : "s"}</button>{/if}
    {#if update}<button type="button" class="chip update-chip" onclick={() => { update.waiting?.postMessage("skip-waiting"); navigator.serviceWorker?.addEventListener("controllerchange", () => location.reload(), { once: true }); }}>A new version is ready · Reload</button>{/if}
    {#if !prefs.console}<button type="button" class="chip console-chip" onclick={() => (prefs.console = true)}><Icon name="code" size={14} /> Console</button>{/if}
    <button type="button" class="chip outline-toggle" aria-label="Toggle outline" onclick={() => (sidebarOpenMobile = !sidebarOpenMobile)}><Icon name="outline" size={16} /></button>

    {#if dialog === "stats" && counts}
      <Dialog title="Word count" onClose={() => (dialog = null)}>
        <table class="stats">
          <tbody>
            <tr><td>Words</td><td>{counts.words}</td></tr>
            <tr><td>Characters</td><td>{counts.characters}</td></tr>
            <tr><td>Characters excluding spaces</td><td>{counts.charactersNoSpaces}</td></tr>
            <tr><td>Paragraphs</td><td>{counts.paragraphs}</td></tr>
            <tr><td>Headings</td><td>{counts.headings}</td></tr>
            <tr><td>Tasks</td><td>{counts.done} of {counts.tasks} done</td></tr>
            <tr><td>Reading time</td><td>about {counts.readingMinutes} min</td></tr>
          </tbody>
        </table>
        <label class="option"><input type="checkbox" bind:checked={prefs.wordCount}> Display word count while typing</label>
      </Dialog>
    {:else if dialog === "share" && backend?.acl}
      <Share acl={backend.acl(active.id)} link={backend.link?.(active.id)} name={active.name} onClose={() => (dialog = null)} />
    {:else if dialog === "keys" && backend?.keys}
      <Keys keys={backend.keys} site={new URL("../", document.baseURI).href.replace(/\/$/, "")} onClose={() => (dialog = null)} />
    {:else if dialog === "details"}
      <Dialog title="Document details" onClose={() => (dialog = null)}>
        <table class="stats"><tbody>
          <tr><td>File</td><td><code>{noteFile(active.file)}</code></td></tr>
          <tr><td>Folder</td><td>{folders.find(f => f.id === active.folder)?.name ?? "—"}</td></tr>
          <tr><td>Import as</td><td><code>import("./{noteFile(active.file)}")</code></td></tr>
          <tr><td>Your access</td><td>{active.role ?? "owner"}{#if active.owner && active.role !== "owner"} · shared by {active.owner}{/if}</td></tr>
          <tr><td>Last edit</td><td>{new Date(active.updated).toLocaleString()}</td></tr>
          <tr><td>Link</td><td><code>{location.href.replace(/\?test/, "")}</code></td></tr>
        </tbody></table>
        <p class="muted">The file name is what other documents and <code>xmd sync</code> use; it follows the first heading until you rename the document yourself.</p>
      </Dialog>
    {:else if dialog === "problems"}
      <Dialog title="Problems" onClose={() => (dialog = null)}>
        {#if problems.length}
          <ul class="problems large">{#each problems as p}<li><button type="button" class={p.severity === 2 ? "warn" : "error"} onclick={() => { dialog = null; jump({ selectionRange: p.range }); }}><span>Line {p.range.start.line + 1}</span>{p.message}</button></li>{/each}</ul>
        {:else}<p>No problems. Every value in this document resolves.</p>{/if}
      </Dialog>
    {:else if dialog === "engine"}
      <Dialog title="About the engine" onClose={() => (dialog = null)}>
        <p>{engine}{engineVersion ? ` · v${engineVersion}` : ""}</p>
        <p>Documents are parsed and evaluated locally by the same Rust engine that powers the language server. Nothing you write leaves this browser. Documents are kept in this browser's storage; use <strong>File → Download</strong> to keep a copy on disk.</p>
        <p>Names are local to each document. To use a value from another document, insert an import and reference it as <code>[other.name]</code>.</p>
      </Dialog>
    {:else if dialog === "syntax"}
      <Dialog title="Writing guide" onClose={() => (dialog = null)} wide>
        <p>A document is plain text. Anything that looks like a value becomes one, and formulas update as you write.</p>
        <table class="guide"><tbody>{#each SYNTAX as [code, what]}<tr><td><code>{code}</code></td><td>{what}</td></tr>{/each}</tbody></table>
        <p>Hover any name in the document to see how its value was computed.</p>
      </Dialog>
    {:else if dialog === "shortcuts"}
      <Dialog title="Keyboard shortcuts" onClose={() => (dialog = null)} wide>
        <div class="shortcut-columns">
          {#each commands.menus.filter(m => m.items.some(i => i.shortcut)) as menu}
            <section><h3>{menu.name}</h3><table><tbody>{#each menu.items.filter(i => i.shortcut) as item}<tr><td>{item.label}</td><td><kbd>{shortcutLabel(item.shortcut)}</kbd></td></tr>{/each}</tbody></table></section>
          {/each}
          <section><h3>General</h3><table><tbody><tr><td>Save now</td><td><kbd>{shortcutLabel("mod+s")}</kbd></td></tr><tr><td>Close find or dialog</td><td><kbd>Esc</kbd></td></tr></tbody></table></section>
        </div>
      </Dialog>
    {/if}
  </div>
{/if}
