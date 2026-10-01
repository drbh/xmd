// Live editing for one document: a Yjs text synced through the document's
// room, wired to the editor's delta API. Loaded on demand by backend.js, so
// Yjs is only ever downloaded when a cloud document is opened.
import * as Y from "yjs";
import YProvider from "y-partyserver/provider";
import { IndexeddbPersistence } from "y-indexeddb";
import { diff } from "../lib/src/edits.js";

const COLORS = ["#1a73e8", "#d93025", "#188038", "#e37400", "#9334e6", "#007b83", "#c5221f", "#3c4043"];
const colorFor = s => COLORS[[...s].reduce((n, c) => (n * 31 + c.charCodeAt(0)) >>> 0, 7) % COLORS.length];

/** Yjs delta (against the old text) → ascending, non-overlapping editor edits. */
export function deltaToEdits(delta) {
  const edits = [];
  let at = 0;
  for (const op of delta) {
    if (op.retain) at += op.retain;
    else if (op.insert !== undefined) {
      const last = edits[edits.length - 1];
      if (last && last.end === at && last.start === at) last.text += op.insert; else edits.push({ start: at, end: at, text: String(op.insert) });
    } else if (op.delete) {
      const last = edits[edits.length - 1];
      if (last && last.end === at) last.end += op.delete; else edits.push({ start: at, end: at + op.delete, text: "" });
      at += op.delete;
    }
  }
  return edits;
}

export function createLive({ base, id, user, role }) {
  const doc = new Y.Doc();
  const text = doc.getText("text");
  const site = new URL(base);
  // With `prefix`, the provider uses the path as given, so the room id is part of it.
  const provider = new YProvider(site.host, id, doc, { prefix: `${site.pathname.replace(/\/$/, "")}/api/rooms/room/${id}`, protocol: site.protocol === "https:" ? "wss" : "ws" });
  // Edits are kept in this browser too, so a document can be edited without a
  // network and the room merges everything when the connection returns.
  let local = null;
  try { local = new IndexeddbPersistence(`xmd-doc-${id}`, doc); } catch { /* private mode or no IndexedDB */ }
  const awareness = provider.awareness;
  awareness.setLocalStateField("user", { name: user.name || user.email, email: user.email, color: colorFor(user.email) });
  const statusListeners = new Set(), presenceListeners = new Set();
  let status = "connecting", editor = null, detach = () => {};
  provider.on("status", ({ status: next }) => { status = next; for (const l of statusListeners) l(status); });
  provider.on("connection-close", () => { status = "disconnected"; for (const l of statusListeners) l(status); });

  const abs = json => { try { return json ? Y.createAbsolutePositionFromRelativePosition(Y.createRelativePositionFromJSON(json), doc)?.index ?? null : null; } catch { return null; } };
  function presence() {
    const list = [...awareness.getStates()].filter(([clientId, state]) => clientId !== doc.clientID && state?.user).map(([clientId, state]) => ({ clientId, user: state.user, anchor: abs(state.cursor?.anchor), head: abs(state.cursor?.head) }));
    for (const l of presenceListeners) l(list);
  }
  awareness.on("change", presence);

  function attach(target) {
    editor = target;
    const undoManager = new Y.UndoManager(text, { trackedOrigins: new Set(["local"]), captureTimeout: 400 });
    editor.setHistory({ undo: () => undoManager.undo(), redo: () => undoManager.redo() });
    // Bring the editor and the shared text together; the shared text wins once synced.
    const reconcile = () => {
      const local = editor.getSource(), shared = text.toString();
      if (shared === local) return;
      if (!text.length && local && role !== "viewer") doc.transact(() => text.insert(0, local), "local");
      else { const edit = diff(local, shared); if (edit) editor.applyEdits([edit]).catch(() => {}); }
    };
    // Until the first sync the editor already shows the saved text; the
    // server's initial state would arrive as an insert of everything, so
    // remote deltas are ignored until then and the two are reconciled once.
    let synced = false;
    const onSync = state => { if (state && !synced) { synced = true; reconcile(); } };
    provider.on("sync", onSync);
    if (provider.synced) onSync(true);
    // Without a network the local copy is the first "sync": it holds any
    // edits made offline that the room has not seen yet. A document never
    // opened online here has no local copy and stays read-only until it has,
    // because seeding it here would duplicate the room's text on merge.
    const offlineStart = () => {
      if (synced || provider.wsconnected) return;
      if (text.length) onSync(true);
      else { status = "locked"; for (const l of statusListeners) l(status); }
    };
    // Give the room a moment to answer; if it has not, the local copy leads.
    let offlineTimer;
    local?.whenSynced.then(() => { offlineTimer = setTimeout(offlineStart, navigator.onLine ? 2500 : 0); });
    const onOffline = () => { if (local?.synced) offlineStart(); };
    addEventListener("offline", onOffline);
    const stopEdits = editor.onEdit(({ edits }) => {
      // Apply from the end so earlier offsets stay valid.
      doc.transact(() => { for (const e of [...edits].reverse()) { if (e.end > e.start) text.delete(e.start, e.end - e.start); if (e.text) text.insert(e.start, e.text); } }, "local");
    });
    const observer = (event, transaction) => {
      if (!synced || transaction.origin === "local") return;
      const edits = deltaToEdits(event.delta);
      if (edits.length) editor.applyEdits(edits).catch(() => {});
      presence();
    };
    text.observe(observer);
    let lastCursor = "";
    const cursor = () => {
      if (!editor || editor.destroyed) return;
      const selection = editor.selection();
      const next = selection ? { anchor: Y.relativePositionToJSON(Y.createRelativePositionFromTypeIndex(text, selection.anchor)), head: Y.relativePositionToJSON(Y.createRelativePositionFromTypeIndex(text, selection.focus)) } : null;
      const key = JSON.stringify(next);
      if (key !== lastCursor) { lastCursor = key; awareness.setLocalStateField("cursor", next); }
    };
    document.addEventListener("selectionchange", cursor);
    detach = () => { clearTimeout(offlineTimer); removeEventListener("offline", onOffline); provider.off("sync", onSync); stopEdits(); text.unobserve(observer); document.removeEventListener("selectionchange", cursor); undoManager.destroy(); if (!editor.destroyed) editor.setHistory(null); };
    return live;
  }
  const live = {
    id,
    /** The shared text as this client currently knows it. */
    get text() { return text.toString(); },
    get status() { return status; },
    get connected() { return status === "connected" && provider.synced; },
    attach,
    onStatus(listener) { statusListeners.add(listener); listener(status); return () => statusListeners.delete(listener); },
    onPresence(listener) { presenceListeners.add(listener); presence(); return () => presenceListeners.delete(listener); },
    destroy() { detach(); awareness.setLocalState(null); provider.destroy(); local?.destroy(); doc.destroy(); },
  };
  return live;
}
