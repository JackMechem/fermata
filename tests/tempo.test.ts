// Unit tests for src/tempo.ts, the UI's mirror of crates/engine/src/tempo.rs. Run with
// `npm test`.
import { test } from "node:test";
import assert from "node:assert/strict";
import * as t from "../src/tempo.ts";
import * as me from "../src/midiEdit.ts";

const SR = 48000;
const close = (a: number, b: number, tol = 1e-6) => assert.ok(Math.abs(a - b) <= tol, `${a} != ${b}`);
const ev = (pos: number, bpm: number, ramp = false): t.TempoEvent => ({ pos, bpm, ramp });

/** time.ts before the tempo map, for comparison. */
function oldBbt(spb: number, num: number, pos: number) {
  const beats = pos / spb;
  const bar = Math.floor(beats / num);
  const inBar = beats - bar * num;
  const beat = Math.floor(inBar);
  const frac = inBar - beat;
  const div = Math.floor(frac * 4);
  const tick = Math.floor((frac * 4 - div) * 240);
  return { bar: bar + 1, beat: beat + 1, div: div + 1, tick: tick + 1 };
}

test("an empty map is the old constant timing", () => {
  for (const [bpm, num, den] of [
    [120, 4, 4],
    [97, 7, 8],
    [133.3, 3, 4],
  ]) {
    const m = t.TempoMap.constant(44100, bpm, num, den);
    const spb = ((44100 * 60) / bpm) * (4 / den);
    assert.ok(m.isConstant);
    for (const k of [0, 1, 7, 100]) close(m.barStart(k + 1), k * spb * num);
    for (const pos of [0, 1, 22050, 123456, 9999999]) {
      assert.deepEqual(m.bbtAt(pos), oldBbt(spb, num, pos), `${bpm} at ${pos}`);
      // The old snapPos: round to the nearest beat or bar.
      const q = m.beatsAt(pos);
      close(m.sampleAt(m.snapQ(q, 4 / den)), Math.round(pos / spb) * spb, 1e-6);
      close(m.sampleAt(m.snapQ(q, (4 / den) * num)), Math.round(pos / (spb * num)) * spb * num, 1e-6);
    }
  }
});

test("steps convert both ways", () => {
  const m = new t.TempoMap(SR, [ev(0, 120), ev(96000, 60)], [], 120, [4, 4]);
  close(m.beatsAt(96000), 4, 1e-12);
  close(m.beatsAt(144000), 5, 1e-12);
  close(m.sampleAt(5), 144000);
  assert.equal(m.bpmAt(95999), 120);
  assert.equal(m.bpmAt(96000), 60);
  for (const s of [0, 1, 50000, 95999.5, 96000, 96001, 1e7]) close(m.sampleAt(m.beatsAt(s)), s);
  close(m.barStart(2), 96000);
  close(m.barStart(3), 96000 + 4 * SR);
  assert.deepEqual(m.bbtAt(96000 + 2 * SR), { bar: 2, beat: 3, div: 1, tick: 1 });
});

test("ramps integrate exactly", () => {
  // 60 → 180 BPM over 4 s: a mean of 120 BPM, so 8 quarters.
  const m = new t.TempoMap(SR, [ev(0, 60, true), ev(4 * SR, 180)], [], 60, [4, 4]);
  close(m.beatsAt(4 * SR), 8, 1e-9);
  close(m.bpmAt(2 * SR), 120, 1e-9);
  close(m.beatsAt(SR), 1.25, 1e-9);
  close(m.beatsAt(5 * SR), 11, 1e-9);
  for (const q of [0, 0.001, 1.25, 3.3, 7.999, 8, 8.5, 40]) close(m.beatsAt(m.sampleAt(q)), q, 1e-9);
  const d = new t.TempoMap(SR, [ev(0, 200, true), ev(96000, 50)], [], 200, [4, 4]);
  close(d.beatsAt(96000), (2 * 125) / 60, 1e-9);
  for (const s of [0, 1000, 48000, 95999, 96000, 200000]) close(d.sampleAt(d.beatsAt(s)), s);
});

