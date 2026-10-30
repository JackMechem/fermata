// Unit tests for src/regionSelect.ts, Edit › Select's criteria.
import { test } from "node:test";
import assert from "node:assert/strict";
import { following, followingFrom, insideLocators, invert, matching, muted, overlapped, sameColor, sameContent, similarContent } from "../src/regionSelect.ts";
import type { Note, Region } from "../src/api.ts";

const audio = (id: number, start: number, length: number, clip = 1, offset = 0, extra: Partial<Region> = {}): Region => ({
  id,
  name: `A${id}`,
  start,
  length,
  muted: false,
  content: { type: "audio", clip, offset, gain_db: 0, fade_in: 0, fade_out: 0 },
  ...extra,
});

const note = (start: number, pitch: number, velocity = 100): Note => ({ start, length: 100, pitch, velocity });
const midi = (id: number, start: number, notes: Note[], length = 1000): Region => ({ id, name: `M${id}`, start, length, muted: false, content: { type: "midi", notes, cc: [] } });

const sorted = (v: number[]) => [...v].sort((a, b) => a - b);

// Track 1 (red): 1 at 0..1000, 2 at 500..1500 (overlapping 1), 3 at 3000..4000 (muted, blue).
// Track 2 (green): 4 at 0..1000 (same audio as 1), 5 at 2000..3000 (same file, later stretch),
// 6 looping 4000..5000 up to 8000.
const tracks = [
  { id: 10, color: "#FF0000", regions: [audio(1, 0, 1000), audio(2, 500, 1000, 2), audio(3, 3000, 1000, 3, 0, { muted: true, color: "#0000ff" })] },
  { id: 20, color: "#00ff00", regions: [audio(4, 0, 1000), audio(5, 2000, 1000, 1, 5000), audio(6, 4000, 1000, 4, 0, { loop_end: 8000 })] },
];

test("all following: from the first selected region or the playhead, on every track or one", () => {
  assert.equal(followingFrom(tracks, [5, 3], 0), 2000);
  assert.equal(followingFrom(tracks, [], 700), 700);
  assert.deepEqual(sorted(following(tracks, 500)), [2, 3, 5, 6]);
  assert.deepEqual(sorted(following(tracks, 500, 20)), [5, 6]);
});

test("inside the locators: wholly between them, loops included", () => {
  assert.deepEqual(sorted(insideLocators(tracks, 0, 4000)), [1, 2, 3, 4, 5]);
  assert.deepEqual(insideLocators(tracks, 3500, 9000), [6]);
  assert.deepEqual(insideLocators(tracks, 3500, 6000), [], "the loop sounds to 8000");
});

test("muted, overlapped and inverted", () => {
  assert.deepEqual(muted(tracks), [3]);
  assert.deepEqual(sorted(overlapped(tracks)), [1, 2]);
  assert.deepEqual(sorted(invert(tracks, [1, 2, 3])), [4, 5, 6]);
});

test("same colour: the region's own, or its track's, in any case", () => {
  assert.deepEqual(sorted(sameColor(tracks, [1])), [1, 2]);
  assert.deepEqual(sameColor(tracks, [3]), [3]);
  const recoloured = [{ ...tracks[0] }, { ...tracks[1], regions: [audio(7, 0, 10, 1, 0, { color: "#ff0000" })] }];
  assert.deepEqual(sorted(sameColor(recoloured, [1])), [1, 2, 7]);
});

test("equal regions play the same; similar ones come from the same file", () => {
  assert.deepEqual(sorted(matching(tracks, [1], sameContent)), [1, 4]);
  assert.deepEqual(sorted(matching(tracks, [1], similarContent)), [1, 4, 5]);
  // MIDI: equal needs the same notes; similar the same pitches in order.
  const a = midi(1, 0, [note(0, 60), note(100, 64)]);
  const b = midi(2, 5000, [note(0, 60), note(100, 64)]);
  const c = midi(3, 0, [note(0, 60, 40), note(400, 64)]);
  const d = midi(4, 0, [note(0, 64), note(100, 60)]);
  assert.ok(sameContent(a, b) && !sameContent(a, c));
  assert.ok(similarContent(a, c) && !similarContent(a, d));
  assert.ok(!sameContent(a, { ...b, length: 2000 }));
  assert.ok(!similarContent(midi(5, 0, []), midi(6, 0, [])), "empty regions are like nothing");
  assert.ok(!sameContent(a, audio(9, 0, 1000)));
});
