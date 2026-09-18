// Optional source editing. The core view owns rendering and controls; the
// workspace owns source. This adapter owns selection, composition and history.
import { mount } from "../src/view.js";
import { textOf, selectionOf, restoreSelection, setCaret, caretOffset, lineChar } from "../src/dom.js";
import { applyTextEdits, indexOf } from "../src/workspace.js";

export async function mountEditor(element, options = {}) {
  const undo = [], redo = [];
  let lastSource, lastSelection, replaying = false, composing = false, compositionVersion;
  const view = await mount(element, {
    ...options,
    onChange(change) {
      if (lastSource !== undefined && change.source !== lastSource && !replaying) {
        undo.push({ source: lastSource, selection: lastSelection });
        if (undo.length > 200) undo.shift();
        redo.length = 0;
      }
      lastSource = change.source;
      options.onChange?.(change);
    },
  });
  const target = view.element;
  target.contentEditable = "true";
  target.spellcheck = false;
  target.setAttribute("role", "textbox");
  target.setAttribute("aria-multiline", "true");
  const abort = new AbortController();
  const listen = (type, fn) => target.addEventListener(type, fn, { signal: abort.signal });
  const error = e => { if (!view.destroyed) options.onError?.(e); };
  lastSource = view.getSource();
  function commit() { return view.setSource(textOf(target)); }
  async function history(from, to) {
    if (view.destroyed || composing || !from.length) return;
    await view.workspace.settled();
    const previous = from.pop();
    to.push({ source: view.getSource(), selection: selectionOf(target) });
    replaying = true;
    try { await view.setSource(previous.source); await view.refresh(); restoreSelection(target, previous.selection); }
    finally { replaying = false; }
  }
  async function format(ch) {
    if (view.destroyed || composing) return;
    const source = view.getSource(), at = caretOffset(target) ?? source.length;
    const version = view.workspace.getDocument(view.uri).version;
    const edits = await view.workspace.query(view.uri, "onTypeFormatting", { position: lineChar(source, at), ch });
    if (!edits?.length || view.destroyed || version !== view.workspace.getDocument(view.uri)?.version) return;
    let caret = at;
    for (const edit of edits) {
      const start = indexOf(source, edit.range.start), end = indexOf(source, edit.range.end);
      if (start <= at) caret += edit.newText.length - (Math.min(end, at) - start);
    }
    await view.setSource(applyTextEdits(source, edits));
    await view.refresh();
    if (!view.destroyed) setCaret(target, caret);
  }
  function insertAtCaret(text) {
    if (view.destroyed) return Promise.reject(new Error("View was destroyed"));
    target.focus();
    lastSelection = selectionOf(target);
    if (!lastSelection) setCaret(target, view.getSource().length);
    const selection = target.ownerDocument.getSelection();
    const range = selection.getRangeAt(0);
    range.deleteContents();
    const node = target.ownerDocument.createTextNode(text);
    range.insertNode(node);
    range.setStartAfter(node); range.collapse(true);
    selection.removeAllRanges(); selection.addRange(range);
    return commit();
  }
  listen("beforeinput", event => {
    lastSelection = selectionOf(target);
    if (event.isComposing || composing) return;
    if (event.inputType.startsWith("history")) {
      event.preventDefault();
      (event.inputType === "historyUndo" ? history(undo, redo) : history(redo, undo)).catch(error);
    } else if (["insertParagraph", "insertLineBreak"].includes(event.inputType)) {
      event.preventDefault();
      insertAtCaret("\n").then(() => format("\n")).catch(error);
    }
  });
  listen("paste", event => {
    event.preventDefault();
    insertAtCaret(event.clipboardData?.getData("text/plain") || "").catch(error);
  });
  listen("input", event => {
    if (composing || event.isComposing) return;
    commit().then(() => event.data === "|" && format("|")).catch(error);
  });
  listen("compositionstart", () => {
    composing = true;
    compositionVersion = view.workspace.getDocument(view.uri).version;
    view.pause(true);
  });
  listen("compositionend", () => {
    composing = false;
    if (view.workspace.getDocument(view.uri)?.version !== compositionVersion) {
      error(new Error("Source changed during composition; the newer source was preserved"));
      view.pause(false);
      view.refresh().catch(error);
      return;
    }
    commit().then(() => { view.pause(false); return view.refresh(); }).catch(error);
  });
  listen("keydown", event => {
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "z") {
      event.preventDefault();
      (event.shiftKey ? history(redo, undo) : history(undo, redo)).catch(error);
    }
  });
  const destroy = view.destroy;
  return Object.assign(view, {
    insertAtCaret,
    caret: () => caretOffset(target),
    select(offset) { target.focus(); setCaret(target, offset); },
    undo: () => history(undo, redo),
    redo: () => history(redo, undo),
    destroy() { abort.abort(); target.contentEditable = "false"; destroy(); },
  });
}
