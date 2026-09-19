// The note both GIFs are about. The typing GIF plays `story`; the terminal
// GIF queries the note it ends with, so the two always agree.
//
// Each step is `type` text at the caret, `after` (move the caret to just after
// the first occurrence of a substring), `end` (caret to the end), or `pause`
// milliseconds. Pauses are where the viewer reads the result.
export const story = [
  { type: "Values are plain text with a name.\n$1,234:car\n$67:groceries\n\n" },
  { type: "Calculations update as you type.\ntotal := car + groceries" },
  { pause: 1400 },
  { after: "$67" },
  { type: "0" },
  { pause: 1800 },
  { end: true },
  { type: "\n\nAny value can sit inside a sentence.\nWe have [total] left for the trip." },
  { pause: 1500 },
  { type: "\n\nDates do arithmetic.\n2026-11-20:departure\nLeaving in [departure - today()]." },
  { pause: 1500 },
  { type: "\n\nTables have typed columns and sum themselves.\nbasket := table\n| item | qty | price |\n| --- | --- | --- |\n| apple | 2 | $3.30 |\n| pear | 4 | $4.30 |\n\nspend := sum(basket, qty * price)" },
  { pause: 1800 },
  { type: "\n\nTasks know when they are due.\n- [ ] Book the flights @due(departure - 14d)\n- [ ] Pack @due(tomorrow)" },
  { pause: 1800 },
  { type: "\n\nA named heading is a checklist that counts.\n## Packing :packing\n- [x] Passport\n- [x] Charger\n- [ ] Sunscreen\n\n[completed(packing)] of [total(packing)] packed." },
  { pause: 1800 },
  { type: "\n\nTimers are values; their controls live in the note.\nfocus := countdown(25m)" },
  { pause: 1600 },
  { type: "\n\nLibraries add functions.\nThat is [round(import(\"units\").convert(100, \"km\", \"mi\"))] miles." },
  { pause: 2600 },
];

/// The clock every GIF is captured at, so relative dates agree between them.
export const now = "2026-09-19T12:00:00Z";

/// The note the story ends with, by replaying its caret moves on a string.
export function finalNote(steps = story) {
  let text = "";
  let caret = 0;
  for (const step of steps) {
    if (step.type) {
      text = text.slice(0, caret) + step.type + text.slice(caret);
      caret += step.type.length;
    } else if (step.after) {
      const at = text.indexOf(step.after);
      if (at === -1) throw new Error(`"${step.after}" is not in the note`);
      caret = at + step.after.length;
    } else if (step.end) {
      caret = text.length;
    }
  }
  return text.endsWith("\n") ? text : `${text}\n`;
}
