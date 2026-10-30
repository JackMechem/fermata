import { test } from "node:test";
import assert from "node:assert/strict";
import { isTrackOff, offTracks, powerTitle } from "../src/trackOff.ts";

const stack = { id: 1, parent: null, off: true };
const inner = { id: 2, parent: 1 };
const deep = { id: 3, parent: 2 };
const free = { id: 4, parent: null };
const own = { id: 5, parent: null, off: true };
const tracks = [stack, inner, deep, free, own];

test("a track is off through its own switch or a stack above it", () => {
  assert.equal(isTrackOff(tracks, free), false);
  assert.equal(isTrackOff(tracks, own), true);
  assert.equal(isTrackOff(tracks, inner), true);
  assert.equal(isTrackOff(tracks, deep), true);
  assert.deepEqual([...offTracks(tracks)].sort(), [1, 2, 3, 5]);
});

test("older projects' tracks, which have no field, are on", () => {
  assert.equal(isTrackOff([{ id: 9, parent: null }], { id: 9, parent: null }), false);
});

test("a parent loop or a missing parent doesn't hang", () => {
  const a = { id: 1, parent: 2 };
  const b = { id: 2, parent: 1 };
  assert.equal(isTrackOff([a, b], a), false);
  assert.equal(isTrackOff([], { id: 7, parent: 8 }), false);
});

test("the tooltip says why a track is off", () => {
  assert.match(powerTitle(tracks, own), /Click to turn it on/);
  assert.match(powerTitle(tracks, inner), /with its stack/);
  assert.match(powerTitle(tracks, free), /Click to turn it off/);
});
