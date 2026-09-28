import { test, expect } from "@playwright/test";
import http from "node:http";
import { readFile } from "node:fs/promises";

// The library loaded by another site, from this one: module scripts, the
// engine worker and the WASM all cross origins, which is how the embed
// example on the site works for anyone.
test("another origin can mount a live editor with a few lines", async ({ page }) => {
  const html = (await readFile(new URL("../embed/index.html", import.meta.url), "utf8")).replaceAll("https://xmd-docs.drbh.workers.dev", "http://127.0.0.1:4173");
  const other = http.createServer((_, res) => { res.writeHead(200, { "content-type": "text/html" }); res.end(html); });
  await new Promise(resolve => other.listen(4175, "127.0.0.1", resolve));
  try {
    const errors = [];
    page.on("pageerror", error => errors.push(error.message));
    page.on("console", m => { if (m.type() === "error") errors.push(m.text()); });
    await page.goto("http://127.0.0.1:4175/");
    const note = page.locator("#note .xmd");
    await expect(note).toContainText("= $260", { timeout: 30_000 });
    await expect(note).toContainText("$130");
    // It is a real editor: typing recalculates.
    await note.click();
    await page.evaluate(() => { const view = document.querySelector("#note .xmd"); const range = document.createRange(); const text = [...view.querySelectorAll(".line")].find(l => l.textContent.startsWith("remaining")); range.setStart(text.firstChild, 0); range.collapse(true); getSelection().removeAllRanges(); getSelection().addRange(range); });
    await page.keyboard.type("tip := $20\n");
    await expect(note).toContainText("tip := $20");
    expect(errors).toEqual([]);
    expect(await page.evaluate(() => document.fonts.check('14px "Ioskeley Mono"'))).toBe(true);
  } finally { other.close(); }
});
