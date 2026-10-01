import { test, expect } from "@playwright/test";
import { mkdtemp, mkdir, writeFile, rm } from "node:fs/promises";
import { execFileSync, spawn } from "node:child_process";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import { createServer } from "node:net";

test("a packed consumer renders in Node and bundles a worker widget under a URL prefix", async ({ page }) => {
  const temporary = await mkdtemp(join(tmpdir(), "xmd-package-"));
  const root = fileURLToPath(new URL("../", import.meta.url));
  let server;
  try {
    execFileSync("npm", ["pack", "--ignore-scripts", "--pack-destination", temporary, "--silent"], { cwd: root });
    const installed = join(temporary, "node_modules/@xmd/web");
    await mkdir(installed, { recursive: true });
    execFileSync("tar", ["-xzf", join(temporary, "xmd-web-0.1.0.tgz"), "--strip-components=1", "-C", installed]);
    await writeFile(join(temporary, "package.json"), '{"type":"module"}');
    const node = execFileSync(process.execPath, ["--input-type=module", "-e", 'import {render} from "@xmd/web"; console.log(await render("answer := 6 * 7\\n", {now:"2026-09-18T12:00:00Z"}));'], { cwd: temporary, encoding: "utf8" });
    expect(node).toContain("= 42");
    await writeFile(join(temporary, "index.html"), '<!doctype html><div id="static"></div><div id="live"></div><script type="module" src="./main.js"></script>');
    await writeFile(join(temporary, "main.js"), `
      import {render, mount} from "@xmd/web";
      import "@xmd/web/style.css";
      async function main() {
        const now = "2026-09-18T12:00:00Z";
        document.querySelector("#static").innerHTML = await render("answer := 6 * 7\\n", {now});
        window.widget = await mount(document.querySelector("#live"), {source:"answer := 10 + 2\\n", now});
        window.ready = true;
      }
      main();
    `);
    const vite = join(dirname(createRequire(import.meta.url).resolve("vite/package.json")), "bin/vite.js");
    execFileSync(process.execPath, [vite, "build", "--base=/embedded/"], { cwd: temporary });
    const probe = createServer();
    await new Promise(resolve => probe.listen(0, "127.0.0.1", resolve));
    const port = probe.address().port;
    await new Promise(resolve => probe.close(resolve));
    server = spawn(process.execPath, [vite, "preview", "--port", String(port), "--host", "127.0.0.1", "--base=/embedded/"], { cwd: temporary, stdio: ["ignore", "pipe", "pipe"] });
    await new Promise((resolve, reject) => { server.stdout.once("data", resolve); server.once("error", reject); server.once("exit", code => reject(new Error(`Preview exited ${code}`))); });
    const errors = [];
    page.on("pageerror", error => errors.push(error.message));
    await page.goto(`http://127.0.0.1:${port}/embedded/`);
    await page.waitForFunction(() => window.ready);
    await expect(page.locator("#static")).toContainText("= 42");
    await expect(page.locator("#live")).toContainText("= 12");
    await page.evaluate(() => widget.destroy());
    expect(errors).toEqual([]);
  } finally {
    if (server) { server.kill("SIGTERM"); await new Promise(resolve => { if (server.exitCode !== null) resolve(); else server.once("exit", resolve); }); }
    await rm(temporary, { recursive: true, force: true });
  }
});
