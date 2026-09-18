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
// A Document is { id, name, text, updated, version?, role? }.
import { loadDocuments, saveDocuments, watchStorage, hasStoredDocuments } from "./store.js";

export class BackendError extends Error {
  constructor(code, message, detail) { super(message); this.code = code; this.detail = detail; }
}

export class LocalBackend {
  label = "Saved in this browser";
  #documents = null;
  #listeners = new Set();
  async list() { return (this.#documents ??= loadDocuments()); }
  /** Documents a person saved here, as opposed to the starter a fresh browser is seeded with. */
  async stored() { return hasStoredDocuments() ? this.list() : []; }
  async save(doc) {
    const documents = await this.list();
    const at = documents.findIndex(d => d.id === doc.id);
    const copy = { id: doc.id, name: doc.name, text: doc.text, updated: doc.updated, opened: doc.opened };
    if (at === -1) documents.unshift(copy); else documents[at] = copy;
    if (!saveDocuments(documents)) throw new BackendError("unavailable", "Saving paused; download your changes");
    return { version: doc.updated };
  }
  async delete(id) {
    this.#documents = (await this.list()).filter(d => d.id !== id);
    saveDocuments(this.#documents);
  }
  /** Removes everything from this browser, after documents were moved elsewhere. */
  async clear() { this.#documents = []; saveDocuments([]); }
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
