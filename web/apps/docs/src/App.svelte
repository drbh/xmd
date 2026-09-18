<script>
  import { onMount } from "svelte";
  import { createWorkspace } from "@wtf/web";
  import { loadDocuments, saveDocuments, titleOf, uriOf, watchStorage } from "./lib/store.js";
  import Editor from "./lib/Editor.svelte";
  import Outline from "./lib/Outline.svelte";

  const initialDocuments = loadDocuments();
  let documents = $state(initialDocuments);
  let activeId = $state(initialDocuments[0].id);
  let engine = $state("Starting the engine…");
  let saved = $state("");
  let symbols = $state([]);
  let problems = $state([]);
  let words = $state(0);
  let zoom = $state(100);
  let ready = $state(false), controller = $state(null);
  const active = $derived(documents.find(d => d.id === activeId));

  const workspace = createWorkspace({ onError: e => { engine = `Engine failed: ${e.message}`; } });
  const rpc = workspace.request;
  onMount(() => {
    const unsubscribe = workspace.onChange(persist);
    const stopWatching = watchStorage(message => (saved = message));
    (async () => {
      for (const d of documents) await workspace.setDocument(uriOf(d.id), d.text);
      ready = true;
      engine = "Rust / WebAssembly · in this tab";
    })().catch(e => { engine = `Engine failed: ${e.message}`; });
    return () => { unsubscribe(); stopWatching(); clearTimeout(saveTimer); saveDocuments(documents); workspace.destroy(); };
  });

  let saveTimer;
  function persist({ uri, source }) {
    const document = documents.find(d => uriOf(d.id) === uri);
    if (!document || document.text === source) return;
    document.text = source;
    document.name = titleOf(source, document.name);
    document.updated = Date.now();
    if (document.id === activeId) words = source.split(/\s+/).filter(Boolean).length;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => { saved = saveDocuments(documents) ? "Saved in this browser" : "Saving paused; download your changes"; }, 300);
  }
  function newDocument() {
    const d = { id: crypto.randomUUID(), name: "Untitled", text: "# Untitled\n\n", updated: Date.now() };
    documents = [d, ...documents];
    activeId = d.id;
    saveDocuments(documents);
  }
  async function deleteDocument(d) {
    if (documents.length === 1 || !confirm(`Delete "${d.name}"?`)) return;
    documents = documents.filter(x => x.id !== d.id);
    if (activeId === d.id) activeId = documents[0].id;
    saveDocuments(documents);
    try { await rpc("removeDocument", { uri: uriOf(d.id) }); } catch { /* the workspace may not know it yet */ }
  }
  function download() {
    const blob = new Blob([active.text], { type: "text/plain" });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = `${active.name.replace(/[^\w.-]+/g, "-") || "document"}.wtf`;
    a.click();
    URL.revokeObjectURL(a.href);
  }
  async function importFiles(event) {
    for (const file of event.target.files) {
      const text = await file.text();
      const d = { id: crypto.randomUUID(), name: titleOf(text, file.name.replace(/\.wtf$/, "")), text, updated: Date.now() };
      documents = [d, ...documents];
      await rpc("setDocument", { uri: uriOf(d.id), text, version: 1 });
      activeId = d.id;
    }
    saveDocuments(documents);
    event.target.value = "";
  }
  const snippets = {
    heading: "\n## Heading\n",
    task: "\n- [ ] ",
    table: "\nitems := table\n| item | quantity | price |\n| ---- | -------- | ----- |\n| tea  | 2        | $4.50 |\n\ntotal := sum(items, quantity * price)\n",
    value: "$100:amount",
    timer: "\nfocus := countdown(25m)\n",
    plan: "\nplan := maximize(3 * bagels + 1.25 * doughnuts)\n| constraint | expression                     |\n| ---------- | ------------------------------ |\n| flour      | 12 * bagels + 6.5 * doughnuts <= 400 |\n",
    comment: "<!-- note to self -->",
  };
  function insert(kind) { controller?.insertAtCaret(snippets[kind]).catch(e => (saved = e.message)); }
  function jump(symbol) {
    if (!controller) return;
    const lines = controller.getSource().split("\n");
    const offset = lines.slice(0, symbol.selectionRange.start.line).reduce((n, l) => n + l.length + 1, 0) + symbol.selectionRange.start.character;
    controller.select(offset);
    getSelection()?.anchorNode?.parentElement?.scrollIntoView({ block: "center", behavior: "smooth" });
  }
  if (new URLSearchParams(location.search).has("test")) {
    window.wtfDocs = { get documents() { return documents; }, get controller() { return controller; }, rpc, workspace, newDocument, get ready() { return ready && !!controller; } };
  }
</script>

<div class="shell" style={`--zoom:${zoom / 100}`}>
  <aside class="files">
    <div class="brand">WTF <span>docs</span></div>
    <div class="actions">
      <button onclick={newDocument}>New</button>
      <label class="button">Import<input type="file" accept=".wtf,text/plain" multiple hidden onchange={importFiles}></label>
    </div>
    <nav aria-label="Documents">
      {#each documents as d (d.id)}
        <div class="doc" class:active={d.id === activeId}>
          <button class="open" onclick={() => (activeId = d.id)}>
            <span class="title">{d.name}</span>
            <span class="when">{new Date(d.updated).toLocaleString()}</span>
          </button>
          <button class="delete" title="Delete" onclick={() => deleteDocument(d)}>×</button>
        </div>
      {/each}
    </nav>
    <p class="engine">{engine}</p>
  </aside>

  <main>
    <header class="top">
      <input class="name" aria-label="Document title" value={active?.name ?? ""} onchange={e => { active.name = e.target.value; saveDocuments(documents); }}>
      <div class="toolbar" role="toolbar" aria-label="Insert">
        <button onclick={() => insert("heading")}>Heading</button>
        <button onclick={() => insert("task")}>Checklist</button>
        <button onclick={() => insert("value")}>Value</button>
        <button onclick={() => insert("table")}>Table</button>
        <button onclick={() => insert("timer")}>Timer</button>
        <button onclick={() => insert("plan")}>Plan</button>
        <button onclick={() => insert("comment")}>Comment</button>
        <span class="gap"></span>
        <button onclick={() => (zoom = Math.max(70, zoom - 10))}>−</button>
        <span class="zoom">{zoom}%</span>
        <button onclick={() => (zoom = Math.min(160, zoom + 10))}>+</button>
        <button onclick={download}>Download</button>
        <button onclick={() => print()}>Print</button>
      </div>
    </header>
    <section class="canvas">
      {#if active && ready}
        {#key active.id}
          <Editor {workspace} uri={uriOf(active.id)} text={active.text}
            onSnapshot={(snapshot, list) => { symbols = snapshot.symbols || []; problems = list; }}
            onError={e => (saved = e.message)} bind:controller />
        {/key}
      {/if}
    </section>
    <footer class="status">
      <span>{words} words</span>
      <span>{problems.length ? `${problems.length} problem${problems.length === 1 ? "" : "s"}` : "No problems"}</span>
      <span>{saved}</span>
    </footer>
  </main>

  <aside class="side">
    <h2>Outline</h2>
    <Outline {symbols} onJump={jump} />
    {#if problems.length}
      <h2>Problems</h2>
      <ul class="problems">
        {#each problems as p}<li class={p.severity === 2 ? "warn" : "error"}>Line {p.range.start.line + 1}: {p.message}</li>{/each}
      </ul>
    {/if}
  </aside>
</div>
