// Unit tests for src/musicalMove.ts. Run with `npm test`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { TempoMap } from "../src/tempo.ts";
import { movedStart, nudgeTarget } from "../src/musicalMove.ts";

const SR = 48000;
const Q120 = (SR * 60) / 120;
const Q90 = (SR * 60) / 90;
const BAR5 = 16 * Q120;
/** 120 BPM in 4/4 for four bars, then 90 BPM from bar 5. */
const map = new TempoMap(SR, [{ pos: 0, bpm: 120, ramp: false }, { pos: BAR5, bpm: 90, ramp: false }], [], 120, [4, 4]);
const bar = (n: number) => Math.round(map.barStart(n));

test("regions dragged across a tempo change land on its bars and beats", () => {
  // The grabbed region goes from bar 2 to bar 4; another at bar 3 beat 2 crosses into the
  // 90 BPM section and lands on bar 5 beat 2.
  assert.equal(movedStart(map, bar(2), bar(2), bar(4), true), bar(4));
  const b3b2 = bar(3) + Q120;
  const got = movedStart(map, b3b2, bar(2), bar(4), true);
  assert.equal(got, Math.round(BAR5 + Q90));
  assert.deepEqual(map.bbtAt(got), { bar: 5, beat: 2, div: 1, tick: 1 });
  // In samples it misses the beat: it lands a sixteenth before it.
  const plain = movedStart(map, b3b2, bar(2), bar(4), false);
  assert.equal(plain, b3b2 + 2 * 4 * Q120);
  assert.deepEqual(map.bbtAt(plain), { bar: 5, beat: 1, div: 4, tick: 1 });
  // Backwards out of the 90 BPM section, and never before 0.
  assert.equal(movedStart(map, bar(6), bar(4), bar(2), true), bar(4));
  assert.equal(movedStart(map, bar(1), bar(3), bar(1), true), 0);
});

test("nudges count bars, beats and note values through the tempo map", () => {
  const b4b3 = bar(4) + 2 * Q120;
  // A bar from bar 4 beat 3 is bar 5 beat 3, at 90 BPM.
  assert.equal(nudgeTarget(map, b4b3, "bar", 1), Math.round(BAR5 + 2 * Q90));
  assert.equal(nudgeTarget(map, BAR5 + 2 * Q90, "bar", -1), b4b3);
  // A beat or an eighth from the last beat of bar 4 crosses into the slower tempo.
  assert.equal(nudgeTarget(map, BAR5 - Q120, "beat", 1), BAR5);
  assert.equal(nudgeTarget(map, BAR5 - Q120 / 2, "8", 1), BAR5);
  assert.equal(nudgeTarget(map, BAR5, "16", 1), Math.round(BAR5 + Q90 / 4));
  assert.equal(nudgeTarget(map, 1000, "bar", -1), 0);
  assert.equal(nudgeTarget(map, 1000, "ms10", 1), null);
});
