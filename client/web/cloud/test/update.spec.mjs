import { test, expect } from "@playwright/test";
import { readFile, writeFile } from "node:fs/promises";
import { writeServiceWorker } from "../../scripts/lib/service-worker.mjs";

// A deployment while the app is open: the page keeps running the build its
// service worker holds, never half of each, until the new build is taken.
const dist = new URL("../../dist/", import.meta.url);
const template = new URL("../../apps/docs/public/sw.js", import.meta.url);
const files = ["lib/src/index.js", "docs/backend.js"].map(f => new URL(f, dist));

test("a new build reaches an open app whole, through the reload chip or the document list", async ({ browser }) => {
  const originals = await Promise.all(files.map(f => readFile(f, "utf8")));
  // A rebuild that changes files whose names stay the same.
  const deploy = async build => {
    await Promise.all(files.map((f, i) => writeFile(f, `${originals[i]}\nglobalThis.xmdBuild${i} = ${JSON.stringify(build)};\n`)));
    await writeServiceWorker(dist, template);
  };
  const context = await browser.newContext({ extraHTTPHeaders: { "x-dev-user": `update-${Date.now()}@example.com` } });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", e => errors.push(e.message));
  const build = () => page.evaluate(() => [globalThis.xmdBuild0 ?? null, globalThis.xmdBuild1 ?? null]);
  const checkForUpdate = () => page.evaluate(async () => (await navigator.serviceWorker.getRegistration()).update());
  try {
    await page.goto("/docs/?test");
    await page.waitForFunction(() => window.xmdDocs?.ready, null, { timeout: 60_000 });
    await page.waitForFunction(async () => !!(await navigator.serviceWorker.getRegistration())?.active, null, { timeout: 30_000 });
    await page.locator(".template", { hasText: "Blank" }).click();
    await page.waitForFunction(() => window.xmdDocs.controller);

    // Inside a document: the new build installs, the page keeps its own build, and a chip offers the switch.
    await deploy("B");
    await checkForUpdate();
    await expect(page.locator(".update-chip")).toBeVisible({ timeout: 30_000 });
    expect(await page.evaluate(() => fetch("../lib/src/index.js").then(r => r.text()))).not.toContain("xmdBuild");
    await Promise.all([page.waitForEvent("load"), page.locator(".update-chip").click()]);
    await page.waitForFunction(() => window.xmdDocs?.ready, null, { timeout: 60_000 });
    await expect.poll(build).toEqual(["B", "B"]);

    // On the document list: the next build is taken as soon as it has installed.
    await page.evaluate(() => window.xmdDocs.home());
    await deploy("C");
    await Promise.all([page.waitForEvent("load", { timeout: 30_000 }), checkForUpdate()]);
    await page.waitForFunction(() => window.xmdDocs?.ready, null, { timeout: 60_000 });
    await expect.poll(build).toEqual(["C", "C"]);
    expect(errors).toEqual([]);
  } finally {
    await Promise.all(files.map((f, i) => writeFile(f, originals[i])));
    await writeServiceWorker(dist, template);
    await context.close();
  }
});
