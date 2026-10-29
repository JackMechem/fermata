// Unit tests for the scrolling helpers: src/wheel.ts (Settings › Editing › Mouse Wheel
// Scrolls) and revealLeft in src/view.ts (bringing the playhead into view).
import { test } from "node:test";
import assert from "node:assert/strict";
import { timeStripWheel, timeWheel } from "../src/wheel.ts";
import { onRevealPlayhead, revealLeft, revealPlayhead } from "../src/view.ts";

const wheel = (deltaY: number, deltaX = 0, shiftKey = false, deltaMode = 0) => ({ deltaX, deltaY, deltaMode, shiftKey });

test("in the horizontal setting the mouse wheel scrolls time", () => {
  assert.deepEqual(timeWheel(wheel(100)), { dx: 100, dy: 0 });
  assert.deepEqual(timeWheel(wheel(-40)), { dx: -40, dy: 0 });
});

test("Shift+wheel scrolls the tracks instead", () => {
  assert.deepEqual(timeWheel(wheel(100, 0, true)), { dx: 0, dy: 100 });
  // Platforms that already turn Shift+wheel into a horizontal delta.
  assert.deepEqual(timeWheel(wheel(0, 100, true)), { dx: 0, dy: 100 });
});

test("a trackpad's sideways swipe still scrolls time", () => {
  assert.deepEqual(timeWheel(wheel(2, 30)), { dx: 30, dy: 0 });
  assert.deepEqual(timeWheel(wheel(0, -12)), { dx: -12, dy: 0 });
});

test("line and page deltas become pixels", () => {
  assert.deepEqual(timeWheel(wheel(3, 0, false, 1)), { dx: 48, dy: 0 });
  assert.deepEqual(timeWheel(wheel(1, 0, false, 2), 640), { dx: 640, dy: 0 });
});

test("strips that only scroll in time follow the setting", () => {
  // Vertical (default): sideways movement and Shift+wheel only, as before.
  assert.equal(timeStripWheel(wheel(100), "vertical"), 0);
  assert.equal(timeStripWheel(wheel(0, 25), "vertical"), 25);
  assert.equal(timeStripWheel(wheel(100, 0, true), "vertical"), 100);
  assert.equal(timeStripWheel(wheel(0, 100, true), "vertical"), 100);
  // Horizontal: the plain wheel too, and Shift+wheel has nothing to scroll.
  assert.equal(timeStripWheel(wheel(100), "horizontal"), 100);
  assert.equal(timeStripWheel(wheel(100, 0, true), "horizontal"), 0);
});

test("revealLeft leaves a visible playhead alone", () => {
  assert.equal(revealLeft(500, 400, 800), null);
  assert.equal(revealLeft(400, 400, 800), null);
});

test("revealLeft scrolls back to show the playhead near the left edge", () => {
  // Back to the start from far along: the very beginning.
  assert.equal(revealLeft(0, 5000, 800), 0);
  assert.equal(revealLeft(30, 5000, 800), 0);
  // To the left locator, a little in from the edge.
  assert.equal(revealLeft(2000, 5000, 800), 1940);
  // Ahead of the view, or in the strip at the right edge.
  assert.equal(revealLeft(9000, 0, 800), 8940);
  assert.equal(revealLeft(790, 0, 800), 730);
  assert.equal(revealLeft(2000, 5000, 800, 24), 1976);
});

test("revealPlayhead reaches every subscribed view until it unsubscribes", () => {
  const got: number[] = [];
  const off = onRevealPlayhead((p) => got.push(p));
  revealPlayhead(42);
  off();
  revealPlayhead(7);
  assert.deepEqual(got, [42]);
});
