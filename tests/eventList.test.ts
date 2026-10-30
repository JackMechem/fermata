// Unit tests for src/eventList.ts, the Event List's positions, typed values and edits.
import { test } from "node:test";
import assert from "node:assert/strict";
import * as el from "../src/eventList.ts";
import { TempoMap } from "../src/tempo.ts";
import type { PCc, PNote } from "../src/midiEdit.ts";

const SR = 48000;
const Q = 24000; // a quarter at 120 BPM
const flat = TempoMap.constant(SR, 120, 4, 4);
/** 120 BPM in 4/4, 60 BPM from bar 3 (sample 192000), 3/4 from bar 5. */
const changing = new TempoMap(
  SR,
  [
    { pos: 0, bpm: 120, ramp: false },
    { pos: 8 * Q, bpm: 60, ramp: false },
  ],
  [
    { bar: 1, num: 4, den: 4 },
    { bar: 5, num: 3, den: 4 },
  ],
  120,
  [4, 4],
);

const note = (start: number, pitch = 60, extra: Partial<PNote> = {}): PNote => ({ region: 1, start, length: Q, pitch, velocity: 100, muted: false, sel: false, ...extra });
const cc = (start: number, kind: PCc["kind"], value: number, extra: Partial<PCc> = {}): PCc => ({ region: 1, start, kind, value, sel: false, ...extra });

test("positions read as bars, beats, divisions and ticks", () => {
  assert.equal(el.formatPosition(flat, 0), "1 1 1 1");
  assert.equal(el.formatPosition(flat, Q), "1 2 1 1");
  assert.equal(el.formatPosition(flat, Q / 4), "1 1 2 1");
  assert.equal(el.formatPosition(flat, 4 * Q + Q / 960), "2 1 1 2");
  assert.equal(el.formatPosition(flat, 17 * Q + Q / 2 + Q / 8), "5 2 3 121");
});

test("positions follow tempo and signature changes", () => {
  // Bars 3 and 4 are at 60 BPM: a beat is 48000 samples.
  assert.equal(el.formatPosition(changing, 8 * Q), "3 1 1 1");
  assert.equal(el.formatPosition(changing, 8 * Q + 48000), "3 2 1 1");
  // Bar 5 starts after two slow bars, in 3/4.
  const bar5 = 8 * Q + 8 * 48000;
  assert.equal(el.formatPosition(changing, bar5), "5 1 1 1");
  assert.equal(el.formatPosition(changing, bar5 + 2 * 48000), "5 3 1 1");
  assert.equal(el.formatPosition(changing, bar5 + 3 * 48000), "6 1 1 1");
  assert.equal(el.bbtToSample(changing, { bar: 6, beat: 1, div: 1, tick: 1 }), bar5 + 3 * 48000);
  // A beat past the bar's end carries into the next bar.
  assert.equal(el.bbtToSample(changing, { bar: 5, beat: 4, div: 1, tick: 1 }), bar5 + 3 * 48000);
});

test("positions round to the nearest tick, so odd sample rates read back as typed", () => {
  const m = TempoMap.constant(44100, 120, 4, 4);
  for (const text of ["1 1 1 2", "3 4 4 240", "7 2 3 17", "12 1 1 1"]) {
    const s = el.parsePosition(text, m)!;
    assert.equal(el.formatPosition(m, s), text);
  }
  // A hair before a bar line shows the bar line.
  assert.equal(el.formatPosition(m, 4 * 22050 - 1), "2 1 1 1");
});

test("typed positions", () => {
  assert.equal(el.parsePosition("5 2 1 1", flat), 17 * Q);
  assert.equal(el.parsePosition("5.2.1.1", flat), 17 * Q);
  assert.equal(el.parsePosition(" 5 . 2 ", flat), 17 * Q);
  assert.equal(el.parsePosition("5", flat), 16 * Q);
  assert.equal(el.parsePosition("5:3", flat), 18 * Q);
  assert.equal(el.parsePosition("0 1 1 1", flat), 0);
  assert.equal(el.parsePosition("1 1 1 0", flat), 0);
  for (const bad of ["", "x", "1 2 3 4 5", "1.5.x", "1,5e3"]) assert.equal(el.parsePosition(bad, flat), null, bad);
  assert.equal(el.parsePosition("6", changing), 8 * Q + 8 * 48000 + 3 * 48000);
});

