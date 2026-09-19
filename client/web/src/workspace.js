import { createRpc } from "./rpc.js";

export const defaultUri = "file:///workspace/main.wtf";
export const canonicalUri = uri => new URL(uri).href;

/** UTF-16 coordinates match LSP and JavaScript string offsets. */
export function indexOf(source, { line, character }) {
  const lines = source.split("\n");
  if (!Number.isInteger(line) || !Number.isInteger(character) || line < 0 || character < 0 || line >= lines.length || character > lines[line].replace(/\r$/, "").length) throw new Error("Invalid edit position");
  const offset = lines.slice(0, line).reduce((n, text) => n + text.length + 1, 0) + character;
  if (offset && /[\uD800-\uDBFF]/.test(source[offset - 1]) && /[\uDC00-\uDFFF]/.test(source[offset] || "")) throw new Error("Edit splits a Unicode character");
  return offset;
}
export function applyTextEdits(source, edits) {
  const sorted = edits.map(e => ({ start: indexOf(source, e.range.start), end: indexOf(source, e.range.end), text: e.newText })).sort((a, b) => b.start - a.start || b.end - a.end);
  let boundary = source.length;
  for (const edit of sorted) {
    if (typeof edit.text !== "string" || edit.start > edit.end || edit.end > boundary) throw new Error("Invalid or overlapping edits");
    source = source.slice(0, edit.start) + edit.text + source.slice(edit.end);
    boundary = edit.start;
  }
  return source;
}

// The engine runs in a module worker next to this file. Browsers only start
// workers from their own origin, so when the library is loaded from another
// site (a CDN, the hosted app) the worker is a same-origin blob that imports
// the real one; the CDN must send CORS headers for lib/, and the shipped
// Worker and static server do.
function engineWorker() {
  // The literal form below is what bundlers recognise to include the worker;
  // the origin check deliberately avoids it so they leave it alone.
  if (typeof location === "undefined" || new URL(import.meta.url).origin === location.origin) return new Worker(new URL("./worker.js", import.meta.url), { type: "module" });
  const script = "./worker.js";
  const shim = URL.createObjectURL(new Blob([`import ${JSON.stringify(new URL(script, import.meta.url).href)};`], { type: "text/javascript" }));
  const worker = new Worker(shim, { type: "module" });
  URL.revokeObjectURL(shim);
  return worker;
}

