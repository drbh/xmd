// Exercises the API through `wrangler dev` with development identities, so the
// D1 schema, ACL rules, versioning, invites, and static assets are all real.
import { test, before, after } from "node:test";
import assert from "node:assert/strict";
import { spawn, execFileSync } from "node:child_process";

const PORT = 8790, BASE = `http://127.0.0.1:${PORT}`;
let server;
const as = user => ({ "x-dev-user": user, "content-type": "application/json", accept: "application/json" });
const call = (path, { user = "alice@example.com", method = "GET", body } = {}) =>
  fetch(`${BASE}${path}`, { method, headers: as(user), body: body && JSON.stringify(body), redirect: "manual" }).then(async r => ({ status: r.status, headers: r.headers, data: r.headers.get("content-type")?.includes("json") ? await r.json() : null }));

before(async () => {
  execFileSync("wrangler", ["d1", "migrations", "apply", "wtf-docs", "--local", "--persist-to", ".wrangler/test-state"], { cwd: new URL("../", import.meta.url), stdio: "ignore" });
  server = spawn("wrangler", ["dev", "--var", "DEV_AUTH:1", "--port", String(PORT), "--persist-to", ".wrangler/test-state"], { cwd: new URL("../", import.meta.url), stdio: ["ignore", "pipe", "pipe"] });
  const started = Date.now();
  while (Date.now() - started < 60_000) {
    try { if ((await fetch(`${BASE}/api/me`, { headers: as("probe@example.com") })).ok) return; } catch { /* not up yet */ }
    await new Promise(r => setTimeout(r, 500));
  }
  throw new Error("wrangler dev did not start");
});
after(() => { server?.kill("SIGTERM"); });

const id = () => crypto.randomUUID();

test("identity and empty library", async () => {
  const me = await call("/api/me");
  assert.equal(me.status, 200);
  assert.equal(me.data.email, "alice@example.com");
  const list = await call("/api/documents");
  assert.equal(list.status, 200);
  assert.ok(Array.isArray(list.data));
});

test("documents are created on first save, versioned, and conflict on stale writes", async () => {
  const doc = id();
  const created = await call(`/api/documents/${doc}`, { method: "PUT", body: { name: "Budget", text: "# Budget\n" } });
  assert.equal(created.status, 201);
  assert.equal(created.data.version, 1);
  const updated = await call(`/api/documents/${doc}`, { method: "PUT", body: { name: "Budget", text: "# Budget\n\nmore\n", version: 1 } });
  assert.equal(updated.status, 200);
  assert.equal(updated.data.version, 2);
  const stale = await call(`/api/documents/${doc}`, { method: "PUT", body: { name: "Budget", text: "old", version: 1 } });
  assert.equal(stale.status, 409);
  assert.equal(stale.data.current.version, 2);
  const fetched = await call(`/api/documents/${doc}`);
  assert.equal(fetched.data.text, "# Budget\n\nmore\n");
  assert.equal(fetched.data.role, "owner");
  const list = await call("/api/documents");
  assert.ok(list.data.some(d => d.id === doc));
  const tooBig = await call(`/api/documents/${id()}`, { method: "PUT", body: { name: "x", text: "x".repeat(1_000_001) } });
  assert.equal(tooBig.status, 413);
});

test("other users cannot see, edit, or delete a document they were not given", async () => {
  const doc = id();
  await call(`/api/documents/${doc}`, { method: "PUT", body: { name: "Private", text: "secret" } });
  assert.equal((await call(`/api/documents/${doc}`, { user: "bob@example.com" })).status, 404);
  assert.equal((await call(`/api/documents/${doc}`, { user: "bob@example.com", method: "PUT", body: { name: "Private", text: "changed", version: 1 } })).status, 404);
  assert.equal((await call(`/api/documents/${doc}`, { user: "bob@example.com", method: "DELETE" })).status, 404);
  assert.equal((await call(`/api/documents/${doc}/acl`, { user: "bob@example.com" })).status, 404);
  // Bob cannot take over the id either.
  const list = await call("/api/documents", { user: "bob@example.com" });
  assert.ok(!list.data.some(d => d.id === doc));
});

