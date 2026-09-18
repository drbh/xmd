// Browser side of the hosted deployment. build.mjs copies this file to
// dist/docs/backend.js, where the app looks for an optional backend module.
// It implements the app's DocumentBackend interface against /api and knows
// nothing about the editor.
const base = new URL("../", import.meta.url); // the site root; the API lives beside the app

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
  const data = response.headers.get("content-type")?.includes("json") ? await response.json() : null;
  if (response.status === 401) throw new BackendError("unauthenticated", data?.error || "Sign in to continue");
  if (response.status === 503) throw new BackendError("unconfigured", data?.error || "Sign-in is not configured");
  if (response.status === 409) throw new BackendError("conflict", data?.error || "The document changed elsewhere", data?.current);
  if (!response.ok) throw new BackendError("failed", data?.error || `Request failed (${response.status})`);
  return data;
}

export async function createBackend() {
  let account = null;
  try { account = await api("me"); }
  catch (e) {
    if (e.code === "unconfigured") return null; // deployed without Access: purely local
    if (e.code !== "unauthenticated") return null; // API unreachable: purely local
  }
  const backend = {
    label: "Saved to your account",
    account,
    signIn() { location.assign(new URL(`api/login?next=${encodeURIComponent(location.pathname + location.hash)}`, base)); },
    signOut() { location.assign(new URL("api/logout", base)); },
    async list() { return api("documents"); },
    async save(doc) { return api(`documents/${doc.id}`, { method: "PUT", body: { name: doc.name, text: doc.text, version: doc.version } }); },
    async delete(id) { await api(`documents/${id}`, { method: "DELETE" }); },
    subscribe() { return () => {}; },
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
