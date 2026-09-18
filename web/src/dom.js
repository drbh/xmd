export const esc = s => s.replace(/&/g, "&amp;").replace(/</g, "&lt;");

// The engine's hover is Markdown; keep the few marks it uses and drop the rest.
export function renderHover(markdown) {
  return esc(markdown)
    .replace(/```text\n([\s\S]*?)```/g, (_, code) => `<pre>${code.trim()}</pre>`)
    .replace(/`([^`]+)`/g, "<code>$1</code>")
    .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
    .replace(/\[([^\]]+)\]\(&lt;[^)]*&gt;\)/g, "$1")
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .split(/\n{2,}/)
    .map(part => part.startsWith("<pre>") ? part : `<p>${part.replace(/\n/g, "<br>")}</p>`)
    .join("");
}

// Text of a view, skipping inlays; <br> and block starts count as newlines.
export function textOf(view) {
  let out = "";
  const walk = node => {
    for (const child of node.childNodes) {
      if (child.nodeType === Node.TEXT_NODE) out += child.data;
      else if (child.classList?.contains("inlay") || child.classList?.contains("eol")) continue;
      else if (child.nodeName === "BR") out += "\n";
      else {
        if (/^(DIV|P)$/.test(child.nodeName) && out.length && !out.endsWith("\n")) out += "\n";
        walk(child);
      }
    }
  };
  walk(view);
  return out;
}
export function offsetOfPoint(view, node, offset) {
  const probe = document.createRange();
  probe.setStart(view, 0);
  probe.setEnd(node, offset);
  const holder = document.createElement("div");
  holder.appendChild(probe.cloneContents());
  return textOf(holder).length;
}
export function caretOffset(view) {
  const selection = getSelection();
  if (!selection.rangeCount || !view.contains(selection.anchorNode)) return null;
  const range = selection.getRangeAt(0);
  return offsetOfPoint(view, range.startContainer, range.startOffset);
}
export function setCaret(view, offset) {
  let remaining = offset;
  const walker = document.createTreeWalker(view, NodeFilter.SHOW_TEXT | NodeFilter.SHOW_ELEMENT, {
    acceptNode: node => node.nodeType === Node.ELEMENT_NODE && (node.classList.contains("inlay") || node.classList.contains("eol")) ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT,
  });
  const place = (node, at) => {
    const range = document.createRange();
    range.setStart(node, at);
    range.collapse(true);
    const selection = getSelection();
    selection.removeAllRanges();
    selection.addRange(range);
  };
  let node, last = null;
  while ((node = walker.nextNode())) {
    if (node.nodeType !== Node.TEXT_NODE) { if (node.nodeName === "BR") { if (remaining === 0) break; remaining -= 1; } continue; }
    last = node;
    if (remaining <= node.data.length) { place(node, remaining); return; }
    remaining -= node.data.length;
  }
  if (last) place(last, last.data.length); else place(view, 0);
}
export function offsetAt(view, x, y) {
  let node, offset;
  if (document.caretPositionFromPoint) {
    const p = document.caretPositionFromPoint(x, y);
    if (!p) return null;
    node = p.offsetNode; offset = p.offset;
  } else {
    const r = document.caretRangeFromPoint(x, y);
    if (!r) return null;
    node = r.startContainer; offset = r.startOffset;
  }
  if (!view.contains(node) || node.parentElement?.closest(".inlay")) return null;
  return offsetOfPoint(view, node, offset);
}
export const lineChar = (text, offset) => {
  const before = text.slice(0, offset).split("\n");
  return { line: before.length - 1, character: before[before.length - 1].length };
};
export { indexOf } from "./workspace.js";

export function selectionOf(view) {
  const selection = view.ownerDocument.getSelection();
  if (!selection?.rangeCount || !view.contains(selection.anchorNode) || !view.contains(selection.focusNode)) return null;
  return { anchor: offsetOfPoint(view, selection.anchorNode, selection.anchorOffset), focus: offsetOfPoint(view, selection.focusNode, selection.focusOffset) };
}
export function restoreSelection(view, saved) {
  if (!saved) return;
  setCaret(view, saved.anchor);
  const selection = view.ownerDocument.getSelection();
  const anchorNode = selection.anchorNode, anchorOffset = selection.anchorOffset;
  setCaret(view, saved.focus);
  selection.setBaseAndExtent(anchorNode, anchorOffset, selection.focusNode, selection.focusOffset);
}

/** A DOM range covering source offsets [start, end) of the view, skipping inlays and the trailing sentinel. */
export function rangeOf(view, start, end) {
  const walker = view.ownerDocument.createTreeWalker(view, NodeFilter.SHOW_TEXT | NodeFilter.SHOW_ELEMENT, {
    acceptNode: node => node.nodeType === Node.ELEMENT_NODE && (node.classList.contains("inlay") || node.classList.contains("eol")) ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT,
  });
  const range = view.ownerDocument.createRange();
  let offset = 0, node, found = false, last = null;
  while ((node = walker.nextNode())) {
    const length = node.nodeType === Node.TEXT_NODE ? node.data.length : node.nodeName === "BR" ? 1 : 0;
    if (!length) continue;
    last = node;
    if (!found && start <= offset + length) { found = true; if (node.nodeType === Node.TEXT_NODE) range.setStart(node, start - offset); else range.setStartBefore(node); }
    if (found && end <= offset + length) { if (node.nodeType === Node.TEXT_NODE) range.setEnd(node, end - offset); else range.setEndAfter(node); return range; }
    offset += length;
  }
  if (!found) { if (last?.nodeType === Node.TEXT_NODE) range.setStart(last, last.data.length); else range.setStart(view, view.childNodes.length - (view.lastElementChild?.classList.contains("eol") ? 1 : 0)); }
  range.collapse(true);
  return range;
}
