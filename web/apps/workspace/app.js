import { createOutline } from "./outline.js";
import { createWorkspace } from "@wtf/web";

const $ = id => document.getElementById(id);
const STORAGE_KEY = "wtf.browser.workspace.v1";
const initialNotes = [
  { name: "trip.wtf", text: "# Trip\n\nOur budget is [$3,000]:budget.\n\nWe've spent [$2,444]:spent.\n\n[remaining] := budget - spent\n\nWe have [remaining] remaining.\n\n<!-- Hover a value for its calculation. Edit spending to watch both hints update. -->\n" },
  { name: "today.wtf", text: "# Today :today_tasks\n\n[focus] := countdown(25m)\n[debugging] := stopwatch()\n\n- [ ] Investigate a flaky test :investigate @timer(debugging) @estimate(30m)\n- [ ] Review the fix @after(investigate) @timer(focus) @due(tomorrow)\n\nTime left: [focus.remaining].\n[progress] := completed(today_tasks) / total(today_tasks)\nProgress: [progress].\n\n<!-- Import a note explicitly to read its values. -->\nTrip money left: [trip.remaining].\n\n[https://github.com/zed-industries/zed]:editor_source\nEditor source: [editor_source].\ntrip := import(\"./trip.wtf\")\n" },
];
function notice(message) { $("notice").textContent = String(message); $("notice").hidden = false; }
window.addEventListener("unhandledrejection", event => notice(event.reason?.message || event.reason));

