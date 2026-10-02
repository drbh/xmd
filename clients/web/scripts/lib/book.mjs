// The book in /book, built into static pages at dist/book/. The pages are the
// same markdown GitHub shows; this turns the small subset the book uses into
// HTML, and a ```xmd block becomes a live editor (clients/web/book/book.js).
//
// Every ```xmd block of every chapter goes into files.json, keyed by the file
// hosts/cli/tests/book.rs writes it to, so the browser evaluates the same workspace
// the tests check: `trip.x.md` from chapter 1 is there for chapter 2.
//
// After the language, a fence's info string may name the file and switch
// features off for that block: ```` ```xmd off=highlight,results,controls ````.
// ```` ```xmd birds.xmd active=chapter ```` turns a module on for every note on
// its page, as hosts/cli/tests/book.rs does for its chapter. A word with `=` is
// an option, never the file.
//
// The book is the site's front page: why xmd exists (HOME) is built as the
// site root, and every other page under book/. A relative link to another
// page becomes that page's address; a link to anything else in the
// repository goes to GitHub. A link to nothing fails the build.
import { cp, mkdir, readFile, readdir, stat, writeFile } from "node:fs/promises";
import { join, posix } from "node:path";
import { fileURLToPath } from "node:url";

const GITHUB = "https://github.com/drbh/xmd/blob/main/";
// The docs app's saved preferences (apps/docs/src/lib/store.js). The book
// shares their theme, so a choice made in either is the other's too.
const PREFS = "xmd.docs.prefs.v1";
/** The page built as the site root. */
export const HOME = "01-blog.md";

/** A page's address from the site root: HOME is `index.html`, and
 * `02-language.md` is `book/language.html`. */
export const pageOf = path => {
  if (path === HOME) return "index.html";
  const dir = posix.dirname(path), name = posix.basename(path);
  const stem = name === "README.md" ? "index" : name.replace(/^\d+-/, "").replace(/\.md$/, "");
  return dir === "." ? `book/${stem}.html` : `book/${dir}/${stem}.html`;
};

/** The relative href from page `from` to the site address `to`; an index is
 * its directory, so the root is `./` or `../`. */
