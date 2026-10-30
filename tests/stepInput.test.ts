// Unit tests for src/stepInput.ts: step lengths through the tempo map, the chord/advance state
// machine (chords, rests, Back, Sustain, Chord mode, Quantize) and the region edits.
import { test } from "node:test";
import assert from "node:assert/strict";
import * as si from "../src/stepInput.ts";
import { TempoMap } from "../src/tempo.ts";
import type { Region } from "../src/api.ts";

const SR = 48000;
const Q = 24000; // a quarter at 120 BPM
const flat = TempoMap.constant(SR, 120, 4, 4);
/** 120 BPM, 60 BPM from bar 3 (sample 192000). */
const slower = new TempoMap(
  SR,
  [
    { pos: 0, bpm: 120, ramp: false },
    { pos: 8 * Q, bpm: 60, ramp: false },
  ],
  [],
  120,
  [4, 4],
);
const opts = (o: Partial<si.StepOpts> = {}): si.StepOpts => ({ ...si.DEFAULT_OPTS, value: 4, ...o });

/** Runs events from a fresh state with the playhead at `playhead` (it follows the locates). */
function run(events: si.StepEvent[], o: si.StepOpts = opts(), playhead = 0, map = flat) {
  let state = si.initialState();
  const effects: si.Effect[] = [];
  let ph = playhead;
  for (const ev of events) {
    const r = si.reduce(state, ev, map, ph, o);
    state = r.state;
    effects.push(...r.effects);
    for (const e of r.effects) if (e.type === "locate") ph = e.pos;
    state = si.observe(state, ph);
  }
  return { state, effects, playhead: ph };
}
const inserts = (effects: si.Effect[]) => effects.flatMap((e) => (e.type === "insert" ? [[e.note.start, e.note.length, e.note.pitch, e.key]] : []));
const locates = (effects: si.Effect[]) => effects.flatMap((e) => (e.type === "locate" ? [e.pos] : []));

test("step lengths: note values, dotted and triplet, through tempo changes", () => {
  assert.equal(si.stepQuarters({ value: 4, dotted: false, triplet: false }), 1);
  assert.equal(si.stepQuarters({ value: 1, dotted: false, triplet: false }), 4);
  assert.equal(si.stepQuarters({ value: 8, dotted: true, triplet: false }), 0.75);
  assert.equal(si.stepQuarters({ value: 8, dotted: false, triplet: true }), 1 / 3);
  assert.equal(si.stepEnd(flat, 0, opts({ value: 16 })) - 0, Q / 4);
  // A quarter at 60 BPM is twice as long; one straddling the change is 1.5 times.
  assert.equal(si.stepEnd(slower, 9 * Q, opts()) - 9 * Q, 2 * Q);
  assert.equal(si.stepEnd(slower, 7.5 * Q, opts()) - 7.5 * Q, Q / 2 + Q);
  // Even a 1/128 triplet is at least a sample.
  assert.ok(si.stepEnd(flat, 5, opts({ value: 128, triplet: true })) > 5);
});

test("quantize snaps to the step grid from the bar line", () => {
  assert.equal(si.snapToStep(flat, Q + 100, opts()), Q);
  assert.equal(si.snapToStep(flat, Q * 1.6, opts()), 2 * Q);
  // Dotted quarters from bar 2: 4Q, 5.5Q, 7Q; the bar line wins when nearer.
  assert.equal(si.snapToStep(flat, 5.6 * Q, opts({ dotted: true })), 5.5 * Q);
  assert.equal(si.snapToStep(flat, 7.8 * Q, opts({ dotted: true })), 8 * Q);
  const r = run([{ type: "down", pitch: 60, velocity: 90 }, { type: "up", pitch: 60 }], opts({ quantize: true }), Q + 300);
  assert.deepEqual(inserts(r.effects), [[Q, Q, 60, 1]]);
  assert.deepEqual(locates(r.effects), [2 * Q]);
});

