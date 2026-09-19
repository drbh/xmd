// Optional source editing. The core view owns rendering and controls; the
// workspace owns source. This adapter owns selection, composition and history.
import { mount } from "../src/view.js";
import { textOf, selectionOf, restoreSelection, setCaret, caretOffset, offsetOfPoint, lineChar, rangeOf } from "../src/dom.js";
import { applyTextEdits, indexOf } from "../src/workspace.js";
import { diff, apply, shift } from "../src/edits.js";

export async function mountEditor(element, options = {}) {
  const undo = [], redo = [];
  let lastSource, lastSelection, replaying = false, composing = false, compositionVersion;
  // Collaboration: a host may listen to local edits as deltas, feed remote
  // edits back in, and take over undo. The editor stays the same otherwise.
  const editListeners = new Set();
  let applyingRemote = false, history = null, pendingRemote = [];
  const view = await mount(element, {
    ...options,
    trailingBreak: true,
    onChange(change) {
      if (lastSource !== undefined && change.source !== lastSource && !replaying && !applyingRemote && !history) {
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
  // Local writes announce their delta before the workspace write, against the
  // exact source they change, so a host can relay them in order.
  function localSet(source) {
    const before = view.getSource();
    const edit = diff(before, source);
    const written = view.setSource(source);
    if (edit) for (const listener of editListeners) listener({ edits: [edit], source });
    return written;
  }
  function commit() { return localSet(textOf(target)); }
  async function step(from, to) {
    if (view.destroyed || composing) return;
    if (history) return from === undo ? history.undo() : history.redo();
    if (!from.length) return;
    await view.workspace.settled();
    const previous = from.pop();
    to.push({ source: view.getSource(), selection: selectionOf(target) });
    replaying = true;
    try { await localSet(previous.source); await view.refresh(); restoreSelection(target, previous.selection); }
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
    await localSet(applyTextEdits(source, edits));
    await view.refresh();
    if (!view.destroyed) setCaret(target, caret);
  }
  function selection() { return view.destroyed ? null : selectionOf(target); }
  function select(anchor, focus = anchor) {
    if (view.destroyed) return;
    target.focus();
    restoreSelection(target, { anchor, focus });
  }
  // Replace source[start, end) and leave the caret or selection where the caller asks.
  async function replaceRange(start, end, text, after = { anchor: start + text.length }) {
    if (view.destroyed) throw new Error("View was destroyed");
    if (composing) return;
    const source = view.getSource();
    if (!(start >= 0 && start <= end && end <= source.length)) throw new Error("Invalid range");
    lastSelection = selectionOf(target);
    await localSet(source.slice(0, start) + text + source.slice(end));
    await view.refresh();
    if (!view.destroyed) select(after.anchor, after.focus ?? after.anchor);
  }
  /** Apply edits made elsewhere (old-source coordinates, ascending, non-overlapping), keeping the local selection in place. */
  function applyEdits(edits) {
    if (view.destroyed) return Promise.reject(new Error("View was destroyed"));
    if (!edits.length) return Promise.resolve();
    // During IME composition the DOM belongs to the browser; hold the edits
    // and settle up at compositionend.
    if (composing) { pendingRemote.push(edits); return Promise.resolve(); }
    const next = apply(view.getSource(), edits);
    // Patch the text nodes synchronously, last edit first so earlier offsets
    // stay valid: the model and the DOM agree before any local keystroke can
    // be diffed against them, and live selections move with the text.
    const selection = selectionOf(target);
    for (const edit of [...edits].reverse()) {
      const range = rangeOf(target, edit.start, edit.end);
      range.deleteContents();
      if (edit.text) range.insertNode(target.ownerDocument.createTextNode(edit.text));
    }
    target.normalize();
    if (selection && !selectionOf(target)) restoreSelection(target, { anchor: shift(selection.anchor, edits), focus: shift(selection.focus, edits) });
    applyingRemote = true;
    const written = view.setSource(next);
    applyingRemote = false;
    return written.then(() => view.refresh()).then(() => {});
  }
  // After composition: apply the held remote edits to the model, express the
  // composed text as a local edit in those coordinates, then repaint.
  async function settleComposition(domBefore) {
    const held = pendingRemote; pendingRemote = [];
    const local = diff(domBefore, textOf(target));
    let source = view.getSource();
    for (const batch of held) source = apply(source, batch);
    if (!local) {
      applyingRemote = true;
      try { await view.setSource(source); } finally { applyingRemote = false; }
      return;
    }
    let start = local.start, end = local.end;
    for (const batch of held) { start = shift(start, batch); end = shift(end, batch); }
    const edit = { start, end: Math.max(start, end), text: local.text };
    const next = apply(source, [edit]);
    if (held.length) { applyingRemote = true; try { await view.setSource(source); } finally { applyingRemote = false; } }
    const written = view.setSource(next);
    for (const listener of editListeners) listener({ edits: [edit], source: next });
    await written;
    view.pause(false);
    await view.refresh();
    if (!view.destroyed) setCaret(target, edit.start + edit.text.length);
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
    // Merge the split text nodes so the caret sits at the end of one text node;
    // browsers cannot hold a caret between a newline node and an empty node.
    const at = offsetOfPoint(target, node, node.data.length);
    target.normalize();
    setCaret(target, at);
    if (text.endsWith("\n")) {
      // Until the next repaint, give the new line its own editable span; the
      // browser otherwise moves the caret back before the newline.
      const line = selection.anchorNode?.parentElement?.closest(".line");
      if (line && selection.anchorNode === line.lastChild && selection.anchorOffset === line.lastChild.length) {
        const fresh = target.ownerDocument.createElement("span");
        fresh.className = "line";
        line.after(fresh);
        selection.collapse(fresh, 0);
      }
    }
    return commit();
  }
  listen("beforeinput", event => {
    lastSelection = selectionOf(target);
    if (event.isComposing || composing) return;
    if (event.inputType.startsWith("history")) {
      event.preventDefault();
      (event.inputType === "historyUndo" ? step(undo, redo) : step(redo, undo)).catch(error);
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
  let compositionText = "";
  listen("compositionstart", () => {
    composing = true;
    compositionText = textOf(target);
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
    // Read the composed text before unpausing, which repaints from the model.
    (pendingRemote.length ? settleComposition(compositionText) : commit()).then(() => { view.pause(false); return view.refresh(); }).catch(error);
  });
  listen("keydown", event => {
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "z") {
      event.preventDefault();
      (event.shiftKey ? step(redo, undo) : step(undo, redo)).catch(error);
    }
  });
  const destroy = view.destroy;
  return Object.assign(view, {
    insertAtCaret,
    caret: () => caretOffset(target),
    selection,
    select,
    replaceRange,
    undo: () => step(undo, redo),
    redo: () => step(redo, undo),
    onEdit(listener) { editListeners.add(listener); return () => editListeners.delete(listener); },
    applyEdits,
    setHistory(handler) { history = handler; },
    destroy() { abort.abort(); target.contentEditable = "false"; destroy(); },
  });
}
