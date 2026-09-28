import { createWorkspace, render, mount, type Snapshot } from "@wtf/web";
import { mountEditor } from "@wtf/web/contenteditable";

const workspace = createWorkspace({ now: () => new Date().toISOString(), onError: console.error });
await workspace.setDocument("file:///workspace/a.x.md", "a := 1\n");
const html: string = await render("a := 2\n", { now: "2026-09-18T12:00:00Z" });
const view = await mount(document.createElement("div"), {
  workspace, uri: "file:///workspace/a.x.md", layout: "document",
  onChange({ uri, source, version }) { console.log(uri, source, version); },
  onRender(snapshot: Snapshot) { console.log(snapshot.html, snapshot.now); },
});
const editor = await mountEditor(document.createElement("div"), { source: html });
await editor.undo(); await editor.redo();
const selection: { anchor: number; focus: number } | null = editor.selection();
editor.select(0, 3);
await editor.replaceRange(0, 3, "b := 1", { anchor: 0, focus: 6 });
console.log(selection);
view.destroy(); editor.destroy(); workspace.destroy();
