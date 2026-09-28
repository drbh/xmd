// Emit a GIF of the note from the typing GIF being queried from a terminal.
//
//   node scripts/terminal-gif.mjs [--theme light|dark] [--out media/terminal-<theme>.gif]
//
// The terminal is a styled page in headless Chrome, so both themes use the
// note's own colors and fonts, but every answer is the real binary's output:
// each command is typed, then run against the note at the frozen clock and its
// stdout/stderr printed. Needs `cargo build` (the wtf binary), headless Chrome
// and ffmpeg on PATH.
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { assemble, binary, keystroke, launch, option, record, requireFfmpeg, web } from "./lib/capture.mjs";
import { finalNote, now } from "./lib/story.mjs";

// Each session step is a comment, then a command whose `wtf …` is shown as
// typed while `args` is what actually runs. A step with `stdin` really pipes
// the note in, and `shown` is then the pipeline as a person would type it.
// Pauses let the answer be read.
const session = [
  { comment: "read one value", args: ["total"], stdin: finalNote(), shown: "cat trip.x.md | wtf 'total'" },
  { comment: "filter and project, like SQL", args: ["query", "trip.x.md", "tasks | where !done | select {title, due}"] },
  { comment: "typed JSON for scripts", args: ["query", "trip.x.md", "car + groceries", "--json"] },
  { comment: "every value in the note, with its type", args: ["query", "trip.x.md", "values | select {name, type, display}"] },
  { comment: "fail a build when a note is broken", args: ["query", "--workspace", 'diagnostics | where severity == "error"', "--fail-on-match"] },
  { comment: "the note with every value written in", args: ["render", "trip.x.md", "--format", "text"] },
];

const theme = option("theme", "light");
const out = resolve(web, option("out", `media/terminal-${theme}.gif`));
const fps = 10;
const frameWidth = 720;
const frameHeight = 360;

requireFfmpeg();
const root = await mkdtemp(join(tmpdir(), "wtf-terminal-"));
await writeFile(join(root, "trip.x.md"), finalNote());

const run = (args, stdin) => {
  const result = spawnSync(binary, [...args, "--root", root, "--now", now], { encoding: "utf8", input: stdin, env: { ...process.env, TZ: "UTC" } });
  if (result.error) throw new Error(`Cannot run ${binary}: ${result.error.message} (run \`cargo build\` first)`);
  return { text: (result.stdout + result.stderr).replace(/\n$/, ""), status: result.status };
};
const shown = step => step.shown ?? `wtf ${step.args.map(a => (/[\s|!{}"()*]/.test(a) ? `'${a}'` : a)).join(" ")}`;

const palette = theme === "light"
  ? { paper: "#ffffff", ink: "#1f1f1f", dim: "#7a8088", prompt: "#2f6f9f", ok: "#3f7d3f", err: "#b2453d" }
  : { paper: "#202124", ink: "#e3e3e3", dim: "#9aa0a6", prompt: "#8ab4f8", ok: "#81c995", err: "#f28b82" };
const fonts = new URL("../theme/fonts.css", import.meta.url).href;
const html = `<!doctype html><html><head><meta charset="utf-8"><link rel="stylesheet" href="${fonts}"><style>
html, body { margin: 0; background: ${palette.paper}; }
#term { box-sizing: border-box; width: ${frameWidth}px; height: ${frameHeight}px; overflow: hidden; padding: 20px 28px;
  color: ${palette.ink}; font: 14px/1.7 "Ioskeley Mono", "SFMono-Regular", Consolas, monospace; white-space: pre-wrap; word-break: break-all; }
.dim { color: ${palette.dim}; } .prompt { color: ${palette.prompt}; font-weight: 700; } .ok { color: ${palette.ok}; } .err { color: ${palette.err}; }
.caret { display: inline-block; width: 8px; height: 1.1em; vertical-align: -2px; background: ${palette.ink}; opacity: .8; }
</style></head><body><div id="term"><span class="caret"></span></div></body></html>`;
const pageFile = join(root, "terminal.html");
await writeFile(pageFile, html);

const browser = await launch();
let camera;
try {
  const page = await browser.newPage({ deviceScaleFactor: 2, viewport: { width: frameWidth, height: frameHeight } });
  await page.goto(`file://${pageFile}`);
  await page.evaluate(() => document.fonts.ready);
  const term = page.locator("#term");
  // Everything is appended before the caret; the pane keeps its tail visible.
  const print = (html) => page.evaluate(html => {
    const term = document.getElementById("term");
    const caret = term.querySelector(".caret");
    caret.insertAdjacentHTML("beforebegin", html);
    term.scrollTop = term.scrollHeight;
  }, html);
  const escape = s => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

  camera = await record(page, { x: 0, y: 0, width: frameWidth, height: frameHeight }, fps);
  await page.waitForTimeout(500);
  for (const step of session) {
    await print(`<span class="dim"># ${escape(step.comment)}</span>\n<span class="prompt">$</span> `);
    await page.waitForTimeout(500);
    for (const ch of shown(step)) {
      await print(escape(ch));
      await page.waitForTimeout(keystroke(ch));
    }
    await page.waitForTimeout(350);
    const { text, status } = run(step.args, step.stdin);
    const body = text ? `${escape(text)}\n` : "";
    const exit = status === 0 && !text ? `<span class="ok">(no output, exit 0)</span>\n` : status !== 0 ? `<span class="err">exit ${status}</span>\n` : "";
    await print(`\n${body}${exit}\n`);
    await page.waitForTimeout(Math.min(3000, 1400 + text.split("\n").length * 120));
  }
  await page.waitForTimeout(1200);
  await camera.stop();
} finally {
  await browser.close();
  await rm(root, { recursive: true, force: true });
}
await assemble(camera.frames, out, { fps, width: frameWidth });
