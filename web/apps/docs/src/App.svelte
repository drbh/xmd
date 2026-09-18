<script>
  import { onMount } from "svelte";
  import { createWorkspace } from "@wtf/web";
  import { loadDocuments, saveDocuments, titleOf, uriOf, watchStorage, createDocument, loadPrefs, savePrefs, relativeTime, TEMPLATES } from "./lib/store.js";
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

  const initialDocuments = loadDocuments();
  let documents = $state(initialDocuments);
  let prefs = $state(loadPrefs());
  let activeId = $state(idFromHash());
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
  let now = $state(Date.now());
  let titleInput = $state(null), sidebarOpenMobile = $state(false);
  const active = $derived(documents.find(d => d.id === activeId));
  const counts = $derived(active ? stats(active.text) : null);
  const symbolNames = $derived.by(() => { const out = new Set(); const walk = list => { for (const s of list) { if (/^[A-Za-z_]\w*$/.test(s.name)) out.add(s.name); walk(s.children || []); } }; walk(symbols); return [...out]; });
  const systemDark = matchMedia("(prefers-color-scheme: dark)");
  let dark = $state(systemDark.matches);
  const theme = $derived(prefs.theme === "system" ? (dark ? "dark" : "light") : prefs.theme);
  $effect(() => { document.documentElement.dataset.theme = theme; document.documentElement.classList.toggle("wtf-light", theme === "light"); });
  $effect(() => { savePrefs($state.snapshot(prefs)); });
  $effect(() => { document.title = active ? `${active.name || "Untitled document"} – WTF Docs` : "WTF Docs"; });

  const workspace = createWorkspace({ onError: e => { engine = `Engine failed: ${e.message}`; } });
  const rpc = workspace.request;
  onMount(() => {
    const unsubscribe = workspace.onChange(persist);
    const stopWatching = watchStorage(message => { saved = message; notice = message; });
    const onScheme = e => (dark = e.matches);
    systemDark.addEventListener("change", onScheme);
    const tick = setInterval(() => (now = Date.now()), 30_000);
    (async () => {
      for (const d of documents) await workspace.setDocument(uriOf(d.id), d.text);
      ready = true;
      engine = "Rust / WebAssembly · runs in this tab";
    })().catch(e => { engine = `Engine failed: ${e.message}`; });
    return () => { unsubscribe(); stopWatching(); systemDark.removeEventListener("change", onScheme); clearInterval(tick); clearTimeout(saveTimer); saveDocuments(documents); workspace.destroy(); };
  });

  // Routing: the home screen is "#/", a document is "#/d/<id>".
  function idFromHash() { const m = /^#\/d\/([\w-]+)/.exec(location.hash); return m && initialDocuments.some(d => d.id === m[1]) ? m[1] : null; }
  $effect(() => {
    const hash = activeId ? `#/d/${activeId}` : "#/";
    if (location.hash !== hash) history.pushState(null, "", hash);
  });
  function onHashChange() { const m = /^#\/d\/([\w-]+)/.exec(location.hash); activeId = m && documents.some(d => d.id === m[1]) ? m[1] : null; }
  function open(id) {
    const d = documents.find(x => x.id === id);
    if (!d) return;
    d.opened = Date.now();
    activeId = id; find = null; dialog = null; symbols = []; problems = []; caretLine = -1;
  }
  function home() { activeId = null; find = null; dialog = null; }

  let saveTimer;
  function persist({ uri, source }) {
    const document = documents.find(d => uriOf(d.id) === uri);
    if (!document || document.text === source) return;
    document.text = source;
    document.name = titleOf(source, document.name);
    document.updated = Date.now();
    now = document.updated;
    saved = "Saving…";
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => { saved = saveDocuments(documents) ? "Saved in this browser" : "Saving paused; download your changes"; }, 300);
  }
  function flush() { clearTimeout(saveTimer); saved = saveDocuments(documents) ? "Saved in this browser" : "Saving paused; download your changes"; }
  async function newDocument(template = TEMPLATES[0]) {
    const d = createDocument(template, template.id === "blank" ? "Untitled document" : undefined);
    documents = [d, ...documents];
    await workspace.setDocument(uriOf(d.id), d.text);
    saveDocuments(documents);
    open(d.id);
    // Land the caret on the empty line after the heading so typing starts immediately.
    queueMicrotask(() => setTimeout(() => controller?.select(d.text.length), 50));
  }
  async function duplicate(d = active) {
    if (!d) return;
    const copy = { ...createDocument({ text: d.text }), name: `Copy of ${d.name}` };
    documents = [copy, ...documents];
    await workspace.setDocument(uriOf(copy.id), copy.text);
    saveDocuments(documents);
    open(copy.id);
  }
  async function remove(d = active) {
    if (!d || !confirm(`Remove "${d.name}"? This cannot be undone.`)) return;
    documents = documents.filter(x => x.id !== d.id);
    if (activeId === d.id) home();
    saveDocuments(documents);
    try { await rpc("removeDocument", { uri: uriOf(d.id) }); } catch { /* the workspace may not know it yet */ }
  }
  function download(d = active) {
    if (!d) return;
    const blob = new Blob([d.text], { type: "text/plain" });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = `${d.name.replace(/[^\w.-]+/g, "-") || "document"}.wtf`;
    a.click();
    URL.revokeObjectURL(a.href);
  }
  async function importFiles(event) {
    let last;
    for (const file of event.target.files) {
      const text = await file.text();
      const d = createDocument({ text }, titleOf(text, file.name.replace(/\.wtf$/, "")));
      documents = [d, ...documents];
      await rpc("setDocument", { uri: uriOf(d.id), text, version: 1 });
      last = d.id;
    }
    saveDocuments(documents);
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
    const lines = d.text.split("\n");
    const at = lines.findIndex(l => /^#+\s+\S/.test(l));
    if (at !== -1) {
      const line = lines[at];
      const m = /^(#+\s+)(.*?)(\s+:\w+)?$/.exec(line);
      const next = `${m[1]}${name}${m[3] || ""}`;
      const start = lines.slice(0, at).reduce((n, l) => n + l.length + 1, 0);
      if (d.id === activeId && controller) await controller.replaceRange(start, start + line.length, next, { anchor: start + next.length }).catch(error);
      else { lines[at] = next; d.text = lines.join("\n"); await workspace.setDocument(uriOf(d.id), d.text); }
    }
    d.name = name; d.updated = Date.now();
    flush();
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
    ['other := import("./other.wtf")', "Reference another document's values as [other.total]"],
    ["<!-- note -->", "A comment that never renders a value"],
  ];
  if (new URLSearchParams(location.search).has("test")) {
    window.wtfDocs = { get documents() { return documents; }, get controller() { return controller; }, get active() { return active; }, rpc, workspace, newDocument, open, home, get ready() { return ready; } };
  }
</script>

<svelte:window onkeydown={keydown} onhashchange={onHashChange} />
<input bind:this={importInput} type="file" accept=".wtf,text/plain" multiple hidden onchange={importFiles}>

{#if !active}
  <Home {documents} {engine} {notice} {theme} onToggleTheme={() => (prefs.theme = theme === "dark" ? "light" : "dark")} onOpen={open} onNew={newDocument} onImport={importFiles} onRename={d => rename(d)} onDuplicate={duplicate} onDownload={download} onDelete={remove} />
{:else}
  <div class="app" class:pageless={prefs.pageless} class:no-outline={!prefs.outline} style={`--zoom:${prefs.zoom / 100}`}>
    <header class="chrome">
      <div class="titlebar">
        <button type="button" class="logo" title="Back to documents" aria-label="Documents home" onclick={home}><Icon name="doc" size={26} /></button>
        <div class="title-block">
          <div class="title-row">
            <input bind:this={titleInput} class="title-input" aria-label="Document title" value={active.name} spellcheck="false"
              onchange={e => rename(active, e.target.value)} onkeydown={e => { if (e.key === "Enter") { e.preventDefault(); e.target.blur(); controller?.select(controller.selection()?.focus ?? 0); } }}>
            <span class="status" role="status" title={saved}>
              <Icon name={saved.startsWith("Saved") ? "cloud" : saved ? "warning" : "cloud"} size={16} />
              <span class="status-text">{saved || (ready ? "Saved in this browser" : engine)}</span>
            </span>
          </div>
          <MenuBar menus={commands.menus} />
        </div>
        <div class="title-actions">
          <span class="edited">Last edit {relativeTime(active.updated, now)}</span>
          <button type="button" class="button" onclick={() => download()}><Icon name="upload" /> Download</button>
          <button type="button" class="tool theme-toggle" title={theme === "dark" ? "Switch to light theme" : "Switch to dark theme"} aria-label="Toggle theme" onclick={() => (prefs.theme = theme === "dark" ? "light" : "dark")}><Icon name={theme === "dark" ? "sun" : "moon"} /></button>
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
          {#key active.id}
            <Editor {workspace} uri={uriOf(active.id)} text={active.text}
              onSnapshot={(snapshot, list) => { symbols = snapshot.symbols || []; problems = list; engineVersion = snapshot.engineVersion || ""; }}
              {onCaret} onError={error} bind:controller />
          {/key}
        {:else}
          <div class="page loading"><p>{engine}</p></div>
        {/if}
      </main>
    </div>

    {#if prefs.console}<Console {rpc} uri={uriOf(active.id)} names={symbolNames} bind:height={prefs.consoleHeight} onClose={() => { prefs.console = false; controller?.element.focus(); }} />{/if}
    {#if find}<FindBar {controller} replace={find.replace} onClose={at => { find = null; controller?.select(at); }} />{/if}
    {#if prefs.wordCount && counts}<button type="button" class="chip words" title="Word count" onclick={() => (dialog = "stats")}>{counts.words} words</button>{/if}
    {#if problems.length}<button type="button" class="chip problems-chip" onclick={() => (dialog = "problems")}><Icon name="warning" size={14} /> {problems.length} problem{problems.length === 1 ? "" : "s"}</button>{/if}
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
