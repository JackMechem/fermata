// Unit tests for src/dragModes.ts, the drag preview's copy of crates/engine/src/drag_modes.rs.
// The cases are the Rust module's, so the two stay in step.
import { test } from "node:test";
import assert from "node:assert/strict";
import { dragOutcome, noOverlap, shuffle, shuffleResize, shuffleSide, type Span } from "../src/dragModes.ts";

const s = (id: number, start: number, len: number): Span => ({ id, start, len });
const sorted = (m: Map<number, number>) => [...m.entries()].sort((a, b) => a[0] - b[0]);

test("No Overlap shortens the earlier region", () => {
  assert.deepEqual([...noOverlap([s(1, 0, 100), s(2, 60, 100)], [2])], [[1, 60]]);
  assert.deepEqual([...noOverlap([s(1, 0, 100), s(2, 60, 100)], [1])], [[1, 60]]);
  assert.deepEqual([...noOverlap([s(1, 0, 1000), s(2, 300, 100)], [2])], [[1, 300]]);
  assert.deepEqual([...noOverlap([s(1, 300, 100), s(2, 0, 1000)], [2])], [[2, 300]]);
  assert.deepEqual([...noOverlap([s(1, 0, 100), s(2, 0, 50)], [2])], [[1, 0]]);
  assert.equal(noOverlap([s(1, 0, 100), s(2, 100, 50)], [2]).size, 0);
  assert.equal(noOverlap([s(1, 0, 100), s(2, 50, 100), s(3, 500, 10)], [3]).size, 0);
});

test("Shuffle L delete pulls the following regions left; Shuffle R the ones before right", () => {
  const lane = [s(1, 0, 10), s(2, 10, 20), s(3, 30, 10), s(4, 50, 10)];
  assert.deepEqual(sorted(shuffle(lane, [2], [], "left")), [[1, 0], [3, 10], [4, 30]]);
  assert.deepEqual(sorted(shuffle(lane, [1, 2], [], "left")), [[3, 0], [4, 20]]);
  assert.deepEqual(sorted(shuffle(lane, [3], [], "right")), [[1, 10], [2, 20], [4, 50]]);
});

test("Shuffle L drags swap neighbours and snap to the region before", () => {
  const lane = [s(1, 0, 10), s(2, 10, 20), s(3, 30, 10)];
  assert.deepEqual(sorted(shuffle(lane, [1], [s(1, 22, 10)], "left")), [[1, 20], [2, 0], [3, 30]]);
  assert.deepEqual(sorted(shuffle(lane, [1], [s(1, 14, 10)], "left")), [[1, 0], [2, 10], [3, 30]]);
  assert.deepEqual(sorted(shuffle(lane, [3], [s(3, 2, 10)], "left")), [[1, 10], [2, 20], [3, 0]]);
  assert.deepEqual(sorted(shuffle(lane, [1], [s(1, 90, 10)], "left")), [[1, 30], [2, 0], [3, 20]]);
});

test("Shuffle L inserts make room only as needed", () => {
  const lane = [s(1, 0, 10), s(2, 50, 10)];
  assert.deepEqual(sorted(shuffle(lane, [], [s(9, 30, 10)], "left")), [[1, 0], [2, 50], [9, 10]]);
  assert.deepEqual(sorted(shuffle(lane, [], [s(9, 30, 45)], "left")), [[1, 0], [2, 55], [9, 10]]);
  assert.deepEqual(sorted(shuffle([s(1, 100, 10)], [], [s(9, 20, 10)], "left")), [[1, 100], [9, 20]]);
  assert.deepEqual(sorted(shuffle([s(1, 0, 10), s(2, 10, 10)], [], [s(8, 6, 5), s(9, 15, 5)], "left")), [[1, 0], [2, 24], [8, 10], [9, 19]]);
});

test("Shuffle R mirrors Shuffle L", () => {
  const lane = [s(1, 100, 10), s(2, 110, 20), s(3, 130, 10)];
  assert.deepEqual(sorted(shuffle(lane, [3], [s(3, 112, 10)], "right")), [[1, 100], [2, 110], [3, 130]]);
  assert.deepEqual(sorted(shuffle(lane, [3], [s(3, 101, 10)], "right")), [[1, 100], [2, 120], [3, 110]]);
  assert.deepEqual(sorted(shuffle([s(1, 100, 10)], [], [s(9, 20, 10)], "right")), [[1, 100], [9, 90]]);
  assert.deepEqual(sorted(shuffle([s(1, 0, 10), s(2, 10, 10)], [], [s(9, 15, 10)], "right")), [[1, 0], [2, 10], [9, 20]]);
});

test("Shuffle trims keep the anchored edge", () => {
  const lane = [s(1, 0, 10), s(2, 10, 20), s(3, 30, 10), s(4, 60, 10)];
  assert.deepEqual(sorted(shuffleResize(lane, 2, 25, "left")), [[1, 0], [2, 10], [3, 35], [4, 65]]);
  assert.deepEqual(sorted(shuffleResize(lane, 2, 5, "left")), [[1, 0], [2, 10], [3, 15], [4, 45]]);
  assert.deepEqual(sorted(shuffleResize(lane, 3, 5, "right")), [[1, 5], [2, 15], [3, 35], [4, 60]]);
  assert.deepEqual(sorted(shuffleResize(lane, 1, 15, "right")), [[1, 0], [2, 15], [3, 35], [4, 65]]);
});

test("only the Shuffle modes have a side", () => {
  assert.equal(shuffleSide("shuffle_l"), "left");
  assert.equal(shuffleSide("shuffle_r"), "right");
  assert.equal(shuffleSide("no_overlap"), null);
  assert.equal(shuffleSide("xfade"), null);
});

test("a drag's outcome covers the lanes it leaves and lands on", () => {
  const lanes = new Map([
    [100, [s(1, 0, 10), s(2, 10, 10), s(3, 20, 10)]],
    [200, [s(4, 0, 10)]],
  ]);
  // Shuffle L: 2 goes to the other track, before 4; 3 closes up behind it.
  const moved = dragOutcome("shuffle_l", lanes, { kind: "move", moved: [{ id: 2, track: 200, start: 1 }] });
  assert.deepEqual(
    [...moved.entries()].sort((a, b) => a[0] - b[0]),
    [
      [1, { start: 0 }],
      [2, { start: 0 }],
      [3, { start: 10 }],
      [4, { start: 10 }],
    ],
  );
  // No Overlap: 3 moved onto 1's end shortens 1.
  assert.deepEqual([...dragOutcome("no_overlap", lanes, { kind: "move", moved: [{ id: 3, track: 100, start: 5 }] })], [[1, { len: 5 }], [3, { len: 5 }]]);
  // A trim: Shuffle R keeps 2's end and moves 1; No Overlap stops it at 3.
  assert.deepEqual([...dragOutcome("shuffle_r", lanes, { kind: "resize", id: 2, start: 10, len: 5 })].sort((a, b) => a[0] - b[0]), [
    [1, { start: 5 }],
    [2, { start: 15 }],
    [3, { start: 20 }],
  ]);
  assert.deepEqual([...dragOutcome("no_overlap", lanes, { kind: "resize", id: 2, start: 10, len: 15 })], [[2, { len: 10 }]]);
  assert.equal(dragOutcome("overlap", lanes, { kind: "resize", id: 2, start: 10, len: 15 }).size, 0);
});
