// Browser side of the hosted deployment. build.mjs bundles this file to
// dist/docs/backend.js, where the app looks for an optional backend module.
// It implements the app's DocumentBackend interface against /api and knows
// nothing about the editor; live editing (live.js, with Yjs) loads on demand.
const base = new URL("../", import.meta.url); // the site root; the API lives beside the app
// Offline: the last library seen and saves that could not be sent yet.
const CACHE = "xmd.docs.cloud.v1", OUTBOX = "xmd.docs.cloud.outbox.v1";
const read = key => { try { return JSON.parse(localStorage.getItem(key) || "null"); } catch { return null; } };
const write = (key, value) => { try { localStorage.setItem(key, JSON.stringify(value)); } catch { /* cache is best effort */ } };
const isNetworkError = e => e instanceof TypeError || e?.code === "offline";

class BackendError extends Error {
  constructor(code, message, detail) { super(message); this.code = code; this.detail = detail; }
}

async function api(path, options = {}) {
  const response = await fetch(new URL(`api/${path}`, base), {
    ...options,
    redirect: "manual",
    headers: { accept: "application/json", ...(options.body ? { "content-type": "application/json" } : {}), ...options.headers },
    body: options.body ? JSON.stringify(options.body) : undefined,
  });
  if (response.type === "opaqueredirect") throw new BackendError("unauthenticated", "Sign in to continue");
  if (response.type === "error") throw new BackendError("offline", "You're offline");
  const data = response.headers.get("content-type")?.includes("json") ? await response.json() : null;
  if (response.status === 401) throw new BackendError("unauthenticated", data?.error || "Sign in to continue");
  if (response.status === 503) throw new BackendError("unconfigured", data?.error || "Sign-in is not configured");
  if (response.status === 409) throw new BackendError("conflict", data?.error || "The document changed elsewhere", data?.current);
  if (!response.ok) throw new BackendError("failed", data?.error || `Request failed (${response.status})`);
  return data;
}

/** A document shared by link, readable without an account. */
export async function sharedDocument(token) {
  const response = await fetch(new URL(`public/v1/${encodeURIComponent(token)}`, base), { headers: { accept: "application/json" } });
  const data = await response.json().catch(() => null);
  if (!response.ok) throw new BackendError("failed", data?.error || "This link no longer works");
  return data;
}

