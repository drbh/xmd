// One table drives the menu bar, the toolbar, and keyboard shortcuts, so every
// action has the same label and shortcut wherever it appears.
import { toggleMark, insertLink, setHeading, toggleList, indent } from "./editing.js";

export const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);

const SNIPPETS = {
  heading: "\n## Heading\n",
  task: "\n- [ ] ",
  bullet: "\n- ",
  value: "$100:amount",
  reference: "[amount]",
  formula: "\ntotal := amount * 2\n",
  table: "\nitems := table\n| item | quantity | price |\n| ---- | -------- | ----- |\n| tea  | 2        | $4.50 |\n\ntotal := sum(items, quantity * price)\n",
  timer: "\nfocus := countdown(25m)\n",
  plan: "\nplan := maximize(3 * bagels + 1.25 * doughnuts)\n| constraint | expression                     |\n| ---------- | ------------------------------ |\n| flour      | 12 * bagels + 6.5 * doughnuts <= 400 |\n",
  comment: "<!-- note to self -->",
  rule: "\n---\n",
  date: () => `${new Date().toISOString().slice(0, 10)}:when`,
  import: 'source := import("./other.wtf")\n',
};

/** `ctx` is the application: it supplies the editor and app-level operations. */
export function createCommands(ctx) {
  // Editing commands do nothing in a read-only document (a viewer, or offline without a local copy).
  const editor = fn => () => { const c = ctx.editor(); if (c && !ctx.readOnly?.()) return Promise.resolve(fn(c)).catch(ctx.error); };
  const insert = key => editor(c => c.insertAtCaret(typeof SNIPPETS[key] === "function" ? SNIPPETS[key]() : SNIPPETS[key]));
  const writable = () => !ctx.readOnly?.();
  const list = [
    // File
    { id: "new", menu: "File", label: "New document", shortcut: "mod+alt+n", run: ctx.newDocument },
    { id: "open", menu: "File", label: "Open…", shortcut: "mod+o", run: ctx.home },
    { id: "copy", menu: "File", label: "Make a copy", run: ctx.duplicate },
    { id: "import", menu: "File", label: "Import .wtf files…", run: ctx.importFiles, separator: true },
    { id: "rename", menu: "File", label: "Rename", when: writable, run: ctx.rename },
    { id: "details", menu: "File", label: "Document details", run: () => ctx.dialog("details") },
    { id: "share", menu: "File", label: "Share…", when: ctx.canShare, run: () => ctx.dialog("share"), separator: true },
    { id: "download", menu: "File", label: "Download (.wtf)", shortcut: "mod+shift+s", run: ctx.download },
    { id: "print", menu: "File", label: "Print", shortcut: "mod+p", icon: "print", run: ctx.print, separator: true },
    { id: "delete", menu: "File", label: "Move to trash", run: ctx.remove, separator: true },
    // Edit
    { id: "undo", menu: "Edit", label: "Undo", shortcut: "mod+z", icon: "undo", run: editor(c => c.undo()), native: true },
    { id: "redo", menu: "Edit", label: "Redo", shortcut: "mod+shift+z", icon: "redo", run: editor(c => c.redo()), native: true },
    { id: "selectAll", menu: "Edit", label: "Select all", shortcut: "mod+a", run: editor(c => c.select(0, c.getSource().length)), native: true, separator: true },
    { id: "find", menu: "Edit", label: "Find", shortcut: "mod+f", icon: "search", run: () => ctx.find(false), separator: true },
    { id: "replace", menu: "Edit", label: "Find and replace", shortcut: "mod+shift+h", run: () => ctx.find(true) },
    // View
    { id: "outline", menu: "View", label: "Show outline", shortcut: "mod+alt+a", icon: "outline", checked: () => ctx.prefs().outline, run: () => ctx.toggle("outline") },
    { id: "wordCount", menu: "View", label: "Show word count while typing", checked: () => ctx.prefs().wordCount, run: () => ctx.toggle("wordCount") },
    { id: "pageless", menu: "View", label: "Pageless", checked: () => ctx.prefs().pageless, run: () => ctx.toggle("pageless") },
    { id: "console", menu: "View", label: "Show console", shortcut: "mod+alt+j", checked: () => ctx.prefs().console, run: () => ctx.toggle("console"), separator: true },
    { id: "zoomIn", menu: "View", label: "Zoom in", run: () => ctx.zoom(10) },
    { id: "zoomOut", menu: "View", label: "Zoom out", run: () => ctx.zoom(-10) },
    { id: "zoomReset", menu: "View", label: "Actual size", run: () => ctx.zoom(0), separator: true },
    { id: "themeLight", menu: "View", label: "Light theme", checked: () => ctx.prefs().theme === "light", run: () => ctx.theme("light") },
    { id: "themeDark", menu: "View", label: "Dark theme", checked: () => ctx.prefs().theme === "dark", run: () => ctx.theme("dark"), separator: true },
    { id: "fullscreen", menu: "View", label: "Full screen", run: ctx.fullscreen },
    // Insert
    { id: "insertHeading", menu: "Insert", label: "Heading", run: insert("heading") },
    { id: "insertTask", menu: "Insert", label: "Checklist item", run: insert("task") },
    { id: "insertBullet", menu: "Insert", label: "Bulleted list", run: insert("bullet"), separator: true },
    { id: "insertValue", menu: "Insert", label: "Named value", hint: "$100:amount", run: insert("value") },
    { id: "insertReference", menu: "Insert", label: "Reference", hint: "[amount]", run: insert("reference") },
    { id: "insertFormula", menu: "Insert", label: "Formula", hint: "total := …", run: insert("formula") },
    { id: "insertTable", menu: "Insert", label: "Table", icon: "table", run: insert("table") },
    { id: "insertTimer", menu: "Insert", label: "Timer", icon: "timer", run: insert("timer") },
    { id: "insertPlan", menu: "Insert", label: "Plan", run: insert("plan"), separator: true },
    { id: "insertDate", menu: "Insert", label: "Today's date", run: insert("date") },
    { id: "insertLink", menu: "Insert", label: "Link", shortcut: "mod+k", icon: "link", run: editor(c => insertLink(c)) },
    { id: "insertRule", menu: "Insert", label: "Horizontal rule", run: insert("rule") },
    { id: "insertComment", menu: "Insert", label: "Comment", shortcut: "mod+alt+m", run: insert("comment") },
    { id: "insertImport", menu: "Insert", label: "Import another document", run: insert("import") },
    // Format
    { id: "bold", menu: "Format", label: "Bold", shortcut: "mod+b", icon: "bold", run: editor(c => toggleMark(c, "bold")) },
    { id: "italic", menu: "Format", label: "Italic", shortcut: "mod+i", icon: "italic", run: editor(c => toggleMark(c, "italic")) },
    { id: "strike", menu: "Format", label: "Strikethrough", shortcut: "mod+shift+x", icon: "strike", run: editor(c => toggleMark(c, "strike")) },
    { id: "code", menu: "Format", label: "Code", shortcut: "mod+e", icon: "code", run: editor(c => toggleMark(c, "code")), separator: true },
    { id: "normal", menu: "Format", label: "Normal text", shortcut: "mod+alt+0", run: editor(c => setHeading(c, 0)) },
    { id: "h1", menu: "Format", label: "Heading 1", shortcut: "mod+alt+1", run: editor(c => setHeading(c, 1)) },
    { id: "h2", menu: "Format", label: "Heading 2", shortcut: "mod+alt+2", run: editor(c => setHeading(c, 2)) },
    { id: "h3", menu: "Format", label: "Heading 3", shortcut: "mod+alt+3", run: editor(c => setHeading(c, 3)), separator: true },
    { id: "bulletList", menu: "Format", label: "Bulleted list", shortcut: "mod+shift+8", icon: "bullet", run: editor(c => toggleList(c, "bullet")) },
    { id: "numberList", menu: "Format", label: "Numbered list", shortcut: "mod+shift+7", icon: "number", run: editor(c => toggleList(c, "number")) },
    { id: "checklist", menu: "Format", label: "Checklist", shortcut: "mod+shift+9", icon: "checklist", run: editor(c => toggleList(c, "task")), separator: true },
    { id: "indent", menu: "Format", label: "Increase indent", shortcut: "mod+]", icon: "indent", run: editor(c => indent(c)) },
    { id: "outdent", menu: "Format", label: "Decrease indent", shortcut: "mod+[", icon: "outdent", run: editor(c => indent(c, true)) },
    // Tools
    { id: "stats", menu: "Tools", label: "Word count", shortcut: "mod+shift+c", run: () => ctx.dialog("stats") },
    { id: "problems", menu: "Tools", label: "Problems", run: () => ctx.dialog("problems") },
    { id: "query", menu: "Tools", label: "Query console", run: () => ctx.toggle("console") },
    { id: "engine", menu: "Tools", label: "About the engine", run: () => ctx.dialog("engine") },
    // Help
    { id: "syntax", menu: "Help", label: "Writing guide", run: () => ctx.dialog("syntax") },
    { id: "shortcuts", menu: "Help", label: "Keyboard shortcuts", shortcut: "mod+/", run: () => ctx.dialog("shortcuts") },
    { id: "book", menu: "Help", label: "The WTF Book", run: ctx.book },
  ];
  const byId = Object.fromEntries(list.map(c => [c.id, c]));
  const menus = ["File", "Edit", "View", "Insert", "Format", "Tools", "Help"].map(name => ({ name, get items() { return list.filter(c => c.menu === name && (!c.when || c.when())); } }));
  return { list, byId, get menus() { return menus.filter(m => writable() || !["Insert", "Format"].includes(m.name)); } };
}

