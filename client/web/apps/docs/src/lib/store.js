// Documents live in this browser. Names are local to each document; explicit
// imports connect virtual files using their URIs, just as with notes on disk.
import { noteFile, noteStem } from "@xmd/web";

const KEY = "xmd.docs.v1";
const PREFS = "xmd.docs.prefs.v1";
let writable = true;

export const TEMPLATES = [
  { id: "blank", name: "Blank", text: "# Untitled document\n\n" },
  { id: "budget", name: "Trip budget", text: `# Trip budget

Our budget is $3,000:budget and we've spent $2,444:spent.
remaining := budget - spent

We have [remaining] left, which is [remaining / budget] of the budget.

## Before we go :prep

- [ ] Book the hotel @due(2026-11-06) @estimate(30m)
- [x] Renew passports
- [ ] Pack :pack
  - [ ] Chargers
  - [ ] Rain jacket

focus := countdown(25m)
Time left: [focus.remaining].
` },
  { id: "meeting", name: "Meeting notes", text: `# Meeting notes

Date: 2026-09-18:when
Attendees: Sam, Priya, Lee

## Agenda

1. Launch timeline
2. Budget review
3. Open questions

## Decisions

- Ship the beta on 2026-10-02:ship
- Keep the launch budget at $12,000:budget

## Action items

- [ ] Sam drafts the announcement @due(2026-09-25)
- [ ] Priya confirms vendor pricing @due(2026-09-23)
- [ ] Lee books the demo room

Beta ships in [ship - when].
` },
  { id: "weekly", name: "Weekly plan", text: `# Week of 2026-09-14

## Goals

- [ ] Finish the onboarding flow @estimate(6h)
- [ ] Review three pull requests @estimate(2h)
- [ ] Write the release notes @estimate(1h)

## Focus

deep := countdown(50m)
Current block: [deep.remaining].

## Notes

<!-- capture anything that comes up here -->
` },
  { id: "expenses", name: "Expense tracker", text: `# Expenses

items := table
| item        | quantity | price  |
| ----------- | -------- | ------ |
| coffee      | 12       | $3.50  |
| lunch       | 5        | $14.00 |
| train pass  | 1        | $86.00 |

total := sum(items, quantity * price)
per_day := total / 7

Spent [total] this week, about [per_day] per day.
` },
];

