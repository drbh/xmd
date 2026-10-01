// Emit a GIF of a note being written and edited in the document app: values
// appear as the note takes shape, and everything downstream moves when a
// number changes.
//
//   node scripts/typing-gif.mjs [--theme light|dark] [--out media/typing-<theme>.gif] [--port 4199]
//
// Needs a built site (`npm run build`), headless Chrome, and ffmpeg on PATH.
// Only the page is captured, not the app's chrome, in a fixed-height frame
// that scrolls with the caret. The story lives in lib/story.mjs.
import { resolve } from "node:path";
import { assemble, keystroke, launch, option, record, requireFfmpeg, serve, web } from "./lib/capture.mjs";
import { opening, story } from "./lib/story.mjs";

const theme = option("theme", "light");
const out = resolve(web, option("out", `media/typing-${theme}.gif`));
const port = Number(option("port", "4199"));
const fps = 10;
const frameWidth = 720;
const frameHeight = 360;

requireFfmpeg();
const site = await serve(port);
const browser = await launch();
let camera;
try {
  const page = await browser.newPage({ deviceScaleFactor: 2, viewport: { width: 880, height: 1000 } });
  await page.goto(`${site.base}/docs/?test`);
  await page.waitForFunction(() => window.xmdDocs?.ready, null, { timeout: 45_000 });
  await page.locator(".template", { hasText: "Blank" }).click();
  await page.waitForFunction(() => window.xmdDocs.controller);
  // The app's own theme toggle, before the chrome that holds it is hidden.
  if ((await page.evaluate(() => document.documentElement.dataset.theme)) !== theme) {
    await page.locator(".theme-toggle").click();
    await page.waitForFunction(theme => document.documentElement.dataset.theme === theme, theme);
  }
  // Start from an empty page (the blank template carries a heading) and let
  // the page fill the frame: no menus, toolbar or console chip.
  await page.addStyleTag({ content: "header.chrome, .toolbar, .chip, .console-chip, .status { display: none !important; } .canvas { padding-top: 0 !important; }" });
  // The note is already written; the first thing the viewer sees is it changing.
  await page.evaluate(text => window.xmdDocs.controller.setSource(text), opening);
  await page.evaluate(() => window.xmdDocs.controller.select(0));
  await page.waitForTimeout(400);
  const box = await page.locator(".view").boundingBox();
  // A fixed frame: the page scrolls beneath it so the caret's line stays in
  // view, the way an editor would.
  const clip = { x: Math.max(0, box.x - 24), y: Math.max(0, box.y - 20), width: frameWidth, height: frameHeight };
  await page.setViewportSize({ width: clip.x + frameWidth, height: clip.y + frameHeight });
  const follow = () => page.evaluate(() => {
    const view = document.querySelector(".view");
    let pane = view;
    while (pane && pane.scrollHeight <= pane.clientHeight) pane = pane.parentElement;
    if (!pane) return;
    const selection = window.getSelection();
    const node = selection?.anchorNode;
    const line = (node?.nodeType === 1 ? node : node?.parentElement)?.closest(".line") ?? view.querySelector(".line:last-child");
    if (!line) return;
    const bottom = line.getBoundingClientRect().bottom - pane.getBoundingClientRect().top;
    const room = pane.clientHeight - 40;
    if (bottom > room) pane.scrollTop += bottom - room;
  });
  const caret = offset => page.evaluate(offset => window.xmdDocs.controller.select(offset), offset);
  const source = () => page.evaluate(() => window.xmdDocs.controller.getSource());

  camera = await record(page, clip, fps);
  await page.waitForTimeout(600);
  for (const step of story) {
    if (step.type) {
      for (const ch of step.type) {
        if (ch === "\n") {
          // Insert line breaks directly: the keyboard's Enter would also run
          // the editor's task-list continuation, and the story types every
          // bullet itself.
          await page.evaluate(async () => {
            const c = window.xmdDocs.controller;
            const at = c.selection()?.focus ?? c.getSource().length;
            await c.replaceRange(at, at, "\n");
          });
          await follow();
        } else {
          await page.keyboard.type(ch);
        }
        await page.waitForTimeout(keystroke(ch));
      }
    } else if (step.after) {
      const at = (await source()).indexOf(step.after);
      if (at === -1) throw new Error(`"${step.after}" is not in the note`);
      await caret(at + step.after.length);
      await follow();
      await page.waitForTimeout(500);
    } else if (step.replace) {
      // Select the old text, hold so the selection reads, then type over it.
      const at = (await source()).indexOf(step.replace);
      if (at === -1) throw new Error(`"${step.replace}" is not in the note`);
      await page.evaluate(([a, b]) => window.xmdDocs.controller.select(a, b), [at, at + step.replace.length]);
      await follow();
      await page.waitForTimeout(700);
      for (const ch of step.with) {
        await page.keyboard.type(ch);
        await page.waitForTimeout(keystroke(ch));
      }
    } else if (step.end) {
      await caret((await source()).length);
      await follow();
      await page.waitForTimeout(300);
    } else if (step.pause) {
      await page.waitForTimeout(step.pause);
    }
  }
  await camera.stop();
} finally {
  await browser.close();
  site.stop();
}
await assemble(camera.frames, out, { fps, width: frameWidth });
