// Emit a GIF of a note being written and edited in the document app: values
// appear as the note takes shape, and everything downstream moves when a
// number changes.
//
//   node scripts/typing-gif.mjs [--theme light|dark] [--out dist/typing-<theme>.gif] [--port 4199]
//
// Needs a built site (`npm run build`), headless Chrome, and ffmpeg on PATH.
// The script serves dist/ itself on a spare port so a running dev server is
// never mistaken for the fresh build. Only the page is captured, not the
// app's chrome, in a fixed-height frame that scrolls with the caret.
import { mkdtemp, rm, mkdir } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn, spawnSync } from "node:child_process";
import { chromium } from "@playwright/test";

// The story: each step is `type` text at the caret, `after` (move the caret to
// just after the first occurrence of a substring), `end` (caret to the end),
// or `pause` milliseconds. Pauses are where the viewer reads the result.
const story = [
  { type: "Values are plain text with a name.\n$1,234:car\n$67:groceries\n\n" },
  { type: "Calculations update as you type.\ntotal := car + groceries" },
  { pause: 1400 },
  { after: "$67" },
  { type: "0" },
  { pause: 1800 },
  { end: true },
  { type: "\n\nAny value can sit inside a sentence.\nWe have [total] left for the trip." },
  { pause: 1500 },
  { type: "\n\nDates do arithmetic.\n2026-11-20:departure\nLeaving in [departure - today()]." },
  { pause: 1500 },
  { type: "\n\nTables have typed columns and sum themselves.\nbasket := table\n| item | qty | price |\n| --- | --- | --- |\n| apple | 2 | $3.30 |\n| pear | 4 | $4.30 |\n\nspend := sum(basket, qty * price)" },
  { pause: 1800 },
  { type: "\n\nTasks know when they are due.\n- [ ] Book the flights @due(departure - 14d)\n- [ ] Pack @due(tomorrow)" },
  { pause: 1800 },
  { type: "\n\nA named heading is a checklist that counts.\n## Packing :packing\n- [x] Passport\n- [x] Charger\n- [ ] Sunscreen\n\n[completed(packing)] of [total(packing)] packed." },
  { pause: 1800 },
  { type: "\n\nTimers are values; their controls live in the note.\nfocus := countdown(25m)" },
  { pause: 1600 },
  { type: "\n\nLibraries add functions.\nThat is [round(import(\"units\").convert(100, \"km\", \"mi\"))] miles." },
  { pause: 2600 },
];

const args = process.argv.slice(2);
const option = (name, fallback) => {
  const at = args.indexOf(`--${name}`);
  return at === -1 ? fallback : args[at + 1];
};
const web = fileURLToPath(new URL("../", import.meta.url));
const theme = option("theme", "light");
const out = resolve(web, option("out", `dist/typing-${theme}.gif`));
const port = Number(option("port", "4199"));
const fps = 10;
const frameWidth = 720;
const frameHeight = 360;

if (spawnSync("ffmpeg", ["-version"]).error) throw new Error("ffmpeg is required to assemble the GIF");

// Serve the built site on our own port.
const server = spawn(process.execPath, [join(web, "serve.mjs")], { env: { ...process.env, WTF_WEB_PORT: String(port) }, stdio: "ignore" });
const base = `http://127.0.0.1:${port}`;
for (let tries = 0; ; tries++) {
  if (await fetch(base).then(r => r.ok, () => false)) break;
  if (tries > 100) throw new Error(`The site did not come up on ${base}; run \`npm run build\` first`);
  await new Promise(r => setTimeout(r, 100));
}

const frames = await mkdtemp(join(tmpdir(), "wtf-gif-"));
const browser = await chromium.launch({ channel: "chrome" }).catch(() => chromium.launch());
try {
  const page = await browser.newPage({ deviceScaleFactor: 2, viewport: { width: 880, height: 1000 } });
  await page.goto(`${base}/docs/?test`);
  await page.waitForFunction(() => window.wtfDocs?.ready, null, { timeout: 45_000 });
  await page.locator(".template", { hasText: "Blank" }).click();
  await page.waitForFunction(() => window.wtfDocs.controller);
  // The app's own theme toggle, before the chrome that holds it is hidden.
  if ((await page.evaluate(() => document.documentElement.dataset.theme)) !== theme) {
    await page.locator(".theme-toggle").click();
    await page.waitForFunction(theme => document.documentElement.dataset.theme === theme, theme);
  }
  // Start from an empty page (the blank template carries a heading) and let
  // the page fill the frame: no menus, toolbar or console chip.
  await page.addStyleTag({ content: "header.chrome, .toolbar, .chip, .console-chip, .status { display: none !important; } .canvas { padding-top: 0 !important; }" });
  await page.evaluate(() => window.wtfDocs.controller.setSource(""));
  await page.evaluate(() => window.wtfDocs.controller.select(0));
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

  // Frames are captured on a fixed clock while the story plays, so pauses in
  // the story are pauses in the GIF.
  let frame = 0;
  let rolling = true;
  const camera = (async () => {
    while (rolling) {
      await page.screenshot({ path: join(frames, `f${String(frame++).padStart(4, "0")}.png`), clip });
      await new Promise(r => setTimeout(r, 1000 / fps));
    }
  })();
  const caret = offset => page.evaluate(offset => window.wtfDocs.controller.select(offset), offset);
  const source = () => page.evaluate(() => window.wtfDocs.controller.getSource());
  await page.waitForTimeout(600);
  for (const step of story) {
    if (step.type) {
      for (const ch of step.type) {
        if (ch === "\n") {
          // Insert line breaks directly: the keyboard's Enter would also run
          // the editor's task-list continuation, and the story types every
          // bullet itself.
          await page.evaluate(async () => {
            const c = window.wtfDocs.controller;
            const at = c.selection()?.focus ?? c.getSource().length;
            await c.replaceRange(at, at, "\n");
          });
          await follow();
          await page.waitForTimeout(160);
        } else {
          await page.keyboard.type(ch);
          await page.waitForTimeout(22 + Math.random() * 22);
        }
      }
    } else if (step.after) {
      const at = (await source()).indexOf(step.after);
      if (at === -1) throw new Error(`"${step.after}" is not in the note`);
      await caret(at + step.after.length);
      await follow();
      await page.waitForTimeout(500);
    } else if (step.end) {
      await caret((await source()).length);
      await follow();
      await page.waitForTimeout(300);
    } else if (step.pause) {
      await page.waitForTimeout(step.pause);
    }
  }
  rolling = false;
  await camera;
} finally {
  await browser.close();
  server.kill();
}

await mkdir(dirname(out), { recursive: true });
const filters = `fps=${fps},scale=${frameWidth}:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=96[p];[b][p]paletteuse=dither=bayer:bayer_scale=3`;
const ffmpeg = spawnSync("ffmpeg", ["-y", "-loglevel", "error", "-framerate", String(fps), "-i", join(frames, "f%04d.png"), "-vf", filters, "-loop", "0", out], { encoding: "utf8" });
await rm(frames, { recursive: true, force: true });
if (ffmpeg.status !== 0) throw new Error(`ffmpeg failed: ${ffmpeg.stderr}`);
console.log(`GIF written to ${out}`);
