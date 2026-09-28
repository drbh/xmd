import { test, expect } from "@playwright/test";

// The hosted app without a network: the shell comes from the service worker,
// the library from the last visit, edits from IndexedDB, and everything
// merges back through the room when the connection returns.
test("a signed-in user keeps working offline and the room catches up afterwards", async ({ browser }) => {
  const user = `offline-${Date.now()}@example.com`;
  const context = await browser.newContext({ extraHTTPHeaders: { "x-dev-user": user } });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", e => errors.push(e.message));
  const source = () => page.evaluate(() => window.xmdDocs.controller?.getSource());

  // Online: create two documents, open only the first, and let the service worker install.
  await page.goto("/docs/?test");
  await page.waitForFunction(() => window.xmdDocs?.ready, null, { timeout: 60_000 });
  await page.waitForFunction(async () => !!(await navigator.serviceWorker.getRegistration())?.active, null, { timeout: 30_000 });
  await page.locator(".template", { hasText: "Trip budget" }).click();
  await page.waitForFunction(() => window.xmdDocs.controller && window.xmdDocs.live?.status === "connected");
  const opened = await page.evaluate(() => window.xmdDocs.active.id);
  // A second document this browser has never opened: created through the API, then seen in the list.
  const unopened = crypto.randomUUID();
  await page.evaluate(async id => { await fetch(`/api/documents/${id}`, { method: "PUT", headers: { "content-type": "application/json" }, body: JSON.stringify({ name: "Never opened", text: "# Never opened\n" }) }); }, unopened);
  await page.locator(".logo").click();
  await page.reload();
  await page.waitForFunction(() => window.xmdDocs?.ready, null, { timeout: 60_000 });
  await expect(page.locator(".doc-list")).toContainText("Never opened");

  // Offline: the app loads, the library is there, and the opened document is editable.
  await context.setOffline(true);
  await page.reload();
  await page.waitForFunction(() => window.xmdDocs?.ready, null, { timeout: 60_000 });
  await expect(page.locator(".home .notice")).toContainText("offline");
  await expect(page.locator(".doc-list")).toContainText("Trip budget");
  await page.evaluate(id => window.xmdDocs.open(id), opened);
  await page.waitForFunction(() => window.xmdDocs.controller);
  await expect(page.locator(".live")).toHaveText(/Offline/);
  await expect.poll(source, { timeout: 15_000 }).toMatch(/Trip budget/);
  await page.evaluate(() => window.xmdDocs.controller.select(window.xmdDocs.controller.getSource().length));
  await page.keyboard.type("\nWritten offline.");
  await expect.poll(source).toMatch(/Written offline\.$/);
  // A new document created offline is queued.
  await page.evaluate(() => window.xmdDocs.home());
  await page.locator(".template", { hasText: "Blank" }).click();
  await page.waitForFunction(() => window.xmdDocs.controller);
  await page.keyboard.type("Made offline");
  await page.waitForTimeout(600);
  const queued = await page.evaluate(() => window.xmdDocs.active.id);
  // The never-opened document is read-only until it has been opened online.
  await page.evaluate(id => window.xmdDocs.open(id), unopened);
  await page.waitForFunction(() => window.xmdDocs.controller);
  await expect(page.locator(".status")).toContainText("Open this document online once", { timeout: 15_000 });
  expect(await page.evaluate(() => window.xmdDocs.controller.element.isContentEditable)).toBe(false);

  // Back online: the offline edit reaches the room and D1, and the queued document is sent.
  await context.setOffline(false);
  await page.evaluate(id => window.xmdDocs.open(id), opened);
  await page.waitForFunction(() => window.xmdDocs.live?.status === "connected", null, { timeout: 30_000 });
  await expect.poll(async () => (await page.evaluate(async id => (await (await fetch(`/api/documents/${id}`)).json()), opened)).text, { timeout: 20_000 }).toMatch(/Written offline\./);
  await expect.poll(async () => (await page.evaluate(async id => (await fetch(`/api/documents/${id}`)).status, queued)), { timeout: 20_000 }).toBe(200);
  expect(errors).toEqual([]);
  await context.close();
});