const ID = /^[a-zA-Z0-9_-]+$/;
const valid = d => ID.test(d.id) && typeof d.name === "string" && typeof d.text === "string" && Number.isFinite(d.updated) && (d.folder == null || ID.test(d.folder)) && (d.file == null || typeof d.file === "string");
const validFolder = f => ID.test(f.id) && typeof f.name === "string" && Number.isFinite(f.updated);
const unique = list => new Set(list.map(x => x.id)).size === list.length;
/** Whether this browser holds documents someone actually saved (not just the starter). */
export function hasStoredDocuments() {
  try { return !!localStorage.getItem(KEY); } catch { return false; }
}
/** Documents and folders saved in this browser; a fresh browser starts with one example. */
export function loadState() {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) {
      const parsed = JSON.parse(raw);
      const documents = parsed.documents, folders = parsed.folders ?? [], trash = parsed.trash ?? [];
      if (!Array.isArray(documents) || !Array.isArray(folders) || !Array.isArray(trash) || documents.some(d => !valid(d)) || trash.some(d => !valid(d)) || folders.some(f => !validFolder(f)) || !unique(documents) || !unique(folders)) throw new Error("Invalid saved documents");
      return { documents, folders, trash };
    }
  } catch { writable = false; }
  return { documents: [createDocument(TEMPLATES[1])], folders: [], trash: [] };
}
export const loadDocuments = () => loadState().documents;
export function createDocument(template = TEMPLATES[0], name) {
  const text = template.text;
  const title = name ?? titleOf(text, template.name);
  return { id: crypto.randomUUID(), name: title, file: fileNameFor(title), named: false, text, updated: Date.now(), opened: Date.now(), folder: null };
}
/** A file name for a document: the name without path separators or control characters, never empty. */
export function fileNameFor(name) {
  const clean = noteStem(String(name ?? "").replace(/[\\/\x00-\x1f]/g, " ").replace(/\s+/g, " ").trim()).slice(0, 120);
  return clean || "Untitled document";
}
/** `file`, or the first "file 2", "file 3"… not used by another document in the same folder. */
export function uniqueFile(file, folder, documents, except) {
  const taken = new Set(documents.filter(d => d.id !== except && (d.folder ?? null) === (folder ?? null)).map(d => (d.file ?? fileNameFor(d.name)).toLowerCase()));
  if (!taken.has(file.toLowerCase())) return file;
  for (let n = 2; ; n++) if (!taken.has(`${file} ${n}`.toLowerCase())) return `${file} ${n}`;
}
export function saveState({ documents, folders = [], trash = [] }) {
  if (!writable) return false;
  try { localStorage.setItem(KEY, JSON.stringify({ documents, folders, trash })); return true; } catch { return false; }
}
export const saveDocuments = documents => saveState({ documents });
export function clearState() {
  try { localStorage.removeItem(KEY); } catch { /* nothing to clear */ }
}
export const createFolder = name => ({ id: crypto.randomUUID(), name, updated: Date.now() });
export function watchStorage(onConflict) {
  if (!writable) onConflict("Saved documents could not be read; original storage is preserved and saving is paused.");
  const handler = event => { if (event.key === KEY || event.key === null) { writable = false; onConflict("Another tab changed these documents; saving is paused. Download your changes before reloading."); } };
  window.addEventListener("storage", handler);
  return () => window.removeEventListener("storage", handler);
}
export function titleOf(text, fallback) {
  const heading = text.split("\n").find(l => /^#+\s+\S/.test(l));
  return heading ? heading.replace(/^#+\s+/, "").replace(/\s+:\w+$/, "").trim() : fallback;
}
// Documents live in a virtual directory shaped like a synced folder on disk:
// docs/<Folder name>/<File name>.x.md, so imports use the same relative paths
// in the app, on disk, and through the sync plugin.
const segment = s => encodeURIComponent(s);
export function uriOf(doc, folders = []) {
  // An example keeps its file name beside the others, so its imports resolve.
  if (doc.example) return `file:///workspace/examples/${segment(doc.file)}`;
  const folder = doc.folder ? folders.find(f => f.id === doc.folder) : null;
  const dir = folder ? `${segment(fileNameFor(folder.name))}/` : "";
  return `file:///workspace/docs/${dir}${segment(noteFile(doc.file ?? fileNameFor(doc.name)))}`;
}

// Appearance preferences are per browser and never block editing when unavailable.
const DEFAULT_PREFS = { theme: "light", zoom: 100, outline: false, pageless: false, wordCount: false, console: false, consoleHeight: 260 };
export function loadPrefs() {
  try { return { ...DEFAULT_PREFS, ...JSON.parse(localStorage.getItem(PREFS) || "{}") }; } catch { return { ...DEFAULT_PREFS }; }
}
export function savePrefs(prefs) {
  try { localStorage.setItem(PREFS, JSON.stringify(prefs)); } catch { /* preferences are optional */ }
}

export function relativeTime(when, now = Date.now()) {
  const seconds = Math.max(0, Math.round((now - when) / 1000));
  if (seconds < 5) return "just now";
  if (seconds < 60) return `${seconds} seconds ago`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} minute${minutes === 1 ? "" : "s"} ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} hour${hours === 1 ? "" : "s"} ago`;
  const days = Math.round(hours / 24);
  if (days < 7) return `${days} day${days === 1 ? "" : "s"} ago`;
  return new Date(when).toLocaleDateString();
}

// Avatar colour for an email, matching the palette the live session uses.
const COLORS = ["#1a73e8", "#d93025", "#188038", "#e37400", "#9334e6", "#007b83", "#c5221f", "#3c4043"];
export const colorFor = s => COLORS[[...(s || "")].reduce((n, c) => (n * 31 + c.charCodeAt(0)) >>> 0, 7) % COLORS.length];
