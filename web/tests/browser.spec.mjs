import { test, expect } from "@playwright/test";
import { readFileSync } from "node:fs";

async function ready(page) {
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.goto("/?test");
  await expect(page.locator("#engine")).toHaveText("Rust / WebAssembly · local", { timeout: 45_000 });
  await page.waitForFunction(() => window.wtfTest?.ready);
  expect(errors).toEqual([]);
  return errors;
}
async function replace(page, before, after) {
  await page.evaluate(({ before, after }) => {
    const { editor } = window.wtfTest;
    const match = editor.getModel().findMatches(before, false, false, true, null, false)[0];
    editor.executeEdits("test", [{ range: match.range, text: after }]);
  }, { before, after });
}

function renderedToken(page, text) {
  // Monaco uses NBSPs in rendered tokens, including the space inside [ ].
  const pattern = text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&").replaceAll(" ", "\\s");
  return page.locator(".view-lines span").filter({ hasText: new RegExp(`^${pattern}$`) }).last();
}

test("Rust semantic tokens render distinct types, declarations, columns and completed tasks", async ({ page }) => {
  const errors = await ready(page);
  await page.evaluate(() => {
    window.wtfTest.editor.setValue("# Highlighting\n[$3.30]:cash\n[clock] := countdown(25m)\n[rows] := table\n|qty|price|\n|---|---|\n|2|$4.30|\n[result] := sum(rows, qty * price)\n- [x] Packed\n- [ ] Book @due(tomorrow)\n");
  });
  // Inspect rendered DOM styles, not just the worker's token arrays.
  const token = text => renderedToken(page, text);
  await expect(token("countdown")).toHaveCSS("color", "rgb(198, 160, 246)");
  await expect(token("cash")).toHaveCSS("color", "rgb(168, 199, 250)");
  await expect(token("cash")).toHaveCSS("font-weight", "700");
  await expect(token("$3.30")).toHaveCSS("color", "rgb(180, 217, 138)");
  await expect(token("25m")).toHaveCSS("color", "rgb(244, 186, 122)");
  await expect(token("qty")).toHaveCSS("color", "rgb(131, 214, 207)");
  await expect(token("@due")).toHaveCSS("color", "rgb(222, 162, 184)");
  await expect(token("tomorrow")).toHaveCSS("color", "rgb(242, 179, 218)");
  await expect(token("tomorrow")).toHaveCSS("font-weight", "700");
  await expect(token("[ ]")).toHaveCSS("color", "rgb(255, 213, 128)");
  await expect(token("[ ]")).toHaveCSS("font-weight", "700");
  await expect(token("[x]")).toHaveCSS("color", "rgb(145, 230, 172)");
  await expect(token("[x]")).toHaveCSS("font-weight", "700");
  await expect(token("[x]")).toHaveCSS("text-decoration-line", "none");
  await expect(token("[")).toHaveCSS("color", "rgb(133, 147, 139)");
  const packed = page.locator(".view-lines span").filter({ hasText: "Packed" }).last();
  await expect(packed).toHaveCSS("text-decoration-line", "line-through");
  expect(errors).toEqual([]);
});

test("plain prose dates, times and values have shared semantic colors and checkbox edits update", async ({ page }) => {
  const errors = await ready(page);
  await page.evaluate(() => window.wtfTest.editor.setValue("# 9:38 AM\nInterview 09/17/2026 10:00 AM – 10:45 AM.\n- [ ] Start at 7AM; allow 30m and $120.\nProgress 80% with 2 copies tomorrow.\n"));
  const token = text => renderedToken(page, text);
  for (const text of ["9:38 AM", "7AM", "10:00 AM", "10:45 AM"]) {
    const label = token(text);
    await expect(label).toHaveCSS("color", "rgb(145, 220, 232)");
    await expect(label).toHaveCSS("font-weight", "700");
  }
  await expect(token("09/17/2026")).toHaveCSS("color", "rgb(242, 179, 218)");
  await expect(token("$120")).toHaveCSS("color", "rgb(180, 217, 138)");
  await expect(token("30m")).toHaveCSS("color", "rgb(244, 186, 122)");
  await expect(token("80%")).toHaveCSS("color", "rgb(234, 192, 128)");
  await expect(token("2")).toHaveCSS("color", "rgb(218, 189, 144)");
  await replace(page, "[ ]", "[x]");
  await expect(token("[x]")).toHaveCSS("color", "rgb(145, 230, 172)");
  await expect(token("[x]")).toHaveCSS("text-decoration-line", "none");
  await page.evaluate(() => window.wtfTest.editor.trigger("test", "undo", null));
  await expect(token("[ ]")).toHaveCSS("color", "rgb(255, 213, 128)");
  expect(errors).toEqual([]);
});