test("lengths count from zero through the tempo map", () => {
  assert.equal(el.formatLength(flat, 0, Q), "0 1 0 0");
  assert.equal(el.formatLength(flat, 0, 4 * Q + Q / 4 + Q / 960), "1 0 1 1");
  // A slow bar holds the same beats in twice the samples.
  assert.equal(el.formatLength(changing, 8 * Q, 48000), "0 1 0 0");
  // Across the change: two fast beats then one slow one.
  assert.equal(el.formatLength(changing, 6 * Q, 2 * Q + 48000), "0 3 0 0");
  assert.equal(el.parseLength("0 1", flat, 0), Q);
  assert.equal(el.parseLength("0.0.2.0", flat, 0), Q / 2);
  assert.equal(el.parseLength("1", changing, 8 * Q), 4 * 48000);
  // At least a tick.
  assert.equal(el.parseLength("0 0 0 0", flat, 0), Q / 960);
  assert.equal(el.parseLength("long", flat, 0), null);
});

test("note names round trip", () => {
  for (let p = 0; p <= 127; p++) assert.equal(el.parseNote(el.noteName(p)), p);
  assert.equal(el.noteName(60), "C3");
  assert.equal(el.parseNote("c3"), 60);
  assert.equal(el.parseNote("C#3"), 61);
  assert.equal(el.parseNote("Db3"), 61);
  assert.equal(el.parseNote("E♭4"), 75);
  assert.equal(el.parseNote("b3"), 71);
  assert.equal(el.parseNote("C-2"), 0);
  assert.equal(el.parseNote("72"), 72);
  assert.equal(el.parseNote("300"), 127);
  assert.equal(el.parseNote("C9"), null);
  assert.equal(el.parseNote("H3"), null);
});

test("rows sort by position, filter by type and find the playing row", () => {
  const l: el.Lists = {
    notes: [note(Q, 64), note(0, 60), note(Q, 62)],
    cc: [cc(Q, { type: "pitch_bend" }, 8192), cc(Q, { type: "controller", cc: 7 }, 100), cc(Q / 2, { type: "aftertouch" }, 30)],
  };
  const rows = el.buildRows(l);
  assert.deepEqual(
    rows.map((r) => r.key),
    ["n1", "c2", "n2", "n0", "c1", "c0"],
  );
  assert.deepEqual(
    el.buildRows(l, (t) => t !== "note").map((r) => r.type),
    ["pressure", "control", "bend"],
  );
  assert.equal(el.rowAt(rows, -1), -1);
  assert.equal(el.rowAt(rows, 0), 0);
  assert.equal(el.rowAt(rows, Q - 1), 1);
  assert.equal(el.rowAt(rows, 10 * Q), 5);
});

test("clicks select, and Shift and ⌘ extend", () => {
  const l: el.Lists = { notes: [note(0), note(Q), note(2 * Q), note(3 * Q)], cc: [] };
  const rows = el.buildRows(l);
  let sel = el.clickSelect(rows, new Set(), 1, null, { toggle: false, range: false });
  assert.deepEqual([...sel], ["n1"]);
  sel = el.clickSelect(rows, sel, 3, 1, { toggle: false, range: true });
  assert.deepEqual([...sel].sort(), ["n1", "n2", "n3"]);
  sel = el.clickSelect(rows, sel, 2, 1, { toggle: true, range: false });
  assert.deepEqual([...sel].sort(), ["n1", "n3"]);
  const picked = el.selectKeys(l, sel);
  assert.deepEqual(
    picked.notes.map((n) => n.sel),
    [false, true, false, true],
  );
  assert.deepEqual(
    el.deleteSelected(picked).notes.map((n) => n.start),
    [0, 2 * Q],
  );
});

test("values clamp to their ranges", () => {
  const l: el.Lists = { notes: [note(0, 126, { velocity: 3 })], cc: [cc(0, { type: "controller", cc: 1 }, 120), cc(0, { type: "pitch_bend" }, 8192)] };
  const rows = el.buildRows(l);
  const [n, c, pb] = rows;
  let out = el.applyChange(flat, l, [n], { field: "num", delta: 5 });
  assert.equal(out.notes[0].pitch, 127);
  out = el.applyChange(flat, l, [n], { field: "val", delta: -10 });
  assert.equal(out.notes[0].velocity, 1, "velocity stays above 0");
  out = el.applyChange(flat, l, [c], { field: "val", delta: 20 });
  assert.equal(out.cc[0].value, 127);
  out = el.applyChange(flat, l, [c], { field: "num", to: 200 });
  assert.deepEqual(out.cc[0].kind, { type: "controller", cc: 127 });
  // Pitch bend: Num is the low 7 bits, Val the high 7.
  assert.equal(el.numOf(l, pb), 0);
  assert.equal(el.valOf(l, pb), 64);
  out = el.applyChange(flat, l, [pb], { field: "val", delta: 1 });
  assert.equal(out.cc[1].value, 8192 + 128);
  out = el.applyChange(flat, out, [pb], { field: "num", delta: 3 });
  assert.equal(out.cc[1].value, 8192 + 128 + 3);
  out = el.applyChange(flat, out, [pb], { field: "val", to: 127 });
  assert.equal(out.cc[1].value, 16383 - 124);
  assert.equal(el.parseNumber("-4", 0, 127), 0);
  assert.equal(el.parseNumber("1.5", 0, 127), null);
  // Fields a row doesn't have are left alone.
  out = el.applyChange(flat, l, [c], { field: "len", delta: 1 });
  assert.equal(out, l);
});

