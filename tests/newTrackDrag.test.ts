// Unit tests for src/newTrackDrag.ts: which source lanes a drag past the last track makes new
// tracks for.
import { test } from "node:test";
import assert from "node:assert/strict";
import { dragOutcome } from "../src/dragModes.ts";
import { lanesPast, newLaneId, newLanes, sourceTracks, splitDrop } from "../src/newTrackDrag.ts";

const entries = (m: Map<number, number>) => [...m.entries()];

test("lanes past the end count from the first pixel below the last track", () => {
  assert.equal(lanesPast(0, 300, 60), 0);
  assert.equal(lanesPast(299.5, 300, 60), 0);
  assert.equal(lanesPast(300, 300, 60), 1);
  assert.equal(lanesPast(359, 300, 60), 1);
  assert.equal(lanesPast(360, 300, 60), 2);
  assert.equal(lanesPast(1000, 300, 0), 701, "a zero lane height can't divide by zero");
});

test("a drag where every lane stays on the tracks makes no lanes", () => {
  assert.equal(newLanes([60, 60, 60], [0], 0).size, 0);
  assert.equal(newLanes([60, 60, 60], [0, 1], 1).size, 0);
  assert.equal(newLanes([60, 60, 60], [0, 1], -1).size, 0);
});

test("a selection's lower lane falls past while the pointer is still on the last track", () => {
  assert.deepEqual(entries(newLanes([60, 60, 60], [1, 2], 1)), [[2, 0]]);
});

test("one region past the end sits on the first new lane however far down the pointer is", () => {
  const h = [60, 60, 60];
  assert.deepEqual(entries(newLanes(h, [2], 1)), [[2, 0]]);
  assert.deepEqual(entries(newLanes(h, [2], 4)), [[2, 0]]);
  assert.deepEqual(entries(newLanes(h, [0], 3)), [[0, 0]]);
  assert.deepEqual(entries(newLanes(h, [0], 9)), [[0, 0]]);
});

test("a selection across lanes: only the lanes past the end are new, in track order", () => {
  const h = [60, 60, 60, 60];
  // Grabbing the lower of two on the last track: it goes first, the other lands on a track.
  assert.deepEqual(entries(newLanes(h, [1, 3], 1)), [[3, 0]]);
  assert.deepEqual(entries(newLanes(h, [2, 3], 1)), [[3, 0]]);
  // Far enough, both go, and the lanes have no gap between them.
  assert.deepEqual(entries(newLanes(h, [2, 3], 2)), [[2, 0], [3, 1]]);
  assert.deepEqual(entries(newLanes(h, [3, 0], 4)), [[0, 0], [3, 1]]);
  // Grabbing the upper one: the lower is past the end as soon as the pointer is.
  assert.deepEqual(entries(newLanes(h, [0, 1], 4)), [[0, 0], [1, 1]]);
});

test("hidden and collapsed tracks don't count as steps", () => {
  // Track 1 is collapsed away, and 3 and 4 hide below the last visible track, 2.
  const h = [60, 0, 60, 0, 0];
  assert.equal(newLanes(h, [0], 1).size, 0, "track 0 lands on track 2");
  assert.deepEqual(entries(newLanes(h, [0], 2)), [[0, 0]]);
  assert.deepEqual(entries(newLanes(h, [2], 1)), [[2, 0]]);
  // A selected region on a collapsed subtrack gets a lane of its own.
  assert.deepEqual(entries(newLanes(h, [0, 1, 2], 2)), [[0, 0], [1, 1], [2, 2]]);
});

test("sourceTracks lists the tracks holding the regions", () => {
  const tracks = [{ regions: [{ id: 5 }] }, { regions: [] }, { regions: [{ id: 6 }, { id: 7 }] }];
  assert.deepEqual(sourceTracks(tracks, [7, 5]), [0, 2]);
  assert.deepEqual(sourceTracks(tracks, [99]), []);
});

test("splitDrop keeps existing-track moves apart and groups the rest by lane", () => {
  const moves = [
    { id: 1, source: 3 },
    { id: 2, source: 0 },
    { id: 3, source: 2 },
    { id: 4, source: 3 },
  ];
  const { onTracks, onNew } = splitDrop(moves, new Map([[3, 1], [2, 0]]));
  assert.deepEqual(onTracks.map((m) => m.id), [2]);
  assert.deepEqual(onNew.map((l) => [l.source, l.moves.map((m) => m.id)]), [[2, [3]], [3, [1, 4]]]);
  assert.deepEqual(splitDrop(moves, new Map()).onNew, []);
});

test("new lanes' stand-in ids can't be a real track's", () => {
  assert.ok(newLaneId(0) < 0 && newLaneId(5) < 0);
  assert.notEqual(newLaneId(0), newLaneId(1));
});

test("the drag preview treats a new lane as an empty one, as the engine does", () => {
  // Shuffle L: the lane left closes up; the region keeps its drop position on the new lane.
  const lanes = new Map([
    [1, [{ id: 10, start: 0, len: 100 }, { id: 11, start: 100, len: 100 }]],
    [newLaneId(0), []],
  ]);
  const out = dragOutcome("shuffle_l", lanes, { kind: "move", moved: [{ id: 10, track: newLaneId(0), start: 700 }] });
  assert.equal(out.get(10)?.start, 700);
  assert.equal(out.get(11)?.start, 0);
});
