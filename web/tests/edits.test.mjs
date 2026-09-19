import { test } from "node:test";
import assert from "node:assert/strict";
import { diff, apply, shift } from "../src/edits.js";

test("diff finds the single contiguous change and apply reverses it", () => {
  for (const [a, b] of [["hello world", "hello brave world"], ["abc", "abc"], ["", "x"], ["x", ""], ["aaa", "aa"], ["a\nb", "a\n\nb"], ["- [ ] task", "- [x] task"]]) {
    const edit = diff(a, b);
    assert.equal(edit ? apply(a, [edit]) : a, b, `${JSON.stringify(a)} -> ${JSON.stringify(b)}`);
  }
  assert.deepEqual(diff("hello world", "hello brave world"), { start: 6, end: 6, text: "brave " });
  assert.deepEqual(diff("aaa", "aa"), { start: 2, end: 3, text: "" });
});

test("apply handles several ordered edits and rejects overlap", () => {
  assert.equal(apply("0123456789", [{ start: 1, end: 3, text: "AB" }, { start: 5, end: 5, text: "x" }, { start: 8, end: 10, text: "" }]), "0AB34x567");
  assert.throws(() => apply("abc", [{ start: 2, end: 3, text: "" }, { start: 1, end: 2, text: "" }]));
});

test("shift keeps a caret in place through remote edits", () => {
  const edits = [{ start: 2, end: 2, text: "XY" }, { start: 6, end: 8, text: "" }];
  assert.equal(shift(1, edits), 1);       // before everything
  assert.equal(shift(2, edits), 4);       // at an insertion point: after the inserted text
  assert.equal(shift(5, edits), 7);       // between edits: moved by the insertion only
  assert.equal(shift(7, edits), 8);       // inside a deleted span: collapses to its start
  assert.equal(shift(10, edits), 10);     // after everything: +2 -2
});
