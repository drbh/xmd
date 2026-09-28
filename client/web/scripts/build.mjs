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
await writeFile(new URL("_headers", dist), "/lib/*\n  Access-Control-Allow-Origin: *\n  Cache-Control: public, max-age=86400\n");
// The site root opens the document app.
await writeFile(new URL("index.html", dist), '<!doctype html><meta charset="utf-8"><meta http-equiv="refresh" content="0; url=docs/"><title>WTF Docs</title><a href="docs/">Open WTF Docs</a>\n');
// The embedding example: a plain page that loads the library like any other site would.
await mkdir(new URL("embed/", dist), { recursive: true });
await cp(new URL("embed/index.html", root), new URL("embed/index.html", dist));
execFileSync("npm", ["run", "build", "--workspace", "wtf-docs"], { cwd: fileURLToPath(root), stdio: "inherit" });
const docsIndex = new URL("docs/index.html", dist);
await writeFile(docsIndex, (await readFile(docsIndex, "utf8")).replace("<head>", '<head><link rel="stylesheet" href="../lib/theme/style.css"><link rel="stylesheet" href="../lib/theme/fonts.css">'));
// Record the actual artifact, so independently cached apps can identify a release.
const { createHash } = await import("node:crypto");
const wasm = await readFile(new URL("pkg/wtf_bg.wasm", root));
const wasmHash = createHash("sha256").update(wasm).digest("hex");
await writeFile(new URL("manifest.json", dist), JSON.stringify({ schemaVersion: 1, wasm: "lib/pkg/wtf_bg.wasm", sha256: wasmHash }, null, 2) + "\n");
// The service worker precaches everything the app needs to open offline; its
// version changes whenever any of those files does.
const { readdir } = await import("node:fs/promises");
const walk = async dir => (await readdir(new URL(dir, dist), { withFileTypes: true, recursive: true })).filter(e => e.isFile()).map(e => `${e.parentPath ?? e.path}/${e.name}`.replace(fileURLToPath(dist), "").replace(/\\/g, "/").replace(/\/+/g, "/"));
const precache = [
  "docs/manifest.webmanifest", "docs/icon.svg", "docs/icon-192.png", "docs/icon-512.png", "docs/apple-touch-icon.png",
  ...(await walk("docs/assets/")),
  ...(await walk("lib/src/")).filter(p => p.endsWith(".js") && !/\/(node|server)\.js$/.test(p)),
  "lib/adapters/contenteditable.js", "lib/theme/style.css", "lib/theme/fonts.css",
  ...(await walk("lib/theme/fonts/")),
  "lib/pkg/wtf.js", "lib/pkg/wtf_bg.wasm",
].map(p => p.replace(/^\//, ""));
const swVersion = createHash("sha256").update(precache.join("\n")).update(wasmHash).digest("hex").slice(0, 12);
const template = new URL("docs/sw.js", dist);
await writeFile(new URL("sw.js", dist), `self.__WTF_VERSION__ = ${JSON.stringify(swVersion)};\nself.__WTF_PRECACHE__ = ${JSON.stringify(precache)};\n${await readFile(template, "utf8")}`);
await rm(template);
console.log("Static site built in client/web/dist; all clients share lib/pkg/wtf_bg.wasm");
