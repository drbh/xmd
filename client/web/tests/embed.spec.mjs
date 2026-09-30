import { test, expect } from "@playwright/test";
import http from "node:http";
import { readFile } from "node:fs/promises";

// The library loaded by another site, from this one: module scripts, the
// engine worker and the WASM all cross origins, which is how the embed
// example on the site works for anyone.
test("another origin can mount a live editor with a few lines", async ({ page }) => {
  const html = (await readFile(new URL("../embed/index.html", import.meta.url), "utf8")).replaceAll("https://xmd.dholtz.com", "http://127.0.0.1:4173");
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

// A page may mount into an element before putting it in the document; the
// chips still have to land on their lines once it is there.
test("code lens chips find their lines when the editor was mounted before it was attached", async ({ page }) => {
  await page.goto("/embed/");
  await page.evaluate(async () => {
    document.body.innerHTML = "<div style='height: 300px'></div>";
    const { mountEditor } = await import("/lib/adapters/contenteditable.js");
    const host = document.createElement("div");
    await mountEditor(host, { source: "- [ ] Pack\nfocus := countdown(25m)\n" });
    document.body.append(host);
  });
  await expect(page.locator(".xmd-lenses")).toHaveCount(2);
  await expect.poll(() => page.evaluate(() => [...document.querySelectorAll(".xmd-lenses")].map(group => {
    const line = group.parentElement.parentElement.querySelector(`.line[data-line="${group.dataset.line}"]`);
    const boxes = line.getClientRects();
    return Math.abs(Math.round(group.getBoundingClientRect().top - boxes[boxes.length - 1].top));
  }))).toEqual([0, 0]);
});

// An edit repaints after a round trip to the engine; focus someone moved to
// another control in the meantime stays there.
test("an edit's repaint does not take focus back from another control", async ({ page }) => {
  await page.goto("/embed/");
  const typed = await page.evaluate(async () => {
    document.body.innerHTML = "<input id='title'><div id='note'></div>";
    const { mountEditor } = await import("/lib/adapters/contenteditable.js");
    const view = await mountEditor(document.querySelector("#note"), { source: "rent := $900\n" });
    view.select(0);
    const edit = view.replaceRange(0, 4, "lease");
    document.querySelector("#title").focus();
    await edit;
    return document.activeElement.id;
  });
  expect(typed).toBe("title");
});
