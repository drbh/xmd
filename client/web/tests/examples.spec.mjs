import { test, expect } from "@playwright/test";

test("an example opens from its link, edits without saving, and saves as a copy", async ({ page }) => {
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.goto("/docs/?test#/example/cross-note-values");
  await page.waitForFunction(() => window.xmdDocs?.ready, null, { timeout: 45_000 });
  await page.waitForFunction(() => window.xmdDocs.controller && window.xmdDocs.active?.example);
  const view = page.locator(".view");
  // Its import loads beside it, so the other note's values resolve.
  await expect(view).toContainText("$3,040");
  await expect(page.locator(".status")).toContainText("Example · not saved");
  await expect(page).toHaveURL(/#\/example\/cross-note-values$/);
  // Edits recalculate but are never kept, and the example stays off the document list.
  await page.evaluate(() => window.xmdDocs.controller.setSource(window.xmdDocs.active.text.replace("this week", "this month")));
  await expect(view).toContainText("this month");
  await page.locator(".logo").click();
  await expect(page.locator(".doc-list")).not.toContainText("Values from other notes");
  // Saving a copy makes an ordinary document.
  await page.goto("/docs/?test#/example/checklists");
  await page.waitForFunction(() => window.xmdDocs?.ready && window.xmdDocs.active?.example === "checklists", null, { timeout: 45_000 });
  await page.locator("button", { hasText: "Save a copy" }).click();
  await page.waitForFunction(() => window.xmdDocs.active && !window.xmdDocs.active.example);
  await expect(page.locator(".status")).toContainText("Saved in this browser");
  await page.locator(".logo").click();
  await expect(page.locator(".doc-list")).toContainText("Copy of Checklists");
  // An unknown name says so on the home screen.
  await page.goto("/docs/?test#/example/nope");
  await page.waitForFunction(() => window.xmdDocs?.ready, null, { timeout: 45_000 });
  await expect(page.locator("body")).toContainText('There is no example called "nope"');
  expect(errors).toEqual([]);
});
