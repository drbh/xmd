// The book in /book, built into static pages at dist/book/. The pages are the
// same markdown GitHub shows; this turns the small subset the book uses into
// HTML, and a ```xmd block becomes a live editor (clients/web/book/book.js).
//
// Every ```xmd block of every chapter goes into files.json, keyed by the file
// hosts/cli/tests/book.rs writes it to, so the browser evaluates the same workspace
// the tests check: `trip.x.md` from chapter 1 is there for chapter 2.
//
// A relative link to another page becomes that page's .html; a link to
// anything else in the repository goes to GitHub. A link to nothing fails
// the build.
import { cp, mkdir, readFile, readdir, stat, writeFile } from "node:fs/promises";
import { join, posix } from "node:path";
import { fileURLToPath } from "node:url";

const GITHUB = "https://github.com/drbh/xmd/blob/main/";

/** A page's address inside dist/book: `01-language.md` is `language.html`. */
export const pageOf = path => {
  const dir = posix.dirname(path), name = posix.basename(path);
  const stem = name === "README.md" ? "index" : name.replace(/^\d+-/, "").replace(/\.md$/, "");
  return dir === "." ? `${stem}.html` : `${dir}/${stem}.html`;
};

/** The book's pages, in reading order: the index, the chapters, the reference. */
export async function readBook(source) {
  const chapters = (await readdir(source)).filter(f => /^\d+-.*\.md$/.test(f)).sort();
  const reference = (await readdir(new URL("reference/", source)).catch(() => []))
    .filter(f => f.endsWith(".md")).sort().map(f => `reference/${f}`);
  const pages = [];
  for (const path of ["README.md", ...chapters, ...reference]) {
    const markdown = await readFile(new URL(path, source), "utf8");
    const title = /^#\s+(.+)$/m.exec(markdown)?.[1].trim() ?? path;
    pages.push({ path, page: pageOf(path), title, markdown, chapter: chapters.includes(path) });
  }
  return pages;
}

/** Each ```xmd block, by its position among the page's fences, with the
 * file hosts/cli/tests/book.rs writes it to. */
export function blocksOf(page) {
  const blocks = [];
  let fence = 0;
  for (const [, info, source] of page.markdown.matchAll(/^```([^\n]*)\n([\s\S]*?)^```$/gm)) {
    fence += 1;
    const [lang, file] = info.trim().split(/\s+/);
    if (lang === "xmd") blocks.push({ fence, file: file ?? `${page.path.replace(/\.md$/, "")}-${fence}.x.md`, source });
  }
  return blocks;
}

const escape = s => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
const slug = text => text.toLowerCase().replace(/<[^>]+>/g, "").replace(/[^\w]+/g, "-").replace(/^-|-$/g, "");

