// Unit tests for src/ccCurves.ts. Run with `npm test`.
import { test } from "node:test";
import assert from "node:assert/strict";
import * as cv from "../src/ccCurves.ts";
import type { PCc } from "../src/midiEdit.ts";

const MOD = { type: "controller", cc: 1 } as const;
const VOL = { type: "controller", cc: 7 } as const;
const ev = (start: number, value: number, sel = false, region = 1, kind: PCc["kind"] = MOD): PCc => ({ region, start, kind, value, sel });

test("curve shapes run from 0 to 1 with their own bends", () => {
  for (const [s] of cv.CURVE_SHAPES) {
    assert.equal(cv.curve(s, 0), 0);
    assert.ok(Math.abs(cv.curve(s, 1) - 1) < 1e-12);
  }
  assert.equal(cv.curve("linear", 0.25), 0.25);
  // Exponential starts slowly, logarithmic quickly, the S-curve is halfway at the middle.
  assert.ok(cv.curve("exponential", 0.5) < 0.25);
  assert.ok(cv.curve("logarithmic", 0.5) > 0.75);
  assert.ok(Math.abs(cv.curve("s", 0.5) - 0.5) < 1e-12);
  assert.ok(cv.curve("s", 0.1) < 0.1 && cv.curve("s", 0.9) > 0.9);
  assert.equal(cv.densityStep(24000, "1/16"), 6000);
});

test("ramps step along the curve without repeating values", () => {
  const lin = cv.rampPoints(0, 0, 1000, 100, "linear", 100, 127);
  assert.deepEqual(
    lin.map((p) => p.value),
    [0, 10, 20, 30, 40, 50, 60, 70, 80, 90, 100],
  );
  // Backwards gives the same events.
  assert.deepEqual(cv.rampPoints(1000, 100, 0, 0, "linear", 100, 127), lin);
  const exp = cv.rampPoints(0, 0, 1000, 100, "exponential", 100, 127);
  assert.ok(exp[5].value < 50 && exp.at(-1)!.value === 100);
  // A flat line is just its ends; values stay in range.
  assert.deepEqual(cv.rampPoints(0, 64, 1000, 64, "s", 100, 127), [
    { start: 0, value: 64 },
    { start: 1000, value: 64 },
  ]);
  assert.ok(cv.rampPoints(0, 0, 100, 500, "linear", 10, 127).every((p) => p.value <= 127));
});

test("a curve replaces the events of its kind and region under it", () => {
  const cc = [ev(0, 5), ev(500, 90), ev(500, 30, false, 1, VOL), ev(500, 30, false, 2), ev(2000, 7)];
  const out = cv.drawCcCurve(cc, 1, MOD, 0, 0, 1000, 100, 250, "s");
  assert.ok(!out.some((c) => c.region === 1 && c.kind === MOD && c.value === 90));
  assert.equal(out.filter((c) => c.region === 1 && c.kind === MOD && c.start <= 1000).length, 5);
  assert.ok(out.some((c) => c.kind === VOL) && out.some((c) => c.region === 2));
  assert.deepEqual(
    out.map((c) => c.start),
    [...out.map((c) => c.start)].sort((a, b) => a - b),
  );
});

test("the nearest event within reach", () => {
  const cc = [ev(0, 0), ev(100, 50), ev(100, 50, false, 1, VOL)];
  const near = cv.nearestEvent(cc, MOD, 103, 52, (t) => t, (v) => v);
  assert.equal(near, cc[1]);
  assert.equal(cv.nearestEvent(cc, MOD, 150, 50, (t) => t, (v) => v), null);
});

test("Create Ramp fills between neighbouring selected events", () => {
  const cc = [ev(0, 0, true), ev(300, 99), ev(1000, 100, true), ev(2000, 0, true), ev(500, 3, true, 1, VOL)];
  assert.ok(cv.canRamp(cc, MOD));
  assert.ok(!cv.canRamp(cc, VOL));
  const out = cv.createRamps(cc, MOD, "linear", 100);
  const mod = out.filter((c) => c.kind === MOD);
  // The unselected event in the first gap is gone; both gaps are filled; the ends stay.
  assert.ok(!mod.some((c) => c.value === 99));
  assert.equal(mod.filter((c) => c.sel).length, 3);
  assert.deepEqual(
    mod.filter((c) => c.start < 1000).map((c) => c.value),
    [0, 10, 20, 30, 40, 50, 60, 70, 80, 90],
  );
  assert.equal(mod.filter((c) => c.start > 1000 && c.start < 2000).length, 9);
  assert.ok(out.some((c) => c.kind === VOL));
  // Nothing to do with one selected event.
  const one = [ev(0, 0, true), ev(10, 5)];
  assert.equal(cv.createRamps(one, MOD, "linear", 1), one);
});

test("Thin Out keeps the shape with fewer events", () => {
  // A straight ramp thins to its ends; a corner is kept.
  const ramp = Array.from({ length: 11 }, (_, i) => ev(i * 100, i * 10, true));
  assert.deepEqual(
    cv.thinOut(ramp, MOD, 1).map((c) => c.start),
    [0, 1000],
  );
  const vee = [...Array.from({ length: 6 }, (_, i) => ev(i * 100, 100 - i * 20, true)), ...Array.from({ length: 5 }, (_, i) => ev(600 + i * 100, 20 + i * 20, true))];
  assert.deepEqual(
    cv.thinOut(vee, MOD, 1).map((c) => c.start),
    [0, 500, 1000],
  );
  // Only the selection, or everything of the kind when nothing is selected.
  const mixed = [ev(0, 0), ev(100, 10), ev(200, 20), ev(300, 30, true), ev(400, 40, true), ev(500, 50, true)];
  assert.deepEqual(
    cv.thinOut(mixed, MOD, 1).map((c) => c.start),
    [0, 100, 200, 300, 500],
  );
  const none = mixed.map((c) => ({ ...c, sel: false }));
  assert.deepEqual(
    cv.thinOut(none, MOD, 1).map((c) => c.start),
    [0, 500],
  );
  // Events of other kinds are left alone.
  assert.equal(cv.thinOut([ev(0, 0, false, 1, VOL), ev(1, 1, false, 1, VOL), ev(2, 2, false, 1, VOL)], MOD, 1).length, 3);
});
