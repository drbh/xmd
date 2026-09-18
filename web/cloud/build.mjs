// Build the static site, then add the cloud backend module beside the app.
import { cp } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
const web = fileURLToPath(new URL("../", import.meta.url));
if (!process.argv.includes("--site-only")) execFileSync("npm", ["run", process.argv.includes("--wasm") ? "build" : "build:site"], { cwd: web, stdio: "inherit" });
await cp(new URL("client/backend.js", import.meta.url), new URL("../dist/docs/backend.js", import.meta.url));
console.log("Cloud backend module placed at web/dist/docs/backend.js");