test("raw file links are styled and clicking opens an imported note via the shared LSP target", async ({ page }) => {
  const errors = await ready(page);
  await page.evaluate(() => window.wtfTest.editor.setValue("# Raw links\nOpen ./today.wtf and https://example.com/docs.\n"));
  const local = page.locator(".view-lines .detected-link").filter({ hasText: "./today.wtf" }).first();
  await expect(local).toHaveCSS("color", "rgb(144, 190, 216)");
  const links = await page.evaluate(() => window.wtfTest.query(window.wtfTest.editor.getModel(), "documentLinks"));
  expect(links.map(l => l.target)).toEqual(["file:///workspace/today.wtf", "https://example.com/docs"]);
  // CodeLens adds a line above the links; wait for its layout before clicking.
  await expect(page.locator(".codelens-decoration").getByText("Open resource", { exact: true }).first()).toBeVisible();
  await local.click({ modifiers: ["ControlOrMeta"] });
  await expect(page.locator("#filename")).toHaveText("today.wtf");
  expect(errors).toEqual([]);
});

test("real Wasm worker renders reactive inlays, saves locally, and downloads plain source", async ({ page }) => {
  const errors = await ready(page);
  await expect(page.locator(".monaco-editor").first()).toHaveCSS("background-color", "rgb(23, 27, 25)");
  await expect(page.locator("html")).toHaveCSS("color-scheme", "dark");
  await expect(page.locator(".view-lines")).toContainText("$556");
  await replace(page, "$2,444", "$1,410");
  await expect(page.locator(".view-lines")).toContainText("$1,590");
  const hover = await page.evaluate(() => window.wtfTest.query(window.wtfTest.editor.getModel(), "hover", { position: { line: 8, character: 10 } }));
  expect(hover.contents.value).toContain("$3,000 - $1,410");
  await expect(page.locator("#save-status")).toHaveText("Saved in this browser");
  await page.reload();
  await page.waitForFunction(() => window.wtfTest?.ready);
  await expect(page.locator(".view-lines")).toContainText("$1,590");
  const downloadPromise = page.waitForEvent("download");
  await page.getByRole("button", { name: "Download", exact: true }).click();
  const download = await downloadPromise;
  const stream = await download.createReadStream();
  const chunks = []; for await (const chunk of stream) chunks.push(chunk);
  const source = Buffer.concat(chunks).toString("utf8");
  expect(source).toContain("We've spent [$1,410]:spent.");
  expect(source).not.toContain("$1,590");
  expect(errors).toEqual([]);
});

test("clickable timer and task controls produce undoable source edits", async ({ page }) => {
  await ready(page);
  await page.getByRole("button", { name: "today.wtf", exact: true }).click();
  await expect(page.locator(".codelens-decoration").filter({ hasText: "Start timer 'focus'" }).first()).toBeVisible();
  await page.locator(".codelens-decoration").getByText("Start timer 'focus'", { exact: true }).first().click();
  await expect.poll(() => page.evaluate(() => window.wtfTest.editor.getValue())).toContain("countdown(25m, 0s,");
  await expect(page.locator(".view-lines")).toContainText("remaining · ▸ running");
  await page.locator(".codelens-decoration").getByText("Pause timer 'focus'", { exact: true }).first().click();
  await expect(page.locator(".view-lines")).toContainText("remaining · ‖ paused");
  await page.evaluate(() => window.wtfTest.editor.trigger("test", "undo", null));
  await expect(page.locator(".view-lines")).toContainText("remaining · ▸ running");
  await page.locator(".codelens-decoration").getByText("Complete task", { exact: true }).first().click();
  await expect.poll(() => page.evaluate(() => window.wtfTest.editor.getValue())).toContain("- [x] Investigate");
});

