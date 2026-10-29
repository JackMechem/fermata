// Unit tests for src/audioRange.ts, the Audio Track editor's range arithmetic.
import { test } from "node:test";
import assert from "node:assert/strict";
import { regionsIn, subRegion } from "../src/audioRange.ts";
import type { Region, Track } from "../src/api.ts";

const audio = (id: number, start: number, length: number, extra: Partial<Extract<Region["content"], { type: "audio" }>> = {}): Region => ({
  id,
  name: `R${id}`,
  start,
  length,
  muted: false,
  content: { type: "audio", clip: 1, offset: 100, gain_db: 0, fade_in: 50, fade_out: 80, ...extra },
});

test("regionsIn finds the regions overlapping a half-open range", () => {
  const t = { regions: [audio(1, 0, 100), audio(2, 100, 100), audio(3, 300, 50)] } as Track;
  assert.deepEqual(regionsIn(t, 50, 150).map((r) => r.id), [1, 2]);
  // Touching ends don't count.
  assert.deepEqual(regionsIn(t, 100, 300).map((r) => r.id), [2]);
  assert.deepEqual(regionsIn(t, 200, 300), []);
});

test("subRegion of audio shifts the file offset and keeps only outer fades", () => {
  const r = audio(1, 1000, 1000);
  // From the middle: no fade-in; the region's own end keeps its fade-out.
  const tail = subRegion(r, 1400, 5000);
  assert.equal(tail.start, 1400);
  assert.equal(tail.length, 600);
  assert.deepEqual(tail.content, { type: "audio", clip: 1, offset: 500, gain_db: 0, fade_in: 0, fade_out: 80 });
  // Wholly inside: no fades at all.
  const mid = subRegion(r, 1200, 1300);
  assert.equal(mid.content.type === "audio" && mid.content.fade_in + mid.content.fade_out, 0);
  // The whole region comes back as it is (a loop is not part of the copy).
  const whole = subRegion({ ...r, loop_end: 4000 }, 0, 9999);
  assert.equal(whole.loop_end, null);
  assert.deepEqual(whole.content, r.content);
});

test("subRegion of MIDI keeps the notes and controllers inside, region-relative", () => {
  const r: Region = {
    id: 2,
    name: "M",
    start: 0,
    length: 1000,
    muted: false,
    content: {
      type: "midi",
      notes: [
        { start: 100, length: 10, pitch: 60, velocity: 100, muted: false },
        { start: 600, length: 10, pitch: 62, velocity: 100, muted: false },
      ],
      cc: [
        { start: 50, kind: { type: "controller", cc: 1 }, value: 1 },
        { start: 700, kind: { type: "controller", cc: 1 }, value: 2 },
      ],
    },
  };
  const part = subRegion(r, 500, 1000);
  assert.equal(part.start, 500);
  assert.ok(part.content.type === "midi");
  assert.deepEqual(part.content.notes.map((n) => n.start), [100]);
  assert.deepEqual(part.content.cc?.map((c) => [c.start, c.value]), [[200, 2]]);
});
