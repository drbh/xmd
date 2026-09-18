// Generate web/dist/book/index.html: prose from chapters.mjs with every example inlined.
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { resolve, dirname } from "node:path";
import { parts } from "./chapters.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const examples = resolve(here, "../../../examples");
const escape = s => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
const slug = s => s.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/(^-|-$)/g, "");

const block = (file, label) => {
  const text = readFileSync(resolve(examples, file), "utf8");
  return `<figure class="wtf-block" data-file="${file}">
  <figcaption><span>${escape(label)}</span><span class="status" aria-live="polite">loading engine…</span></figcaption>
  <div class="editor"><pre class="view" contenteditable="true" spellcheck="false" role="textbox" aria-multiline="true" aria-label="${escape(label)}">${escape(text)}</pre></div>
  <div class="hover" hidden></div>
  <ul class="problems" hidden></ul>
</figure>`;
};

let toc = "", body = "";
parts.forEach((part, p) => {
  const partId = `part-${p + 1}`;
  toc += `<li><a href="#${partId}">${escape(part.title)}</a><ol>`;
  body += `<section class="part" id="${partId}"><p class="kicker">Part ${p + 1}</p><h1>${escape(part.title)}</h1><p class="lead">${part.intro}</p>`;
  part.chapters.forEach((chapter, c) => {
    const id = slug(chapter.title);
    const number = `${p + 1}.${c + 1}`;
    toc += `<li><a href="#${id}">${number} ${escape(chapter.title)}</a></li>`;
    body += `<section class="chapter" id="${id}"><h2><span class="number">${number}</span> ${escape(chapter.title)}</h2>`;
    for (const paragraph of chapter.prose) body += `<p>${paragraph}</p>`;
    body += block(chapter.file, `examples/${chapter.file}`);
    if (chapter.companion) body += block(chapter.companion, `examples/${chapter.companion}`);
    body += `</section>`;
  });
  toc += `</ol></li>`;
  body += `</section>`;
});

const html = `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta name="referrer" content="no-referrer">
  <title>The WTF Book</title>
  <link rel="icon" href="data:,">
  <link rel="preload" href="../lib/theme/fonts/IoskeleyMono-Regular.subset.woff2" as="font" type="font/woff2" crossorigin>
  <link rel="stylesheet" href="../lib/theme/style.css">
  <link rel="stylesheet" href="../lib/theme/fonts.css">
  <link rel="stylesheet" href="./book.css">
  <script type="importmap">{"imports":{"@wtf/web":"../lib/src/index.js","@wtf/web/contenteditable":"../lib/adapters/contenteditable.js"}}</script>
</head>
<body>
  <nav class="toc" aria-label="Table of contents">
    <a class="home" href="#top">The WTF Book</a>
    <ol>${toc}</ol>
    <p class="engine" id="engine">Starting the engine…</p>
  </nav>
  <main id="top">
    <header class="title">
      <h1>The WTF Book</h1>
      <p class="lead">WTF, the written text format, is a language for notes: plain text that stays readable and starts to calculate, count, plan, and remember. This book walks through every feature with a live example. Each block below is a real note running on the same Rust engine as the editor, compiled to WebAssembly; edit it and the inlays, colors, and diagnostics update as you type. Click a name to see its hover.</p>
      <p>The examples are the files in <code>examples/</code>. Open them in Zed for the full experience, including completion, rename, code actions, and the command line.</p>
    </header>
    ${body}
    <footer><p>Generated from <code>examples/</code> by <code>node web/apps/book/build.mjs</code>. Runs on <code>web/src/worker.js</code>.</p></footer>
  </main>
  <script type="module" src="./book.js"></script>
</body>
</html>
`;
const output = resolve(here, "../../dist/book");
mkdirSync(output, { recursive: true });
writeFileSync(resolve(output, "index.html"), html);
console.log("web/dist/book/index.html written");