test("completion, signature, diagnostics and cross-note refactorings reuse Rust", async ({ page }) => {
  await ready(page);
  await page.getByRole("button", { name: "today.wtf", exact: true }).click();
  const results = await page.evaluate(async () => {
    const { editor, query } = window.wtfTest;
    const model = editor.getModel();
    const line = model.getLineContent(6);
    return {
      completion: await query(model, "completion", { position: { line: 5, character: line.indexOf("@timer(") + 7 } }),
      signature: await query(model, "signature", { position: { line: 2, character: model.getLineContent(3).indexOf("25m") } }),
      definition: await query(model, "definition", { position: { line: 13, character: 24 } }),
    };
  });
  expect(results.completion.map(c => c.label).sort()).toEqual(["debugging", "focus"]);
  expect(results.signature.signatures[0].label).toContain("countdown(");
  expect(results.definition.uri).toBe("file:///workspace/trip.wtf");
  await page.getByRole("button", { name: "trip.wtf", exact: true }).click();
  await replace(page, "budget - spent", "budget - 5m");
  await expect(page.locator("#problems")).toContainText("Money - Duration");
  await page.evaluate(() => window.wtfTest.editor.trigger("test", "undo", null));
  await expect(page.locator("#problems")).toBeHidden();
  const rename = await page.evaluate(async () => {
    const { editor, query, ui } = window.wtfTest;
    const edit = await query(editor.getModel(), "rename", { position: { line: 6, character: 4 }, newName: "trip_cash" });
    ui.applyEdit(edit); return edit;
  });
  expect(rename.documentChanges).toHaveLength(2);
  await expect(page.locator(".view-lines")).toContainText("[trip_cash]");
  await page.getByRole("button", { name: "today.wtf", exact: true }).click();
  await expect(page.locator(".view-lines")).toContainText("[trip.trip_cash]");
});

test("imports are local, duplicate filenames never overwrite, no backend connections", async ({ page }) => {
  const connections = [];
  page.on("websocket", ws => connections.push(ws.url()));
  await ready(page);
  await page.locator("#file-input").setInputFiles({ name: "trip.wtf", mimeType: "text/plain", buffer: Buffer.from("# Imported\n[7]:days\nWe have [days] days.\n") });
  await expect(page.locator("#filename")).toHaveText("trip-2.wtf");
  await expect(page.locator(".view-lines")).toContainText("7 days.");
  await page.getByRole("button", { name: "trip.wtf", exact: true }).click();
  await expect(page.locator(".view-lines")).toContainText("$556");
  expect(connections).toEqual([]);
});

test("Monaco renders contextual suggestions and signature help", async ({ page }) => {
  const errors = await ready(page);
  await page.evaluate(() => {
    const { editor } = window.wtfTest;
    editor.getModel().setValue("[focus] := countdown(25m)\n[watch] := stopwatch()\n- [ ] Work @timer()\n");
    editor.setPosition({ lineNumber: 3, column: editor.getModel().getLineContent(3).indexOf("@timer(") + 8 });
    editor.trigger("test", "editor.action.triggerSuggest", {});
  });
  await expect(page.locator(".suggest-widget.visible")).toContainText("focus");
  await expect(page.locator(".suggest-widget.visible")).toContainText("watch");
  await page.keyboard.press("Escape");
  await page.evaluate(() => {
    const { editor } = window.wtfTest;
    editor.setPosition({ lineNumber: 1, column: editor.getModel().getLineContent(1).indexOf("25m") + 1 });
    editor.trigger("test", "editor.action.triggerParameterHints", {});
  });
  await expect(page.locator(".parameter-hints-widget.visible")).toContainText("duration: Duration");
  expect(errors).toEqual([]);
});

