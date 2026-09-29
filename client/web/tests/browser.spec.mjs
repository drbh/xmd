import { test, expect, devices } from "@playwright/test";

test("the document view edits, toggles checkboxes, persists, and shares a workspace", async ({ page }) => {
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.goto("/docs/?test");
  await page.waitForFunction(() => window.xmdDocs?.ready, null, { timeout: 45_000 });
  // The app opens on a home screen listing templates and documents.
  await expect(page.locator(".template")).toHaveCount(5);
  await page.locator(".doc-row .open", { hasText: "Trip budget" }).click();
  await page.waitForFunction(() => window.xmdDocs.controller);
  const view = page.locator(".view");
  await expect(view).toContainText("= $556");
  await expect(view.locator(".line.h1").first()).toHaveText(/Trip budget/);
  await expect(page.locator("input.title-input")).toHaveValue("Trip budget");
  // Clicking a checkbox flips it in the text and the engine repaints.
  const box = view.locator(".t-xmdCheckbox").first();
  await box.click();
  await expect(view).toContainText("[x] Book the hotel");
  await expect(page.locator(".status")).toContainText("Saved in this browser");
  // Code lenses are chips at the end of their lines rather than a control bar under the page.
  await expect(page.locator(".xmd-controls")).toHaveCount(0);
  const lens = page.locator(".lens", { hasText: "○ reopen" }).first();
  await expect(lens).toBeVisible();
  await lens.click();
  await expect(view).toContainText("[ ] Book the hotel");
  await expect(page.locator(".lens", { hasText: "○ reopen" })).toHaveCount(1);
  // The bundled font ships its checkbox ligature: the marker and box shape as one run.
  await expect(view).toHaveCSS("font-family", /Ioskeley Mono/);
  expect(await page.evaluate(async () => { await document.fonts.ready; return document.fonts.check('14px "Ioskeley Mono"'); })).toBe(true);
  // The sidebar is hidden by default.
  await expect(page.locator(".sidebar")).toBeHidden();
  // A second document must import the first explicitly.
  await page.evaluate(() => window.xmdDocs.newDocument());
  await page.waitForFunction(() => window.xmdDocs.controller && window.xmdDocs.active.name === "Untitled document");
  await page.evaluate(() => window.xmdDocs.controller.setSource("# Second\n\nStill [remaining] to spend.\n"));
  await expect(view).toContainText("Still [remaining]");
  await expect(view).not.toContainText("$556");
  // Documents are addressed by file name, exactly as on disk.
  await page.evaluate(() => window.xmdDocs.controller.setSource(`# Second\n\nStill [source.remaining] to spend.\nsource := import("./Trip budget.x.md")\n`));
  await expect(view).toContainText("$556");
  // The title follows the first heading, and saves are debounced briefly.
  await expect(page.locator("input.title-input")).toHaveValue("Second");
  await page.waitForTimeout(600);
  // Documents survive a reload; the URL reopens the same document.
  await page.reload();
  await page.waitForFunction(() => window.xmdDocs?.ready, null, { timeout: 45_000 });
  await page.waitForFunction(() => window.xmdDocs.controller);
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
  await page.waitForFunction(() => window.xmdDocs?.ready, null, { timeout: 45_000 });
  await page.locator(".template", { hasText: "Blank" }).click();
  await page.waitForFunction(() => window.xmdDocs.controller);
  // The editor remounts when a rename changes the document URI, so the controller
  // can briefly be null; undefined keeps a poll retrying instead of throwing.
  const source = () => page.evaluate(() => window.xmdDocs.controller?.getSource());
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
  await page.evaluate(() => window.xmdDocs.controller.select(0));
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
  // Shortcuts are ignored while focus is inside a dialog, so wait for it to close.
  await expect(page.locator(".dialog")).toHaveCount(0);
  // The console runs read-only queries against the resolved document.
  await page.keyboard.press("ControlOrMeta+Alt+j");
  const query = page.locator(".console input");
  await expect(page.locator(".console")).toHaveAttribute("data-fields", "ready");
  await query.fill("lease * 2");
  await page.keyboard.press("Enter");
  await expect(page.locator(".console .entry").last()).toContainText("$1,800");
  // Typeahead knows collections, learned record fields, and functions.
  await query.fill("");
  await query.pressSequentially("val");
  await expect(page.locator(".typeahead li .label")).toHaveText(["values"]);
  await page.keyboard.press("Tab");
  await expect(query).toHaveValue("values");
  await query.fill("");
  await query.pressSequentially("map(values, fn(v) => v.");
  await expect(page.locator(".typeahead li .label", { hasText: /^name$/ })).toBeVisible();
  await page.keyboard.press("Escape");
  await query.fill("map(values, fn(v) => {name: v.name, type: v.type})");
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
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem("xmd.docs.prefs.v1")).theme)).toBe("dark");
  await page.locator(".menubar > .menu > button", { hasText: "View" }).click();
  await page.locator(".dropdown [role=menuitemcheckbox]", { hasText: "Light theme" }).click();
  await expect(page.locator("html")).toHaveClass(/xmd-light/);
  expect(errors).toEqual([]);
});

test.describe("on a phone", () => {
  const { defaultBrowserType, ...phone } = devices["iPhone 14"];
  test.use(phone);
  test("the app fits the screen, edits, and shows actions for the caret's line", async ({ page }) => {
    const errors = [];
    page.on("pageerror", error => errors.push(error.message));
    await page.goto("/docs/?test");
    await page.waitForFunction(() => window.xmdDocs?.ready, null, { timeout: 45_000 });
    const noSideways = () => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth && [...document.querySelectorAll(".home, .app, .canvas")].every(el => el.scrollWidth <= el.clientWidth + 1));
    expect(await noSideways()).toBe(true);
    await expect(page.locator(".home-bar .search")).toBeVisible();
    await page.locator(".doc-row .open").first().tap();
    await page.waitForFunction(() => window.xmdDocs.controller);
    expect(await noSideways()).toBe(true);
    // Only the caret's line shows its lens, under the line.
    await expect(page.locator(".lens")).toHaveCount(0);
    await page.evaluate(() => window.xmdDocs.controller.select(window.xmdDocs.controller.getSource().indexOf("focus :=") + 3));
    await expect(page.locator(".lenses.below .lens")).toHaveCount(1);
    await page.locator(".lenses.below .lens").tap();
    await expect(page.locator(".view")).toContainText("running", { timeout: 10_000 });
    // Typing works and menus open as sheets.
    await page.evaluate(() => window.xmdDocs.controller.select(window.xmdDocs.controller.getSource().length));
    await page.keyboard.type("\nfrom a phone");
    await expect.poll(() => page.evaluate(() => window.xmdDocs.controller?.getSource())).toMatch(/from a phone$/);
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
