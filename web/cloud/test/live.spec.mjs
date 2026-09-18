import { test, expect } from "@playwright/test";

// Two editors and a viewer on one document, each a separate browser context
// with its own development identity.
async function openAs(browser, email, hash = "") {
  const context = await browser.newContext({ extraHTTPHeaders: { "x-dev-user": email } });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", e => errors.push(`${email}: ${e.message}`));
  await page.goto(`/docs/?test${hash}`);
  await page.waitForFunction(() => window.wtfDocs?.ready, null, { timeout: 60_000 });
  return { page, context, errors };
}
const source = page => page.evaluate(() => window.wtfDocs.controller.getSource());
const share = (page, id, email, role) => page.evaluate(([id, email, role]) => fetch(`/api/documents/${id}/acl`, { method: "PUT", headers: { "content-type": "application/json" }, body: JSON.stringify({ email, role }) }), [id, email, role]);

test("editors see each other's typing, keep their carets, undo their own edits, and the room persists", async ({ browser }) => {
  const alice = await openAs(browser, "alice@example.com");
  await alice.page.locator(".template", { hasText: "Trip budget" }).click();
  await alice.page.waitForFunction(() => window.wtfDocs.controller && window.wtfDocs.live?.status === "connected");
  await expect(alice.page.locator(".live")).toHaveText("Live");
  const id = await alice.page.evaluate(() => window.wtfDocs.active.id);
  await share(alice.page, id, "bob@example.com", "editor");

  const bob = await openAs(browser, "bob@example.com", `#/d/${id}`);
  await bob.page.waitForFunction(() => window.wtfDocs.controller && window.wtfDocs.live?.status === "connected");
  await expect(alice.page.locator(".people-here .avatar")).toHaveCount(1);

  // Bob types at the end; Alice sees it and Bob's caret.
  await bob.page.evaluate(() => window.wtfDocs.controller.select(window.wtfDocs.controller.getSource().length));
  await bob.page.keyboard.type("\nBob was here.");
  await expect.poll(() => source(alice.page)).toMatch(/Bob was here\.$/);
  await expect(alice.page.locator(".presence-caret .presence-name")).toHaveText("bob");

  // Alice inserts near the top while Bob keeps typing at the end: nothing is lost and Bob's caret stays put.
  await alice.page.evaluate(() => window.wtfDocs.controller.select(window.wtfDocs.controller.getSource().indexOf("\n")));
  await alice.page.keyboard.type(" (shared)");
  await bob.page.keyboard.type(" Twice.");
  await expect.poll(() => source(bob.page)).toMatch(/^# Trip budget \(shared\)\n[\s\S]*Bob was here\. Twice\.$/);
  await expect.poll(() => source(alice.page)).toBe(await source(bob.page));
  const bobSel = await bob.page.evaluate(() => [window.wtfDocs.controller.selection().focus, window.wtfDocs.controller.getSource().length]);
  expect(bobSel[0]).toBe(bobSel[1]);

  // Undo is per person: Bob's undo removes only Bob's last run of typing.
  await bob.page.keyboard.press("ControlOrMeta+z");
  await expect.poll(() => source(alice.page)).toMatch(/\(shared\)/);
  await expect.poll(() => source(alice.page)).not.toMatch(/Twice/);

  // The room mirrors the text into D1 with a new version.
  await expect.poll(async () => (await alice.page.evaluate(async id => (await (await fetch(`/api/documents/${id}`)).json()), id)).text, { timeout: 15_000 }).toMatch(/\(shared\)/);

  // A viewer follows along read-only; a stranger cannot open the document at all.
  await share(alice.page, id, "carol@example.com", "viewer");
  const carol = await openAs(browser, "carol@example.com", `#/d/${id}`);
  await carol.page.waitForFunction(() => window.wtfDocs.controller && window.wtfDocs.live?.status === "connected");
  expect(await carol.page.evaluate(() => window.wtfDocs.controller.element.isContentEditable)).toBe(false);
  await expect.poll(() => source(carol.page)).toMatch(/\(shared\)/);
  await expect(carol.page.locator(".status")).toContainText("View only");
  const dave = await openAs(browser, "dave@example.com", `#/d/${id}`);
  expect(await dave.page.evaluate(() => !!window.wtfDocs.active)).toBe(false);

  expect([...alice.errors, ...bob.errors, ...carol.errors, ...dave.errors]).toEqual([]);
  for (const c of [alice, bob, carol, dave]) await c.context.close();
});

test("a document edited over the REST API while a room is open reaches the live editors", async ({ browser }) => {
  const alice = await openAs(browser, "alice@example.com");
  await alice.page.locator(".template", { hasText: "Blank" }).click();
  await alice.page.waitForFunction(() => window.wtfDocs.controller && window.wtfDocs.live?.status === "connected");
  const id = await alice.page.evaluate(() => window.wtfDocs.active.id);
  const before = await alice.page.evaluate(async id => (await (await fetch(`/api/documents/${id}`)).json()), id);
  const response = await alice.page.evaluate(async ([id, version]) => (await fetch(`/api/documents/${id}`, { method: "PUT", headers: { "content-type": "application/json" }, body: JSON.stringify({ name: "Via API", text: "# Via API\n\nwritten by a script\n", version }) })).status, [id, before.version]);
  expect(response).toBe(200);
  await expect.poll(() => source(alice.page)).toBe("# Via API\n\nwritten by a script\n");
  await expect(alice.page.locator("input.title-input")).toHaveValue("Via API");
  expect(alice.errors).toEqual([]);
  await alice.context.close();
});
