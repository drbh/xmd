// Turns every ```xmd block on a book page into a live editor. All the book's
// notes load into one workspace first (files.json), the same workspace
// hosts/cli/tests/book.rs checks, so a chapter can use a note an earlier one wrote.
// Edits are never saved. `?now=<rfc3339>` freezes the clock, as the tests do.
// A block's data-off (```` ```xmd off=… ````) turns features off: book.css hides
// highlight and results, and a block without controls mounts non-interactive.
import { createWorkspace } from "../lib/src/index.js";
import { mountEditor } from "../lib/adapters/contenteditable.js";

const now = new URLSearchParams(location.search).get("now") ?? undefined;
const uriOf = file => `file:///workspace/book/${file}`;
// `?fresh` tells the site's service worker, which pins the app to its build,
// to give this page's engine the files deployed beside it.
const workerFactory = () => new Worker(new URL("../lib/src/worker.js?fresh", import.meta.url), { type: "module" });
const workspace = createWorkspace({ now, workerFactory });
const files = await (await fetch(new URL("files.json", import.meta.url))).json();
for (const [file, source] of Object.entries(files)) await workspace.setDocument(uriOf(file), source);
// A module block marked active=chapter is on for every note on this page, and
// editing it recompiles them. While the edit does not compile, the last
// version that did stays on.
const active = [...document.querySelectorAll(".xmd-block[data-active]")].map(block => block.dataset.file);
const modules = Object.fromEntries(active.map(file => [file, files[file]]));
if (active.length) await workspace.setModules(modules);
workspace.onChange(({ uri, source }) => {
  const file = active.find(f => uriOf(f) === uri);
  if (!file) return;
  const next = { ...modules, [file]: source };
  workspace.setModules(next).then(() => Object.assign(modules, next), () => {});
});

const views = {};
for (const block of document.querySelectorAll(".xmd-block")) {
  const file = block.dataset.file;
  // The editor mounts in place of the static code, already in the page.
  const host = document.createElement("div");
  host.className = "xmd-live";
  block.replaceChildren(host);
  const off = (block.dataset.off ?? "").split(" ");
  views[file] = await mountEditor(host, { workspace, uri: uriOf(file), interactive: !off.includes("controls") });
  const reset = document.createElement("button");
  reset.type = "button";
  reset.className = "reset";
  reset.textContent = "Reset";
  reset.title = "Put the example back the way the book wrote it";
  reset.addEventListener("click", () => views[file].setSource(files[file]));
  block.append(reset);
}
// Terminals open a note the page already wrote, beside the commands run on it.
const terminals = {};
if (document.querySelector(".xmd-terminal")) {
  const { mountTerminal } = await import("./terminal.js");
  for (const block of document.querySelectorAll(".xmd-terminal")) {
    terminals[block.dataset.file] = await mountTerminal(block, { workspace, uriOf, mountEditor, files });
  }
}
// For the browser tests.
window.xmdBook = { ready: true, workspace, views, terminals, files, uriOf };
