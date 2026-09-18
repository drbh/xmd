// Every example is a document in the same public workspace.
import { createWorkspace } from "@wtf/web";
import { mountEditor } from "@wtf/web/contenteditable";

const engine = document.getElementById("engine");
const fail = error => { engine.textContent = `Engine failed: ${error.message}`; };
const workspace = createWorkspace({ onError: fail });
const blocks = [...document.querySelectorAll(".wtf-block")];
for (const block of blocks) {
  block.uri = `file:///workspace/book/${block.dataset.file}`;
  await workspace.setDocument(block.uri, block.querySelector(".view").textContent);
}
await Promise.all(blocks.map(async block => {
  const list = block.querySelector(".problems"), status = block.querySelector(".status");
  block.editor = await mountEditor(block.querySelector(".view"), {
    workspace, uri: block.uri, hover: block.querySelector(".hover"),
    onRender(snapshot) {
      list.replaceChildren(...snapshot.diagnostics.map(d => {
        const item = document.createElement("li");
        item.className = d.severity === 2 ? "warn" : "error";
        item.textContent = `Line ${d.range.start.line + 1}: ${d.message}`;
        return item;
      }));
      list.hidden = !snapshot.diagnostics.length;
      status.textContent = snapshot.live ? "live" : "";
    },
    onError: e => { status.textContent = e.message; },
  });
  block.setText = source => block.editor.setSource(source);
}));
engine.textContent = "Rust / WebAssembly · running in this page";
window.addEventListener("pagehide", () => { for (const block of blocks) block.editor.destroy(); workspace.destroy(); });
if (new URLSearchParams(location.search).has("test")) window.wtfBook = { blocks, rpc: workspace.request, workspace, ready: true };
