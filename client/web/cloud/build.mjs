// Build the static site, then bundle the cloud backend module beside the app.
// Yjs lands in a separate chunk that only loads when a document goes live.
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";
const web = fileURLToPath(new URL("../", import.meta.url));
if (!process.argv.includes("--site-only")) execFileSync("npm", ["run", process.argv.includes("--wasm") ? "build" : "build:site"], { cwd: web, stdio: "inherit" });
await build({
  entryPoints: [fileURLToPath(new URL("client/backend.js", import.meta.url))],
  outdir: fileURLToPath(new URL("../dist/docs/", import.meta.url)),
  bundle: true, format: "esm", splitting: true, minify: true, sourcemap: false, target: "es2022",
  external: ["../lib/*"], // the site's own library is loaded at runtime, not bundled twice
  chunkNames: "live-[hash]",
  logLevel: "warning",
});
// The service worker precaches the backend module and its chunks too, so the
// account mode is available offline; its version changes with them.
const { writeServiceWorker } = await import("../scripts/lib/service-worker.mjs");
await writeServiceWorker(new URL("../dist/", import.meta.url), new URL("../apps/docs/public/sw.js", import.meta.url));
console.log("Cloud backend module bundled to client/web/dist/docs/backend.js and precached");
