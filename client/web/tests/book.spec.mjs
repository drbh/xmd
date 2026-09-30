import { test, expect } from "@playwright/test";
import { readFile } from "node:fs/promises";
import { readBook, blocksOf } from "../scripts/lib/book.mjs";

// The clock cli/tests/book.rs freezes, so the browser and the native
// snapshots describe the same moment.
const NOW = "2026-09-16T14:00:00-04:00";
const book = new URL("../../../book/", import.meta.url);

/** How far each code-lens chip sits from the end of its line, in pixels. */
const lensOffsets = () => [...document.querySelectorAll(".xmd-lenses")].map(group => {
  const line = group.parentElement.parentElement.querySelector(`.line[data-line="${group.dataset.line}"]`);
  const boxes = line.getClientRects(), end = boxes[boxes.length - 1], chip = group.getBoundingClientRect();
  return { chip: group.textContent.trim(), dy: Math.round(chip.top - end.top), dx: Math.round(chip.left - end.right) };
});

/** A chapter snapshot's notes by file: the text after each `=== file`. */
function notes(snapshot) {
  const sections = {};
  for (const [, file, body] of snapshot.matchAll(/^=== (?!\$ )(\S+)\n([\s\S]*?)(?=^=== |(?![\s\S]))/gm)) sections[file] = body;
  return sections;
}

test("every chapter's notes are live and render what the native tests recorded", async ({ page }) => {
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  for (const chapter of (await readBook(book)).filter(p => p.chapter)) {
    const blocks = blocksOf(chapter);
    if (!blocks.length) continue;
    await page.goto(`/book/${chapter.page}?now=${encodeURIComponent(NOW)}`);
    await page.waitForFunction(() => window.xmdBook?.ready, null, { timeout: 45_000 });
    await expect(page.locator(".xmd-block .xmd-live")).toHaveCount(blocks.length);
    const expected = notes(await readFile(new URL(`snapshots/${chapter.path.replace(/\.md$/, ".txt")}`, book), "utf8"));
    for (const { file } of blocks) {
      const rendered = await page.evaluate(async file => {
        const { workspace, uriOf } = window.xmdBook;
        const snapshot = await workspace.analyze(uriOf(file), { force: true });
        const scratch = document.createElement("pre");
        scratch.innerHTML = snapshot.html;
        return { text: scratch.textContent, diagnostics: snapshot.diagnostics.map(d => d.message) };
      }, file);
      expect(rendered.diagnostics, `${chapter.path}: ${file}`).toEqual([]);
      // The native snapshots write the workspace root as <root>.
      expect(rendered.text.replaceAll("/workspace/book", "<root>"), `${chapter.path}: ${file}`).toBe(expected[file]);
    }
  }
  expect(errors).toEqual([]);
});

test("a book example is a real editor, and Reset puts it back", async ({ page }) => {
  await page.goto(`/book/language.html?now=${encodeURIComponent(NOW)}`);
  await page.waitForFunction(() => window.xmdBook?.ready, null, { timeout: 45_000 });
  const trip = page.locator('.xmd-block[data-file="trip.x.md"]');
  await expect(trip).toContainText("= $1,301");
  await page.evaluate(() => window.xmdBook.views["trip.x.md"].setSource(window.xmdBook.files["trip.x.md"].replace("$67", "$100")));
  await expect(trip).toContainText("= $1,334");
  await trip.locator("button.reset").click();
  await expect(trip).toContainText("= $1,301");
});

test("the reference pages load without the engine", async ({ page }) => {
  const requests = [];
  page.on("request", request => requests.push(request.url()));
  await page.goto("/book/reference/functions.html");
  await expect(page.locator("h1")).toHaveText("functions");
  await expect(page.locator("h3").filter({ hasText: "sort_by(" })).toHaveCount(1);
  expect(requests.filter(url => url.endsWith(".wasm"))).toEqual([]);
});

test("code lens chips sit at the end of their lines, on a phone too", async ({ page }) => {
  for (const width of [1100, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto(`/book/language.html?now=${encodeURIComponent(NOW)}`);
    await page.waitForFunction(() => window.xmdBook?.ready, null, { timeout: 45_000 });
    await expect(page.locator(".xmd-lenses")).toHaveCount(2);
    await expect.poll(() => page.evaluate(lensOffsets)).toEqual([
      { chip: "✓ done", dy: expect.any(Number), dx: expect.any(Number) },
      { chip: "▸ start focus", dy: expect.any(Number), dx: expect.any(Number) },
    ]);
    for (const { chip, dy, dx } of await page.evaluate(lensOffsets)) {
      expect(Math.abs(dy), `${chip} at ${width}px`).toBeLessThanOrEqual(2);
      expect(dx, `${chip} at ${width}px`).toBeGreaterThanOrEqual(0);
      expect(dx, `${chip} at ${width}px`).toBeLessThanOrEqual(20);
    }
  }
});
