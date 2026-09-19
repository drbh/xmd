import { test, expect } from "@playwright/test";

async function openAs(browser, email, { hash = "", before } = {}) {
  const context = await browser.newContext({ extraHTTPHeaders: { "x-dev-user": email } });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", e => errors.push(`${email}: ${e.message}`));
  page.on("dialog", d => d.accept(d.type() === "prompt" ? "Trips" : undefined));
  if (before) { await page.goto("/docs/"); await page.evaluate(before); }
  await page.goto(`/docs/?test${hash}`);
  await page.waitForFunction(() => window.wtfDocs?.ready, null, { timeout: 60_000 });
  return { page, context, errors };
}

test("documents saved before signing in can be moved to the account once, and the banner then stays gone", async ({ browser }) => {
  const user = `mover-${Date.now()}@example.com`;
  const local = await openAs(browser, user, { before: () => localStorage.setItem("wtf.docs.v1", JSON.stringify({ documents: [{ id: "local-one", name: "Kept locally", text: "# Kept locally\n", updated: Date.now() }] })) });
  await expect(local.page.locator(".notice.info")).toContainText("1 document saved in this browser");
  await local.page.locator(".notice.info .link").click();
  await expect(local.page.locator(".doc-list")).toContainText("Kept locally");
  await expect(local.page.locator(".notice.info")).toHaveCount(0);
  await local.page.reload();
  await local.page.waitForFunction(() => window.wtfDocs?.ready, null, { timeout: 60_000 });
  await expect(local.page.locator(".notice.info")).toHaveCount(0);
  await expect(local.page.locator(".doc-list")).toContainText("Kept locally");
  expect(await local.page.evaluate(() => localStorage.getItem("wtf.docs.v1"))).toBeNull();
  expect(local.errors).toEqual([]);
  await local.context.close();
});

test("template thumbnails are rendered by the engine", async ({ browser }) => {
  const alice = await openAs(browser, "alice@example.com");
  const thumb = alice.page.locator(".template", { hasText: "Trip budget" }).locator(".thumb .wtf");
  await expect(thumb).toBeVisible();
  await expect(thumb.locator(".t-wtfMoney").first()).toBeVisible();
  await expect(thumb.locator(".inlay").first()).toBeVisible();
  await alice.context.close();
});

test("folders group documents and sharing a folder shares its documents", async ({ browser }) => {
  const owner = `owner-${Date.now()}@example.com`, guest = `guest-${Date.now()}@example.com`;
  const alice = await openAs(browser, owner);
  await alice.page.locator(".section-head .link", { hasText: "New folder" }).click();
  await expect(alice.page.locator(".folder-card")).toHaveCount(1);
  await alice.page.locator(".folder-open").click();
  await expect(alice.page.locator(".crumbs")).toContainText("Trips");
  await alice.page.locator(".template", { hasText: "Meeting notes" }).click();
  await alice.page.waitForFunction(() => window.wtfDocs.controller && window.wtfDocs.live?.status === "connected");
  const id = await alice.page.evaluate(() => window.wtfDocs.active.id);
  expect(await alice.page.evaluate(() => window.wtfDocs.active.folder)).toBeTruthy();
  // Leaving lands back in the folder; the breadcrumb goes up to all folders.
  await alice.page.locator(".logo").click();
  await expect(alice.page.locator(".crumbs")).toContainText("Trips");
  await alice.page.locator(".crumbs .link").click();
  await expect(alice.page.locator(".folder-card .folder-meta")).toContainText("1 document");
  // Share the folder.
  await alice.page.locator(".folder-card .doc-menu .tool").click();
  await alice.page.locator(".dropdown [role=menuitem]", { hasText: "Share folder" }).click();
  await alice.page.locator(".share-add input").fill(guest);
  await alice.page.locator(".share-add button").click();
  await expect(alice.page.locator(".people")).toContainText(guest);
  await alice.page.keyboard.press("Escape");

  const bob = await openAs(browser, guest);
  await expect(bob.page.locator(".recent.shared .folder-card")).toContainText("Trips");
  await bob.page.locator(".folder-card.shared .folder-open").click();
  await bob.page.locator(".doc-row .open", { hasText: "Meeting notes" }).click();
  await bob.page.waitForFunction(() => window.wtfDocs.controller && window.wtfDocs.live?.status === "connected");
  expect(await bob.page.evaluate(() => window.wtfDocs.controller.element.isContentEditable)).toBe(true);

  // Moving the document out of the folder takes it away from folder members.
  await alice.page.locator(".folder-open").click();
  await alice.page.locator(".doc-row .doc-menu .tool").click();
  await alice.page.locator(".dropdown [role=menuitemradio]", { hasText: "No folder" }).click();
  await expect(alice.page.locator(".empty")).toContainText("This folder is empty");
  await bob.page.reload();
  await bob.page.waitForFunction(() => window.wtfDocs?.ready, null, { timeout: 60_000 });
  expect(await bob.page.evaluate(id => window.wtfDocs.documents.some(d => d.id === id), id)).toBe(false);

  // Deleting the folder keeps the documents.
  await alice.page.locator(".crumbs .link").click();
  await alice.page.locator(".folder-card .doc-menu .tool").click();
  await alice.page.locator(".dropdown [role=menuitem]", { hasText: "Delete folder" }).click();
  await expect(alice.page.locator(".folder-card")).toHaveCount(0);
  await expect(alice.page.locator(".doc-list")).toContainText("Meeting notes");
  expect([...alice.errors, ...bob.errors]).toEqual([]);
  await alice.context.close(); await bob.context.close();
});