test("4/4 → 7/8 → 3/4 numbers bars across the changes", () => {
  const m = new t.TempoMap(SR, [], [{ bar: 1, num: 4, den: 4 }, { bar: 3, num: 7, den: 8 }, { bar: 5, num: 3, den: 4 }], 120, [4, 4]);
  const Q = 24000;
  [0, 4, 8, 11.5, 15, 18, 21].forEach((s, i) => {
    close(m.barStart(i + 1), s * Q);
    assert.equal(m.barAt(s * Q), i + 1);
    assert.deepEqual(m.bbtAt(s * Q), { bar: i + 1, beat: 1, div: 1, tick: 1 });
  });
  assert.deepEqual(m.sigAtBar(4), [7, 8]);
  assert.deepEqual(m.sigAtBar(99), [3, 4]);
  assert.deepEqual(m.bbtAt(8.5 * Q), { bar: 3, beat: 2, div: 1, tick: 1 });
  assert.deepEqual(
    [...m.bars(8 * Q, 15 * Q)].map((b) => [b.bar, b.num, b.den]),
    [
      [3, 7, 8],
      [4, 7, 8],
    ],
  );
  const beats = [...m.grid(8 * Q, 15 * Q, 1)];
  assert.equal(beats.length, 14);
  assert.equal(beats[7].sub, 0);
  close(beats[7].pos, 11.5 * Q);
  close(m.nextBar(8 * Q), 11.5 * Q);
  close(m.prevBar(11.5 * Q), 8 * Q);
  close(m.prevBeat(8 * Q), 7 * Q);
  close(m.nextBeat(11 * Q), 11.5 * Q);
  close(m.barPosition(13.25 * Q), 3 + 1.75 / 3.5, 1e-9);
  // Snapping counts from the bar line, and the next bar line wins when it is nearer.
  close(m.snapQ(11.3, 1), 11.5, 1e-9);
  close(m.snapQ(11.2, 1), 11, 1e-9);
  close(m.floorQ(11.4, 1), 11, 1e-9);
});

test("normalizing sorts, dedupes and anchors at the start", () => {
  assert.deepEqual(t.normalizeTempo([ev(500, 90, true), ev(100, 1000), ev(500, 80, true)], 110), [ev(0, 110), ev(100, 990), ev(500, 80)]);
  assert.deepEqual(t.normalizeSigs([{ bar: 4, num: 5, den: 3 }, { bar: 0, num: 3, den: 4 }], [4, 4]), [
    { bar: 1, num: 3, den: 4 },
    { bar: 4, num: 5, den: 4 },
  ]);
});

test("the map is cached per project snapshot", () => {
  const p = { sample_rate: SR, tempo: 120, sig_num: 4, sig_den: 4, tempo_map: [ev(0, 120), ev(SR, 60)], signatures: [] } as never;
  assert.equal(t.tempoMap(p), t.tempoMap(p));
  close(t.tempoMap(p).beatsAt(2 * SR), 3);
});

test("grid lines follow the map", () => {
  const m = new t.TempoMap(SR, [ev(0, 120), ev(96000, 60)], [], 120, [4, 4]);
  const lines: [number, string][] = [];
  t.forEachGridLine(m, 0, 96000 + 4 * SR, 1, { beatsMinPx: 0 }, (pos, kind) => lines.push([Math.round(pos), kind]));
  assert.deepEqual(lines.slice(0, 6), [
    [0, "bar"],
    [24000, "beat"],
    [48000, "beat"],
    [72000, "beat"],
    [96000, "bar"],
    [144000, "beat"],
  ]);
});

test("quantize in musical time follows a tempo change", () => {
  // 120 BPM, then 60 from bar 2: a sixteenth there is 12000 samples, not 6000.
  const m = new t.TempoMap(SR, [ev(0, 120), ev(96000, 60)], [], 120, [4, 4]);
  const w = t.musicalWarp(m, 24000);
  const tb: me.TimeBase = { sampleRate: SR, tempo: 120, sigNum: 4, sigDen: 4 };
  const notes: me.PNote[] = [5000, 96000 + 13000].map((start) => ({ region: 1, start, length: 3000, pitch: 60, velocity: 100, muted: false, sel: false }));
  const v = notes.map((n) => ({ ...n, start: w.toV(n.start), length: w.toV(n.start + n.length) - w.toV(n.start) }));
  const q = me.quantize(v, { grid: me.gridLength(tb, "1/16"), bar: w.bars, strength: 1, swing: 0, ends: false });
  const back = q.map((n) => Math.round(w.fromV(n.start)));
  assert.deepEqual(back, [6000, 96000 + 12000]);
  // Bar lines in warped time: bar 2 at 4 quarters, bar 3 at 8.
  close(w.bars.floor(w.toV(96000 + 5 * SR)), 8 * 24000);
  close(t.barLines(m).ceil(96001), 96000 + 4 * SR);
});
