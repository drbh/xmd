// Documents live in this browser. One workspace holds all of them, so names
// resolve across documents the way they do across notes on disk.
const KEY = "wtf.docs.v1";
let writable = true;
const STARTER = `# Trip budget

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
`;

export function loadDocuments() {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) {
      const parsed = JSON.parse(raw);
      if (!Array.isArray(parsed.documents) || !parsed.documents.length || parsed.documents.some(d => !/^[a-zA-Z0-9_-]+$/.test(d.id) || typeof d.name !== "string" || typeof d.text !== "string" || !Number.isFinite(d.updated)) || new Set(parsed.documents.map(d => d.id)).size !== parsed.documents.length) throw new Error("Invalid saved documents");
      return parsed.documents;
    }
  } catch { writable = false; }
  return [{ id: crypto.randomUUID(), name: "Trip budget", text: STARTER, updated: Date.now() }];
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
