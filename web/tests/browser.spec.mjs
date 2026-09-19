import { test, expect, devices } from "@playwright/test";

test("the book runs every example as a live block inside the document app", async ({ page }) => {
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.goto("/docs/?test#/book");
  await page.waitForFunction(() => window.wtfDocs?.ready, null, { timeout: 45_000 });
  const first = page.locator('.wtf-block[data-file="01-values.wtf"]');
  await expect(first.locator(".view")).toContainText("$556", { timeout: 20_000 });
  // Tokens use the light-paper palette on the book's white page.
  await expect(page.locator("html")).toHaveClass(/wtf-light/);
  await expect(first.locator(".view .t-wtfMoney").first()).toHaveCSS("color", "rgb(94, 132, 52)");
  // Editing a block re-solves it on the app's own workspace.
  await page.evaluate(() => window.wtfDocs.workspace.setDocument("file:///workspace/book/01-values.wtf", "[$10]:a\n[$4]:b\n[c] := a + b\n"));
  await expect(first.locator(".view")).toContainText("= $14");
  // Inlays sit inline at their anchor, right after the definition, not at the line end.
  const inlay = first.locator(".view .inlay").first();
  await expect(inlay).toHaveText("= $14");
  expect(await inlay.evaluate(el => el.previousSibling?.textContent?.endsWith("a + b") || el.previousSibling?.textContent?.endsWith("b"))).toBe(true);
  // Cross-note values resolve because every block shares one workspace.
  await expect(page.locator('.wtf-block[data-file="27-cross-note-values.wtf"] .view')).toContainText("$3,040");
  // Plans solve in the page, and itineraries paint their kinds.
  await expect(page.locator('.wtf-block[data-file="14-plans.wtf"] .view')).toContainText("= $94.75");
  await expect(page.locator('.wtf-block[data-file="19-itinerary.wtf"] .view .t-wtfDepart').first()).toBeVisible();
  // Hovering a name shows a floating box with the engine's hover, not a panel below.
  const box = page.locator('.wtf-block[data-file="02-calculations.wtf"]');
  await box.scrollIntoViewIfNeeded();
  const view = box.locator(".view");
  const text = await view.evaluate(el => el.textContent);
  const line = text.split("\n").findIndex(l => l.startsWith("total :="));
  const lineBox = await view.evaluate((el, line) => {
    const style = getComputedStyle(el);
    const r = el.getBoundingClientRect();
    return { x: r.left + parseFloat(style.paddingLeft), y: r.top + parseFloat(style.paddingTop) + (line + 0.5) * parseFloat(style.lineHeight), char: parseFloat(style.fontSize) * 0.6 };
  }, line);
  await page.mouse.move(lineBox.x + lineBox.char * 3, lineBox.y);
  await page.mouse.move(lineBox.x + lineBox.char * 3.2, lineBox.y);
  const hover = box.locator(".hover");
  await expect(hover).toBeVisible();
  await expect(hover).toContainText("total");
  expect(await hover.evaluate(el => getComputedStyle(el).position)).toBe("fixed");
  await page.mouse.move(5, 5);
  await expect(hover).toBeHidden();
  // The bundled monospace font is served and applied to blocks.
  await expect(first.locator(".view")).toHaveCSS("font-family", /Ioskeley Mono/);
  expect(await page.evaluate(async () => { await document.fonts.ready; return document.fonts.check('14px "Ioskeley Mono"'); })).toBe(true);
  // Unfetched lookups are warnings, listed under the block.
  await expect(page.locator('.wtf-block[data-file="05-currencies.wtf"] .problems .warn').first()).toContainText("No cached rate");
  // Contents navigation and the way back to documents.
  await page.locator(".toc .row", { hasText: "3.1 Tables" }).click();
  await expect(page).toHaveURL(/#\/book\/tables$/);
  await page.locator(".logo").click();
  await expect(page.locator(".template").first()).toBeVisible();
  // The old address still lands on the book.
  await page.goto("/book/");
  await expect(page).toHaveURL(/docs\/#\/book$/);
  expect(errors).toEqual([]);
});

test("the document view edits, toggles checkboxes, persists, and shares a workspace", async ({ page }) => {
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.goto("/docs/?test");
  await page.waitForFunction(() => window.wtfDocs?.ready, null, { timeout: 45_000 });
  // The app opens on a home screen listing templates and documents.
  await expect(page.locator(".template")).toHaveCount(5);
  await page.locator(".doc-row .open", { hasText: "Trip budget" }).click();
  await page.waitForFunction(() => window.wtfDocs.controller);
  const view = page.locator(".view");
  await expect(view).toContainText("= $556");
  await expect(view.locator(".line.h1").first()).toHaveText(/Trip budget/);
  await expect(page.locator("input.title-input")).toHaveValue("Trip budget");
  // Clicking a checkbox flips it in the text and the engine repaints.
  const box = view.locator(".t-wtfCheckbox").first();
  await box.click();
  await expect(view).toContainText("[x] Book the hotel");
  await expect(page.locator(".status")).toContainText("Saved in this browser");
  // Code lenses are chips at the end of their lines rather than a control bar under the page.
  await expect(page.locator(".wtf-controls")).toHaveCount(0);
  const lens = page.locator(".lens", { hasText: "Reopen task" }).first();
  await expect(lens).toBeVisible();
  await lens.click();
  await expect(view).toContainText("[ ] Book the hotel");
  await expect(page.locator(".lens", { hasText: "Reopen task" })).toHaveCount(1);
  // The bundled font ships its checkbox ligature: the marker and box shape as one run.
  await expect(view).toHaveCSS("font-family", /Ioskeley Mono/);
  expect(await page.evaluate(async () => { await document.fonts.ready; return document.fonts.check('14px "Ioskeley Mono"'); })).toBe(true);
  // The sidebar is hidden by default.
  await expect(page.locator(".sidebar")).toBeHidden();
  // A second document must import the first explicitly.
  await page.evaluate(() => window.wtfDocs.newDocument());
  await page.waitForFunction(() => window.wtfDocs.controller && window.wtfDocs.active.name === "Untitled document");
  await page.evaluate(() => window.wtfDocs.controller.setSource("# Second\n\nStill [remaining] to spend.\n"));
  await expect(view).toContainText("Still [remaining]");
  await expect(view).not.toContainText("$556");
  await page.evaluate(() => {
    const first = window.wtfDocs.documents.find(d => d.name === "Trip budget");
    return window.wtfDocs.controller.setSource(`# Second\n\nStill [source.remaining] to spend.\nsource := import("./${first.id}.wtf")\n`);
  });
  await expect(view).toContainText("$556");
  // The title follows the first heading, and saves are debounced briefly.
  await expect(page.locator("input.title-input")).toHaveValue("Second");
  await page.waitForTimeout(600);
  // Documents survive a reload; the URL reopens the same document.
  await page.reload();
  await page.waitForFunction(() => window.wtfDocs?.ready, null, { timeout: 45_000 });
  await page.waitForFunction(() => window.wtfDocs.controller);
  await expect(page.locator("input.title-input")).toHaveValue("Second");
  await page.locator(".logo").click();
  await expect(page.locator(".doc-list")).toContainText("Second");
  await expect(page.locator(".doc-list")).toContainText("Trip budget");
  expect(errors).toEqual([]);
});

test("the document app writes like a document editor: typing, formatting, find, menus, and themes", async ({ page }) => {
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.goto("/docs/?test");
  await page.waitForFunction(() => window.wtfDocs?.ready, null, { timeout: 45_000 });
  await page.locator(".template", { hasText: "Blank" }).click();
  await page.waitForFunction(() => window.wtfDocs.controller);
  const source = () => page.evaluate(() => window.wtfDocs.controller.getSource());
  const view = page.locator(".view");
  // A blank document accepts prose immediately, including Enter at the end of the text.
  await page.waitForTimeout(200);
  await page.keyboard.type("First line");
  await page.keyboard.press("Enter");
  await page.keyboard.type("rent is $900:rent");
  await page.keyboard.press("Enter");
  await page.keyboard.type("we pay [rent]");
  await expect.poll(source).toBe("# Untitled document\n\nFirst line\nrent is $900:rent\nwe pay [rent]");
  await expect(view).toContainText("$900");
  // Keyboard formatting wraps the word at the caret and toggles line styles.
  await page.keyboard.press("ControlOrMeta+b");
  await expect.poll(source).toContain("we pay **[rent]**");
  await page.keyboard.press("ControlOrMeta+b");
  await expect.poll(source).toContain("we pay [rent]\n".trimEnd());
  await page.locator(".select.style").selectOption("h2");
  await expect.poll(source).toContain("## we pay [rent]");
  await expect(page.locator(".select.style")).toHaveValue("h2");
  await page.keyboard.press("ControlOrMeta+Shift+9");
  await expect.poll(source).toContain("- [ ] we pay [rent]");
  await expect(page.locator(".toolbar [aria-label=Checklist]")).toHaveAttribute("aria-pressed", "true");
  // Find walks forward from the caret; replace works on the source and keeps the document consistent.
  await page.evaluate(() => window.wtfDocs.controller.select(0));
  await page.keyboard.press("ControlOrMeta+Shift+h");
  await page.locator(".findbar input[type=search]").fill("rent");
  await expect(page.locator(".findbar .count")).toHaveText("1 of 3");
  await page.locator(".findbar input[name=replacement]").fill("lease");
  await page.locator(".findbar button", { hasText: "Replace all" }).click();
  await expect.poll(source).toContain("lease is $900:lease");
  await page.keyboard.press("Escape");
  await expect(page.locator(".findbar")).toHaveCount(0);
  // Menus run the same commands with the same labels.
  await page.locator(".menubar > .menu > button", { hasText: "Format" }).click();
  await page.locator(".dropdown [role=menuitem]", { hasText: "Normal text" }).click();
  await expect.poll(source).toContain("\nwe pay [lease]");
  // Renaming through the title edits the first heading.
  await page.locator("input.title-input").fill("Housing");
  await page.keyboard.press("Enter");
  await expect.poll(source).toMatch(/^# Housing\n/);
  // Word count and theme preferences.
  await page.keyboard.press("ControlOrMeta+Shift+c");
  await expect(page.locator(".dialog")).toContainText("Words");
  await page.keyboard.press("Escape");
  // The console runs read-only queries against the resolved document.
  await page.keyboard.press("ControlOrMeta+Alt+j");
  const query = page.locator(".console input");
  await expect(page.locator(".console")).toHaveAttribute("data-fields", "ready");
  await query.fill("lease * 2");
  await page.keyboard.press("Enter");
  await expect(page.locator(".console .entry").last()).toContainText("$1,800");
  // Typeahead knows collections, pipeline stages, learned record fields, and functions.
  await query.fill("");
  await query.pressSequentially("val");
  await expect(page.locator(".typeahead li .label")).toHaveText(["values"]);
  await page.keyboard.press("Tab");
  await expect(query).toHaveValue("values");
  await query.pressSequentially(" | ");
  await expect(page.locator(".typeahead li .label").first()).toHaveText("where");
  await page.keyboard.press("Escape");
  await query.fill("");
  await query.pressSequentially("map(values, fn(v) => v.");
  await expect(page.locator(".typeahead li .label", { hasText: /^name$/ })).toBeVisible();
  await page.keyboard.press("Escape");
  await query.fill("values | select {name, type}");
  await page.keyboard.press("Enter");
  await expect(page.locator(".console .entry").last().locator("table")).toContainText("lease");
  await query.fill("oops(");
  await page.keyboard.press("Enter");
  await expect(page.locator(".console .entry").last().locator(".error")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.locator(".console")).toHaveCount(0);
  await expect.poll(source).toMatch(/^# Housing\n/);
  await page.locator(".menubar > .menu > button", { hasText: "View" }).click();
  await page.locator(".dropdown [role=menuitemcheckbox]", { hasText: "Dark theme" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem("wtf.docs.prefs.v1")).theme)).toBe("dark");
  await page.locator(".menubar > .menu > button", { hasText: "View" }).click();
  await page.locator(".dropdown [role=menuitemcheckbox]", { hasText: "Light theme" }).click();
  await expect(page.locator("html")).toHaveClass(/wtf-light/);
  expect(errors).toEqual([]);
});

test.describe("on a phone", () => {
  const { defaultBrowserType, ...phone } = devices["iPhone 14"];
  test.use(phone);
  test("the app fits the screen, edits, and shows actions for the caret's line", async ({ page }) => {
    const errors = [];
    page.on("pageerror", error => errors.push(error.message));
    await page.goto("/docs/?test");
    await page.waitForFunction(() => window.wtfDocs?.ready, null, { timeout: 45_000 });
    const noSideways = () => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth && [...document.querySelectorAll(".home, .app, .canvas")].every(el => el.scrollWidth <= el.clientWidth + 1));
    expect(await noSideways()).toBe(true);
    await expect(page.locator(".home-bar .search")).toBeVisible();
    await page.locator(".doc-row .open").first().tap();
    await page.waitForFunction(() => window.wtfDocs.controller);
    expect(await noSideways()).toBe(true);
    // Only the caret's line shows its lens, under the line.
    await expect(page.locator(".lens")).toHaveCount(0);
    await page.evaluate(() => window.wtfDocs.controller.select(window.wtfDocs.controller.getSource().indexOf("focus :=") + 3));
    await expect(page.locator(".lenses.below .lens")).toHaveCount(1);
    await page.locator(".lenses.below .lens").tap();
    await expect(page.locator(".view")).toContainText("running", { timeout: 10_000 });
    // Typing works and menus open as sheets.
    await page.evaluate(() => window.wtfDocs.controller.select(window.wtfDocs.controller.getSource().length));
    await page.keyboard.type("\nfrom a phone");
    await expect.poll(() => page.evaluate(() => window.wtfDocs.controller.getSource())).toMatch(/from a phone$/);
    await page.locator(".menubar > .menu > button", { hasText: "Insert" }).tap();
    await expect(page.locator(".dropdown")).toBeVisible();
    expect(await page.locator(".dropdown").evaluate(el => getComputedStyle(el).position)).toBe("fixed");
    await page.keyboard.press("Escape");
    // The outline opens as a drawer.
    await page.locator(".chip.outline-toggle").tap();
    await expect(page.locator(".sidebar")).toBeVisible();
    expect(errors).toEqual([]);
  });
});