test("moving a note keeps its length in beats, and drags step by the part dragged", () => {
  const l: el.Lists = { notes: [note(6 * Q, 60, { length: Q })], cc: [cc(0, { type: "controller", cc: 1 }, 0)] };
  const [n] = el.buildRows(l, (t) => t === "note");
  // Into the slow bars: a beat is now 48000 samples.
  let out = el.applyChange(changing, l, [n], { field: "pos", part: 0, delta: 1 });
  assert.equal(out.notes[0].start, 8 * Q + 2 * 48000);
  assert.equal(out.notes[0].length, 48000);
  out = el.applyChange(changing, l, [n], { field: "pos", part: 1, delta: -1 });
  assert.equal(el.formatPosition(changing, out.notes[0].start), "2 2 1 1");
  out = el.applyChange(changing, l, [n], { field: "pos", part: 3, delta: 1 });
  assert.equal(el.formatPosition(changing, out.notes[0].start), "2 3 1 2");
  // Not before the start of the project.
  out = el.applyChange(changing, l, [n], { field: "pos", part: 0, delta: -9 });
  assert.equal(el.formatPosition(changing, out.notes[0].start), "1 3 1 1");
  out = el.applyChange(changing, l, [n], { field: "pos", to: el.parsePosition("3 1 1 1", changing)! });
  assert.equal(out.notes[0].start, 8 * Q);
  out = el.applyChange(changing, l, [n], { field: "len", part: 2, delta: 2 });
  assert.equal(el.formatLength(changing, out.notes[0].start, out.notes[0].length), "0 1 2 0");
  out = el.applyChange(changing, l, [n], { field: "len", part: 1, delta: -3 });
  assert.equal(el.formatLength(changing, out.notes[0].start, out.notes[0].length), "0 0 0 1");
});

test("typed cells", () => {
  const l: el.Lists = { notes: [note(Q, 61, { velocity: 90 })], cc: [cc(0, { type: "aftertouch" }, 30)] };
  const rows = el.buildRows(l);
  const n = rows.find((r) => r.note)!;
  const at = rows.find((r) => !r.note)!;
  assert.equal(el.cellText(flat, l, n, "pos"), "1 2 1 1");
  assert.equal(el.cellText(flat, l, n, "num"), "C♯3");
  assert.equal(el.cellText(flat, l, n, "val"), "90");
  assert.equal(el.cellText(flat, l, n, "len"), "0 1 0 0");
  assert.deepEqual(el.parseCell(flat, l, n, "num", "d3"), { field: "num", to: 62 });
  assert.deepEqual(el.parseCell(flat, l, n, "pos", "2"), { field: "pos", to: 4 * Q });
  assert.deepEqual(el.parseCell(flat, l, n, "val", "0"), { field: "val", to: 1 });
  assert.equal(el.parseCell(flat, l, n, "num", "?"), null);
  assert.deepEqual(el.parseCell(flat, l, at, "val", "200"), { field: "val", to: 127 });
  assert.ok(!el.hasField("pressure", "num"));
});

test("new events copy the event they are modelled on", () => {
  const like = note(0, 67, { velocity: 80, length: Q / 2 });
  const { note: n } = el.newEvent(flat, "note", Q + 3, 4, like);
  assert.deepEqual(n, { region: 4, start: Q, length: Q / 2, pitch: 67, velocity: 80, muted: false, sel: true });
  assert.equal(el.newEvent(flat, "note", 0, 4, null).note!.length, Q);
  const { cc: c } = el.newEvent(flat, "control", 0, 4, cc(0, { type: "controller", cc: 11 }, 90));
  assert.deepEqual(c, { region: 4, start: 0, kind: { type: "controller", cc: 11 }, value: 90, sel: true });
  assert.equal(el.newEvent(flat, "bend", 0, 4, null).cc!.value, 8192);
  assert.deepEqual(el.newEvent(flat, "pressure", 0, 4, like).cc!.kind, { type: "aftertouch" });
});