test("countdown expiry refreshes inlays and controls without source edits", async ({ page }) => {
  await ready(page);
  await page.evaluate(() => window.wtfTest.editor.getModel().setValue("[tea] := countdown(2s)\nTime left: [tea.remaining].\n"));
  await page.locator(".codelens-decoration").getByText("Start timer 'tea'", { exact: true }).first().click();
  await expect(page.locator(".view-lines")).toContainText("remaining · ▸ running");
  const running = await page.evaluate(() => window.wtfTest.editor.getValue());
  await expect(page.locator(".view-lines")).toContainText("00:00 remaining · ✓ done", { timeout: 6000 });
  await expect(page.locator(".codelens-decoration").getByText("Pause timer 'tea'", { exact: true })).toHaveCount(0);
  await expect(page.locator(".codelens-decoration").getByText("Reset timer 'tea'", { exact: true }).first()).toBeVisible();
  expect(await page.evaluate(() => window.wtfTest.editor.getValue())).toBe(running);
});

test("virtual Unicode filenames round-trip and invalid saved data is preserved", async ({ page }) => {
  await ready(page);
  await page.locator("#file-input").setInputFiles({ name: "🦀 values.wtf", mimeType: "text/plain", buffer: Buffer.from("[42]:answer\nAnswer [answer].\n") });
  await expect(page.locator(".view-lines")).toContainText("42.");
  const definition = await page.evaluate(() => window.wtfTest.query(window.wtfTest.editor.getModel(), "definition", { position: { line: 1, character: 10 } }));
  expect(decodeURI(definition.uri)).toBe("file:///workspace/🦀 values.wtf");
  await page.addInitScript(() => localStorage.setItem("wtf.browser.workspace.v1", "broken-json"));
  await page.reload();
  await page.waitForFunction(() => window.wtfTest?.ready);
  await expect(page.locator("#notice")).toContainText("Original storage has been preserved");
  expect(await page.evaluate(() => localStorage.getItem("wtf.browser.workspace.v1"))).toBe("broken-json");
});

test("Monaco selection refactor applies the shared versioned workspace edit", async ({ page }) => {
  await ready(page);
  await page.evaluate(() => {
    const { editor, monaco } = window.wtfTest;
    const line = editor.getModel().getLineContent(7);
    const start = line.indexOf("budget - spent") + 1;
    editor.setSelection(new monaco.Range(7, start, 7, start + "budget - spent".length));
    editor.trigger("test", "editor.action.codeAction", { kind: "refactor.extract", apply: "first" });
  });
  await expect.poll(() => page.evaluate(() => window.wtfTest.editor.getValue())).toContain("[calculation] := budget - spent\n[remaining] := calculation");
  await expect(page.locator(".view-lines")).toContainText("$556");
  await page.evaluate(() => window.wtfTest.editor.trigger("test", "undo", null));
  await expect.poll(() => page.evaluate(() => window.wtfTest.editor.getValue())).not.toContain("[calculation]");
});

test("another tab cannot silently overwrite this tab's unsaved changes", async ({ page, context }) => {
  await ready(page);
  const other = await context.newPage();
  await ready(other);
  await replace(other, "$2,444", "$2,000");
  await expect(other.locator("#save-status")).toHaveText("Saved in this browser");
  await expect(page.locator("#notice")).toContainText("Another tab changed the saved workspace");
  await replace(page, "$2,444", "$1,111");
  await expect(page.locator(".view-lines")).toContainText("$1,889");
  expect(await page.evaluate(() => localStorage.getItem("wtf.browser.workspace.v1"))).toContain("$2,000");
  await other.close();
});

