// The reference is the engine's own: asked for at load time and shaped into
// sections here, so the page cannot drift from the language. The only
// hand-written part is the sample note the collection snippets query.
export const SAMPLE_URI = "file:///workspace/reference/sample.wtf";

// A small note with something in every collection a query snippet reaches:
// values, a heading, tasks (one done, one due) and a table.
export const SAMPLE = `$1,234:car
total := car + $67
2026-11-20:departure
## Trip :trip
- [x] Choose the dates
- [ ] Pack @due(departure - 14d)
items := table
| item | price |
| --- | --- |
| coffee | $3 |
| lunch | $12 |
`;

// The order the engine's function groups are read in.
const GROUPS = ["Values", "Text", "Lists", "Numbers", "Dates", "Tasks", "Timers", "Lookups", "Plans", "Modules", "Control"];

export const slug = s => String(s).toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/(^-|-$)/g, "");
export const tryUri = (section, name) => `file:///workspace/reference/try/${section}/${slug(name) || "entry"}.wtf`;
// Where a row's static snippet is rendered from: one scratch note per entry,
// separate from the note its Try edits.
export const snippetUri = id => `file:///workspace/reference/snippets/${id}.wtf`;

const escape = s => String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

// A signature, marked up with the editor's own token classes so it reads
// like the language: the name as a function (or an attribute), each
// parameter as a variable, its type muted, and the result after an arrow.
export function signatureHtml(name, params = [], result) {
  const parts = params.map(param => {
    const at = param.indexOf(":");
    if (at < 0) return `<span class="t-variable">${escape(param.trim())}</span>`;
    return `<span class="t-variable">${escape(param.slice(0, at).trim())}</span>: <span class="sig-type">${escape(param.slice(at + 1).trim())}</span>`;
  });
  const kind = name.startsWith("@") ? "t-decorator" : "t-function";
  const call = `<span class="${kind}">${escape(name)}</span>(${parts.join(", ")})`;
  return result ? `${call} <span class="sig-result">→ ${escape(result)}</span>` : call;
}

// Every row is the same shape, so one table and one Try widget serve them all.
// `tier` says what the row is for: `note` is the language a note is written
// in, `toolkit` the functions a note can call but rarely needs, `module` what
// only a .wtf module uses. The page shows a badge for the last two; nothing
// is folded away, so the browser's own find reaches every row.
function row(section, name, extra) {
  return { section, name, signature: escape(name), documentation: "", fields: null, try: null, tier: "note", ...extra };
}

// A module-tier snippet is module code, so the reference shows it without a
// snippet or a Try: pasted into a note it would only report an unknown function.
const fnRow = (section, f) => row(section, f.name, { signature: signatureHtml(f.name, f.params, f.result), documentation: f.documentation, try: f.tier === "module" ? null : f.try, tier: f.tier ?? "note" });

