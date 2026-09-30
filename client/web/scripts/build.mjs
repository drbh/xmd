import { cp, mkdir, rm, readFile, writeFile } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import "./theme.mjs";

const root = new URL("../", import.meta.url);
const dist = new URL("dist/", root);
await rm(dist, { recursive: true, force: true });
await mkdir(new URL("lib/", dist), { recursive: true });
for (const name of ["src", "adapters", "theme", "pkg"]) {
  await cp(new URL(name, root), new URL(`lib/${name}`, dist), { recursive: true });
}
// Static hosts that honour _headers (Cloudflare) let other sites load lib/.
// The service worker is always revalidated so a new build is noticed at once;
// files whose names carry a hash of their contents never change.
await writeFile(new URL("_headers", dist), [
  "/lib/*\n  Access-Control-Allow-Origin: *\n  Cache-Control: public, max-age=86400",
  "/sw.js\n  Cache-Control: no-cache",
  "/docs/assets/*\n  Cache-Control: public, max-age=31536000, immutable",
  "/docs/live-*\n  Cache-Control: public, max-age=31536000, immutable",
].join("\n") + "\n");
// The site root opens the document app.
await writeFile(new URL("index.html", dist), '<!doctype html><meta charset="utf-8"><meta http-equiv="refresh" content="0; url=docs/"><title>XMD Docs</title><a href="docs/">Open XMD Docs</a>\n');
// The embedding example: a plain page that loads the library like any other site would.
await mkdir(new URL("embed/", dist), { recursive: true });
await cp(new URL("embed/index.html", root), new URL("embed/index.html", dist));
// The examples, fetched one at a time when a #/example/<name> link opens them.
const { writeExamples } = await import("./lib/examples.mjs");
await writeExamples(new URL("../../lang/examples/", root), new URL("examples/", dist));
// The book: static pages from /book, each ```xmd block a live editor.
const { writeBook } = await import("./lib/book.mjs");
await writeBook(new URL("../../book/", root), new URL("book/", dist), new URL("book/", root), new URL("../../", root));
execFileSync("npm", ["run", "build", "--workspace", "xmd-docs"], { cwd: fileURLToPath(root), stdio: "inherit" });
const docsIndex = new URL("docs/index.html", dist);
await writeFile(docsIndex, (await readFile(docsIndex, "utf8")).replace("<head>", '<head><link rel="stylesheet" href="../lib/theme/style.css"><link rel="stylesheet" href="../lib/theme/fonts.css">'));
// Record the actual artifact, so independently cached apps can identify a release.
const { createHash } = await import("node:crypto");
const wasm = await readFile(new URL("pkg/xmd_bg.wasm", root));
const wasmHash = createHash("sha256").update(wasm).digest("hex");
await writeFile(new URL("manifest.json", dist), JSON.stringify({ schemaVersion: 1, wasm: "lib/pkg/xmd_bg.wasm", sha256: wasmHash }, null, 2) + "\n");
// The service worker precaches everything the app needs to open offline; its
// version changes whenever any of those files does.
const { writeServiceWorker } = await import("./lib/service-worker.mjs");
await writeServiceWorker(dist, new URL("apps/docs/public/sw.js", root));
await rm(new URL("docs/sw.js", dist)); // Vite's copy; the worker is served from the site root
console.log("Static site built in client/web/dist; all clients share lib/pkg/xmd_bg.wasm");
