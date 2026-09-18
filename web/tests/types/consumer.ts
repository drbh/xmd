import { createWorkspace, render, mount, type Snapshot } from "@wtf/web";
import { mountEditor } from "@wtf/web/contenteditable";

const workspace = createWorkspace({ now: () => new Date().toISOString(), onError: console.error });
await workspace.setDocument("file:///workspace/a.wtf", "a := 1\n");
const html: string = await render("a := 2\n", { now: "2026-09-18T12:00:00Z" });
const view = await mount(document.createElement("div"), {
  workspace, uri: "file:///workspace/a.wtf", layout: "document",
  onChange({ uri, source, version }) { console.log(uri, source, version); },
  onRender(snapshot: Snapshot) { console.log(snapshot.html, snapshot.now); },
});
const editor = await mountEditor(document.createElement("div"), { source: html });
await editor.undo(); await editor.redo();
view.destroy(); editor.destroy(); workspace.destroy();