test("LSP outline displays values, navigates exact names, and filters collapsed sections", async ({ page }) => {
  const errors = await ready(page);
  const outline = page.getByRole("navigation", { name: "Document outline" });
  await expect(outline).toContainText("Money · $556");
  await outline.getByRole("button", { name: "remaining", exact: true }).click();
  expect(await page.evaluate(() => window.wtfTest.editor.getModel().getValueInRange(window.wtfTest.editor.getSelection()))).toBe("remaining");
  await expect(outline.getByRole("button", { name: "remaining", exact: true })).toHaveAttribute("aria-current", "location");
  await outline.getByRole("button", { name: "Collapse Trip" }).click();
  await expect(outline.getByRole("button", { name: "remaining", exact: true })).toHaveCount(0);
  await page.getByRole("searchbox", { name: "Filter document symbols" }).fill("remaining");
  await expect(outline.getByRole("button", { name: "Trip", exact: true })).toBeVisible();
  await expect(outline.getByRole("button", { name: "remaining", exact: true })).toBeVisible();
  await expect(outline.getByRole("button", { name: "budget", exact: true })).toHaveCount(0);
  await page.getByRole("searchbox", { name: "Filter document symbols" }).fill("nothing-matches");
  await expect(page.locator("#outline-empty")).toHaveText("No matching symbols.");
  await page.getByRole("searchbox", { name: "Filter document symbols" }).fill("");
  await expect(outline.getByRole("button", { name: "Expand Trip" })).toBeVisible();
  await outline.getByRole("button", { name: "Expand Trip" }).click();
  await replace(page, "$2,444", "$1,410");
  await expect(outline).toContainText("Money · $1,590");
  expect(errors).toEqual([]);
});

test("document symbols feed Monaco's actual Go to Symbol picker", async ({ page }) => {
  await ready(page);
  await page.evaluate(() => window.wtfTest.editor.trigger("test", "editor.action.quickOutline", {}));
  const picker = page.locator(".quick-input-widget");
  await expect(picker).toBeVisible();
  await picker.locator("input").fill("@remaining");
  await expect(picker.locator(".monaco-list-row").filter({ hasText: "remaining" })).toBeVisible();
  await picker.locator("input").press("Enter");
  await expect(picker).toBeHidden();
  expect(await page.evaluate(() => window.wtfTest.editor.getPosition().lineNumber)).toBe(7);
});

test("outline switches notes, preserves hierarchy, and follows task and source edits", async ({ page }) => {
  await ready(page);
  const outline = page.getByRole("navigation", { name: "Document outline" });
  await page.getByRole("button", { name: "today.wtf", exact: true }).click();
  await expect(outline.getByRole("button", { name: "Today", exact: true })).toBeVisible();
  await expect(outline.getByRole("button", { name: "budget", exact: true })).toHaveCount(0);
  await page.evaluate(() => window.wtfTest.editor.getModel().setValue("# Trip\n## Packing\n- [ ] Pack :pack\n  - [x] Passport\n  - [ ] Tickets\n## Money\n[42]:cash\n"));
  await expect(outline.getByRole("button", { name: "Pack", exact: true })).toContainText("Incomplete");
  await outline.getByRole("button", { name: "Collapse Pack", exact: true }).click();
  await expect(outline.getByRole("button", { name: "Tickets", exact: true })).toHaveCount(0);
  await expect(outline.getByRole("button", { name: "cash", exact: true })).toBeVisible();
  await replace(page, "- [ ] Tickets", "- [x] Tickets");
  await expect(outline.getByRole("button", { name: "Pack", exact: true })).toContainText("Complete");
  await expect(outline.getByRole("button", { name: "Expand Pack", exact: true })).toBeVisible();
  await page.evaluate(() => window.wtfTest.editor.getModel().setValue("Just prose, no symbols.\n"));
  await expect(page.locator("#outline-empty")).toContainText("Add a heading");
  await expect(outline.getByRole("button")).toHaveCount(0);
});

