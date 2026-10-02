import { test, expect } from "@playwright/test";
import { readFile } from "node:fs/promises";
import { readBook, blocksOf } from "../scripts/lib/book.mjs";

// The clock hosts/cli/tests/book.rs freezes, so the browser and the native
// snapshots describe the same moment.
const NOW = "2026-09-16T14:00:00-04:00";
const book = new URL("../../../book/", import.meta.url);

/** How far each code-lens chip in `scope` sits from the end of its line, in pixels. */
const lensOffsets = scope => [...document.querySelectorAll(`${scope} .xmd-lenses`)].map(group => {
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
    await page.goto(`/${chapter.page}?now=${encodeURIComponent(NOW)}`);
    await page.waitForFunction(() => window.xmdBook?.ready, null, { timeout: 45_000 });
    await expect(page.locator(".xmd-block .xmd-live")).toHaveCount(blocks.length);
    const expected = notes(await readFile(new URL(`snapshots/${chapter.path.replace(/\.md$/, ".txt")}`, book), "utf8"));
    // An active module is recorded as written, not rendered as a note.
    for (const { file } of blocks.filter(b => !b.active)) {
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
  await page.goto(`/?now=${encodeURIComponent(NOW)}`);
  await page.waitForFunction(() => window.xmdBook?.ready, null, { timeout: 45_000 });
  const weekend = page.locator('.xmd-block[data-file="weekend.x.md"]');
  await expect(weekend).toContainText("= $1,324");
  await page.evaluate(() => window.xmdBook.views["weekend.x.md"].setSource(window.xmdBook.files["weekend.x.md"].replace("$90", "$100")));
  await expect(weekend).toContainText("= $1,334");
  await weekend.locator("button.reset").click();
  await expect(weekend).toContainText("= $1,324");
});

test("the reference pages load without the engine", async ({ page }) => {
  const requests = [];
  page.on("request", request => requests.push(request.url()));
  await page.goto("/book/reference/functions.html");
  await expect(page.locator("h1")).toHaveText("functions");
  await expect(page.locator("h3").filter({ hasText: "sort_by(" })).toHaveCount(1);
  // Example modules come highlighted from the build, with nothing inline.
  await page.goto("/book/reference/writing-modules.html");
  const feature = page.locator(".xmd-static").first();
  await expect(feature.locator(".t-function").first()).toBeVisible();
  await expect(feature.locator(".inlay, .diagnostic")).toHaveCount(0);
  expect(requests.filter(url => url.endsWith(".wasm"))).toEqual([]);
});

test("code lens chips sit at the end of their lines, on a phone too", async ({ page }) => {
  for (const width of [1100, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto(`/?now=${encodeURIComponent(NOW)}`);
    await page.waitForFunction(() => window.xmdBook?.ready, null, { timeout: 45_000 });
    // The editor-features block offers a lens on each of its tasks, and its
    // lines are short enough for every chip to sit after its line.
    const block = '.xmd-block[data-file="01-blog-3.x.md"]';
    await expect(page.locator(`${block} .xmd-lenses`)).toHaveCount(3);
    await expect.poll(() => page.evaluate(lensOffsets, block).then(l => l.map(c => c.chip))).toEqual(["○ reopen", "✓ done", "✓ done"]);
    for (const { chip, dy, dx } of await page.evaluate(lensOffsets, block)) {
      expect(Math.abs(dy), `${chip} at ${width}px`).toBeLessThanOrEqual(2);
      expect(dx, `${chip} at ${width}px`).toBeGreaterThanOrEqual(0);
      expect(dx, `${chip} at ${width}px`).toBeLessThanOrEqual(20);
    }
  }
});

test("a block's off= turns highlight, results and controls off on the page", async ({ page }) => {
  await page.goto(`/?now=${encodeURIComponent(NOW)}`);
  await page.waitForFunction(() => window.xmdBook?.ready, null, { timeout: 45_000 });
  const [plain, colored, full] = ["01-blog-1.x.md", "01-blog-2.x.md", "01-blog-3.x.md"].map(file => page.locator(`.xmd-block[data-file="${file}"]`));
  const money = block => block.locator(".t-xmdMoney").evaluate(span => [getComputedStyle(span).color, getComputedStyle(span.closest("pre")).color]);
  const [plainMoney, plainText] = await money(plain);
  expect(plainMoney).toBe(plainText);
  const [coloredMoney, coloredText] = await money(colored);
  expect(coloredMoney).not.toBe(coloredText);
  for (const block of [plain, colored]) {
    await expect(block.locator(".inlay")).toBeHidden();
    await expect(block.locator(".xmd-lens")).toHaveCount(0);
  }
  await expect(full.locator(".inlay")).toBeVisible();
  await expect(full.locator(".xmd-lens").first()).toBeVisible();
});

test("a module block marked active=chapter works on every note on its page", async ({ page }) => {
  await page.goto(`/?now=${encodeURIComponent(NOW)}`);
  await page.waitForFunction(() => window.xmdBook?.ready, null, { timeout: 45_000 });
  const spotted = page.locator('.xmd-block[data-file="spotted.x.md"]');
  await expect(spotted.locator(".inlay")).toHaveText([/Ardea herodias/, /Falco sparverius/, /Troglodytes aedon/]);
  await expect(spotted.locator(".t-xmdCategory3").first()).toHaveText("heron");
  // Editing the module changes the notes it reads.
  await page.evaluate(() => window.xmdBook.views["birds.xmd"].setSource(window.xmdBook.files["birds.xmd"].replace("Ardea herodias", "great blue heron")));
  await expect(spotted.locator(".inlay").first()).toHaveText(/great blue heron/);
});

test("the contents open from the top bar: beside the text on wide screens, over it on phones", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/book/reference/collections.html");
  const sidebar = page.locator(".sidebar"), toggle = page.locator(".topbar .toggle");
  await expect(page.locator(".topbar")).toBeVisible();
  await expect(page.locator('.topbar a[href="../../docs/"]')).toBeVisible();
  await expect(page.locator('.topbar a[href="https://github.com/drbh/xmd"]')).toBeVisible();
  // Closed until asked for, then beside the text.
  await expect(sidebar).toBeHidden();
  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-expanded", "true");
  await expect(sidebar).toBeVisible();
  const edge = await sidebar.evaluate(el => el.getBoundingClientRect().right);
  const text = await page.locator("main h1").evaluate(el => el.getBoundingClientRect().left);
  expect(text).toBeGreaterThan(edge);
  await expect(sidebar.locator('a[aria-current="page"]')).toHaveText("collections");
  // Scrolling to a section marks it in the contents.
  await page.evaluate(() => document.querySelector("#tasks-and-time").scrollIntoView());
  await expect(sidebar.locator("a.here")).toHaveText("tasks and time");
  // The choice holds on the next page.
  await page.goto("/book/reference/functions.html");
  await expect(sidebar).toBeVisible();

  // A phone starts closed, and following a link puts the contents away.
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(sidebar).toBeHidden();
  await toggle.click();
  await expect(sidebar).toBeVisible();
  await sidebar.locator("ol ol a").first().click();
  await expect(sidebar).toBeHidden();
});

test("a terminal prints what xmd printed, and runs what a reader types", async ({ page }) => {
  await page.goto(`/?now=${encodeURIComponent(NOW)}`);
  await page.waitForFunction(() => window.xmdBook?.ready, null, { timeout: 45_000 });
  // Each command's output, as the native book test recorded it.
  const snapshot = await readFile(new URL("snapshots/01-blog.txt", book), "utf8");
  const recorded = Object.fromEntries([...snapshot.matchAll(/^=== \$ (.*)\n([\s\S]*?)(?=^=== |(?![\s\S]))/gm)].map(([, command, out]) => [command, out.trimEnd()]));
  const terminal = page.locator('.xmd-terminal[data-file="weekend.x.md"]');
  const entries = terminal.locator(".term-entry");
  const commands = Object.keys(recorded);
  await expect(entries).toHaveCount(commands.length);
  for (const [i, command] of commands.entries()) {
    const out = await entries.nth(i).locator(".term-out").allTextContents();
    expect(out.join("").trimEnd(), command).toBe(recorded[command]);
  }
  // A query on its own runs on the note, and sees an edit to it.
  await page.evaluate(() => window.xmdBook.views["weekend.x.md"].setSource(window.xmdBook.files["weekend.x.md"].replace("$90", "$100")));
  await terminal.locator("input").fill("each");
  await terminal.locator("input").press("Enter");
  await expect(entries.last().locator(".term-out")).toHaveText("$444.67");
  await terminal.locator("input").fill("xmd nowhere.x.md 'each'");
  await terminal.locator("input").press("Enter");
  await expect(entries.last().locator(".term-err")).toHaveText("xmd: nowhere.x.md: No such file or directory");
});

test("the app's service worker pins only the app to its build, never the book", async ({ page }) => {
  // Install the worker the way a visit to the app does.
  await page.goto("/docs/");
  await page.waitForFunction(() => navigator.serviceWorker?.controller !== null, null, { timeout: 30_000 }).catch(() => page.reload());
  await page.waitForFunction(() => !!navigator.serviceWorker?.controller, null, { timeout: 30_000 });
  // Stand in for an older build: the cached engine is not the deployed one.
  await page.evaluate(async () => {
    for (const name of await caches.keys()) {
      const cache = await caches.open(name);
      if (await cache.match("/lib/pkg/xmd_bg.wasm")) await cache.put("/lib/pkg/xmd_bg.wasm", new Response("stale", { headers: { "content-type": "application/wasm" } }));
    }
  });
  // The book takes the deployed engine, so its terminal still runs.
  await page.goto(`/?now=${encodeURIComponent(NOW)}`);
  await page.waitForFunction(() => window.xmdBook?.ready, null, { timeout: 45_000 });
  const terminal = page.locator('.xmd-terminal[data-file="weekend.x.md"]');
  await terminal.locator("input").fill("each");
  await terminal.locator("input").press("Enter");
  await expect(terminal.locator(".term-entry").last().locator(".term-out")).toHaveText("$441.33");
  // The app still gets its own build's copy.
  const served = await page.evaluate(async () => {
    const frame = document.createElement("iframe");
    frame.src = "/docs/";
    document.body.append(frame);
    await new Promise(r => frame.onload = r);
    return frame.contentWindow.fetch("/lib/pkg/xmd_bg.wasm").then(r => r.text());
  });
  expect(served).toBe("stale");
});