function sectionsOf(reference) {
  const list = [];
  const functions = (reference.functions ?? []).filter(f => (f.tier ?? "note") !== "module");
  const groups = [...new Set([...GROUPS, ...functions.map(f => f.group)])].filter(g => functions.some(f => f.group === g));
  if (functions.length) {
    list.push({
      id: "functions", title: "Functions", contents: true,
      groups: groups.map(group => ({
        id: `functions-${slug(group)}`, title: group,
        rows: functions.filter(f => f.group === group).map(f => fnRow("functions", f)),
      })),
    });
  }
  const plain = (id, title, rows) => rows.length && list.push({ id, title, groups: [{ id: `${id}-list`, title, rows }] });
  plain("attributes", "Attributes", (reference.attributes ?? []).map(a => row("attributes", a.name, { signature: signatureHtml(a.name, a.params), documentation: a.documentation, try: a.try })));
  // The scalars a note writes as rows, then the objects it only reads as a
  // compact list: what each one is, and a note that reads one.
  const typeRow = t => row("types", t.name, { documentation: t.documentation, fields: t.fields?.length ? t.fields : null, try: t.try, tier: t.tier === "object" ? "object" : "note" });
  const types = reference.types ?? [];
  const typeGroups = [];
  const scalars = types.filter(t => t.tier !== "object").map(typeRow);
  if (scalars.length) typeGroups.push({ id: "types-list", title: "Types", rows: scalars });
  const objects = types.filter(t => t.tier === "object").map(typeRow);
  if (objects.length) typeGroups.push({ id: "types-objects", title: "Engine objects", compact: true, documentation: "Values the engine builds and a note reads through properties.", rows: objects });
  if (typeGroups.length) list.push({ id: "types", title: "Types", groups: typeGroups });
  const collectionRow = (section, c) => row(section, c.name, { documentation: c.documentation, fields: c.fields, try: c.try, kind: "query", tier: c.tier === "module" ? "module" : "note" });
  const query = [];
  const collections = (reference.collections ?? []).filter(c => c.tier !== "module").map(c => collectionRow("query", c));
  if (collections.length) query.push({ id: "query-collections", title: "Collections", rows: collections });
  const commands = (reference.commands ?? []).map(c => row("query", c.name, { signature: escape(c.usage || c.name), documentation: c.documentation }));
  if (commands.length) query.push({ id: "query-commands", title: "Commands", rows: commands });
  if (query.length) list.push({ id: "query", title: "Query", groups: query });
  // Only what a note can import: each library and the members it exports.
  const library = (reference.library ?? []).filter(m => (m.exports ?? []).length).map(m => ({
    id: `library-${slug(m.id)}`, title: m.id, documentation: m.documentation,
    rows: m.exports.map(e => row("library", `${m.id}.${e.name}`, { signature: signatureHtml(e.name, e.params, e.result), documentation: e.documentation, try: e.try })),
  }));
  if (library.length) list.push({ id: "library", title: "Library", groups: library });
  // Everything a .wtf module is written with, and nothing a note can use:
  // one section at the end of the reference.
  const authoring = reference.authoring ?? {};
  const parts = [];
  const primitives = (authoring.functions ?? []).map(f => fnRow("authoring", { ...f, tier: "module" }));
  if (primitives.length) parts.push({ id: "authoring-functions", title: "Functions", rows: primitives });
  const structure = (authoring.collections ?? []).map(c => collectionRow("authoring", { ...c, tier: "module" }));
  if (structure.length) parts.push({ id: "authoring-collections", title: "Collections", rows: structure });
  // The link and feature modules that ship with the engine: what each one
  // is, in a line, since the host calls them and a note never names them.
  const bundled = (authoring.modules ?? []).map(m => row("authoring", m.id, { signature: `${escape(m.id)} <span class="sig-type">${escape(m.kind)}</span>`, documentation: m.documentation, fields: m.hosts?.length ? m.hosts : null, tier: "module" }));
  if (bundled.length) parts.push({ id: "authoring-bundled", title: "Bundled modules", compact: true, rows: bundled });
  const hooks = (authoring.hooks ?? []).map(h => row("authoring", h.name, { signature: `<span class="t-function">${escape(h.name)}</span><span class="sig-type">/${h.arity ?? 0}</span>`, documentation: h.documentation, kind: h.kind, tier: "module" }));
  if (hooks.length) parts.push({ id: "authoring-hooks", title: "Hooks", rows: hooks });
  if (parts.length) list.push({ id: "authoring", title: "Module authoring", groups: parts });
  // One stable anchor per entry, so `#/reference/sum` lands on the function.
  const taken = new Set([...list.map(s => s.id), ...list.flatMap(s => s.groups.map(g => g.id))]);
  for (const section of list) for (const group of section.groups) for (const entry of group.rows) {
    let id = slug(entry.name);
    if (!id || taken.has(id)) id = `${section.id}-${id}`;
    let n = 2;
    while (taken.has(id)) id = `${section.id}-${slug(entry.name)}-${n++}`;
    taken.add(id);
    entry.id = id;
    entry.uri = tryUri(section.id, entry.name);
  }
  return list;
}