async function boot() {
  if (location.protocol === "file:") throw new Error("Serve this folder over HTTP, for example: node web/serve.mjs");
  window.MonacoEnvironment = { getWorker: () => new Worker(new URL("./monaco-worker.js", import.meta.url), { type: "module" }) };
  const { monaco, createEditor } = await import("@wtf/web/monaco");
  const error = e => notice(e?.message || e);
  const workspace = createWorkspace({ onError: error });
  const rpc = workspace.request;
  const legend = await rpc("semanticLegend", {});
  const notes = new Map();
  let active, refreshTimer, saveTimer, storageWritable = true, unsubscribe;
  const query = (model, method, params = {}) => workspace.query(model.uri.toString(), method, params);
  const analyze = (model, force = false) => workspace.analyze(model.uri.toString(), { force });
  const modelVersion = target => {
    const model = notes.get(monaco.Uri.parse(target.uri).toString())?.model;
    const doc = workspace.getDocument(target.uri);
    if (!model || !doc || doc.version !== target.version || doc.source !== model.getValue()) throw new Error("Note changed; request fresh controls.");
    return model.getVersionId();
  };
  const ui = createEditor($("editor"), { legend, query, analyze, modelVersion, error, open: openResource, execute: async (command, versions) => {
    await workspace.settled();
    // Monaco can retain a same-title lens briefly after edits/undo. Revalidate its
    // exact target and action, never just its title, against the current workspace.
    const model = notes.get(monaco.Uri.parse(command.arguments[0]).toString())?.model;
    if (!model) throw new Error("The action's note is no longer open.");
    const current = await analyze(model, true);
    if (!current?.lenses.some(lens => JSON.stringify(lens.command) === JSON.stringify(command))) throw new Error("Source changed; request fresh controls.");
    versions = current.versions;
    return workspace.execute(command, versions, { apply: false });
  } });
  const { editor } = ui;
  const outline = createOutline(editor, { list: $("outline"), filter: $("outline-filter"), empty: $("outline-empty") });
  function sync(model) {
    workspace.setDocument(model.uri.toString(), model.getValue()).catch(error);
  }
  function refresh(force = false) {
    clearTimeout(refreshTimer);
    if (active) return analyze(active.model, force).catch(error);
  }
  function nameIsValid(name) {
    return typeof name === "string" && name.endsWith(".wtf") && !name.startsWith("/") && !name.includes("\\") && !name.includes("\0")
      && name.split("/").every(part => part && part !== "." && part !== "..");
  }
  function uniqueName(name) {
    let result = name, n = 2;
    while ([...notes.values()].some(note => note.name === result)) result = name.replace(/\.wtf$/, `-${n++}.wtf`);
    return result;
  }
  function addNote(name, text) {
    if (!nameIsValid(name)) throw new Error("Use a relative .wtf filename without '..' or backslashes.");
    if (new TextEncoder().encode(text).length > 1_000_000) throw new Error("Notes are limited to 1 MB in the browser.");
    name = uniqueName(name);
    const uri = monaco.Uri.file(`/workspace/${name}`);
    const model = monaco.editor.createModel(text, ui.language, uri);
    // Models are created separately from the editor; suppress their automatic
    // bracket rainbow so Rust's punctuation and inert-code colors stay intact.
    model.updateOptions({ bracketColorizationOptions: { enabled: false, independentColorPoolPerBracketType: false } });
    const note = { name, model, viewState: null };
    notes.set(uri.toString(), note);
    sync(model);
    model.onDidChangeContent(() => {
      sync(model);
      $("save-status").textContent = storageWritable ? "Saving locally…" : "Local saving unavailable — download a backup";
      clearTimeout(saveTimer); saveTimer = setTimeout(save, 300);
      clearTimeout(refreshTimer); refreshTimer = setTimeout(refresh, 100);
    });
    return note;
  }
  function choose(note, selection) {
    if (active) active.viewState = editor.saveViewState();
    unsubscribe?.();
    active = note;
    unsubscribe = workspace.subscribe(note.model.uri.toString(), snapshot => {
      if (snapshot.source !== note.model.getValue()) return;
      ui.publish(note.model, snapshot);
      updateStatus(snapshot);
    });
    editor.setModel(note.model);
    outline.reset(note.model);
    if (note.viewState) editor.restoreViewState(note.viewState);
    if (selection) {
      if (selection.startLineNumber !== undefined) { editor.setSelection(selection); editor.revealRangeInCenter(selection); }
      else { editor.setPosition(selection); editor.revealPositionInCenter(selection); }
    }
    $("filename").textContent = note.name;
    renderNotes(); refresh(true); editor.focus();
  }
  function renderNotes() {
    $("notes").replaceChildren();
    for (const note of notes.values()) {
      const button = document.createElement("button");
      button.textContent = note.name;
      if (note === active) button.setAttribute("aria-current", "page");
      button.onclick = () => { choose(note); save(); };
      $("notes").append(button);
    }
  }
  function save() {
    if (!storageWritable) return;
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify({ active: active?.name, notes: [...notes.values()].map(n => ({ name: n.name, text: n.model.getValue() })) }));
      $("save-status").textContent = "Saved in this browser";
    } catch { $("save-status").textContent = "Not saved — download a backup"; notice("Browser storage is unavailable or full. Download your notes before closing this tab."); }
  }
  function updateStatus(snapshot) {
    outline.update(active.model, snapshot);
    $("engine").textContent = "Rust / WebAssembly · local";
    $("language-status").textContent = `${snapshot.hints.length} inlays · ${snapshot.diagnostics.length} problems${snapshot.live ? " · live" : ""}`;
    $("problems").hidden = snapshot.diagnostics.length === 0;
    $("problems").replaceChildren();
    for (const diagnostic of snapshot.diagnostics) {
      const button = document.createElement("button");
      button.textContent = `${diagnostic.range.start.line + 1}: ${diagnostic.message}`;
      button.onclick = () => { const p = { lineNumber: diagnostic.range.start.line + 1, column: diagnostic.range.start.character + 1 }; editor.setPosition(p); editor.revealPositionInCenter(p); editor.focus(); };
      $("problems").append(button);
    }
  }
  function openResource(target, selection) {
    let url;
    try { url = new URL(target); } catch { notice("Unsupported resource URL."); return true; }
    if (url.protocol === "file:") {
      const line = /^#L(\d+)$/.exec(url.hash);
      url.hash = "";
      const key = monaco.Uri.parse(url.href).toString();
      const note = notes.get(key);
      if (!note) { notice("This file is not in the browser workspace. Import linked .wtf files first; local image files are not supported yet."); return true; }
      choose(note, selection || (line && { lineNumber: Number(line[1]), column: 1 }));
      return true;
    }
    if (["https:", "http:"].includes(url.protocol)) { window.open(url.href, "_blank", "noopener,noreferrer"); return true; }
    notice("Only imported notes and HTTP(S) resources can be opened."); return true;
  }
  let restored = null;
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) {
      restored = JSON.parse(raw);
      if (!Array.isArray(restored.notes) || !restored.notes.length || restored.notes.some(n => !nameIsValid(n.name) || typeof n.text !== "string")) throw new Error("Invalid saved workspace");
    }
  } catch { restored = null; storageWritable = false; notice("Saved workspace could not be read. Original storage has been preserved; autosaving is disabled. Download any new work before leaving."); }
  for (const note of restored?.notes || initialNotes) addNote(note.name, note.text);
  choose([...notes.values()].find(n => n.name === restored?.active) || notes.values().next().value);
  await workspace.settled();
  await refresh(true);
  if (storageWritable) save();
  else $("save-status").textContent = "Local saving unavailable — download a backup";

  $("new-note").onclick = () => {
    const input = prompt("Note filename", "untitled.wtf");
    if (input === null || !input.trim()) return;
    try { choose(addNote(input.endsWith(".wtf") ? input : `${input}.wtf`, "# New note\n\n")); save(); } catch (e) { error(e); }
  };
  $("import-notes").onclick = () => $("file-input").click();
  async function importFiles(files) {
    let last;
    for (const file of files) {
      try { if (file.size > 1_000_000) throw new Error(`${file.name} exceeds the 1 MB limit.`); last = addNote(file.webkitRelativePath || file.name, await file.text()); }
      catch (e) { error(e); }
    }
    if (last) { choose(last); save(); }
  }
  $("file-input").onchange = async event => { await importFiles(event.target.files); event.target.value = ""; };
  window.addEventListener("dragover", event => event.preventDefault());
  window.addEventListener("drop", event => { event.preventDefault(); importFiles(event.dataTransfer.files); });
  $("download").onclick = () => {
    const url = URL.createObjectURL(new Blob([active.model.getValue()], { type: "text/plain;charset=utf-8" }));
    const link = document.createElement("a"); link.href = url; link.download = active.name.split("/").pop(); link.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  };
  window.addEventListener("pagehide", save);
  window.addEventListener("storage", event => {
    if (event.key === STORAGE_KEY) {
      storageWritable = false;
      clearTimeout(saveTimer);
      $("save-status").textContent = "Another tab changed this workspace — saving paused";
      notice("Another tab changed the saved workspace. Download your changes, then reload to see its notes. Autosaving is paused to prevent overwriting either tab's work.");
    }
  });
  document.addEventListener("visibilitychange", () => { if (!document.hidden) refresh(true); else save(); });
  window.addEventListener("pagehide", () => { unsubscribe?.(); ui.destroy(); workspace.destroy(); for (const note of notes.values()) note.model.dispose(); });
  // Opt-in test harness; the normal page does not expose editor/worker internals globally.
  if (new URLSearchParams(location.search).has("test")) window.wtfTest = { editor, monaco, notes, query, analyze, rpc, ui, workspace, choose, ready: true };
}

boot().catch(error => {
  $("engine").textContent = "Could not start";
  notice(`Could not start WTF: ${error.message}. Run bash web/build.sh, serve web/ over HTTP, and check CDN access.`);
});