/** Owns source and language state; views and persistence subscribe independently. */
export function createWorkspace(options = {}) {
  const worker = options.workerFactory ? options.workerFactory() : options.transport ? null : engineWorker();
  const rpc = options.transport || createRpc(worker, options);
  const documents = new Map(), counters = new Map(), snapshots = new Map(), analyses = new Map(), subscribers = new Map(), changes = new Set();
  let revision = 0, tail = Promise.resolve(), disposed = false, refreshQueued = false, refreshAgain = false, timer, refreshing = false;
  let day = new Date().toDateString();
  const clock = () => typeof options.now === "function" ? options.now() : options.now;
  const call = (method, params) => rpc(method, params, clock());
  const report = error => { if (!disposed) options.onError?.(error); };
  const enqueue = operation => {
    if (disposed) return Promise.reject(new Error("Workspace was destroyed"));
    const result = tail.then(() => { if (disposed) throw new Error("Workspace was destroyed"); return operation(); });
    tail = result.catch(() => {});
    return result;
  };
  const emit = change => { if (!disposed) for (const listener of changes) listener(change); };
  const notify = snapshot => { if (!disposed) for (const [listener, editing] of subscribers.get(snapshot.uri) || []) if (snapshot.editing === editing) listener(snapshot); };
  function invalidate() {
    revision++;
    snapshots.clear();
    refreshAgain = true;
    if (refreshQueued || disposed) return;
    refreshQueued = true;
    queueMicrotask(async () => {
      try {
        do {
          refreshAgain = false;
          await tail;
          if (!disposed) await refresh();
        } while (refreshAgain && !disposed);
      }
      catch (error) { report(error); }
      finally { refreshQueued = false; }
    });
  }
  async function refresh() {
    // One clock scheduler for every mounted view, never a timer per document.
    for (const [uri, listeners] of subscribers) if (documents.has(uri)) {
      for (const editing of new Set(listeners.values())) await api.analyze(uri, { force: true, editing });
    }
  }
  function scheduleClock() {
    clearInterval(timer);
    if (!subscribers.size || disposed || typeof options.now === "string") return;
    timer = setInterval(async () => {
      if (refreshing || globalThis.document?.hidden) return;
      const nextDay = new Date().toDateString();
      const live = [...subscribers.keys()].some(uri => snapshots.get(uri)?.live);
      if (nextDay === day && !live) return;
      day = nextDay;
      refreshing = true;
      try { await refresh(); } catch (error) { report(error); } finally { refreshing = false; }
    }, options.refreshInterval ?? 1000);
  }
  function reserve(uri, source) {
    if (typeof source !== "string" || new TextEncoder().encode(source).length > 1_000_000) throw new Error("Notes are limited to 1 MB of text");
    const previous = documents.get(uri);
    const record = { uri, source, version: (counters.get(uri) || 0) + 1 };
    counters.set(uri, record.version);
    documents.set(uri, record);
    invalidate();
    return { record, previous };
  }
  async function write({ record, previous }) {
    try { await call("setDocument", { uri: record.uri, text: record.source, version: record.version }); }
    catch (error) {
      if (documents.get(record.uri) === record) { if (previous) documents.set(record.uri, previous); else documents.delete(record.uri); invalidate(); }
      throw error;
    }
    if (documents.get(record.uri) === record) emit({ ...record });
    return record;
  }
  const api = {
    get revision() { return revision; },
    get disposed() { return disposed; },
    getDocument: uri => { const doc = documents.get(canonicalUri(uri)); return doc && { ...doc }; },
    hasDocument: uri => documents.has(canonicalUri(uri)),
    setDocument(uri, source) {
      uri = canonicalUri(uri);
      if (disposed) return Promise.reject(new Error("Workspace was destroyed"));
      if (documents.get(uri)?.source === source) return tail.then(() => api.getDocument(uri));
      const reserved = reserve(uri, source);
      return enqueue(() => write(reserved));
    },
    removeDocument(uri) {
      uri = canonicalUri(uri);
      documents.delete(uri);
      invalidate();
      return enqueue(() => call("removeDocument", { uri }));
    },
    request(method, params = {}) {
      if (params.uri) params = { ...params, uri: canonicalUri(params.uri) };
      if (method === "setDocument") return api.setDocument(params.uri, params.text);
      if (method === "removeDocument") return api.removeDocument(params.uri);
      if (["setModules", "setResourceData"].includes(method)) return enqueue(async () => { const result = await call(method, params); invalidate(); return result; });
      return enqueue(() => call(method, params));
    },
    query(uri, method, params = {}) {
      uri = canonicalUri(uri);
      const before = revision;
      return api.request(method, { ...params, uri }).then(result => before === revision && !disposed ? result : null);
    },
    analyze(uri, { force = false, editing = true } = {}) {
      uri = canonicalUri(uri);
      const cached = snapshots.get(uri);
      if (!force && cached?.revision === revision && cached.editing === editing) return Promise.resolve(cached);
      const before = revision;
      const key = JSON.stringify([uri, before, editing]);
      if (analyses.has(key)) return analyses.get(key);
      const result = api.request(editing ? "analyze" : "render", { uri, editing }).then(snapshot => {
        if (disposed || before !== revision || snapshot.version !== documents.get(uri)?.version) return null;
        snapshot.revision = before;
        snapshots.set(uri, snapshot);
        notify(snapshot);
        return snapshot;
      }).finally(() => analyses.delete(key));
      analyses.set(key, result);
      return result;
    },
    onChange(listener) { changes.add(listener); return () => changes.delete(listener); },
    subscribe(uri, listener, { editing = true } = {}) {
      uri = canonicalUri(uri);
      if (disposed) throw new Error("Workspace was destroyed");
      if (!subscribers.has(uri)) subscribers.set(uri, new Map());
      subscribers.get(uri).set(listener, editing);
      scheduleClock();
      return () => { const listeners = subscribers.get(uri); listeners?.delete(listener); if (!listeners?.size) subscribers.delete(uri); scheduleClock(); };
    },
    async execute(command, versions, { apply = true } = {}) {
      return enqueue(async () => {
        const before = revision;
        const result = await call("execute", { command, versions });
        if (before !== revision) throw new Error("Notes changed; request fresh controls");
        if (apply && result.edit) await applyEdit(result.edit);
        return result;
      });
    },
    applyEdit(edit) { return enqueue(() => applyEdit(edit)); },
    setModules: sources => api.request("setModules", { sources }),
    setResourceData: (url, data) => api.request("setResourceData", { url, data }),
    refresh,
    settled: () => tail,
    destroy() {
      if (disposed) return;
      disposed = true;
      clearInterval(timer);
      subscribers.clear(); changes.clear(); snapshots.clear(); analyses.clear(); documents.clear();
      rpc.destroy?.();
    },
  };
  async function applyEdit(edit) {
    const targets = new Set();
    const updates = (edit.documentChanges || []).map(change => {
      const { uri, version } = change.textDocument;
      const doc = documents.get(uri);
      if (!doc || doc.version !== version || targets.has(uri)) throw new Error("Note changed; request fresh controls");
      targets.add(uri);
      return { uri, source: applyTextEdits(doc.source, change.edits) };
    });
    const reserved = updates.map(({ uri, source }) => reserve(uri, source));
    for (const update of reserved) await write(update);
  }
  return api;
}