test("notes advance one step each; a chord held together is one step", () => {
  const r = run([
    { type: "down", pitch: 60, velocity: 90 },
    { type: "up", pitch: 60 },
    { type: "down", pitch: 64, velocity: 80 },
    { type: "down", pitch: 67, velocity: 70 },
    { type: "up", pitch: 64 },
    { type: "down", pitch: 72, velocity: 70 },
    { type: "up", pitch: 67 },
    { type: "up", pitch: 72 },
  ]);
  assert.deepEqual(inserts(r.effects), [
    [0, Q, 60, 1],
    [Q, Q, 64, 2],
    [Q, Q, 67, 2],
    [Q, Q, 72, 2],
  ]);
  assert.deepEqual(locates(r.effects), [Q, 2 * Q]);
  assert.equal(r.state.history.length, 2);
  // Velocities come through as played.
  assert.deepEqual(
    r.effects.flatMap((e) => (e.type === "insert" ? [e.note.velocity] : [])),
    [90, 80, 70, 70],
  );
});

test("Chord mode keeps the position until it is turned off or Rest ends the chord", () => {
  const o = opts({ chord: true });
  const r = run(
    [
      { type: "down", pitch: 60, velocity: 90 },
      { type: "up", pitch: 60 },
      { type: "down", pitch: 64, velocity: 90 },
      { type: "up", pitch: 64 },
      { type: "rest" },
      { type: "down", pitch: 65, velocity: 90 },
      { type: "up", pitch: 65 },
    ],
    o,
  );
  assert.deepEqual(inserts(r.effects), [
    [0, Q, 60, 1],
    [0, Q, 64, 1],
    [Q, Q, 65, 2],
  ]);
  assert.deepEqual(locates(r.effects), [Q]);
  assert.ok(r.state.open);
  const off = si.reduce(r.state, { type: "chord-off" }, flat, r.playhead, opts());
  assert.deepEqual(locates(off.effects), [2 * Q]);
  assert.equal(off.state.open, null);
});

test("rests move on; Back removes the last step and steps back", () => {
  const r = run([{ type: "down", pitch: 60, velocity: 90 }, { type: "up", pitch: 60 }, { type: "rest" }, { type: "down", pitch: 62, velocity: 90 }, { type: "up", pitch: 62 }]);
  assert.deepEqual(inserts(r.effects), [
    [0, Q, 60, 1],
    [2 * Q, Q, 62, 3],
  ]);
  assert.equal(r.playhead, 3 * Q);
  let { state, effects } = si.reduce(r.state, { type: "back" }, flat, r.playhead, opts());
  assert.deepEqual(effects, [
    { type: "remove", notes: [{ start: 2 * Q, length: Q, pitch: 62, velocity: 90 }] },
    { type: "locate", pos: 2 * Q },
  ]);
  // Back over the rest only moves; over the first note it removes it.
  ({ state, effects } = si.reduce(state, { type: "back" }, flat, 2 * Q, opts()));
  assert.deepEqual(effects, [{ type: "locate", pos: Q }]);
  ({ state, effects } = si.reduce(state, { type: "back" }, flat, Q, opts()));
  assert.equal(effects[0].type, "remove");
  assert.deepEqual(locates(effects), [0]);
  ({ state, effects } = si.reduce(state, { type: "back" }, flat, 0, opts()));
  assert.deepEqual(effects, []);
});

test("Sustain lengthens the last notes by a step, and Back shortens them again", () => {
  const r = run([
    { type: "down", pitch: 60, velocity: 90 },
    { type: "down", pitch: 64, velocity: 90 },
    { type: "up", pitch: 60 },
    { type: "up", pitch: 64 },
    { type: "sustain" },
    { type: "sustain" },
  ]);
  const resizes = r.effects.filter((e) => e.type === "resize");
  assert.equal(resizes.length, 2);
  assert.deepEqual(
    resizes.map((e) => e.type === "resize" && e.notes.map((x) => [x.note.pitch, x.note.length, x.length])),
    [
      [
        [60, Q, 2 * Q],
        [64, Q, 2 * Q],
      ],
      [
        [60, 2 * Q, 3 * Q],
        [64, 2 * Q, 3 * Q],
      ],
    ],
  );
  assert.equal(r.playhead, 3 * Q);
  const back = si.reduce(r.state, { type: "back" }, flat, r.playhead, opts());
  assert.deepEqual(
    back.effects.map((e) => (e.type === "resize" ? e.notes.map((x) => [x.note.pitch, x.length]) : e)),
    [
      [
        [60, 2 * Q],
        [64, 2 * Q],
      ],
      { type: "locate", pos: 2 * Q },
    ],
  );
  // With nothing entered yet, Sustain is a rest.
  const empty = run([{ type: "sustain" }]);
  assert.deepEqual(empty.effects, [{ type: "locate", pos: Q }]);
});

