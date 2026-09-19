// Exercises the API through `wrangler dev` with development identities, so the
// D1 schema, ACL rules, versioning, invites, and static assets are all real.
import { test, before, after } from "node:test";
import assert from "node:assert/strict";
import { spawn, execFileSync } from "node:child_process";
import { rmSync } from "node:fs";

const PORT = 8790, BASE = `http://127.0.0.1:${PORT}`;
let server;
const as = user => ({ "x-dev-user": user, "content-type": "application/json", accept: "application/json" });
const call = (path, { user = "alice@example.com", method = "GET", body } = {}) =>
  fetch(`${BASE}${path}`, { method, headers: as(user), body: body && JSON.stringify(body), redirect: "manual" }).then(async r => ({ status: r.status, headers: r.headers, data: r.headers.get("content-type")?.includes("json") ? await r.json() : null }));

before(async () => {
  // Every run starts from an empty local database.
  rmSync(new URL("../.wrangler/test-state", import.meta.url), { recursive: true, force: true });
  execFileSync("npx", ["wrangler", "d1", "migrations", "apply", "wtf-docs", "--local", "--persist-to", ".wrangler/test-state"], { cwd: new URL("../", import.meta.url), stdio: "ignore" });
  server = spawn("npx", ["wrangler", "dev", "--var", "DEV_AUTH:1", "--port", String(PORT), "--persist-to", ".wrangler/test-state"], { cwd: new URL("../", import.meta.url), stdio: ["ignore", "pipe", "pipe"] });
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

test("folders: filing is owner-only, folder members reach its documents, deleting unfiles", async () => {
  const folder = id(), doc = id();
  assert.equal((await call(`/api/folders/${folder}`, { method: "PUT", body: { name: "Team" } })).status, 201);
  assert.equal((await call(`/api/folders/${folder}`, { user: "bob@example.com", method: "PUT", body: { name: "Hijack" } })).status, 404);
  assert.equal((await call(`/api/documents/${doc}`, { method: "PUT", body: { name: "Filed", text: "x", folder } })).status, 201);
  assert.equal((await call(`/api/documents/${doc}`, { method: "PUT", body: { folder: id() } })).status, 404); // not a folder of mine
  await call("/api/me", { user: "bob@example.com" });
  assert.equal((await call(`/api/folders/${folder}/acl`, { method: "PUT", body: { email: "bob@example.com", role: "viewer" } })).status, 200);
  const bobFolders = await call("/api/folders", { user: "bob@example.com" });
  assert.ok(bobFolders.data.some(f => f.id === folder && f.role === "viewer" && f.owner === "alice@example.com"));
  const bobDoc = await call(`/api/documents/${doc}`, { user: "bob@example.com" });
  assert.equal(bobDoc.status, 200); assert.equal(bobDoc.data.role, "viewer"); assert.equal(bobDoc.data.folder, folder);
  assert.equal((await call(`/api/documents/${doc}`, { user: "bob@example.com", method: "PUT", body: { name: "Filed", text: "y", version: 1 } })).status, 403);
  // A direct editor grant outranks the folder's viewer role.
  await call(`/api/documents/${doc}/acl`, { method: "PUT", body: { email: "bob@example.com", role: "editor" } });
  assert.equal((await call(`/api/documents/${doc}`, { user: "bob@example.com" })).data.role, "editor");
  // Bob cannot move the document; Alice can unfile it, and bob keeps only his direct grant.
  assert.equal((await call(`/api/documents/${doc}`, { user: "bob@example.com", method: "PUT", body: { folder: null } })).status, 403);
  assert.equal((await call(`/api/documents/${doc}`, { method: "PUT", body: { folder: null } })).status, 200);
  assert.equal((await call(`/api/documents/${doc}`, { user: "bob@example.com" })).data.folder, null);
  assert.equal((await call(`/api/documents/${doc}`, { method: "PUT", body: { folder } })).status, 200);
  assert.equal((await call(`/api/folders/${folder}`, { user: "bob@example.com", method: "DELETE" })).status, 403);
  assert.equal((await call(`/api/folders/${folder}`, { method: "DELETE" })).status, 200);
  assert.equal((await call(`/api/documents/${doc}`)).data.folder, null);
  assert.equal((await call("/api/folders")).data.some(f => f.id === folder), false);
});

test("API keys drive the sync API outside the browser sign-in, without sharing or key management", async () => {
  const created = await call("/api/keys", { method: "POST", body: { name: "laptop" } });
  assert.equal(created.status, 201);
  assert.match(created.data.key, /^wtf_/);
  assert.ok((await call("/api/keys")).data.some(k => k.id === created.data.id && k.name === "laptop"));
  const sync = (path, options = {}) => fetch(`${BASE}/sync/v1${path}`, { ...options, headers: { authorization: `Bearer ${created.data.key}`, "content-type": "application/json", accept: "application/json" }, body: options.body && JSON.stringify(options.body) }).then(async r => ({ status: r.status, data: r.headers.get("content-type")?.includes("json") ? await r.json() : null }));
  assert.equal((await fetch(`${BASE}/sync/v1/me`)).status, 401);
  assert.equal((await fetch(`${BASE}/sync/v1/me`, { headers: { authorization: "Bearer wtf_not_a_real_key_at_all_00000" } })).status, 401);
  const me = await sync("/me");
  assert.equal(me.data.email, "alice@example.com"); assert.equal(me.data.viaKey, true);
  const doc = id();
  const put = await sync(`/documents/${doc}`, { method: "PUT", body: { name: "From the CLI", text: "# From the CLI\n", file: "From the CLI" } });
  assert.equal(put.status, 201); assert.equal(put.data.file, "From the CLI");
  assert.ok((await sync("/documents")).data.some(d => d.id === doc && d.file === "From the CLI"));
  // File names are unique within a folder.
  assert.equal((await sync(`/documents/${id()}`, { method: "PUT", body: { name: "x", text: "y", file: "From the CLI" } })).status, 409);
  assert.equal((await sync(`/documents/${doc}`, { method: "PUT", body: { file: "Renamed by CLI" } })).status, 200);
  assert.equal((await call(`/api/documents/${doc}`)).data.file, "Renamed by CLI");
  assert.equal((await sync(`/documents/${doc}/acl`)).status, 403);
  assert.equal((await sync("/keys")).status, 403);
  assert.equal((await call(`/api/keys/${created.data.id}`, { method: "DELETE" })).status, 200);
  assert.equal((await sync("/me")).status, 401);
});

test("the trash keeps removed documents until restored or purged, and file names follow headings until pinned", async () => {
  const doc = id();
  const created = await call(`/api/documents/${doc}`, { method: "PUT", body: { name: "Draft", text: "# Draft\n", file: "Draft", named: false } });
  assert.equal(created.data.named, false);
  assert.equal((await call(`/api/documents/${doc}`, { method: "PUT", body: { file: "Draft final", named: true } })).data.named, true);
  assert.equal((await call(`/api/documents/${doc}`, { method: "DELETE" })).status, 200);
  assert.equal((await call(`/api/documents/${doc}`)).status, 404);
  const trash = await call("/api/trash");
  assert.ok(trash.data.some(d => d.id === doc && d.deleted));
  assert.equal((await call(`/api/trash/${doc}`, { user: "bob@example.com", method: "POST" })).status, 404);
  const restored = await call(`/api/trash/${doc}`, { method: "POST" });
  assert.equal(restored.status, 200); assert.equal(restored.data.file, "Draft final");
  assert.equal((await call(`/api/documents/${doc}`)).data.role, "owner");
  await call(`/api/documents/${doc}`, { method: "DELETE" });
  assert.equal((await call(`/api/trash/${doc}`, { method: "DELETE" })).status, 200);
  assert.equal((await call("/api/trash")).data.some(d => d.id === doc), false);
  assert.equal((await call(`/api/documents/${doc}`, { method: "PUT", body: { name: "Draft", text: "again" } })).status, 201); // the id is free again
});

test("a view-only link lets anyone read a document until the owner turns it off", async () => {
  const doc = id();
  await call(`/api/documents/${doc}`, { method: "PUT", body: { name: "Public", text: "# Public\n\nhello\n" } });
  assert.deepEqual((await call(`/api/documents/${doc}/link`)).data, { enabled: false, token: null, created: null });
  assert.equal((await call(`/api/documents/${doc}/link`, { user: "bob@example.com", method: "POST" })).status, 404);
  const on = await call(`/api/documents/${doc}/link`, { method: "POST" });
  assert.equal(on.status, 201); assert.ok(on.data.token.length >= 20);
  assert.equal((await call(`/api/documents/${doc}/link`)).data.token, on.data.token);
  // No sign-in of any kind on the public path.
  const anon = await fetch(`${BASE}/public/v1/${on.data.token}`);
  assert.equal(anon.status, 200);
  const shared = await anon.json();
  assert.equal(shared.role, "link"); assert.equal(shared.text, "# Public\n\nhello\n"); assert.equal(shared.owner, "alice@example.com");
  assert.equal((await fetch(`${BASE}/public/v1/not-a-real-token-at-all-000`)).status, 404);
  assert.equal((await call(`/api/documents/${doc}/link`, { method: "DELETE" })).data.enabled, false);
  assert.equal((await fetch(`${BASE}/public/v1/${on.data.token}`)).status, 404);
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