test("a document's file follows its heading until renamed; viewers cannot edit; the trash restores", async ({ browser }) => {
  const alice = await openAs(browser, `polish-${Date.now()}@example.com`);
  await alice.page.locator(".template", { hasText: "Blank" }).click();
  await alice.page.waitForFunction(() => window.wtfDocs.controller && window.wtfDocs.live?.status === "connected");
  await alice.page.evaluate(() => window.wtfDocs.controller.select(2));
  await alice.page.keyboard.press("Shift+End");
  await alice.page.keyboard.type("Grocery plan");
  await expect.poll(() => alice.page.evaluate(() => window.wtfDocs.active.name)).toBe("Grocery plan");
  const id = await alice.page.evaluate(() => window.wtfDocs.active.id);
  await alice.page.locator(".logo").click();
  await expect.poll(() => alice.page.evaluate(id => window.wtfDocs.documents.find(d => d.id === id).file, id)).toBe("Grocery plan");
  await expect.poll(async () => (await alice.page.evaluate(async id => (await (await fetch(`/api/documents/${id}`)).json()).file, id))).toBe("Grocery plan");
  // Two of the same template are told apart by their headings.
  await alice.page.locator(".template", { hasText: "Trip budget" }).click();
  await alice.page.waitForFunction(() => window.wtfDocs.active?.version !== undefined);
  await alice.page.locator(".logo").click();
  await alice.page.locator(".template", { hasText: "Trip budget" }).click();
  await expect(alice.page.locator("input.title-input")).toHaveValue("Trip budget 2");
  await alice.page.locator(".logo").click();
  // Explicit rename pins the file name and reports a clash.
  await alice.page.locator(".doc-row .open", { hasText: "Trip budget 2" }).click();
  await alice.page.waitForFunction(() => window.wtfDocs.controller);
  await alice.page.locator("input.title-input").fill("Grocery plan");
  await alice.page.keyboard.press("Enter");
  await expect.poll(() => alice.page.evaluate(() => window.wtfDocs.active.file)).toBe("Grocery plan 2");
  await alice.page.locator(".logo").click();
  await expect(alice.page.locator(".notice")).toContainText("already called");
  await alice.page.locator(".notice .dismiss").click();
  await expect(alice.page.locator(".notice")).toHaveCount(0);
  // A viewer's shortcuts do nothing and the editing menus are gone.
  const guest = `guest-${Date.now()}@example.com`;
  await alice.page.evaluate(([id, email]) => fetch(`/api/documents/${id}/acl`, { method: "PUT", headers: { "content-type": "application/json" }, body: JSON.stringify({ email, role: "viewer" }) }), [id, guest]);
  const bob = await openAs(browser, guest, { hash: `#/d/${id}` });
  await bob.page.waitForFunction(() => window.wtfDocs.controller && window.wtfDocs.live?.status === "connected");
  const before = await bob.page.evaluate(() => window.wtfDocs.controller.getSource());
  await bob.page.evaluate(() => window.wtfDocs.controller.select(0, 5));
  await bob.page.keyboard.press("ControlOrMeta+b");
  await bob.page.waitForTimeout(300);
  expect(await bob.page.evaluate(() => window.wtfDocs.controller.getSource())).toBe(before);
  await expect(bob.page.locator(".menubar > .menu > button", { hasText: "Format" })).toHaveCount(0);
  // Remove goes to the trash; restore brings it back.
  await alice.page.locator(".doc-row .open", { hasText: "Grocery plan" }).first().click();
  await alice.page.waitForFunction(() => window.wtfDocs.controller);
  await alice.page.locator(".menubar > .menu > button", { hasText: "File" }).click();
  await alice.page.locator(".dropdown [role=menuitem]", { hasText: "Move to trash" }).click();
  await expect(alice.page.locator(".template").first()).toBeVisible();
  await alice.page.locator(".trash-section .link").first().click();
  await expect(alice.page.locator(".trash-section .doc-row")).toHaveCount(1);
  await alice.page.locator(".trash-section .doc-row .link", { hasText: "Restore" }).click();
  await expect(alice.page.locator(".notice")).toContainText("Restored");
  await expect(alice.page.locator(".trash-section .doc-row")).toHaveCount(0);
  expect([...alice.errors, ...bob.errors]).toEqual([]);
  await alice.context.close(); await bob.context.close();
});

test("anyone with the link can read a document without signing in, read-only", async ({ browser }) => {
  const owner = await openAs(browser, `linker-${Date.now()}@example.com`);
  await owner.page.locator(".template", { hasText: "Trip budget" }).click();
  await owner.page.waitForFunction(() => window.wtfDocs.controller && window.wtfDocs.live?.status === "connected");
  await owner.page.locator(".share-button").click();
  await owner.page.locator(".link-row .button", { hasText: "Turn on" }).click();
  const url = await owner.page.locator(".link-url code").textContent();
  // A visitor with no account at all.
  const anon = await browser.newContext();
  const page = await anon.newPage();
  const errors = [];
  page.on("pageerror", e => errors.push(e.message));
  await page.goto(url.replace("/docs/#", "/docs/?test#"));
  await page.waitForFunction(() => window.wtfDocs?.ready && window.wtfDocs.controller, null, { timeout: 60_000 });
  await expect(page.locator(".view")).toContainText("$556");
  await expect(page.locator(".status")).toContainText("view only");
  expect(await page.evaluate(() => window.wtfDocs.controller.element.isContentEditable)).toBe(false);
  await expect(page.locator(".share-button")).toHaveCount(0);
  await owner.page.locator(".link-row .button", { hasText: "Turn off" }).click();
  await page.reload();
  await page.waitForFunction(() => window.wtfDocs?.ready, null, { timeout: 60_000 });
  await expect(page.locator(".notice")).toContainText("no longer works");
  expect(errors).toEqual([]);
  await anon.close(); await owner.context.close();
});
