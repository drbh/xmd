// The sync plugin (`xmd run sync.x.md`) against wrangler dev: push, pull,
// merge, conflict, delete. Needs the CLI built (cargo build); skipped otherwise.
import { test, before, after } from "node:test";
import assert from "node:assert/strict";
import { spawn, execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, writeFileSync, readFileSync, rmSync, readdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const PORT = 8792, BASE = `http://127.0.0.1:${PORT}`;
const XMD = fileURLToPath(new URL("../../../../target/debug/xmd", import.meta.url));
const PLUGIN = fileURLToPath(new URL("../sync.x.md", import.meta.url));
const user = "sync@example.com";
let server;
const api = (path, options = {}) => fetch(`${BASE}${path}`, { ...options, headers: { "x-dev-user": user, "content-type": "application/json", accept: "application/json" }, body: options.body && JSON.stringify(options.body) }).then(r => r.json());

before(async t => {
  if (!existsSync(XMD)) { t.skip("build the CLI first: cargo build"); return; }
  // Every run starts from an empty local database.
  rmSync(new URL("../.wrangler/test-state", import.meta.url), { recursive: true, force: true });
  execFileSync("npx", ["wrangler", "d1", "migrations", "apply", "xmd-docs", "--local", "--persist-to", ".wrangler/test-state"], { cwd: new URL("../", import.meta.url), stdio: "ignore" });
  server = spawn("npx", ["wrangler", "dev", "--var", "DEV_AUTH:1", "--port", String(PORT), "--persist-to", ".wrangler/test-state"], { cwd: new URL("../", import.meta.url), stdio: ["ignore", "pipe", "pipe"] });
  const started = Date.now();
  while (Date.now() - started < 60_000) {
    try { if ((await fetch(`${BASE}/api/me`, { headers: { "x-dev-user": user } })).ok) return; } catch { /* not up yet */ }
    await new Promise(r => setTimeout(r, 500));
  }
  throw new Error("wrangler dev did not start");
});
after(() => { server?.kill("SIGTERM"); });

test("the sync plugin mirrors a directory with a folder, merging and flagging conflicts", { skip: !existsSync(XMD) }, async () => {
  const dir = mkdtempSync(join(tmpdir(), "xmd-sync-")), config = mkdtempSync(join(tmpdir(), "xmd-config-"));
  const sync = (...args) => execFileSync(XMD, ["run", PLUGIN, dir, ...args], { env: { ...process.env, XDG_CONFIG_HOME: config }, encoding: "utf8" });
  const { key } = await api("/api/keys", { method: "POST", body: { name: "test" } });
  const folderName = `Synced ${Date.now()}`;
  writeFileSync(join(dir, "Budget.x.md"), "# Budget\n\nrent := $900\n\nfood := $200\n");
  writeFileSync(join(dir, "Trip.x.md"), '# Trip\n\nb := import("./Budget.x.md")\nTotal [b.rent]\n');
  // First run creates the folder and both documents; the second has nothing to do.
  let out = sync("--url", BASE, "--folder", folderName, "--key", key);
  assert.match(out, /created folder/); assert.match(out, /created Budget/); assert.match(out, /created Trip/);
  assert.match(sync(), /Up to date/);
  const manifest = JSON.parse(readFileSync(join(dir, ".xmd-sync/manifest.json"), "utf8"));
  const folder = manifest.folder_id, budget = manifest.files["Budget.x.md"].id;
  const remote = async () => (await api(`/api/documents/${budget}`)).text;
  const putRemote = text => api(`/api/documents/${budget}`, { method: "PUT", body: { name: "Budget", file: "Budget", folder, text } });
  // A change in the web app is pulled.
  await putRemote("# Budget\n\nrent := $900\n\nfood := $250\n");
  assert.match(sync(), /pulled Budget/);
  assert.match(readFileSync(join(dir, "Budget.x.md"), "utf8"), /food := \$250/);
  // Different lines changed on both sides merge.
  writeFileSync(join(dir, "Budget.x.md"), "# Budget\n\nrent := $950\n\nfood := $250\n");
  await putRemote("# Budget\n\nrent := $900\n\nfood := $250\n\nfun := $50\n");
  assert.match(sync(), /merged Budget/);
  assert.equal(await remote(), "# Budget\n\nrent := $950\n\nfood := $250\n\nfun := $50\n");
  // The same line changed both ways is a conflict file; the local file is untouched and frozen until resolved.
  writeFileSync(join(dir, "Budget.x.md"), "# Budget\n\nrent := $1,000\n\nfood := $250\n\nfun := $50\n");
  await putRemote("# Budget\n\nrent := $975\n\nfood := $250\n\nfun := $50\n");
  assert.match(sync(), /conflict in Budget/);
  assert.match(readFileSync(join(dir, "Budget.conflict.x.md"), "utf8"), /<<<<<<<[\s\S]*\$1,000[\s\S]*\$975/);
  assert.match(readFileSync(join(dir, "Budget.x.md"), "utf8"), /\$1,000/);
  assert.match(sync(), /still has Budget\.conflict\.x\.md/);
  assert.match(await remote(), /\$975/);
  writeFileSync(join(dir, "Budget.x.md"), "# Budget\n\nrent := $980\n\nfood := $250\n\nfun := $50\n");
  rmSync(join(dir, "Budget.conflict.x.md"));
  assert.match(sync(), /pushed Budget/);
  assert.match(await remote(), /\$980/);
  // Deleting locally removes it from the web app; removing in the web app moves the file to trash.
  rmSync(join(dir, "Trip.x.md"));
  assert.match(sync(), /removed Trip/);
  const list = await api("/api/documents");
  assert.ok(!list.some(d => d.file === "Trip" && d.folder === folder));
  await api(`/api/documents/${budget}`, { method: "DELETE" });
  assert.match(sync(), /moved Budget to/);
  assert.ok(!existsSync(join(dir, "Budget.x.md")));
  assert.ok(readdirSync(join(dir, ".xmd-sync/trash")).includes("Budget.x.md"));
  // Dry runs report without touching anything.
  writeFileSync(join(dir, "New.x.md"), "# New\n");
  assert.match(sync("--dry-run"), /would create New/);
  assert.ok(!(await api("/api/documents")).some(d => d.file === "New"));
  rmSync(dir, { recursive: true }); rmSync(config, { recursive: true });
});
