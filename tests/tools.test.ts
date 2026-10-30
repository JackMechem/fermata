// Unit tests for src/tools.ts (the Tracks area's tools) and src/marquee.ts (the marquee).
import { test } from "node:test";
import assert from "node:assert/strict";
import { DEFAULT_TOOLS, equalPieces, fadeToolEdit, MAX_PIECES, nearerEnd, toolFor, toolForKey, TRACK_TOOLS, type FadeRegion } from "../src/tools.ts";
import { dragMarquee, inMarquee, marqueeRows, tracksBetween } from "../src/marquee.ts";

test("the Tool menu's keys are Logic's, and each picks one tool", () => {
  assert.equal(toolForKey("t"), "pointer");
  assert.equal(toolForKey("T"), "pointer");
  assert.equal(toolForKey("Escape"), "pointer");
  assert.equal(toolForKey("p"), "pencil");
  assert.equal(toolForKey("e"), "eraser");
  assert.equal(toolForKey("i"), "scissors");
  assert.equal(toolForKey("g"), "glue");
  assert.equal(toolForKey("m"), "mute");
  assert.equal(toolForKey("a"), "fade");
  assert.equal(toolForKey("r"), "marquee");
  assert.equal(toolForKey("x"), null);
  assert.equal(toolForKey("Enter"), null);
  assert.equal(new Set(TRACK_TOOLS.map((t) => t.key)).size, TRACK_TOOLS.length);
});

test("the Command-click tool is used while the modifier is held", () => {
  assert.equal(toolFor(DEFAULT_TOOLS, false), "pointer");
  assert.equal(toolFor(DEFAULT_TOOLS, true), "marquee");
  assert.equal(toolFor({ left: "scissors", cmd: "glue" }, true), "glue");
});

test("an Alt-click with the Scissors cuts equal pieces", () => {
  assert.deepEqual(equalPieces(1000, 2000, 1250), [1250, 1500, 1750]);
  // The last piece is whatever is left.
  assert.deepEqual(equalPieces(0, 1000, 300), [300, 600, 900]);
  // Not inside the region: nothing.
  assert.deepEqual(equalPieces(1000, 2000, 1000), []);
  assert.deepEqual(equalPieces(1000, 2000, 900), []);
  assert.deepEqual(equalPieces(1000, 2000, 2000), []);
  // A click right after the start doesn't make millions of pieces.
  assert.equal(equalPieces(0, 1e9, 1).length, MAX_PIECES - 1);
});

const reg = (id: number, start: number, length: number, fadeIn = 0, fadeOut = 0): FadeRegion => ({ id, start, length, fadeIn, fadeOut });

test("the Fade tool fades the nearer end over the drag", () => {
  const r = reg(1, 1000, 1000);
  // Over the start, either way round.
  assert.deepEqual(fadeToolEdit(r, undefined, undefined, 900, 1200), { kind: "in", id: 1, length: 200 });
  assert.deepEqual(fadeToolEdit(r, undefined, undefined, 1300, 1000), { kind: "in", id: 1, length: 300 });
  // Over the end.
  assert.deepEqual(fadeToolEdit(r, undefined, undefined, 1700, 2100), { kind: "out", id: 1, length: 300 });
  // An existing fade-out limits the fade-in, and the other way round.
  assert.deepEqual(fadeToolEdit(reg(1, 1000, 1000, 0, 600), undefined, undefined, 1000, 1450), { kind: "in", id: 1, length: 400 });
  assert.deepEqual(fadeToolEdit(reg(1, 1000, 1000, 700, 0), undefined, undefined, 1600, 2000), { kind: "out", id: 1, length: 300 });
  // No drag, no fade.
  assert.equal(fadeToolEdit(r, undefined, undefined, 1500, 1500), null);
});

test("the Fade tool crossfades across the boundary with a neighbour", () => {
  const a = reg(1, 0, 1000);
  const b = reg(2, 1000, 1000);
  assert.deepEqual(fadeToolEdit(a, undefined, b, 900, 1100), { kind: "xfade", id: 1, length: 200 });
  // Pressed on the second region, dragging back over the boundary: still a's crossfade.
  assert.deepEqual(fadeToolEdit(b, a, undefined, 1150, 950), { kind: "xfade", id: 1, length: 200 });
  // Short of the boundary: a fade on the region pressed.
  assert.deepEqual(fadeToolEdit(a, undefined, b, 800, 950), { kind: "out", id: 1, length: 200 });
  // Overlapping regions meet in the middle of the overlap.
  const c = reg(3, 800, 1000);
  assert.deepEqual(fadeToolEdit(a, undefined, c, 850, 950), { kind: "xfade", id: 1, length: 100 });
});

test("an Alt-click clears the nearer fade", () => {
  assert.equal(nearerEnd(0, 1000, 400), "in");
  assert.equal(nearerEnd(0, 1000, 600), "out");
});

test("a marquee covers the tracks in view between the two rows", () => {
  const ids = [10, 11, 12, 13];
  const laneH = [60, 0, 60, 60];
  assert.deepEqual(tracksBetween(ids, laneH, 3, 0), [10, 12, 13]);
  assert.deepEqual(tracksBetween(ids, laneH, 1, 1), []);
  assert.deepEqual(dragMarquee(ids, laneH, 5000, 2, 1000, 0), { start: 1000, end: 5000, tracks: [10, 12] });
  assert.equal(dragMarquee(ids, laneH, 1000, 0, 1000, 2), null, "no length");
  // Shift-drag grows the old marquee.
  const base = { start: 1000, end: 2000, tracks: [10] };
  assert.deepEqual(dragMarquee(ids, laneH, 3000, 3, 4000, 3, base), { start: 1000, end: 4000, tracks: [10, 12, 13] });
});

test("points inside the marquee, and the rows it spans", () => {
  const m = { start: 1000, end: 2000, tracks: [11, 12] };
  assert.ok(inMarquee(m, 11, 1500));
  assert.ok(inMarquee(m, 12, 2000));
  assert.ok(!inMarquee(m, 10, 1500));
  assert.ok(!inMarquee(m, 11, 2500));
  assert.ok(!inMarquee(null, 11, 1500));
  assert.deepEqual(marqueeRows(m, [10, 11, 12], [0, 60, 120], [58, 58, 40]), { top: 60, bottom: 160 });
  assert.equal(marqueeRows(m, [10, 11, 12], [0, 60, 120], [58, 0, 0]), null);
});
