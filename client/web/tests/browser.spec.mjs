import { test, expect, devices } from "@playwright/test";

test("the reference lists the language and runs its snippets", async ({ page }) => {
  const errors = [], logs = [];
  page.on("pageerror", error => errors.push(error.message));
  page.on("console", message => { if (message.text().startsWith("reference:")) logs.push(message.text()); });
  const opened = Date.now();
  await page.goto("/docs/?test#/reference");
  await page.waitForFunction(() => window.wtfDocs?.ready, null, { timeout: 45_000 });
  // The page opens on its name and the syntax table, one highlighted row each.
  await expect(page.locator(".prose .title h1")).toHaveText("Reference");
  await expect(page.locator("#syntax table.syntaxes tbody tr")).toHaveCount(7);
  await expect(page.locator("#syntax table.syntaxes tbody tr").first()).toContainText("a value with a name");
  await expect(page.locator("#syntax .syntaxes .snippet.wtf .t-wtfMoney").first()).toBeVisible({ timeout: 20_000 });
  await expect(page.locator("#syntax .syntaxes .snippet.wtf:not(.pending)")).toHaveCount(7);
  const firstPaint = Date.now() - opened;
  // The reference is generated: the live request when the engine answers it,
  // the checked-in fixture otherwise. Either way the sections are the same,
  // in the reader's order, with module authoring last.
  const ids = await page.locator("section.ref-section").evaluateAll(list => list.map(s => s.id));
  expect(ids).toEqual(["functions", "attributes", "types", "query", "library", "authoring"]);
  // A signature reads like the language: the name as a function, its
  // parameters as variables, the types muted.
  const sum = page.locator('#functions .ref-row[data-name="sum"]');
  await expect(sum.locator(".sig .t-function")).toHaveText("sum");
  await expect(sum.locator(".sig .t-variable").first()).toHaveText("items");
  await expect(sum.locator(".sig .sig-type").first()).toHaveText("List or Table");
  await expect(sum.locator(".sig .tier")).toHaveCount(0);
  await expect(page.locator('#functions .ref-row[data-name="map"] .sig .tier')).toHaveText("toolkit");
  // Every row shows its snippet, drawn by the engine with its result, before
  // any click; the whole page renders in batches, and the canvas says when.
  await expect(sum.locator(".run .snippet.wtf")).toContainText("= $7.50", { timeout: 20_000 });
  await expect(sum.locator(".run .snippet.wtf .inlay").last()).toHaveText(/= \$7\.50/);
  await page.waitForFunction(() => document.querySelector("main.canvas").dataset.snippets, null, { timeout: 60_000 });
  const rendered = await page.locator("main.canvas").getAttribute("data-snippets");
  console.log(`reference: first paint in ${firstPaint} ms; ${rendered}; ${logs.join("; ")}`);
  expect(rendered).toMatch(/^\d+ snippets in \d+ ms$/);
  await expect(page.locator(".snippet.pending")).toHaveCount(0);
  // Module authoring is one section at the end: its primitives carry the
  // module badge and are not among the functions a note calls.
  await expect(page.locator('#functions .ref-row[data-name="eval"]')).toHaveCount(0);
  await expect(page.locator('#authoring .ref-row[data-name="eval"]')).toBeVisible();
  await expect(page.locator('#authoring .ref-row[data-name="eval"] .sig .tier')).toHaveText("module");
  await expect(page.locator('#authoring-bundled .ref-row[data-name="github"]')).toBeVisible();
  await expect(page.locator('#authoring-hooks .ref-row[data-name="collect"]')).toBeVisible();
  // Types: the scalars a note writes as rows, the engine's objects as a list.
  await expect(page.locator('#types-list .ref-row[data-name="Money"]')).toBeVisible();
  for (const name of ["Choice", "Namespace", "Function"]) {
    await expect(page.locator(`#types-list .ref-row[data-name="${name}"]`)).toHaveCount(0);
    await expect(page.locator(`#types-objects .ref-row[data-name="${name}"]`)).toHaveCount(1);
  }
  await expect(page.locator('#types-objects .ref-row[data-name="Countdown"] .snippet.wtf')).toBeVisible();
  // The library lists only what a note can import, and a private member is not a row.
  await expect(page.locator('#library .ref-row[data-name="units.convert"]')).toBeVisible();
  await expect(page.locator('#library .ref-row[data-name="format.clamp"]')).toHaveCount(0);
  await expect(page.locator("#library-github")).toHaveCount(0);
  // "edit" swaps the static snippet for one live block, seeded with the entry's snippet.
  await sum.locator(".edit").click();
  await expect(sum.locator(".run .snippet")).toHaveCount(0);
  const widget = page.locator("#functions .try-widget");
  await expect(widget.locator(".view")).toContainText("= $7.50", { timeout: 20_000 });
  // Only one try is open per section.
  await page.locator('.ref-row[data-name="filter"] .edit').click();
  await expect(page.locator("#functions .try-widget")).toHaveCount(1);
  await expect(sum.locator(".run .snippet.wtf")).toBeVisible();
  await expect(widget.locator(".view")).toContainText("[2, 3]");
  // Reset puts the snippet back after an edit.
  await page.evaluate(() => window.wtfDocs.workspace.setDocument("file:///workspace/reference/try/functions/filter.wtf", "prices := [$3, $12, $4.50]\nbig := sum(prices)\n"));
  await expect(widget.locator(".view")).toContainText("= $19.50");
  await widget.locator(".try-reset").click();
  await expect(widget.locator(".view")).toContainText("[2, 3]");
  await expect(widget.locator(".view")).not.toContainText("= $19.50");
  // A collection's snippet is a query, run against the sample note.
  await page.locator('#query .ref-row[data-name="tasks"] .edit').click();
  await expect(page.locator("#query .try-rows")).toContainText('"title": "Pack"', { timeout: 20_000 });
  await expect(page.locator("#query .try-rows")).not.toContainText("Choose the dates");
  await page.locator('#query .ref-row[data-name="values"] .edit').click();
  await expect(page.locator("#query .try-rows")).toContainText('"name": "total"');
  // The contents scroll to a group, and an entry can be linked to directly.
  await page.locator(".toc .row", { hasText: "Dates" }).click();
  await expect(page).toHaveURL(/#\/reference\/functions-dates$/);
  await page.goto("/docs/?test#/reference/sum");
  await page.waitForFunction(() => window.wtfDocs?.ready, null, { timeout: 45_000 });
  await expect(page.locator('.ref-row[data-name="sum"]')).toBeInViewport({ timeout: 20_000 });
  // The way back to the documents, and the way in from the home screen.
  await page.locator(".logo").click();
  await expect(page.locator(".template").first()).toBeVisible();
  await page.locator(".home-bar .button", { hasText: "Reference" }).click();
  await expect(page).toHaveURL(/#\/reference$/);
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
  await page.evaluate(() => window.wtfDocs.newDocument());
  await page.waitForFunction(() => window.wtfDocs.controller && window.wtfDocs.active.name === "Untitled document");
  await page.evaluate(() => window.wtfDocs.controller.setSource("# Second\n\nStill [remaining] to spend.\n"));
  await expect(view).toContainText("Still [remaining]");
  await expect(view).not.toContainText("$556");
  // Documents are addressed by file name, exactly as on disk.
  await page.evaluate(() => window.wtfDocs.controller.setSource(`# Second\n\nStill [source.remaining] to spend.\nsource := import("./Trip budget.wtf")\n`));
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
