// Documents live in this browser. Names are local to each document; explicit
// imports connect virtual files using their URIs, just as with notes on disk.
const KEY = "wtf.docs.v1";
const PREFS = "wtf.docs.prefs.v1";
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

const valid = d => /^[a-zA-Z0-9_-]+$/.test(d.id) && typeof d.name === "string" && typeof d.text === "string" && Number.isFinite(d.updated);
export function loadDocuments() {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) {
      const parsed = JSON.parse(raw);
      if (!Array.isArray(parsed.documents) || !parsed.documents.length || parsed.documents.some(d => !valid(d)) || new Set(parsed.documents.map(d => d.id)).size !== parsed.documents.length) throw new Error("Invalid saved documents");
      return parsed.documents;
    }
  } catch { writable = false; }
  return [createDocument(TEMPLATES[1])];
}
export function createDocument(template = TEMPLATES[0], name) {
  const text = template.text;
  return { id: crypto.randomUUID(), name: name ?? titleOf(text, template.name), text, updated: Date.now(), opened: Date.now() };
}
export function saveDocuments(documents) {
  if (!writable) return false;
  try { localStorage.setItem(KEY, JSON.stringify({ documents })); return true; } catch { return false; }
}
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
export const uriOf = id => `file:///workspace/docs/${id}.wtf`;

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
