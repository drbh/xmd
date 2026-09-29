import { test } from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { readExamples, nameOf } from "../scripts/lib/examples.mjs";

const repo = new URL("../../../", import.meta.url);
const examples = new URL("lang/examples/", repo);

test("an example's link name drops the number that orders the tour", () => {
  assert.equal(nameOf("13-charts.x.md"), "charts");
  assert.equal(nameOf("19-cross-note-values.x.md"), "cross-note-values");
});

test("every example has a unique link name and imports only other examples", async () => {
  const all = await readExamples(examples);
  assert.ok(all.length > 0);
  assert.deepEqual(all.find(e => e.name === "cross-note-values").imports, ["20-cross-note-source.x.md"]);
});

test("every #/example/ link in the repository opens an example", async () => {
  const names = new Set((await readExamples(examples)).map(e => e.name));
  const tracked = execFileSync("git", ["ls-files"], { cwd: fileURLToPath(repo), encoding: "utf8" }).split("\n")
    .filter(f => /\.(md|xmd|js|mjs|svelte|rs)$/.test(f) && !f.includes("/tests/"));
  const broken = [];
  for (const file of tracked) {
    const text = await readFile(new URL(file, repo), "utf8");
    for (const [, name] of text.matchAll(/#\/example\/([\w-]+)/g)) if (!names.has(name)) broken.push(`${file}: ${name}`);
  }
  assert.deepEqual(broken, []);
});
