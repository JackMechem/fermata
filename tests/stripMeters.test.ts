// Unit tests for src/stripMeters.ts (the channel strip's clip indicator and gain-reduction
// meter). Run with `npm test`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { clipResetTargets, GR_FALL, GR_RANGE, grFraction, isClip, nextGr, peakText, reducesGain } from "../src/stripMeters.ts";

test("only a peak over full scale is a clip", () => {
  assert.equal(isClip(0), false);
  assert.equal(isClip(1), false);
  assert.equal(isClip(1.0001), true);
  assert.equal(isClip(Number.NaN), false);
});

test("the peak readout shows dBFS to a tenth", () => {
  assert.equal(peakText(0), "-∞");
  assert.equal(peakText(-1), "-∞");
  assert.equal(peakText(1e-6), "-∞");
  assert.equal(peakText(Number.NaN), "-∞");
  assert.equal(peakText(1), "0.0");
  assert.equal(peakText(0.5), "-6.0");
  assert.equal(peakText(2), "+6.0");
  assert.equal(peakText(10 ** (-21.5 / 20)), "-21.5");
  // Just under full scale rounds to 0.0, never "-0.0".
  assert.equal(peakText(0.99999), "0.0");
});

test("Alt-click resets every strip", () => {
  assert.deepEqual(clipResetTargets(7, false), [7]);
  assert.equal(clipResetTargets(7, true), null);
});

test("the gain-reduction meter spaces small reductions out", () => {
  assert.equal(grFraction(0), 0);
  assert.equal(grFraction(-3), 0);
  assert.equal(grFraction(GR_RANGE), 1);
  assert.equal(grFraction(100), 1);
  assert.ok(grFraction(3) > 0.3, "3 dB takes a third of the meter");
  assert.ok(grFraction(1) < grFraction(2) && grFraction(2) < grFraction(6));
});

test("gain reduction jumps up and falls back slowly", () => {
  assert.equal(nextGr(0, 6, 0.016), 6);
  assert.equal(nextGr(6, 2, 0.05), 6 - GR_FALL * 0.05);
  assert.equal(nextGr(6, undefined, 1), 0, "never below zero");
  assert.equal(nextGr(1, 4, 0.1), 4);
});

test("dynamics effects are told by their gain-reduction meter", () => {
  assert.equal(reducesGain(["Input", "Output", "Gain Reduction", "Level"]), true);
  assert.equal(reducesGain(["Input", "Key", "Reduction"]), true);
  assert.equal(reducesGain(["momentary", "short_term"]), false);
  assert.equal(reducesGain([]), false);
});
