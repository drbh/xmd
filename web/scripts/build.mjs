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
// The site root opens the document app; the book lives beside it.
await writeFile(new URL("index.html", dist), '<!doctype html><meta charset="utf-8"><meta http-equiv="refresh" content="0; url=docs/"><title>WTF Docs</title><a href="docs/">Open WTF Docs</a>\n');
// The book is a view inside the document app; keep its old address working.
await mkdir(new URL("book/", dist), { recursive: true });
await writeFile(new URL("book/index.html", dist), '<!doctype html><meta charset="utf-8"><meta http-equiv="refresh" content="0; url=../docs/#/book"><title>The WTF Book</title><a href="../docs/#/book">Open the book</a>\n');
execFileSync("npm", ["run", "build", "--workspace", "wtf-docs"], { cwd: fileURLToPath(root), stdio: "inherit" });
const docsIndex = new URL("docs/index.html", dist);
await writeFile(docsIndex, (await readFile(docsIndex, "utf8")).replace("<head>", '<head><link rel="stylesheet" href="../lib/theme/style.css"><link rel="stylesheet" href="../lib/theme/fonts.css">'));
// Record the actual artifact, so independently cached apps can identify a release.
const { createHash } = await import("node:crypto");
const wasm = await readFile(new URL("pkg/wtf_bg.wasm", root));
await writeFile(new URL("manifest.json", dist), JSON.stringify({ schemaVersion: 1, wasm: "lib/pkg/wtf_bg.wasm", sha256: createHash("sha256").update(wasm).digest("hex") }, null, 2) + "\n");
console.log("Static site built in web/dist; all clients share lib/pkg/wtf_bg.wasm");
