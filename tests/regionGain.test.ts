// Unit tests for src/regionGain.ts. Run with `npm test`.
import { test } from "node:test";
import assert from "node:assert/strict";
import * as g from "../src/regionGain.ts";

test("the handle sits at the top centre of the region's visible part", () => {
  const r = g.handleRect(100, 20, 400, 14, 0, 1000)!;
  assert.deepEqual(r, { x: 300 - g.HANDLE_W / 2, y: 21, w: g.HANDLE_W, h: 12 });
  // A long region scrolled partly out of view keeps its handle on screen.
  const long = g.handleRect(-5000, 0, 6000, 14, 0, 800)!;
  assert.equal(long.x, 400 - g.HANDLE_W / 2);
  // Too narrow, or too little of it in view: no handle.
  assert.equal(g.handleRect(0, 0, g.MIN_REGION_W - 1, 14, 0, 1000), null);
  assert.equal(g.handleRect(950, 0, 400, 14, 0, 1000), null);
  assert.ok(g.inRect(r, 300, 25));
  assert.ok(!g.inRect(r, 300, 40));
  assert.ok(!g.inRect(null, 0, 0));
});

test("dragging changes every region's gain by the same amount", () => {
  // Up 8 px is +2 dB, with Shift +0.4 dB; down 40 px is -10 dB.
  assert.equal(g.dragDelta(0, -8, false), 2);
  assert.ok(Math.abs(g.dragDelta(0, -8, true) - 0.4) < 1e-9);
  assert.equal(g.dragDelta(2, 48, false), -10);
  // Never further than the whole range.
  assert.equal(g.dragDelta(0, 10_000, false), -126);
  assert.deepEqual(
    g.shiftedGains(
      [
        [1, 0],
        [2, -3.25],
        [3, 28],
        [4, -90],
      ],
      4,
    ),
    [
      [1, 4],
      [2, 0.8],
      [3, 30],
      [4, -86],
    ],
  );
  assert.deepEqual(g.shiftedGains([[1, -90]], -20), [[1, -96]]);
});

test("labels and waveform scale", () => {
  assert.equal(g.gainLabel(0), "0.0 dB");
  assert.equal(g.gainLabel(1.54), "+1.5 dB");
  assert.equal(g.gainLabel(-6), "−6.0 dB");
  assert.equal(g.gainLabel(-96), "−∞ dB");
  assert.equal(g.gainScale(-96), 0);
  assert.ok(Math.abs(g.gainScale(6) - 1.995) < 0.001);
});
