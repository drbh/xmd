// Source-level editing operations on top of the contenteditable adapter. The
// document is plain text, so bold, headings, and lists are text transformations
// that keep the selection where a writer expects it.
const MARKS = { bold: "**", italic: "_", code: "`", strike: "~~" };

function range(controller) {
  const selection = controller.selection();
  const source = controller.getSource();
  if (!selection) return { source, start: source.length, end: source.length };
  return { source, start: Math.min(selection.anchor, selection.focus), end: Math.max(selection.anchor, selection.focus) };
}

/** Wrap the selection in a mark, or remove the mark when already wrapped. Without a selection, wrap the word at the caret. */
export function toggleMark(controller, kind) {
  const mark = MARKS[kind];
  let { source, start, end } = range(controller);
  if (start === end) {
    while (start > 0 && /\S/.test(source[start - 1]) && !/[*_`~]/.test(source[start - 1])) start--;
    while (end < source.length && /\S/.test(source[end]) && !/[*_`~]/.test(source[end])) end++;
  }
  const inner = source.slice(start, end);
  if (source.slice(start - mark.length, start) === mark && source.slice(end, end + mark.length) === mark) {
    return controller.replaceRange(start - mark.length, end + mark.length, inner, { anchor: start - mark.length, focus: end - mark.length });
  }
  if (inner.startsWith(mark) && inner.endsWith(mark) && inner.length >= mark.length * 2) {
    const text = inner.slice(mark.length, -mark.length);
    return controller.replaceRange(start, end, text, { anchor: start, focus: start + text.length });
  }
  return controller.replaceRange(start, end, mark + inner + mark, { anchor: start + mark.length, focus: start + mark.length + inner.length });
}

export function insertLink(controller, url = "https://") {
  const { source, start, end } = range(controller);
  const label = source.slice(start, end) || "link";
  const text = `[${label}](${url})`;
  const at = start + label.length + 3;
  return controller.replaceRange(start, end, text, { anchor: at, focus: at + url.length });
}

/** Apply `fn` to every line touched by the selection, keeping those lines selected. */
export function transformLines(controller, fn) {
  const { source, start, end } = range(controller);
  const first = source.lastIndexOf("\n", start - 1) + 1;
  let last = source.indexOf("\n", Math.max(end - (end > start ? 1 : 0), first));
  if (last === -1) last = source.length;
  const lines = source.slice(first, last).split("\n");
  const changed = lines.map((line, i) => fn(line, i, lines)).join("\n");
  const collapsed = start === end;
  const delta = changed.length - (last - first);
  return controller.replaceRange(first, last, changed, collapsed ? { anchor: Math.max(first, Math.min(start + (changed.split("\n")[0].length - lines[0].length), first + changed.length)) } : { anchor: first, focus: first + changed.length });
}

