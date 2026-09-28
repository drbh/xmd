import { test, expect } from "@playwright/test";
import { spawn } from "node:child_process";

// The app starts on its home screen; tests edit the first document.
async function openFirst(page) {
  await page.waitForFunction(() => window.wtfDocs?.ready);
  await page.evaluate(() => wtfDocs.open(wtfDocs.documents[0].id));
  await page.waitForFunction(() => wtfDocs.controller);
}

test("switching documents with a running timer preserves the new document and supports remounting", async ({ page }) => {
  const errors = [];
  page.on("pageerror", e => errors.push(e.message));
  await page.goto("/docs/?test");
  await openFirst(page);
  await page.evaluate(async () => {
    await wtfDocs.controller.setSource(`# First\nwatch := stopwatch(0s, ${new Date().toISOString()})\n`);
    window.firstId = wtfDocs.documents[0].id;
    wtfDocs.newDocument();
  });
  await page.waitForFunction(() => wtfDocs.controller && wtfDocs.active?.id === wtfDocs.documents[0].id);
  await page.waitForTimeout(1300);
  const documents = await page.evaluate(() => wtfDocs.documents.map(d => ({ id: d.id, text: d.text })));
  expect(documents[0].text).toBe("# Untitled document\n\n");
  expect(documents[1].text).toContain("# First\nwatch := stopwatch");
  await page.locator(".logo").click();
  await page.locator(".doc-row .open", { hasText: "First" }).click();
  await page.waitForFunction(() => wtfDocs.controller && wtfDocs.active?.id === firstId);
  await page.evaluate(async () => { for (let n = 0; n < 20; n++) await wtfDocs.controller.setSource(`# First\nx := ${n}\n`); });
  await expect(page.locator(".view")).toContainText("= 19");
  expect(errors).toEqual([]);
});

test("docs preserves unreadable storage while editing", async ({ page }) => {
  await page.goto("/docs/?test");
  await page.evaluate(() => localStorage.setItem("wtf.docs.v1", "unreadable"));
  await page.reload();
  await openFirst(page);
  await page.evaluate(() => wtfDocs.controller.setSource("# New text\n"));
  await page.waitForTimeout(400);
  expect(await page.evaluate(() => localStorage.getItem("wtf.docs.v1"))).toBe("unreadable");
  await expect(page.locator(".status")).toContainText("Saving paused");
});

test("another tab pauses docs autosave without overwriting either tab's text", async ({ page, context }) => {
  await page.goto("/docs/?test");
  await openFirst(page);
  const other = await context.newPage();
  try {
    await other.goto("/docs/?test");
    await openFirst(other);
    await other.evaluate(() => wtfDocs.controller.setSource("# Other tab\n"));
    await expect(page.locator(".status")).toContainText("Another tab changed");
    const saved = await page.evaluate(() => localStorage.getItem("wtf.docs.v1"));
    await page.evaluate(() => wtfDocs.controller.setSource("# My unsaved changes\n"));
    await page.waitForTimeout(400);
    expect(await page.evaluate(() => localStorage.getItem("wtf.docs.v1"))).toBe(saved);
    expect(await page.evaluate(() => wtfDocs.controller.getSource())).toBe("# My unsaved changes\n");
  } finally { await other.close(); }
});

test("the assembled apps work below a URL prefix and load the same WASM asset", async ({ page, request }) => {
  const server = spawn(process.execPath, ["serve.mjs"], { env: { ...process.env, WTF_WEB_PORT: "4274", WTF_WEB_BASE: "/nested/site" }, stdio: ["ignore", "pipe", "pipe"] });
  try {
    await new Promise((resolve, reject) => { server.stdout.once("data", resolve); server.once("error", reject); server.once("exit", code => reject(new Error(`Static server exited ${code}`))); });
    const wasm = [];
    page.on("request", r => { if (r.url().endsWith(".wasm")) wasm.push(r.url()); });
    const base = "http://127.0.0.1:4274/nested/site";
    for (const [path, ready] of [["/docs/?test", () => window.wtfDocs?.ready]]) {
      await page.goto(base + path);
      await page.waitForFunction(ready);
    }
    expect(new Set(wasm)).toEqual(new Set([`${base}/lib/pkg/wtf_bg.wasm`]));
    const manifest = await (await request.get(`${base}/manifest.json`)).json();
    expect(manifest.wasm).toBe("lib/pkg/wtf_bg.wasm");
    expect(manifest.sha256).toMatch(/^[a-f0-9]{64}$/);
  } finally {
    server.kill("SIGTERM");
    await new Promise(resolve => { if (server.exitCode !== null) resolve(); else server.once("exit", resolve); });
  }
});