test("editors edit, viewers only read, and only the owner shares or deletes", async () => {
  const doc = id();
  await call(`/api/documents/${doc}`, { method: "PUT", body: { name: "Team", text: "v1" } });
  await call("/api/me", { user: "bob@example.com" }); await call("/api/me", { user: "carol@example.com" });
  assert.equal((await call(`/api/documents/${doc}/acl`, { method: "PUT", body: { email: "Bob@example.com", role: "editor" } })).data.invited, false);
  assert.equal((await call(`/api/documents/${doc}/acl`, { method: "PUT", body: { email: "carol@example.com", role: "viewer" } })).status, 200);
  const bob = await call(`/api/documents/${doc}`, { user: "bob@example.com" });
  assert.equal(bob.data.role, "editor");
  const edit = await call(`/api/documents/${doc}`, { user: "bob@example.com", method: "PUT", body: { name: "Team", text: "v2", version: 1 } });
  assert.equal(edit.status, 200);
  const carol = await call(`/api/documents/${doc}`, { user: "carol@example.com" });
  assert.equal(carol.data.role, "viewer");
  assert.equal(carol.data.text, "v2");
  assert.equal((await call(`/api/documents/${doc}`, { user: "carol@example.com", method: "PUT", body: { name: "Team", text: "v3", version: 2 } })).status, 403);
  assert.equal((await call(`/api/documents/${doc}/acl`, { user: "bob@example.com", method: "PUT", body: { email: "dave@example.com", role: "viewer" } })).status, 403);
  assert.equal((await call(`/api/documents/${doc}`, { user: "bob@example.com", method: "DELETE" })).status, 403);
  const acl = await call(`/api/documents/${doc}/acl`, { user: "carol@example.com" });
  assert.deepEqual(acl.data.entries.map(e => [e.email, e.role]), [["bob@example.com", "editor"], ["carol@example.com", "viewer"]]);
  assert.equal((await call(`/api/documents/${doc}/acl`, { method: "DELETE", body: { email: "carol@example.com" } })).status, 200);
  assert.equal((await call(`/api/documents/${doc}`, { user: "carol@example.com" })).status, 404);
  assert.equal((await call(`/api/documents/${doc}`, { method: "DELETE" })).status, 200);
  assert.equal((await call(`/api/documents/${doc}`, { user: "bob@example.com" })).status, 404);
});

test("an invite for an unseen email becomes access on first sign-in", async () => {
  const doc = id(), newcomer = `new-${doc.slice(0, 8)}@example.com`;
  await call(`/api/documents/${doc}`, { method: "PUT", body: { name: "Invite", text: "hello" } });
  const invite = await call(`/api/documents/${doc}/acl`, { method: "PUT", body: { email: newcomer, role: "editor" } });
  assert.equal(invite.data.invited, true);
  assert.deepEqual((await call(`/api/documents/${doc}/acl`)).data.invites, [{ email: newcomer, role: "editor" }]);
  const first = await call("/api/documents", { user: newcomer });
  assert.ok(first.data.some(d => d.id === doc && d.role === "editor" && d.owner === "alice@example.com"));
  assert.deepEqual((await call(`/api/documents/${doc}/acl`)).data.invites, []);
});

test("validation and unauthenticated requests", async () => {
  assert.equal((await call("/api/documents/bad id", { method: "PUT", body: { name: "x", text: "y" } })).status, 400);
  assert.equal((await call(`/api/documents/${id()}`, { method: "PUT", body: { name: "x" } })).status, 400);
  assert.equal((await call(`/api/documents/${id()}/acl`, { method: "PUT", body: { email: "nope", role: "editor" } })).status, 404);
  const login = await call("/api/login?next=/docs/%23/");
  assert.equal(login.status, 302);
  assert.equal(new URL(login.headers.get("location")).pathname, "/docs/");
  const open = await call("/api/login?next=//evil.example");
  assert.equal(new URL(open.headers.get("location")).pathname, "/");
});

test("the site and the backend module are served beside the API", async () => {
  const module = await fetch(`${BASE}/docs/backend.js`);
  assert.equal(module.status, 200);
  assert.match(module.headers.get("content-type"), /javascript/);
  assert.match(await module.text(), /createBackend/);
  assert.equal((await fetch(`${BASE}/docs/`)).status, 200);
  assert.equal((await fetch(`${BASE}/lib/pkg/wtf_bg.wasm`)).headers.get("content-type"), "application/wasm");
});
