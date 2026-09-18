import { test, expect } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.route("**/embed-test.html", route => route.fulfill({ contentType: "text/html", body: '<!doctype html><link rel="stylesheet" href="/lib/theme/style.css"><div id="one"></div><div id="two"></div><p class="error" id="outside">Outside</p>' }));
  await page.goto("/embed-test.html");
  await page.evaluate(async () => { window.lib = await import("/lib/src/index.js"); });
});

test("multiple views share core task actions and preserve source separately from hints", async ({ page }) => {
  await page.evaluate(async () => {
    window.changes = [];
    window.ws = lib.createWorkspace({ now: "2026-09-18T12:00:00Z" });
    window.one = await lib.mount(document.querySelector("#one"), { workspace: ws, uri: "file:///workspace/🦀 tasks.wtf", source: "- [ ] Parent\n  - [ ] Child\n", onChange: c => changes.push(c) });
    window.two = await lib.mount(document.querySelector("#two"), { workspace: ws, uri: "file:///workspace/value.wtf", source: "a := 1 + 2\n" });
  });
  await page.locator("#one .t-wtfCheckbox").first().click();
  await expect(page.locator("#one pre")).toContainText("[x] Child @completed(2026-09-18)");
  await expect(page.locator("#two .inlay")).toContainText("= 3");
  expect(await page.evaluate(() => two.getSource())).toBe("a := 1 + 2\n");
  await page.evaluate(() => one.destroy());
  await page.evaluate(() => ws.setDocument("file:///workspace/value.wtf", "a := 8\n"));
  await expect(page.locator("#two .inlay")).toContainText("= 8");
  expect(await page.evaluate(() => ws.hasDocument("file:///workspace/🦀 tasks.wtf"))).toBe(true);
  await expect(page.locator("#outside")).not.toHaveCSS("text-decoration-style", "wavy");
  await page.evaluate(() => { two.destroy(); ws.destroy(); });
});

test("static and live HTML match at a fixed clock; heading metadata respects code fences", async ({ page }) => {
  const result = await page.evaluate(async () => {
    const source = '# Heading 🦀\na := 1 + 2\n```\n# Not a heading\n```\n[Site](https://example.com/)\n';
    const now = "2026-09-18T12:00:00Z";
    const html = await lib.render(source, { now });
    document.querySelector("#one").innerHTML = html;
    const view = await lib.mount(document.querySelector("#two"), { source, now, controls: false });
    const snapshot = view.snapshot;
    const same = document.querySelector("#one code").innerHTML === view.element.innerHTML;
    view.destroy();
    return { same, schema: snapshot.schemaVersion, now: snapshot.now, source: snapshot.source };
  });
  expect(result.same).toBe(true);
  expect(result.schema).toBe(1);
  expect(result.now).toBe("2026-09-18T12:00:00+00:00");
  await expect(page.locator("#one .h1")).toHaveCount(1);
  expect(await page.locator('#one a[href="https://example.com/"]').count()).toBeGreaterThan(0);
});

test("dependent views refresh for source and plugin changes, without spurious source notifications", async ({ page }) => {
  await page.evaluate(async () => {
    window.ws = lib.createWorkspace({ now: "2026-09-18T12:00:00Z" });
    await ws.setDocument("file:///workspace/a.wtf", "a := 1\n");
    window.edits = [];
    window.one = await lib.mount(document.querySelector("#one"), { workspace: ws, uri: "file:///workspace/b.wtf", source: "Value [a].\n", onChange: e => edits.push(e) });
    edits.length = 0;
    await ws.setDocument("file:///workspace/a.wtf", "a := 42\n");
  });
  await expect(page.locator("#one .inlay")).toContainText("42");
  await page.evaluate(() => ws.setPlugins({ "custom.wtf": 'plugin := {api: 1, id: "custom", kind: "inlay", inputs: []}\ncollect := fn(ctx) => [{line: 0, label: "CUSTOM"}]' }));
  await expect(page.locator("#one pre")).toContainText("CUSTOM");
  expect(await page.evaluate(() => edits)).toEqual([]);
  await page.evaluate(() => { one.destroy(); ws.destroy(); });
});

test("optional editing preserves ranges, composition, task undo and redo", async ({ page }) => {
  await page.evaluate(async () => {
    const { mountEditor } = await import("/lib/adapters/contenteditable.js");
    window.ws = lib.createWorkspace({ now: "2026-09-18T12:00:00Z" });
    await ws.setDocument("file:///workspace/a.wtf", "a := 1\n");
    window.one = await mountEditor(document.querySelector("#one"), { workspace: ws, uri: "file:///workspace/b.wtf", source: "Value [a].\n- [ ] Parent\n  - [ ] Child\n" });
    one.select(0);
  });
  await page.keyboard.down("Shift");
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("ArrowRight");
  await page.keyboard.up("Shift");
  expect(await page.evaluate(() => getSelection().toString())).toBe("Va");
  await page.evaluate(() => ws.setDocument("file:///workspace/a.wtf", "a := 22\n"));
  await expect(page.locator("#one .inlay").first()).toContainText("22");
  expect(await page.evaluate(() => getSelection().toString())).toBe("Va");
  // A dependency refresh must not destroy an in-progress IME composition.
  await page.evaluate(async () => {
    one.element.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
    one.element.textContent = "Composing 日本語";
    await ws.setDocument("file:///workspace/a.wtf", "a := 33\n");
    await ws.refresh();
  });
  await expect(page.locator("#one pre")).toHaveText("Composing 日本語");
  await page.evaluate(() => one.element.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true })));
  await expect.poll(() => page.evaluate(() => one.getSource())).toBe("Composing 日本語");
  await page.evaluate(() => one.undo());
  await expect(page.locator("#one pre")).toContainText("Value [a]");
  await page.locator("#one .t-wtfCheckbox").first().click();
  await expect(page.locator("#one pre")).toContainText("[x] Child @completed(2026-09-18)");
  await page.evaluate(() => one.undo());
  await expect(page.locator("#one .t-wtfCheckbox")).toHaveCount(2);
  await page.evaluate(() => one.redo());
  await expect(page.locator("#one .t-wtfCheckboxChecked")).toHaveCount(2);
  await page.evaluate(() => { one.destroy(); ws.destroy(); });
});
