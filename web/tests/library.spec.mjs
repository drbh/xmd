import { test, expect } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.route("**/embed-test.html", route => route.fulfill({ contentType: "text/html", body: '<!doctype html><link rel="stylesheet" href="/theme/style.css"><div id="one"></div><div id="two"></div><p class="error" id="outside">Outside</p>' }));
  await page.goto("/embed-test.html");
  await page.evaluate(async () => { window.lib = await import("/src/index.js"); });
});

test("multiple views share core task actions and preserve source separately from hints", async ({ page }) => {
  await page.evaluate(async () => {
    window.changes = [];
    window.ws = lib.createWorkspace({ now: "2026-09-18T12:00:00Z" });
    window.one = await lib.mount(document.querySelector("#one"), { workspace: ws, uri: "file:///workspace/tasks.wtf", source: "- [ ] Parent\n  - [ ] Child\n", onChange: c => changes.push(c) });
    window.two = await lib.mount(document.querySelector("#two"), { workspace: ws, uri: "file:///workspace/value.wtf", source: "a := 1 + 2\n" });
  });
  await page.locator("#one .t-wtfCheckbox").first().click();
  await expect(page.locator("#one pre")).toContainText("[x] Child @completed(2026-09-18)");
  await expect(page.locator("#two .inlay")).toContainText("= 3");
  expect(await page.evaluate(() => two.getSource())).toBe("a := 1 + 2\n");
  await page.evaluate(() => one.destroy());
  await page.evaluate(() => ws.setDocument("file:///workspace/value.wtf", "a := 8\n"));
  await expect(page.locator("#two .inlay")).toContainText("= 8");
  expect(await page.evaluate(() => ws.hasDocument("file:///workspace/tasks.wtf"))).toBe(true);
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
