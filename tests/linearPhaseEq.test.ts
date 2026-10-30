// Unit tests for src/linearPhaseEq.ts (the Linear Phase EQ's parameters as Channel EQ
// settings). Run with `npm test`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { changedParams, eqFromValues, latencyFrames, OUTPUT, paramId, QUALITY } from "../src/linearPhaseEq.ts";
import { defaultEq } from "../src/eqMath.ts";

/** The engine's defaults (linear_phase_eq.rs), which mirror the Channel EQ's. */
function defaults(): Record<number, number> {
  const v: Record<number, number> = { [OUTPUT]: 0, [QUALITY]: 1 };
  defaultEq().bands.forEach((b, i) => {
    v[paramId(i, "on")] = b.on ? 1 : 0;
    v[paramId(i, "freq")] = b.freq;
    v[paramId(i, "q")] = b.q;
    if (i === 0 || i === 7) v[paramId(i, "slope")] = 1;
    else v[paramId(i, "gain")] = 0;
  });
  return v;
}

test("ids follow the engine's layout", () => {
  assert.equal(paramId(0, "on"), 0);
  assert.equal(paramId(4, "gain"), 34);
  assert.equal(paramId(7, "slope"), 60);
});

test("default values are the Channel EQ's defaults", () => {
  assert.deepEqual(eqFromValues(defaults()), defaultEq());
});

test("an edit maps back to the parameters it changed", () => {
  const before = eqFromValues(defaults());
  const after = { ...before, bands: before.bands.map((b, i) => (i === 4 ? { ...b, freq: 1000, gain: 6 } : i === 0 ? { ...b, on: true, slope: 48 } : b)) };
  assert.deepEqual(changedParams(before, after), [
    [paramId(0, "on"), 1],
    [paramId(0, "slope"), 2],
    [paramId(4, "freq"), 1000],
    [paramId(4, "gain"), 6],
  ]);
  assert.deepEqual(changedParams(before, { ...before, output_db: -3 }), [[OUTPUT, -3]]);
  assert.deepEqual(changedParams(before, before), []);
  // And the values round-trip.
  const v = defaults();
  for (const [id, x] of changedParams(before, after)) v[id] = x;
  assert.deepEqual(eqFromValues(v), after);
});

test("latency is half the kernel at each quality and rate", () => {
  assert.deepEqual([0, 1, 2].map((q) => latencyFrames(q, 48000)), [1023, 2047, 4095]);
  assert.equal(latencyFrames(1, 44100), 2047);
  assert.equal(latencyFrames(1, 96000), 4095);
  assert.equal(latencyFrames(2, 192000), 16383);
});
