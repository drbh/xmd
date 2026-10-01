// The examples in examples/, served as plain files the docs app fetches
// when one is opened: nothing is bundled or precached. An example's link name
// is its file name without the number that orders the tour, so renumbering
// never breaks a link: `13-charts.x.md` opens at `#/example/charts`.
import { cp, mkdir, readFile, readdir, writeFile } from "node:fs/promises";

export const nameOf = file => file.replace(/^\d+-/, "").replace(/\.x\.md$/, "");

/** Every example: its link name, file, title and the example files it imports. */
export async function readExamples(source) {
  const files = (await readdir(source)).filter(f => f.endsWith(".x.md")).sort();
  const examples = [];
  for (const file of files) {
    const text = await readFile(new URL(file, source), "utf8");
    const title = /^#\s+(.+)$/m.exec(text)?.[1].trim() ?? nameOf(file);
    const imports = [...text.matchAll(/import\("\.\/([^"/]+\.x\.md)"\)/g)].map(m => m[1]);
    for (const target of imports) if (!files.includes(target)) throw new Error(`${file} imports ${target}, which is not an example`);
    examples.push({ name: nameOf(file), file, title, imports });
  }
  const seen = new Map();
  for (const e of examples) {
    if (seen.has(e.name)) throw new Error(`${seen.get(e.name)} and ${e.file} would share the link #/example/${e.name}`);
    seen.set(e.name, e.file);
  }
  return examples;
}

/** Copy the examples and their assets into `target`, with an index.json. */
export async function writeExamples(source, target) {
  const examples = await readExamples(source);
  await mkdir(target, { recursive: true });
  for (const { file } of examples) await cp(new URL(file, source), new URL(file, target));
  await cp(new URL("assets/", source), new URL("assets/", target), { recursive: true }).catch(() => {});
  await writeFile(new URL("index.json", target), JSON.stringify(examples, null, 2) + "\n");
  return examples;
}