const PREFIX = /^(\s*)(?:(#{1,6})\s+|([-*+])\s+(\[[ xX]\]\s+)?|(\d+)[.)]\s+)?/;
export function lineStyle(line) {
  const m = PREFIX.exec(line);
  if (m[2]) return { kind: "heading", level: m[2].length };
  if (m[4]) return { kind: "task" };
  if (m[3]) return { kind: "bullet" };
  if (m[5]) return { kind: "number" };
  return { kind: "text" };
}
const strip = line => { const m = PREFIX.exec(line); return { indent: m[1], rest: line.slice(m[0].length) }; };

/** level 0 clears the heading; otherwise the line becomes exactly that level. */
export const setHeading = (controller, level) => transformLines(controller, line => {
  const { rest } = strip(line);
  const current = lineStyle(line);
  if (!level || (current.kind === "heading" && current.level === level)) return rest;
  return `${"#".repeat(level)} ${rest}`;
});

/** Toggle a list kind on the selected lines: applying it when any line lacks it, else clearing. */
export const toggleList = (controller, kind) => transformLines(controller, (line, i, lines) => {
  const all = lines.every(l => !l.trim() || lineStyle(l).kind === kind);
  const { indent, rest } = strip(line);
  if (all) return indent + rest;
  if (!line.trim()) return line;
  const prefix = kind === "task" ? "- [ ] " : kind === "bullet" ? "- " : `${i + 1}. `;
  return indent + prefix + rest;
});

export const indent = (controller, outdent = false) => transformLines(controller, line => outdent ? line.replace(/^ {1,2}/, "") : (line.trim() ? "  " + line : line));

export function lineOf(source, offset) {
  let line = 0;
  for (let i = 0; i < offset && i < source.length; i++) if (source[i] === "\n") line++;
  return line;
}

/** Scroll the selection's line into view inside the nearest scrolling ancestor. */
export function reveal(controller) {
  const selection = controller.element.ownerDocument.getSelection();
  if (!selection?.rangeCount) return;
  let node = selection.getRangeAt(0).startContainer;
  if (node.nodeType !== Node.ELEMENT_NODE) node = node.parentElement;
  node?.scrollIntoView({ block: "center", behavior: "smooth" });
}

export function findMatches(source, query, { caseSensitive = false } = {}) {
  if (!query) return [];
  const haystack = caseSensitive ? source : source.toLowerCase();
  const needle = caseSensitive ? query : query.toLowerCase();
  const matches = [];
  for (let at = haystack.indexOf(needle); at !== -1; at = haystack.indexOf(needle, at + Math.max(1, needle.length))) matches.push({ start: at, end: at + needle.length });
  return matches;
}

export function stats(source) {
  const words = source.split(/\s+/).filter(Boolean).length;
  const lines = source.split("\n");
  return {
    words,
    characters: source.length,
    charactersNoSpaces: source.replace(/\s/g, "").length,
    paragraphs: source.split(/\n\s*\n/).filter(p => p.trim()).length,
    headings: lines.filter(l => /^#{1,6}\s/.test(l)).length,
    tasks: lines.filter(l => /^\s*[-*+]\s+\[[ xX]\]/.test(l)).length,
    done: lines.filter(l => /^\s*[-*+]\s+\[[xX]\]/.test(l)).length,
    readingMinutes: Math.max(1, Math.round(words / 200)),
  };
}

/** DOM range for a source span inside the view, skipping inlays and the trailing sentinel. */
export function rangeOf(view, start, end) {
  const walker = view.ownerDocument.createTreeWalker(view, NodeFilter.SHOW_TEXT | NodeFilter.SHOW_ELEMENT, {
    acceptNode: node => node.nodeType === Node.ELEMENT_NODE && (node.classList.contains("inlay") || node.classList.contains("eol")) ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT,
  });
  const range = view.ownerDocument.createRange();
  let offset = 0, node, from = false;
  while ((node = walker.nextNode())) {
    const length = node.nodeType === Node.TEXT_NODE ? node.data.length : node.nodeName === "BR" ? 1 : 0;
    if (!length) continue;
    if (!from && start <= offset + length) { from = true; if (node.nodeType === Node.TEXT_NODE) range.setStart(node, start - offset); else range.setStartBefore(node); }
    if (from && end <= offset + length) { if (node.nodeType === Node.TEXT_NODE) range.setEnd(node, end - offset); else range.setEndAfter(node); return range; }
    offset += length;
  }
  return from ? range : null;
}

/** Paint find matches with the CSS Custom Highlight API where available. */
export function highlightMatches(view, matches, current) {
  if (!globalThis.CSS?.highlights || !view) return;
  const ranges = kind => matches.filter((_, i) => kind === "current" ? i === current : i !== current).map(m => rangeOf(view, m.start, m.end)).filter(Boolean);
  CSS.highlights.set("wtf-find", new Highlight(...ranges("all")));
  CSS.highlights.set("wtf-find-current", new Highlight(...ranges("current")));
}
export function clearHighlights() {
  if (!globalThis.CSS?.highlights) return;
  CSS.highlights.delete("wtf-find");
  CSS.highlights.delete("wtf-find-current");
}