export async function createBackend() {
  let account = null, offline = false;
  const cached = read(CACHE) || {};
  try { account = await api("me"); }
  catch (e) {
    if (e.code === "unconfigured") return null; // deployed without Access: purely local
    if (e.code === "unauthenticated") account = null; // signed out
    else if (cached.account) { account = cached.account; offline = true; } // no network: the cached library
    else return null;
  }
  const rooms = new Map(); // document id -> live session
  const listeners = new Set();
  const notify = event => { for (const l of listeners) l(event); };
  // When the network comes back, send what is queued and let the app refresh.
  // Browsers do not always announce it, so the queue is also retried on a timer.
  let flushing = null;
  const retry = () => { if ((read(OUTBOX) || []).length) flushOutbox().catch(() => {}); };
  addEventListener("online", () => { flushOutbox().then(() => notify({ type: "online" })).catch(() => {}); });
  addEventListener("offline", () => { offline = true; notify({ type: "offline" }); });
  setInterval(retry, 10_000);
  function flushOutbox() {
    return flushing ??= (async () => {
      const queued = read(OUTBOX) || [];
      const remaining = [];
      for (const doc of queued) {
        try { await api(`documents/${doc.id}`, { method: "PUT", body: { name: doc.name, text: doc.text, folder: doc.folder ?? null, file: doc.file } }); }
        catch (e) { if (isNetworkError(e)) remaining.push(doc); /* a rejected save is dropped; the server copy wins */ }
      }
      write(OUTBOX, remaining);
      if (queued.length && !remaining.length && offline) { offline = false; notify({ type: "online" }); }
    })().finally(() => { flushing = null; });
  }
  if (account && !offline) { await flushOutbox(); write(CACHE, { ...cached, account }); }
  function remember(patch) { write(CACHE, { ...(read(CACHE) || {}), account, ...patch }); }
  const backend = {
    get label() { return offline ? "Saved on this device · syncs when online" : "Saved to your account"; },
    get offline() { return offline; },
    account,
    signIn() { location.assign(new URL(`api/login?next=${encodeURIComponent(location.pathname + location.hash)}`, base)); },
    signOut() { location.assign(new URL("api/logout", base)); },
    async list() {
      if (offline) return (cached.documents || []).concat((read(OUTBOX) || []).filter(d => !(cached.documents || []).some(x => x.id === d.id)));
      const documents = await api("documents");
      remember({ documents });
      return documents;
    },
    async save(doc) {
      // A document with a live session is saved by its room; nothing to send.
      if (rooms.has(doc.id)) return { version: doc.version };
      const body = { name: doc.name, text: doc.text, version: doc.version, folder: doc.folder ?? null, file: doc.file, named: !!doc.named };
      try {
        const result = await api(`documents/${doc.id}`, { method: "PUT", body });
        remember({ documents: (read(CACHE)?.documents || []).filter(d => d.id !== doc.id).concat([{ ...doc, version: result.version }]) });
        return result;
      } catch (e) {
        if (!isNetworkError(e)) throw e;
        // Offline: keep it here and send it when the network returns.
        const queued = (read(OUTBOX) || []).filter(d => d.id !== doc.id);
        queued.push({ id: doc.id, name: doc.name, file: doc.file, text: doc.text, folder: doc.folder ?? null, updated: doc.updated, role: "owner" });
        write(OUTBOX, queued);
        offline = true;
        return { version: doc.version };
      }
    },
    /** Filing changes the folder only; the text is left alone. */
    async file(doc) { return api(`documents/${doc.id}`, { method: "PUT", body: { folder: doc.folder ?? null, file: doc.file, name: doc.name, named: !!doc.named } }); },
    trash: {
      list: () => api("trash"),
      restore: id => api(`trash/${id}`, { method: "POST" }),
      purge: id => api(`trash/${id}`, { method: "DELETE" }),
    },
    async listFolders() {
      if (offline) return cached.folders || [];
      const folders = await api("folders");
      remember({ folders });
      return folders;
    },
    async saveFolder(folder) { return api(`folders/${folder.id}`, { method: "PUT", body: { name: folder.name } }); },
    async deleteFolder(id) { await api(`folders/${id}`, { method: "DELETE" }); },
    link(id) {
      return {
        get: () => api(`documents/${id}/link`),
        enable: () => api(`documents/${id}/link`, { method: "POST" }),
        disable: () => api(`documents/${id}/link`, { method: "DELETE" }),
      };
    },
    keys: {
      list: () => api("keys"),
      create: name => api("keys", { method: "POST", body: { name } }),
      revoke: id => api(`keys/${id}`, { method: "DELETE" }),
    },
    folderAcl(id) {
      return {
        list: () => api(`folders/${id}/acl`),
        add: (email, role) => api(`folders/${id}/acl`, { method: "PUT", body: { email, role } }),
        remove: email => api(`folders/${id}/acl`, { method: "DELETE", body: { email } }),
      };
    },
    async collaborate(doc) {
      const { createLive } = await import("./live.js");
      const live = createLive({ base, id: doc.id, user: account, role: doc.role });
      rooms.set(doc.id, live);
      const destroy = live.destroy;
      live.destroy = () => { rooms.delete(doc.id); destroy(); };
      return live;
    },
    async delete(id) { await api(`documents/${id}`, { method: "DELETE" }); },
    subscribe(listener) { listeners.add(listener); return () => listeners.delete(listener); },
    acl(id) {
      return {
        list: () => api(`documents/${id}/acl`),
        add: (email, role) => api(`documents/${id}/acl`, { method: "PUT", body: { email, role } }),
        remove: email => api(`documents/${id}/acl`, { method: "DELETE", body: { email } }),
      };
    },
  };
  return backend;
}
