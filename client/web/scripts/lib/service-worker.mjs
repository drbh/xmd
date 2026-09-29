// Stamps dist/sw.js with the files the docs app needs to open offline. Every
// entry carries a hash of its contents and the version is a hash of all of
// them, so any change to any file is a new service worker. Both the site build
// and the cloud build call this after their last file is written.
import { readFile, readdir, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";
import { join, relative, sep } from "node:path";

const sha = bytes => createHash("sha256").update(bytes).digest("hex");

export async function writeServiceWorker(dist, template) {
  const root = fileURLToPath(dist);
  const files = async dir => (await readdir(new URL(dir, dist), { recursive: true, withFileTypes: true }).catch(() => []))
    .filter(e => e.isFile()).map(e => relative(root, join(e.parentPath ?? e.path, e.name)).split(sep).join("/")).sort();
  const urls = [
    "docs/manifest.webmanifest", "docs/icon.svg", "docs/icon-192.png", "docs/icon-512.png", "docs/apple-touch-icon.png",
    ...await files("docs/assets/"),
    // The hosted deployment's backend module and its chunks, when the cloud build has placed them.
    ...(await files("docs/")).filter(p => /^docs\/(backend|live-[^/]*)\.js$/.test(p)),
    ...(await files("lib/src/")).filter(p => p.endsWith(".js") && !/\/(node|server)\.js$/.test(p)),
    "lib/adapters/contenteditable.js", "lib/theme/style.css", "lib/theme/fonts.css",
    ...await files("lib/theme/fonts/"),
    "lib/pkg/xmd.js", "lib/pkg/xmd_bg.wasm",
  ];
  const precache = await Promise.all(urls.map(async url => ({ url, revision: sha(await readFile(new URL(url, dist))).slice(0, 16) })));
  // The shell is part of the build too: a change to the page alone is a new version.
  const shell = sha(await readFile(new URL("docs/index.html", dist))).slice(0, 16);
  const version = sha(JSON.stringify([shell, precache])).slice(0, 12);
  await writeFile(new URL("sw.js", dist), `self.__XMD_VERSION__ = ${JSON.stringify(version)};\nself.__XMD_PRECACHE__ = ${JSON.stringify(precache)};\n${await readFile(template, "utf8")}`);
  return { version, precache };
}