const hrefTo = (from, to) => {
  const href = posix.relative(posix.dirname(from), to);
  return to.endsWith("index.html") && posix.basename(href) === "index.html" ? (posix.dirname(href) === "." ? "./" : `${posix.dirname(href)}/`) : href;
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

/** What a block can turn off: the colors, the inline results, and the
 * lenses and clickable checkboxes. */
export const FEATURES = ["highlight", "results", "controls"];

/** A fence's info string as its language, file, the features it turns off,
 * and whether it is a module the chapter activates. */
export function infoOf(info) {
  const [lang, ...words] = info.trim().split(/\s+/);
  for (const word of words) if (word.includes("=") && !/^off=|^active=chapter$/.test(word)) throw new Error(`\`${word}\` is not an option; use off=… or active=chapter`);
  const off = words.find(w => w.startsWith("off="))?.slice(4).split(",").filter(Boolean) ?? [];
  for (const name of off) if (!FEATURES.includes(name)) throw new Error(`\`off=${name}\` is not one of ${FEATURES.join(", ")}`);
  return { lang, file: words.find(w => !w.includes("=")), off, active: words.includes("active=chapter") };
}

/** Each ```xmd block, by its position among the page's fences, with the
 * file hosts/cli/tests/book.rs writes it to. */
export function blocksOf(page) {
  const blocks = [];
  let fence = 0;
  for (const [, info, source] of page.markdown.matchAll(/^```([^\n]*)\n([\s\S]*?)^```$/gm)) {
    fence += 1;
    const { lang, file, active } = infoOf(info);
    if (lang === "xmd") blocks.push({ fence, file: file ?? `${page.path.replace(/\.md$/, "")}-${fence}.x.md`, source, ...(active && { active }) });
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

/** The book's markdown subset as HTML; `link` maps each href. An xmd fence
 * with a `file` is a live note; one `rendered` gives HTML for is shown as
 * that, highlighted and still; any other is plain code. */
export function toHtml(markdown, { link = href => href, file = () => undefined, rendered = () => undefined } = {}) {
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
      const { lang, off, active } = infoOf(open[1]);
      const code = `<pre class="code"><code>${escape(body.join("\n"))}</code></pre>`;
      const attrs = (off.length ? ` data-off="${escape(off.join(" "))}"` : "") + (active ? " data-active" : "");
      const live = lang === "xmd" && file(fence) !== undefined;
      const still = lang === "xmd" && !live ? rendered(fence) : undefined;
      // A terminal: a note and the commands run beside it, typed live by book.js.
      const term = lang === "terminal" ? infoOf(open[1]).file : undefined;
      out.push(live ? `<div class="xmd-block" data-file="${escape(file(fence))}"${attrs}>${code}</div>`
        : term ? `<div class="xmd-terminal" data-file="${escape(term)}">${code}</div>`
        : still ? `<div class="xmd-static">${still}</div>` : code);
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
      out.push(`<div class="table"><table><thead><tr>${cells(head).map(c => `<th>${c}</th>`).join("")}</tr></thead><tbody>${
        body.map(r => `<tr>${cells(r).map(c => `<td>${c}</td>`).join("")}</tr>`).join("")}</tbody></table></div>`);
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
    return hrefTo(pageOf(path), page.page) + (hash ? `#${hash}` : "");
  }
  const exists = await stat(join(fileURLToPath(repo), inRepo)).catch(() => null);
  if (!exists) throw new Error(`book/${path} links to ${href}, which does not exist`);
  return GITHUB + inRepo + (hash ? `#${hash}` : "");
}

/** The contents in the margin: the book's index, its chapters, then the
 * reference, with the sections of the page being read under it. */
function contents(page, pages, html) {
  const sections = [...html.matchAll(/<h2 id="([^"]+)">([\s\S]*?)<\/h2>/g)]
    .map(([, id, text]) => `<li><a href="#${id}">${text.replace(/<[^>]+>/g, "")}</a></li>`);
  const item = p => {
    const title = escape(p.path === "README.md" ? "the book" : p.title.replace(/^\d+\.\s*/, ""));
    const here = p === page;
    return `<li><a href="${hrefTo(page.page, p.page)}"${here ? ' aria-current="page"' : ""}>${title}</a>${
      here && sections.length && p.path !== "README.md" ? `<ol>${sections.join("")}</ol>` : ""}</li>`;
  };
  const reference = pages.filter(p => p.path.startsWith("reference/"));
  return `<nav class="contents" aria-label="contents"><ol>${
    pages.filter(p => !p.path.startsWith("reference/")).map(item).join("")}</ol><p>reference</p><ol>${
    reference.map(item).join("")}</ol></nav>`;
}

/** Opens and closes the contents, marks the section being read in them and
 * gives each heading a link to itself. Small enough to inline; pages without
 * notes load no script otherwise. On a wide screen the contents sit beside
 * the text and the choice is remembered (the head script restores it before
 * the first paint); on a phone they cover the page, so following a link,
 * tapping outside or Escape puts them away. */
const pageScript = `
const root = document.documentElement, toggle = document.querySelector(".topbar .toggle");
const sidebar = document.querySelector(".sidebar"), wide = matchMedia("(min-width: 1024px)");
const isOpen = () => root.classList.contains("contents-open");
const setOpen = (open, remember) => {
  root.classList.toggle("contents-open", open);
  toggle.setAttribute("aria-expanded", String(open));
  if (remember && wide.matches) try { localStorage.setItem("xmd-book-contents", open ? "open" : "closed"); } catch {}
};
setOpen(isOpen(), false);
toggle.addEventListener("click", () => setOpen(!isOpen(), true));
wide.addEventListener("change", () => {
  let saved = null;
  try { saved = localStorage.getItem("xmd-book-contents"); } catch {}
  setOpen(wide.matches && saved === "open", false);
});
document.addEventListener("click", event => {
  if (wide.matches || !isOpen() || event.target.closest(".toggle")) return;
  if (!sidebar.contains(event.target) || event.target.closest("a")) setOpen(false, false);
});
addEventListener("keydown", event => {
  if (event.key === "Escape" && !wide.matches && isOpen()) { setOpen(false, false); toggle.focus(); }
});
// The theme: the button saves it into the docs app's preferences; without a
// saved one the page follows the system.
const setTheme = theme => { root.dataset.theme = theme; root.classList.toggle("xmd-light", theme === "light"); };
const saved = () => { try { return JSON.parse(localStorage.getItem("${PREFS}") || "{}"); } catch { return {}; } };
document.querySelector(".topbar .theme").addEventListener("click", () => {
  const theme = root.classList.contains("xmd-light") ? "dark" : "light";
  setTheme(theme);
  try { localStorage.setItem("${PREFS}", JSON.stringify({ ...saved(), theme })); } catch {}
});
matchMedia("(prefers-color-scheme: light)").addEventListener("change", event => {
  if (!["light", "dark"].includes(saved().theme)) setTheme(event.matches ? "light" : "dark");
});
for (const h of document.querySelectorAll("main :is(h2, h3)[id]")) {
  const a = document.createElement("a");
  a.className = "anchor"; a.href = "#" + h.id; a.textContent = "#";
  a.setAttribute("aria-label", "Link to this section");
  h.append(a);
}
const links = new Map([...document.querySelectorAll(".contents ol ol a")].map(a => [a.hash.slice(1), a]));
const heads = [...document.querySelectorAll("main h2[id]")].filter(h => links.has(h.id));
let queued = false;
const mark = () => {
  queued = false;
  let current = heads[0];
  for (const h of heads) if (h.getBoundingClientRect().top < 120) current = h;
  for (const [id, a] of links) a.classList.toggle("here", id === current?.id);
};
if (heads.length) {
  addEventListener("scroll", () => { if (!queued) { queued = true; requestAnimationFrame(mark); } }, { passive: true });
  mark();
}
`;

/** A page's first paragraph as plain text, for search results and link previews. */
function describe(html) {
  const text = (/<p>([\s\S]*?)<\/p>/.exec(html)?.[1] ?? "").replace(/<[^>]+>/g, "").replace(/\s+/g, " ").trim();
  return text.length > 160 ? text.slice(0, 157).replace(/\s+\S*$/, "") + "…" : text;
}

// The docs app's sun and moon (apps/docs/src/lib/Icon.svelte).
const icon = (name, d) => `<svg class="${name}" viewBox="0 0 24 24" width="20" height="20" aria-hidden="true"><path d="${d}" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg>`;
const SUN = icon("sun", "M12 3v2M12 19v2M3 12h2M19 12h2M5.6 5.6l1.4 1.4M17 17l1.4 1.4M5.6 18.4 7 17M17 7l1.4-1.4M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8z");
const MOON = icon("moon", "M20 14A8 8 0 0 1 10 4a8 8 0 1 0 10 10z");
const MENU = '<svg viewBox="0 0 16 16" width="18" height="18" aria-hidden="true"><path d="M2 4h12M2 8h12M2 12h12" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>';
// The app's own icon, a page with its corner folded, drawn in one color.
const APP_MARK = '<svg viewBox="0 0 16 16" width="20" height="20" aria-hidden="true"><g fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" stroke-linecap="round"><path d="M3.5 1.5h6l3 3v10h-9z"/><path d="M9.5 1.5v3h3"/><path d="M6 8.5h4M6 11h4"/></g></svg>';
// GitHub's mark (Octicons, MIT).
const GITHUB_MARK = '<svg viewBox="0 0 16 16" width="20" height="20" aria-hidden="true"><path fill="currentColor" d="M8 0c4.42 0 8 3.58 8 8a8.013 8.013 0 0 1-5.45 7.59c-.4.08-.55-.17-.55-.38 0-.27.01-1.13.01-2.2 0-.75-.25-1.23-.54-1.48 1.78-.2 3.65-.88 3.65-3.95 0-.88-.31-1.59-.82-2.15.08-.2.36-1.02-.08-2.12 0 0-.67-.22-2.2.82-.64-.18-1.32-.27-2-.27-.68 0-1.36.09-2 .27-1.53-1.03-2.2-.82-2.2-.82-.44 1.1-.16 1.92-.08 2.12-.51.56-.82 1.28-.82 2.15 0 3.06 1.86 3.75 3.64 3.95-.23.2-.44.55-.51 1.07-.46.21-1.61.55-2.33-.66-.15-.24-.6-.83-1.23-.82-.67.01-.27.38.01.53.34.19.73.9.82 1.13.16.45.68 1.31 2.69.94 0 .67.01 1.3.01 1.49 0 .21-.15.45-.55.38A7.995 7.995 0 0 1 0 8c0-4.42 3.58-8 8-8Z"/></svg>';

function template(page, pages, html, hasBlocks) {
  // The way back to the site root, where lib/, docs/ and book/ sit.
  const up = "../".repeat(page.page.split("/").length - 1);
  const title = page.path === "README.md" ? "the xmd book" : `${page.title.replace(/^\d+\.\s*/, "")} · xmd`;
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${escape(title)}</title>
<meta name="description" content="${escape(describe(html))}">
<link rel="icon" href="${up}docs/icon.svg" type="image/svg+xml">
<link rel="stylesheet" href="${up}lib/theme/style.css">
<link rel="stylesheet" href="${up}lib/theme/fonts.css">
<link rel="stylesheet" href="${up}book/book.css">
<script>{
  const root = document.documentElement;
  let theme;
  try { theme = JSON.parse(localStorage.getItem("${PREFS}") || "{}").theme; } catch {}
  if (theme !== "light" && theme !== "dark") theme = matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
  root.dataset.theme = theme;
  root.classList.toggle("xmd-light", theme === "light");
  try { if (matchMedia("(min-width: 1024px)").matches && localStorage.getItem("xmd-book-contents") === "open") root.classList.add("contents-open"); } catch {}
}</script>
</head>
<body>
<header class="topbar">
<button class="toggle" type="button" aria-label="contents" aria-controls="contents" aria-expanded="false">${MENU}</button>
<a class="brand" href="${up || "./"}">xmd</a>
<nav class="links" aria-label="site"><button class="theme" type="button" aria-label="switch theme" title="switch theme">${SUN}${MOON}</button><a href="${up}docs/" aria-label="open the app" title="open the app">${APP_MARK}</a><a href="https://github.com/drbh/xmd" aria-label="xmd on GitHub" title="xmd on GitHub">${GITHUB_MARK}</a></nav>
</header>
<aside class="sidebar" id="contents">
${contents(page, pages, html)}
</aside>
<main>
${html}
</main>
<footer><a href="https://github.com/drbh/xmd">github</a><span>MIT license</span></footer>
<script type="module">${pageScript}</script>
${hasBlocks ? `<script type="module" src="${up}book/book.js"></script>\n` : ""}</body>
</html>
`;
}

/** Each ```xmd fence of a page with no live notes, like the reference, as the
 * engine highlights it at build time: its colors, without the results a note
 * would show inline, so the page needs no engine. */
async function highlighted(page) {
  const { render } = await import("../../src/node.js");
  const out = new Map();
  let fence = 0;
  for (const [, info, source] of page.markdown.matchAll(/^```([^\n]*)\n([\s\S]*?)^```$/gm)) {
    fence += 1;
    if (infoOf(info).lang !== "xmd") continue;
    // A module is a library file, and reads as one: as a note it would be
    // told to rename itself .xmd.
    const uri = /^module := /m.test(source) ? "file:///workspace/example.xmd" : "file:///workspace/example.x.md";
    const html = await render(source.replace(/\n$/, ""), { uri, now: "2026-01-01T00:00:00Z" });
    out.set(fence, html.replace(/<span class="inlay"[^>]*>[^<]*<\/span>/g, ""));
  }
  return out;
}

/** Build every page into the site at `site`, and files.json, the page script
 * and the style into its book/. */
export async function writeBook(source, site, shell, repo) {
  const target = new URL("book/", site);
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
    for (const [, note] of page.markdown.matchAll(/^```terminal (\S+)/gm)) {
      if (!blocks.some(b => b.file === note)) throw new Error(`book/${page.path}: a terminal opens ${note}, which no block on the page writes`);
    }
    const still = page.chapter ? new Map() : await highlighted(page);
    const html = toHtml(page.markdown, { link: href => links.get(href) ?? href, file: n => byFence.get(n), rendered: n => still.get(n) });
    const out = new URL(page.page, site);
    await mkdir(new URL(".", out), { recursive: true });
    await writeFile(out, template(page, pages, html, blocks.length > 0));
  }
  await writeFile(new URL("files.json", target), JSON.stringify(files, null, 2) + "\n");
  for (const name of ["book.js", "book.css", "terminal.js"]) await cp(new URL(name, shell), new URL(name, target));
  return { pages, files };
}