test("tables run in Wasm with reactive totals, column completion, rename, and diagnostics", async ({ page }) => {
  const errors = await ready(page);
  const source = readFileSync(new URL("./fixtures/tables.wtf", import.meta.url));
  await page.locator("#file-input").setInputFiles({ name: "tables.wtf", mimeType: "text/plain", buffer: source });
  await expect(page.locator(".view-lines")).toContainText("$23.80");
  await expect(page.locator("#problems")).toBeHidden();
  const outline = page.getByRole("navigation", { name: "Document outline" });
  await expect(outline.getByRole("button", { name: "groceries", exact: true })).toContainText("Table · 2 rows · 3 columns");
  await expect(outline.getByRole("button", { name: "price", exact: true })).toContainText("Money");
  await page.evaluate(() => {
    const { editor } = window.wtfTest;
    editor.setPosition({ lineNumber: 9, column: editor.getModel().getLineContent(9).indexOf("price") + 3 });
    editor.trigger("test", "editor.action.triggerSuggest", {});
  });
  await expect(page.locator(".suggest-widget.visible")).toContainText("price");
  await page.keyboard.press("Escape");
  await outline.getByRole("button", { name: "price", exact: true }).click();
  expect(await page.evaluate(() => window.wtfTest.editor.getSelection().startLineNumber)).toBe(4);
  await page.evaluate(async () => {
    const { editor, query, ui } = window.wtfTest;
    const p = editor.getPosition();
    const edit = await query(editor.getModel(), "rename", { position: { line: p.lineNumber - 1, character: p.column - 1 }, newName: "unit_price" });
    ui.applyEdit(edit);
  });
  await expect.poll(() => page.evaluate(() => window.wtfTest.editor.getValue())).toContain("quantity * unit_price");
  await expect(outline.getByRole("button", { name: "unit_price", exact: true })).toBeVisible();
  await expect(page.locator(".view-lines")).toContainText("$23.80");
  await replace(page, "$4.30", "$5.30");
  await expect(page.locator(".view-lines")).toContainText("$27.80");
  await replace(page, "$5.30", "oops");
  await expect(page.locator("#problems")).toContainText("Column 'unit_price' expects Money, found Text");
  await page.evaluate(() => window.wtfTest.editor.trigger("test", "undo", null));
  await expect(page.locator("#problems")).toBeHidden();
  expect(errors).toEqual([]);
});

test("Monaco Format Document applies shared table formatting and remains undoable", async ({ page }) => {
  await ready(page);
  const source = "[fruit] := table\n|item|quantity|price|\n|---|---|---|\n|apple|2|$3.30|\n|pear|4|$4.30|\n[total] := sum(fruit, quantity * price)\n\n| ordinary | table |\n|---|---|\n|leave|alone|\n";
  await page.evaluate(source => window.wtfTest.editor.getModel().setValue(source), source);
  await expect(page.locator(".view-lines")).toContainText("$23.80");
  await page.evaluate(() => window.wtfTest.editor.trigger("test", "editor.action.formatDocument", {}));
  await expect.poll(() => page.evaluate(() => window.wtfTest.editor.getValue())).toContain("| item  | quantity | price |");
  expect(await page.evaluate(() => window.wtfTest.editor.getValue())).toContain("| ordinary | table |\n|---|---|\n|leave|alone|\n");
  await page.evaluate(() => window.wtfTest.editor.trigger("test", "undo", null));
  await expect.poll(() => page.evaluate(() => window.wtfTest.editor.getValue())).toBe(source);
});

test("Typing a closing pipe or Enter after a task formats on type through the shared engine", async ({ page }) => {
  await ready(page);
  const source = "[fruit] := table\n|item|qty|\n|---|---|\n|apple|2|\n|watermelon|10\n- [ ] first";
  await page.evaluate(source => window.wtfTest.editor.getModel().setValue(source), source);
  await page.evaluate(() => {
    const { editor } = window.wtfTest;
    editor.setPosition({ lineNumber: 5, column: 15 });
    editor.focus();
  });
  await page.keyboard.type("|");
  await expect.poll(() => page.evaluate(() => window.wtfTest.editor.getValue())).toContain("| item       | qty |\n| ---------- | --- |\n| apple      | 2   |\n| watermelon | 10  |\n");
  await page.evaluate(() => {
    const { editor } = window.wtfTest;
    editor.setPosition({ lineNumber: 6, column: 12 });
  });
  await page.keyboard.press("Enter");
  await expect.poll(() => page.evaluate(() => window.wtfTest.editor.getValue())).toMatch(/- \[ \] first\n- \[ \] $/);
  await page.keyboard.press("Enter");
  await expect.poll(() => page.evaluate(() => window.wtfTest.editor.getValue())).toMatch(/- \[ \] first\n$/);
});

