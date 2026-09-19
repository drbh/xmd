// Emit a PNG of a rendered note, by default the README's minimal example.
//
//   node scripts/screenshot.mjs [--out media/screenshot.png] [--theme dark|light]
//                               [--source note.wtf] [--now 2026-09-18T12:00:00Z]
//
// The note is resolved by the native binary (`wtf render --format html`, so the
// picture shows exactly the inline values and colors the editor shows), the web
// fonts are attached from theme/, and a headless Chrome captures the page.
// Build the binary first: `cargo build` at the repository root.
import { mkdtemp, readFile, rm, writeFile, mkdir } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { chromium } from "@playwright/test";

const example = `The most basic note could simply be a note with some expense

$1,234:car

$67:groceries

$0.01:peanuts

total := car + groceries + peanuts

total was [total]
`;

const args = process.argv.slice(2);
const option = (name, fallback) => {
  const at = args.indexOf(`--${name}`);
  return at === -1 ? fallback : args[at + 1];
};
const web = fileURLToPath(new URL("../", import.meta.url));
const out = resolve(web, option("out", "media/screenshot.png"));
const theme = option("theme", "dark");
const now = option("now", "2026-09-18T12:00:00Z");
const binary = resolve(web, "../../target/debug/wtf");
const source = option("source") ? await readFile(resolve(option("source")), "utf8") : example;

// The binary only renders notes it can find under a root.
const root = await mkdtemp(join(tmpdir(), "wtf-shot-"));
try {
  await writeFile(join(root, "example.wtf"), source);
  const rendered = spawnSync(binary, ["render", "example.wtf", "--root", root, "--now", now, "--format", "html"], { encoding: "utf8" });
  if (rendered.error) throw new Error(`Cannot run ${binary}: ${rendered.error.message} (run \`cargo build\` first)`);
  if (rendered.stderr.trim()) console.error(rendered.stderr.trim());
  if (!rendered.stdout) throw new Error(`wtf render produced no HTML (exit ${rendered.status})`);

  // The export embeds the dark palette but neither the fonts nor the light
  // rules; attach the full theme from theme/ and pad the note with its own paper.
  const link = name => `<link rel="stylesheet" href="${new URL(`../theme/${name}`, import.meta.url).href}">`;
  const html = rendered.stdout
    .replace("</head>", `${link("fonts.css")}${link("style.css")}<style>pre.wtf { display: inline-block; margin: 0; padding: 32px 40px 32px 32px; }</style></head>`)
    .replace("<html", theme === "light" ? '<html class="wtf-light"' : "<html");
  const pageFile = join(root, "example.html");
  await writeFile(pageFile, html);

  const browser = await chromium.launch({ channel: "chrome" }).catch(() => chromium.launch());
  try {
    const page = await browser.newPage({ deviceScaleFactor: 2, viewport: { width: 900, height: 600 } });
    await page.goto(`file://${pageFile}`);
    await page.evaluate(() => document.fonts.ready);
    await mkdir(dirname(out), { recursive: true });
    await page.locator("pre.wtf").screenshot({ path: out });
  } finally {
    await browser.close();
  }
  console.log(`Screenshot written to ${out}`);
} finally {
  await rm(root, { recursive: true, force: true });
}
