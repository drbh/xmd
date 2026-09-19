import { test, expect } from "@playwright/test";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const binary = fileURLToPath(new URL("../../../target/debug/wtf", import.meta.url));

test("resolved HTML matches inline text, editor colors, links and diagnostics without external assets", async ({ page }) => {
  const root = mkdtempSync(join(tmpdir(), "wtf-render-"));
  try {
    const source = '\n# Resolved 🦀\n[$10]:budget\nremaining := (\n  budget - $3\n)\nAmount [remaining].\n- [x] Done\n- [ ] Next\n[Site](https://example.com/?a=1&b=2)\nbad := missing + 1\n// </code><script>globalThis.pwned=1</script>\n';
    writeFileSync(join(root, "main.wtf"), source);
    mkdirSync(join(root, ".wtf/modules"), { recursive: true });
    writeFileSync(join(root, ".wtf/modules/custom.wtf"), 'module := {api: 1, id: "custom", kind: "feature", inputs: []}\ncollect := fn(ctx) => [{line: 1, label: "<img src=x onerror=bad()>", tooltip: "tip <&>"}]');
    const render = format => spawnSync(binary, ["render", "main.wtf", "--root", root, "--now", "2026-09-18T12:00:00Z", "--format", format], { encoding: "utf8" });
    const html = render("html"), text = render("text");
    expect(html.status).toBe(1); // The missing name remains a visible diagnostic.
    expect(html.stderr).toContain("Unknown name 'missing'");
    expect(text.status).toBe(1);
    const requests = [];
    page.on("request", request => requests.push(request.url()));
    await page.setContent(html.stdout);
    expect(await page.locator("pre code").textContent()).toBe(text.stdout);
    await expect(page).toHaveTitle("main.wtf");
    await expect(page.locator(".t-wtfMoney").filter({ hasText: "$10" })).toHaveCSS("color", "rgb(180, 217, 138)");
    await expect(page.locator(".t-variable.declaration").filter({ hasText: "remaining" })).toHaveCSS("font-weight", "700");
    await expect(page.locator(".inlay").filter({ hasText: "= $7" })).toHaveCSS("background-color", "rgb(45, 49, 56)");
    await expect(page.locator(".diagnostic.error").filter({ hasText: "missing" })).toHaveCSS("text-decoration-style", "wavy");
    await expect(page.locator(".t-wtfTaskDone").filter({ hasText: "Done" })).toHaveCSS("text-decoration-line", "line-through");
    expect(await page.locator('a[href="https://example.com/?a=1&b=2"]').count()).toBeGreaterThan(0);
    await expect(page.locator("script, img, link, iframe")).toHaveCount(0);
    expect(await page.evaluate(() => globalThis.pwned)).toBeUndefined();
    expect(requests).toEqual([]);
    if (process.env.WTF_RENDER_SCREENSHOT) await page.screenshot({ path: resolve(process.env.WTF_RENDER_SCREENSHOT), fullPage: true });
    await page.emulateMedia({ media: "print" });
    await expect(page.locator(".t-variable.declaration").filter({ hasText: "remaining" })).toHaveCSS("color", "rgb(32, 40, 32)");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
