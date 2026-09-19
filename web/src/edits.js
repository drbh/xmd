// Plain-text edits as {start, end, text} in the coordinates of the source they
// apply to. This is the shape a collaborative host exchanges with the editor.

/** The single edit that turns `before` into `after` (edits from the editor are contiguous). */
export function diff(before, after) {
  if (before === after) return null;
  let start = 0;
  const max = Math.min(before.length, after.length);
  while (start < max && before.charCodeAt(start) === after.charCodeAt(start)) start++;
  let endBefore = before.length, endAfter = after.length;
  while (endBefore > start && endAfter > start && before.charCodeAt(endBefore - 1) === after.charCodeAt(endAfter - 1)) { endBefore--; endAfter--; }
  return { start, end: endBefore, text: after.slice(start, endAfter) };
}

/** Apply non-overlapping edits, given in ascending order of `start`, to `source`. */
export function apply(source, edits) {
  let out = "", at = 0;
  for (const edit of edits) {
    if (edit.start < at || edit.end < edit.start || edit.end > source.length) throw new Error("Invalid or overlapping edits");
    out += source.slice(at, edit.start) + edit.text;
    at = edit.end;
  }
  return out + source.slice(at);
}

/** Where `offset` in the old source lands after `edits`; positions inside a replaced span go to its end. */
export function shift(offset, edits) {
  let moved = offset;
  for (const edit of edits) {
    if (edit.end <= offset) moved += edit.text.length - (edit.end - edit.start);
    else if (edit.start < offset) { moved += edit.start + edit.text.length - offset; break; }
    else break;
  }
  return moved;
}