/** Human-readable shortcut label, e.g. "⌘⇧Z" or "Ctrl+Shift+Z". */
export function shortcutLabel(shortcut) {
  if (!shortcut) return "";
  const parts = shortcut.split("+");
  const key = parts.pop();
  const name = { "/": "/", "[": "[", "]": "]" }[key] || key.toUpperCase();
  if (isMac) return parts.map(p => ({ mod: "⌘", shift: "⇧", alt: "⌥" })[p]).join("") + name;
  return [...parts.map(p => ({ mod: "Ctrl", shift: "Shift", alt: "Alt" })[p]), name].join("+");
}

/** Match a keydown event against a shortcut such as "mod+shift+z". */
export function matches(event, shortcut) {
  if (!shortcut) return false;
  const parts = shortcut.split("+");
  const key = parts.pop();
  const want = { mod: false, shift: false, alt: false };
  for (const p of parts) want[p] = true;
  const mod = isMac ? event.metaKey : event.ctrlKey;
  if (mod !== want.mod || event.shiftKey !== want.shift || event.altKey !== want.alt) return false;
  if (isMac && event.ctrlKey) return false;
  // On macOS, Option changes event.key; compare physical keys for letters and digits.
  const code = event.code || "";
  if (/^[a-z0-9]$/.test(key)) return code === (/\d/.test(key) ? `Digit${key}` : `Key${key.toUpperCase()}`);
  return event.key === key || ({ "/": "Slash", "[": "BracketLeft", "]": "BracketRight" })[key] === code;
}
