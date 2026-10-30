// Unit tests for src/noteSelect.ts: Edit › Select in the Piano Roll and the Event List.
import { test } from "node:test";
import assert from "node:assert/strict";
import { isEditorSelect, selectEvents, type SelectItem } from "../src/noteSelect.ts";

const note = (start: number, pitch: number, extra: Partial<SelectItem> = {}): SelectItem => ({ start, sel: false, equal: `n${pitch}`, velocity: 100, muted: false, ...extra });
const cc = (start: number, value: number, extra: Partial<SelectItem> = {}): SelectItem => ({ start, sel: false, equal: `c7:${value}`, ...extra });
const ctx = { playhead: 0, locators: null };

test("the editors take the Select commands that make sense for notes, the rest stay with the regions", () => {
  for (const id of ["sel-following", "sel-inside-locators", "sel-muted", "sel-same-color", "sel-equal", "sel-invert"]) assert.ok(isEditorSelect(id), id);
  for (const id of ["sel-overlapped", "sel-similar", "sel-following-track", "select-all", "delete-regions"]) {
    assert.ok(!isEditorSelect(id), id);
    assert.equal(selectEvents(id, [note(0, 60)], ctx), null);
  }
});

test("Invert Selection flips every event", () => {
  const items = [note(0, 60, { sel: true }), note(10, 62), cc(5, 3)];
  assert.deepEqual(selectEvents("sel-invert", items, ctx), [false, true, true]);
});

test("All Following starts at the first selected event, or at the playhead", () => {
  const items = [note(0, 60), note(10, 62, { sel: true }), cc(15, 3), note(20, 64), note(5, 65, { sel: true })];
  assert.deepEqual(selectEvents("sel-following", items, ctx), [false, true, true, true, true]);
  const none = items.map((n) => ({ ...n, sel: false }));
  assert.deepEqual(selectEvents("sel-following", none, { playhead: 12, locators: null }), [false, false, true, true, false]);
});

test("Inside Locators selects what starts between them; without locators nothing changes", () => {
  const items = [note(0, 60, { sel: true }), note(10, 62), note(20, 64), cc(19, 1)];
  assert.deepEqual(selectEvents("sel-inside-locators", items, { playhead: 0, locators: { start: 10, end: 20 } }), [false, true, false, true]);
  assert.deepEqual(selectEvents("sel-inside-locators", items, ctx), [true, false, false, false]);
});

test("Muted selects the muted notes only", () => {
  const items = [note(0, 60, { muted: true }), note(10, 62, { sel: true }), cc(5, 3)];
  assert.deepEqual(selectEvents("sel-muted", items, ctx), [true, false, false]);
});

test("Same Colored selects the notes with a selected note's velocity", () => {
  const items = [note(0, 60, { velocity: 80, sel: true }), note(10, 62, { velocity: 80 }), note(20, 64, { velocity: 90 }), cc(5, 80)];
  assert.deepEqual(selectEvents("sel-same-color", items, ctx), [true, true, false, false]);
  // Nothing selected: nothing changes.
  const none = items.map((n) => ({ ...n, sel: false }));
  assert.deepEqual(selectEvents("sel-same-color", none, ctx), [false, false, false, false]);
});

test("Equal selects the notes of a selected pitch and the controllers of a selected value", () => {
  const items = [note(0, 60, { sel: true }), note(10, 60), note(20, 62), cc(5, 3, { sel: true }), cc(6, 3), cc(7, 4)];
  assert.deepEqual(selectEvents("sel-equal", items, ctx), [true, true, false, true, true, false]);
  const none = items.map((n) => ({ ...n, sel: false }));
  assert.deepEqual(selectEvents("sel-equal", none, ctx), [false, false, false, false, false, false]);
});