/** Inline markdown: code spans, links and bold. Code spans are left alone. */
function inline(text, link) {
  return text.split(/(`+)([\s\S]*?[^`])\1(?!`)/).map((part, i, parts) => {
    if (i % 3 === 1) return "";
    if (i % 3 === 2) return `<code>${escape(part.replace(/^ (.*) $/, "$1"))}</code>`;
    return escape(part)
      .replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, (_, label, href) => `<a href="${escape(link(href.replace(/&amp;/g, "&")))}">${label}</a>`)
      .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
  }).join("");
}

/** The book's markdown subset as HTML; `link` maps each href. */
export function toHtml(markdown, { link = href => href, file = () => undefined } = {}) {
  const lines = markdown.split("\n");
  const out = [];
  let fence = 0;
  for (let i = 0; i < lines.length;) {
    const line = lines[i];
    if (!line.trim()) { i++; continue; }
    const open = /^```(.*)$/.exec(line);
    if (open) {
      const body = [];
      for (i++; i < lines.length && lines[i] !== "```"; i++) body.push(lines[i]);
      i++;
      fence += 1;
      const lang = open[1].trim().split(/\s+/)[0];
      const code = `<pre class="code"><code>${escape(body.join("\n"))}</code></pre>`;
      out.push(lang === "xmd" ? `<div class="xmd-block" data-file="${escape(file(fence))}">${code}</div>` : code);
      continue;
    }
    const heading = /^(#{1,6})\s+(.*)$/.exec(line);
    if (heading) {
      const html = inline(heading[2], link), n = heading[1].length;
      out.push(`<h${n} id="${slug(html)}">${html}</h${n}>`);
      i++;
      continue;
    }
    if (line.startsWith("<!--")) {
      while (i < lines.length && !lines[i].includes("-->")) i++;
      i++;
      continue;
    }
    if (line.startsWith("<")) {
      const block = [];
      while (i < lines.length && lines[i].trim()) block.push(lines[i++]);
      out.push(block.join("\n"));
      continue;
    }
    if (line.startsWith("|")) {
      const rows = [];
      while (i < lines.length && lines[i].startsWith("|")) rows.push(lines[i++]);
      const cells = row => row.replace(/^\||\|$/g, "").split(/(?<!\\)\|/).map(c => inline(c.trim().replace(/\\\|/g, "|"), link));
      const [head, , ...body] = rows;
      out.push(`<table><thead><tr>${cells(head).map(c => `<th>${c}</th>`).join("")}</tr></thead><tbody>${
        body.map(r => `<tr>${cells(r).map(c => `<td>${c}</td>`).join("")}</tr>`).join("")}</tbody></table>`);
      continue;
    }
    const item = /^(-|\d+\.)\s+/;
    if (item.test(line)) {
      const tag = line.startsWith("-") ? "ul" : "ol", items = [];
      while (i < lines.length && (item.test(lines[i]) || /^\s{2,}\S/.test(lines[i]))) {
        if (item.test(lines[i])) items.push(lines[i].replace(item, ""));
        else items[items.length - 1] += " " + lines[i].trim();
        i++;
      }
      out.push(`<${tag}>${items.map(t => `<li>${inline(t, link)}</li>`).join("")}</${tag}>`);
      continue;
    }
    const paragraph = [];
    while (i < lines.length && lines[i].trim() && !/^(```|#|<|\||-\s|\d+\.\s)/.test(lines[i])) paragraph.push(lines[i++]);
    out.push(`<p>${inline(paragraph.join("\n"), link)}</p>`);
  }
  return out.join("\n");
}

/** Where a relative href in `path` leads: another page, or GitHub. */
async function resolveLink(href, path, pages, repo) {
  if (/^[a-z]+:|^#/.test(href)) return href;
  const [target, hash] = href.split("#");
  const inRepo = posix.normalize(posix.join("book", posix.dirname(path), target));
  const page = pages.find(p => posix.join("book", p.path) === inRepo);
  if (page) {
    const from = posix.dirname(pageOf(path));
    return posix.relative(from, page.page) + (hash ? `#${hash}` : "");
  }
  const exists = await stat(join(fileURLToPath(repo), inRepo)).catch(() => null);
  if (!exists) throw new Error(`book/${path} links to ${href}, which does not exist`);
  return GITHUB + inRepo + (hash ? `#${hash}` : "");
}

function template(page, pages, html, hasBlocks) {
  const up = "../".repeat(page.page.split("/").length - 1);
  const chapters = pages.filter(p => p.chapter);
  const at = chapters.indexOf(page);
  const pager = at < 0 ? "" : `<nav class="pager">${
    at > 0 ? `<a rel="prev" href="${up}${chapters[at - 1].page}">← ${escape(chapters[at - 1].title)}</a>` : `<a rel="prev" href="${up}index.html">← the book</a>`}${
    at < chapters.length - 1 ? `<a rel="next" href="${up}${chapters[at + 1].page}">${escape(chapters[at + 1].title)} →</a>` : ""}</nav>`;
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${escape(page.title.replace(/^\d+\.\s*/, ""))} · xmd</title>
<link rel="stylesheet" href="${up}../lib/theme/style.css">
<link rel="stylesheet" href="${up}../lib/theme/fonts.css">
<link rel="stylesheet" href="${up}book.css">
<script>try { if (matchMedia("(prefers-color-scheme: light)").matches) document.documentElement.classList.add("xmd-light"); } catch {}</script>
</head>
<body>
<header><a href="${up}index.html">the xmd book</a><a href="${up}../docs/">open the app</a></header>
<main>
${html}
</main>
${pager}
${hasBlocks ? `<script type="module" src="${up}book.js"></script>\n` : ""}</body>
</html>
`;
}

/** Build every page, files.json, and the page script and style into `target`. */
export async function writeBook(source, target, shell, repo) {
  const pages = await readBook(source);
  const files = {};
  for (const page of pages) {
    const blocks = page.chapter ? blocksOf(page) : [];
    for (const { file, source: text } of blocks) {
      if (file in files) throw new Error(`book/${page.path} writes ${file} a second time`);
      files[file] = text;
    }
    const links = new Map();
    for (const [, href] of page.markdown.matchAll(/\]\(([^)\s]+)\)/g)) links.set(href, await resolveLink(href, page.path, pages, repo));
    const byFence = new Map(blocks.map(b => [b.fence, b.file]));
    const html = toHtml(page.markdown, { link: href => links.get(href) ?? href, file: n => byFence.get(n) });
    const out = new URL(page.page, target);
    await mkdir(new URL(".", out), { recursive: true });
    await writeFile(out, template(page, pages, html, blocks.length > 0));
  }
  await writeFile(new URL("files.json", target), JSON.stringify(files, null, 2) + "\n");
  for (const name of ["book.js", "book.css"]) await cp(new URL(name, shell), new URL(name, target));
  return { pages, files };
}
