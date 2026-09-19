// Where documents live. The editor never knows whether that is this browser or
// an account: it calls one backend, chosen at startup by `resolveBackend`.
//
// A backend implements:
//   list(): Promise<Document[]>                   every document, with text (imports need it)
//   save(doc): Promise<{ version }>               upsert; rejects with BackendError("conflict") when stale
//   delete(id): Promise<void>
//   subscribe(listener): () => void               listener({ type: "paused", message })
//   label: string                                 e.g. "Saved in this browser"
// Optional cloud capabilities: account { email }, signIn(), signOut(), acl(id).
// Folders: listFolders(), saveFolder(folder), deleteFolder(id); a document's
// `folder` is a folder id or null. Cloud backends also offer folderAcl(id).
// A Document is { id, name, text, updated, folder, version?, role?, owner? }.
import { loadState, saveState, clearState, watchStorage, hasStoredDocuments } from "./store.js";

export class BackendError extends Error {
  constructor(code, message, detail) { super(message); this.code = code; this.detail = detail; }
}

export class LocalBackend {
  label = "Saved in this browser";
  #state = null;
  #listeners = new Set();
  #load() { return (this.#state ??= loadState()); }
  #write() { if (!saveState(this.#load())) throw new BackendError("unavailable", "Saving paused; download your changes"); }
  async list() { return this.#load().documents; }
  /** Documents a person saved here, as opposed to the starter a fresh browser is seeded with. */
  async stored() { return hasStoredDocuments() ? this.list() : []; }
  async save(doc) {
    const documents = this.#load().documents;
    const at = documents.findIndex(d => d.id === doc.id);
    const copy = { id: doc.id, name: doc.name, text: doc.text, updated: doc.updated, opened: doc.opened, folder: doc.folder ?? null };
    if (at === -1) documents.unshift(copy); else documents[at] = copy;
    this.#write();
    return { version: doc.updated };
  }
  async file(doc) { return this.save(doc); }
  async delete(id) {
    const state = this.#load();
    state.documents = state.documents.filter(d => d.id !== id);
    this.#write();
  }
  async listFolders() { return this.#load().folders; }
  async saveFolder(folder) {
    const folders = this.#load().folders;
    const at = folders.findIndex(f => f.id === folder.id);
    const copy = { id: folder.id, name: folder.name, updated: folder.updated };
    if (at === -1) folders.push(copy); else folders[at] = copy;
    this.#write();
    return copy;
  }
  async deleteFolder(id) {
    const state = this.#load();
    state.folders = state.folders.filter(f => f.id !== id);
    for (const d of state.documents) if (d.folder === id) d.folder = null;
    this.#write();
  }
  /** Removes everything from this browser, after documents were moved elsewhere. */
  async clear() { this.#state = { documents: [], folders: [] }; clearState(); }
  subscribe(listener) {
    this.#listeners.add(listener);
    const stop = watchStorage(message => listener({ type: "paused", message }));
    return () => { this.#listeners.delete(listener); stop(); };
  }
}

/**
 * A deployment may place a `backend.js` module beside the app that exports
 * `createBackend()`. Without one, or if it reports no account, documents stay
 * local. The static build ships no such module.
 */
export async function resolveBackend() {
  const local = new LocalBackend();
  let cloud = null;
  try {
    const url = new URL("./backend.js", document.baseURI).href;
    if ((await fetch(url, { method: "HEAD" })).ok) {
      const module = await import(/* @vite-ignore */ url);
      cloud = await module.createBackend?.();
    }
  } catch { cloud = null; }
  if (!cloud) return { backend: local, local, cloud: null };
  return { backend: cloud.account ? cloud : local, local, cloud };
}