test("the cursor follows Step Input's own moves, and the user's", () => {
  let s = si.initialState();
  ({ state: s } = si.reduce(s, { type: "rest" }, flat, 1000, opts()));
  assert.equal(s.cursor, 1000 + Q);
  // The engine still reports the old position: that isn't the user moving it.
  assert.equal(si.position(s, 1000), 1000 + Q);
  s = si.observe(s, 1000);
  assert.equal(si.position(s, 1000), 1000 + Q);
  // Now it caught up; a later report elsewhere is the user.
  s = si.observe(s, 1000 + Q);
  assert.deepEqual(s.stale, []);
  s = si.observe(s, 5 * Q);
  assert.equal(s.cursor, null);
  assert.equal(si.position(s, 5 * Q), 5 * Q);
});

test("repeated keys and keys released that were never pressed are ignored", () => {
  const r = run([{ type: "up", pitch: 50 }, { type: "down", pitch: 60, velocity: 90 }, { type: "down", pitch: 60, velocity: 90 }, { type: "up", pitch: 60 }]);
  assert.equal(inserts(r.effects).length, 1);
  assert.deepEqual(locates(r.effects), [Q]);
});

// ---- region edits ---------------------------------------------------------------------------

const region = (id: number, start: number, length: number, notes: [number, number][] = []): Region => ({
  id,
  name: "r",
  start,
  length,
  muted: false,
  content: { type: "midi", notes: notes.map(([s, p]) => ({ start: s, length: Q, pitch: p, velocity: 100, muted: false })), cc: [] },
});

test("the region a step goes into", () => {
  const bar = 4 * Q;
  const a = region(1, 0, bar);
  const b = region(2, 2 * bar, bar);
  const audio: Region = { ...region(3, 3 * bar, bar), content: { type: "audio", clip: 1, offset: 0, gain_db: 0, fade_in: 0, fade_out: 0 } as Region["content"] };
  assert.equal(si.regionFor([a, b], Q, flat)?.id, 1);
  // Just past a region: it grows. Over a bar past: a new one.
  assert.equal(si.regionFor([a, b], bar + Q, flat)?.id, 1);
  assert.equal(si.regionFor([a, b], 3 * bar + 2 * Q, flat)?.id, 2);
  assert.equal(si.regionFor([a], 3 * bar, flat), null);
  assert.equal(si.regionFor([audio], 3 * bar + Q, flat), null);
  // Overlapping regions: the newest.
  assert.equal(si.regionFor([a, region(4, 0, bar)], Q, flat)?.id, 4);
  assert.deepEqual(si.newRegionSpan(flat, bar + Q), [bar, bar]);
});

test("insert, remove and resize edits", () => {
  const r = region(7, 4 * Q, 4 * Q, [[0, 60]]);
  // A note past the end grows the region to the next bar line, selected.
  const ins = si.insertEdits(r, [{ start: 8 * Q, length: Q, pitch: 64, velocity: 70 }], flat);
  assert.equal(ins.edits.length, 1);
  assert.equal(ins.edits[0].length, 8 * Q);
  assert.deepEqual(
    ins.edits[0].notes.map((n) => [n.start, n.pitch, n.velocity]),
    [
      [0, 60, 100],
      [4 * Q, 64, 70],
    ],
  );
  assert.deepEqual([...ins.selNotes], ["7:1"]);
  const rem = si.removeEdits([r], [{ start: 4 * Q, length: Q, pitch: 60, velocity: 100 }], flat);
  assert.deepEqual(rem[0].notes, []);
  assert.deepEqual(si.removeEdits([r], [{ start: 5 * Q, length: Q, pitch: 60, velocity: 100 }], flat), []);
  const res = si.resizeEdits([r], [{ note: { start: 4 * Q, length: Q, pitch: 60, velocity: 100 }, length: 2 * Q }], flat);
  assert.equal(res[0].notes[0].length, 2 * Q);
});
