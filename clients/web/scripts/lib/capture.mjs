// Shared plumbing for the GIF scripts: options, the built site served on a
// spare port, a headless Chrome page, a frame camera on a fixed clock, and
// ffmpeg turning the frames into a looping GIF.
import { mkdtemp, rm, mkdir } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn, spawnSync } from "node:child_process";
import { chromium } from "@playwright/test";

export const web = fileURLToPath(new URL("../../", import.meta.url));
export const binary = resolve(web, "../../target/debug/xmd");

/// `--name value` from the command line, or the fallback.
export function option(name, fallback) {
  const args = process.argv.slice(2);
  const at = args.indexOf(`--${name}`);
  return at === -1 ? fallback : args[at + 1];
}

export function requireFfmpeg() {
  if (spawnSync("ffmpeg", ["-version"]).error) throw new Error("ffmpeg is required to assemble the GIF");
}

/// Serve dist/ on our own port so a running dev server is never mistaken for
/// the fresh build. Returns the base URL and a function that stops it.
export async function serve(port) {
  const server = spawn(process.execPath, [join(web, "serve.mjs")], { env: { ...process.env, XMD_WEB_PORT: String(port) }, stdio: "ignore" });
  const base = `http://127.0.0.1:${port}`;
  for (let tries = 0; ; tries++) {
    if (await fetch(base).then(r => r.ok, () => false)) break;
    if (tries > 100) throw new Error(`The site did not come up on ${base}; run \`npm run build\` first`);
    await new Promise(r => setTimeout(r, 100));
  }
  return { base, stop: () => server.kill() };
}

export function launch() {
  return chromium.launch({ channel: "chrome" }).catch(() => chromium.launch());
}

/// Screenshots `clip` on a fixed clock until stopped, so a pause in the story
/// is a pause in the GIF. Returns the frame directory and the stop function.
export async function record(page, clip, fps) {
  const frames = await mkdtemp(join(tmpdir(), "xmd-gif-"));
  let frame = 0;
  let rolling = true;
  const camera = (async () => {
    while (rolling) {
      await page.screenshot({ path: join(frames, `f${String(frame++).padStart(4, "0")}.png`), clip });
      await new Promise(r => setTimeout(r, 1000 / fps));
    }
  })();
  return { frames, stop: async () => { rolling = false; await camera; } };
}

/// Frames → looping GIF with a per-file palette; the frame directory is removed.
export async function assemble(frames, out, { fps, width, colors = 96 }) {
  await mkdir(dirname(out), { recursive: true });
  const filters = `fps=${fps},scale=${width}:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=${colors}[p];[b][p]paletteuse=dither=bayer:bayer_scale=3`;
  const ffmpeg = spawnSync("ffmpeg", ["-y", "-loglevel", "error", "-framerate", String(fps), "-i", join(frames, "f%04d.png"), "-vf", filters, "-loop", "0", out], { encoding: "utf8" });
  await rm(frames, { recursive: true, force: true });
  if (ffmpeg.status !== 0) throw new Error(`ffmpeg failed: ${ffmpeg.stderr}`);
  console.log(`GIF written to ${out}`);
}

/// A human-ish delay for one typed character.
export const keystroke = ch => (ch === "\n" ? 160 : 22 + Math.random() * 22);