test("plans solve inside the Wasm engine with inlays, hovers, and reactive edits", async ({ page }) => {
  await ready(page);
  const source = readFileSync(new URL("./fixtures/plans.wtf", import.meta.url));
  await page.locator("#file-input").setInputFiles({ name: "plans.wtf", mimeType: "text/plain", buffer: source });
  await expect(page.locator(".view-lines")).toContainText("= $94.75 · bagels 25.75 · doughnuts 14");
  await expect(page.locator(".view-lines")).toContainText("400 ≤ 400 · ● binding");
  await expect(page.locator("#problems")).toBeHidden();
  await replace(page, "[400]:flour_stock", "[300]:flour_stock");
  await expect(page.locator(".view-lines")).toContainText("= $69.75");
});

test("itinerary stops render one hue per kind with bold markers", async ({ page }) => {
  await ready(page);
  await page.evaluate(() => {
    window.wtfTest.editor.setValue("## Friday, November 20, 2026 · Oaxaca\n\n07:04 AM  > Depart JFK for MEX\n11:55 AM  < Arrive at MEX\n06:00 PM  @ Check in to Majagua\n");
  });
  const token = text => renderedToken(page, text);
  await expect(token("Depart JFK for MEX")).toHaveCSS("color", "rgb(255, 176, 112)");
  await expect(token(">")).toHaveCSS("font-weight", "700");
  await expect(token("Arrive at MEX")).toHaveCSS("color", "rgb(156, 232, 160)");
  await expect(token("Check in to Majagua")).toHaveCSS("color", "rgb(201, 164, 255)");
  await expect(token("November 20, 2026")).toHaveCSS("color", "rgb(255, 158, 207)");
  await expect(token("07:04 AM")).toHaveCSS("color", "rgb(145, 220, 232)");
});

test("the book runs every example as a live block on the shared engine", async ({ page }) => {
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.goto("/book/?test");
  await expect(page.locator("#engine")).toHaveText("Rust / WebAssembly · running in this page", { timeout: 45_000 });
  await page.waitForFunction(() => window.wtfBook?.ready);
  const first = page.locator('.wtf-block[data-file="01-values.wtf"]');
  await expect(first.locator(".view")).toContainText("$556");
  await expect(first.locator(".view .t-wtfMoney").first()).toHaveCSS("color", "rgb(180, 217, 138)");
  // Editing a block re-solves it on the engine.
  await first.evaluate(el => window.wtfBook.blocks.find(b => b === el).setText("[$10]:a\n[$4]:b\n[c] := a + b\n"));
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
  expect(errors).toEqual([]);
});

test("the document view edits, toggles checkboxes, persists, and shares a workspace", async ({ page }) => {
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.goto("/docs/?test");
  await page.waitForFunction(() => window.wtfDocs?.ready, null, { timeout: 45_000 });
  const view = page.locator(".view");
  await expect(view).toContainText("= $556");
  await expect(view.locator(".line.h1").first()).toHaveText(/Trip budget/);
  // Clicking a checkbox flips it in the text and the engine repaints.
  const box = view.locator(".t-wtfCheckbox").first();
  await box.click();
  await expect(view).toContainText("[x] Book the hotel");
  await expect(page.locator(".status")).toContainText("Saved in this browser");
  // A second document must import the first explicitly.
  await page.evaluate(() => window.wtfDocs.newDocument());
  await page.waitForFunction(() => window.wtfDocs.controller);
  await page.evaluate(() => window.wtfDocs.controller.setSource("# Second\n\nStill [remaining] to spend.\n"));
  await expect(view).toContainText("Still [remaining]");
  await expect(view).not.toContainText("$556");
  await page.evaluate(() => {
    const first = window.wtfDocs.documents.find(d => d.name === "Trip budget");
    return window.wtfDocs.controller.setSource(`# Second\n\nStill [source.remaining] to spend.\nsource := import("./${first.id}.wtf")\n`);
  });
  await expect(view).toContainText("$556");
  // The title follows the first heading, and saves are debounced briefly.
  await expect(page.locator(".files nav")).toContainText("Second");
  await page.waitForTimeout(600);
  // Documents survive a reload.
  await page.reload();
  await page.waitForFunction(() => window.wtfDocs?.ready, null, { timeout: 45_000 });
  await expect(page.locator(".files nav")).toContainText("Second");
  await expect(page.locator(".files nav")).toContainText("Trip budget");
  expect(errors).toEqual([]);
});
