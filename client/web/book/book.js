// Turns every ```xmd block on a book page into a live editor. All the book's
// notes load into one workspace first (files.json), the same workspace
// cli/tests/book.rs checks, so a chapter can use a note an earlier one wrote.
// Edits are never saved. `?now=<rfc3339>` freezes the clock, as the tests do.
import { createWorkspace } from "../lib/src/index.js";
import { mountEditor } from "../lib/adapters/contenteditable.js";

const now = new URLSearchParams(location.search).get("now") ?? undefined;
const uriOf = file => `file:///workspace/book/${file}`;
const workspace = createWorkspace({ now });
const files = await (await fetch(new URL("files.json", import.meta.url))).json();
for (const [file, source] of Object.entries(files)) await workspace.setDocument(uriOf(file), source);

const views = {};
for (const block of document.querySelectorAll(".xmd-block")) {
  const file = block.dataset.file;
  // The editor mounts in place of the static code, already in the page.
  const host = document.createElement("div");
  host.className = "xmd-live";
  block.replaceChildren(host);
  views[file] = await mountEditor(host, { workspace, uri: uriOf(file) });
  const reset = document.createElement("button");
  reset.type = "button";
  reset.className = "reset";
  reset.textContent = "Reset";
  reset.title = "Put the example back the way the book wrote it";
  reset.addEventListener("click", () => views[file].setSource(files[file]));
  block.append(reset);
}
// For the browser tests.
window.xmdBook = { ready: true, workspace, views, files, uriOf };
