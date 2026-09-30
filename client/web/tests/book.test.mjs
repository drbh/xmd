import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, readFile, writeFile, rm, stat } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { pageOf, readBook, blocksOf, toHtml, writeBook } from "../scripts/lib/book.mjs";

const repo = new URL("../../../", import.meta.url);

test("a page's address drops the number that orders the chapters", () => {
  assert.equal(pageOf("README.md"), "index.html");
  assert.equal(pageOf("01-language.md"), "language.html");
  assert.equal(pageOf("reference/functions.md"), "reference/functions.html");
});

test("a block's file is the one cli/tests/book.rs writes", () => {
  const page = { path: "03-files.md", markdown: "```xmd budget.x.md\na := 1\n```\n\n```bash\nxmd\n```\n\n```xmd\nb := 2\n```\n" };
  assert.deepEqual(blocksOf(page), [
    { fence: 1, file: "budget.x.md", source: "a := 1\n" },
    { fence: 3, file: "03-files-3.x.md", source: "b := 2\n" },
  ]);
});

test("the markdown subset the book uses", () => {
  const html = toHtml([
    "# 1. the `x` language",
    "",
    "a [link](02-queries.md) and ```` ```xmd name ```` and **bold**",
    "",
    "- one",
    "  continued",
    "- two",
    "",
    "| field | value |",
    "| --- | --- |",
    "| `a` | b \\| c |",
    "",
    "<!-- dropped -->",
    "```xmd",
    "a := <b>",
    "```",
  ].join("\n"), { link: href => href.replace(".md", ".html"), file: () => "f.x.md" });
  assert.match(html, /<h1 id="1-the-x-language">1\. the <code>x<\/code> language<\/h1>/);
  assert.match(html, /<a href="02-queries\.html">link<\/a>/);
  assert.match(html, /<code>```xmd name<\/code>/);
  assert.match(html, /<strong>bold<\/strong>/);
  assert.match(html, /<li>one continued<\/li><li>two<\/li>/);
  assert.match(html, /<td><code>a<\/code><\/td><td>b \| c<\/td>/);
  assert.doesNotMatch(html, /dropped/);
  assert.match(html, /<div class="xmd-block" data-file="f\.x\.md"><pre class="code"><code>a := &lt;b&gt;<\/code><\/pre><\/div>/);
});

test("the book builds, and every chapter is in reading order", async () => {
  const pages = await readBook(new URL("book/", repo));
  assert.equal(pages[0].page, "index.html");
  const chapters = pages.filter(p => p.chapter).map(p => p.path);
  assert.deepEqual(chapters, [...chapters].sort());
  assert.ok(chapters.length > 0);
});

test("a link to nothing fails the build", async () => {
  const dir = await mkdtemp(join(tmpdir(), "xmd-book-"));
  try {
    await mkdir(join(dir, "book"));
    await writeFile(join(dir, "book/README.md"), "# book\n\n[gone](01-missing.md)\n");
    const root = pathToFileURL(dir + "/");
    await assert.rejects(
      writeBook(new URL("book/", root), new URL("out/", root), new URL("client/web/book/", repo), root),
      /links to 01-missing\.md, which does not exist/,
    );
  } finally { await rm(dir, { recursive: true, force: true }); }
});

test("every relative link in the repository's markdown leads somewhere", async () => {
  const root = fileURLToPath(repo);
  const pages = execFileSync("git", ["ls-files", "*.md"], { cwd: root, encoding: "utf8" }).split("\n")
    .filter(f => f && !f.endsWith(".x.md") && !f.includes("/tests/"));
  const broken = [];
  for (const page of pages) {
    for (const [, href] of (await readFile(join(root, page), "utf8")).matchAll(/\]\(([^)\s]+)\)/g)) {
      if (/^[a-z]+:|^#/.test(href)) continue;
      if (!await stat(join(root, dirname(page), href.split("#")[0])).catch(() => null)) broken.push(`${page}: ${href}`);
    }
  }
  assert.deepEqual(broken, []);
});
