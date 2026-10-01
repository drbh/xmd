import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, writeFile, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { writeServiceWorker } from "../scripts/lib/service-worker.mjs";

const template = new URL("../apps/docs/public/sw.js", import.meta.url);

// The smallest site the precache list expects, with every file's contents distinct.
async function site() {
  const dir = await mkdtemp(join(tmpdir(), "xmd-sw-"));
  const files = [
    "docs/index.html", "docs/manifest.webmanifest", "docs/icon.svg", "docs/icon-192.png", "docs/icon-512.png", "docs/apple-touch-icon.png",
    "docs/assets/index-abc.js", "docs/backend.js", "docs/live-XYZ.js",
    "lib/src/index.js", "lib/src/node.js", "lib/adapters/contenteditable.js", "lib/theme/style.css", "lib/theme/fonts.css",
    "lib/theme/fonts/a.woff2", "lib/pkg/xmd.js", "lib/pkg/xmd_bg.wasm",
  ];
  for (const f of files) { await mkdir(join(dir, f, ".."), { recursive: true }); await writeFile(join(dir, f), `// ${f}\n`); }
  return { dir, url: pathToFileURL(`${dir}/`) };
}

test("every file of the build is precached with a hash of its contents", async () => {
  const { dir, url } = await site();
  try {
    const { precache } = await writeServiceWorker(url, template);
    const urls = precache.map(e => e.url);
    for (const f of ["docs/assets/index-abc.js", "docs/backend.js", "docs/live-XYZ.js", "lib/src/index.js", "lib/pkg/xmd_bg.wasm"]) assert.ok(urls.includes(f), f);
    assert.ok(!urls.includes("lib/src/node.js"), "the Node entry is not part of the app");
    assert.ok(precache.every(e => /^[0-9a-f]{16}$/.test(e.revision)));
    assert.match(await readFile(new URL("sw.js", url), "utf8"), /^self\.__XMD_VERSION__ = "[0-9a-f]{12}";\nself\.__XMD_PRECACHE__ = \[/);
  } finally { await rm(dir, { recursive: true }); }
});

test("a change to any file's contents is a new version, even under the same name", async () => {
  const { dir, url } = await site();
  try {
    const first = (await writeServiceWorker(url, template)).version;
    assert.equal((await writeServiceWorker(url, template)).version, first, "an unchanged build keeps its version");
    const seen = new Set([first]);
    for (const f of ["lib/src/index.js", "docs/backend.js", "lib/pkg/xmd.js", "docs/index.html"]) {
      await writeFile(join(dir, f), `// ${f}, changed\n`);
      const { version } = await writeServiceWorker(url, template);
      assert.ok(!seen.has(version), `${f} changed the version`);
      seen.add(version);
    }
  } finally { await rm(dir, { recursive: true }); }
});