// The seven constructs, each with an anchor the snippet renderer can use.
const syntaxesOf = reference => (reference.syntaxes ?? []).map((s, i) => ({ id: `syntax-${i + 1}`, syntax: s.syntax, meaning: s.meaning }));

// The syntax rows are one note: each line names what the ones above define,
// so they render together and the table takes one rendered line per row.
export const SYNTAX_URI = "file:///workspace/reference/syntax.wtf";
export async function renderSyntaxes(workspace, syntaxes) {
  await workspace.setDocument(SYNTAX_URI, syntaxes.map(s => s.syntax).join("\n") + "\n");
  const snapshot = await workspace.request("render", { uri: SYNTAX_URI, editing: false });
  const template = document.createElement("template");
  template.innerHTML = snapshot?.html ?? "";
  const lines = {};
  for (const line of template.content.querySelectorAll(".line[data-line]")) lines[Number(line.dataset.line)] = line.outerHTML;
  return Object.fromEntries(syntaxes.map((s, i) => [s.id, lines[i] ?? ""]));
}

// The live engine first; the checked-in copy (written by scripts/build.mjs
// from `wtf reference --json`, never by hand) is what an offline build shows.
export async function loadReference(workspace) {
  try {
    const live = await workspace.request("reference", {});
    if (live?.version) return { sections: sectionsOf(live), syntaxes: syntaxesOf(live), live: true };
  } catch { /* fall through to the fixture */ }
  const fixture = (await import("./reference.fixture.json")).default;
  return { sections: sectionsOf(fixture), syntaxes: syntaxesOf(fixture), live: false };
}

// Every snippet is rendered the way the editor shows a note: set as a
// document, then asked for as static HTML with token classes and inlays.
// Setting a hundred documents before first paint would delay the page, so
// the renderer takes a queue in page order, draws the first batch at once
// and the rest when the browser is idle; a snippet that scrolls into view
// before its turn jumps the queue.
export function createSnippetRenderer(workspace, { batch = 12, onRender, onDone } = {}) {
  const queue = [], pending = new Map(), rendered = new Set();
  let running = false, done = 0, total = 0, started = 0;
  const idle = typeof requestIdleCallback === "function" ? fn => requestIdleCallback(fn, { timeout: 200 }) : fn => setTimeout(fn, 16);
  async function render(item) {
    const uri = snippetUri(item.id);
    rendered.add(item.id);
    await workspace.setDocument(uri, item.text);
    const snapshot = await workspace.request("render", { uri, editing: false });
    return snapshot?.html ?? "";
  }
  async function drain() {
    if (running) return;
    running = true;
    try {
      while (queue.length) {
        const slice = queue.splice(0, batch);
        await Promise.all(slice.map(async item => {
          if (!pending.has(item.id)) return;
          pending.delete(item.id);
          try { onRender?.(item.id, await render(item)); } catch (error) { onRender?.(item.id, "", error); }
          done += 1;
        }));
        if (queue.length) await new Promise(idle);
      }
    } finally {
      running = false;
      if (!queue.length && total && done === total) onDone?.({ total, ms: Math.round(performance.now() - started) });
    }
  }
  return {
    // Queue every snippet in page order; the first batch renders now.
    enqueue(items) {
      if (!started) started = performance.now();
      for (const item of items) if (item.text && !pending.has(item.id)) { pending.set(item.id, item); queue.push(item); total += 1; }
      drain();
    },
    // A snippet the reader can see renders before the ones above it.
    prioritise(id) {
      const at = queue.findIndex(item => item.id === id);
      if (at > 0) queue.unshift(...queue.splice(at, 1));
      drain();
    },
    get progress() { return { done, total }; },
    // The scratch notes leave the workspace with the page.
    dispose() {
      queue.length = 0;
      pending.clear();
      for (const id of rendered) Promise.resolve(workspace.removeDocument(snippetUri(id))).catch(() => {});
      rendered.clear();
    },
  };
}
