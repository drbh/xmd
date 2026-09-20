// The note both GIFs are about. The typing GIF plays `story`; the terminal
// GIF queries the note it ends with, so the two always agree.
//
// Each step is `type` text at the caret, `after` (move the caret to just after
// the first occurrence of a substring), `replace` a substring `with` another
// (selected, then typed over), `end` (caret to the end), or `pause`
// milliseconds. Pauses are where the viewer reads the result.
// The note is already written when the GIF starts: the reveal is a number
// changing and the note following. Only then are the features typed in.
export const opening = `Trip budget

$1,234:car
$67:groceries

total := car + groceries
We have [total] left for the trip.

2026-11-20:departure
Leaving in [departure - today()].

- [ ] Book the flights @due(departure - 14d)
`;

export const story = [
  { pause: 1600 },
  { after: "$67" },
  { pause: 500 },
  { type: "0" },
  { pause: 2200 },
  { replace: "2026-11-20", with: "2026-11-28" },
  { pause: 2200 },
  { end: true },
  { type: "\nTables have typed columns and sum themselves.\nbasket := table\n| item | qty | price |\n| --- | --- | --- |\n| apple | 2 | $3.30 |\n| pear | 4 | $4.30 |\n\nspend := sum(basket, qty * price)" },
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
  let text = opening;
  let caret = 0;
  for (const step of steps) {
    if (step.type) {
      text = text.slice(0, caret) + step.type + text.slice(caret);
      caret += step.type.length;
    } else if (step.after) {
      const at = text.indexOf(step.after);
      if (at === -1) throw new Error(`"${step.after}" is not in the note`);
      caret = at + step.after.length;
    } else if (step.replace) {
      const at = text.indexOf(step.replace);
      if (at === -1) throw new Error(`"${step.replace}" is not in the note`);
      text = text.slice(0, at) + step.with + text.slice(at + step.replace.length);
      caret = at + step.with.length;
    } else if (step.end) {
      caret = text.length;
    }
  }
  return text.endsWith("\n") ? text : `${text}\n`;
}
