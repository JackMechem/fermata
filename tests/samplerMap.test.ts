import { test } from "node:test";
import assert from "node:assert/strict";
import { dragZone, grabOf, isSfzPath, keyAt, keyName, moveHandle, parseKey, pointsOf, velAt, zoneAt, zoneRect, zoomed, type ZoneView } from "../src/samplerMap.ts";

const zone = (over: Partial<ZoneView> = {}): ZoneView => ({
  id: 1,
  name: "z",
  file: "a.wav",
  key_lo: 0,
  key_hi: 127,
  vel_lo: 0,
  vel_hi: 127,
  root: 60,
  fine: 0,
  gain_db: 0,
  pan: 0,
  start: 0,
  end: 0,
  loop_on: false,
  loop_start: 0,
  loop_end: 0,
  xfade: 0,
  equal_power: false,
  one_shot: false,
  reverse: false,
  rr_group: 0,
  frames: 1000,
  rate: 48000,
  channels: 1,
  error: null,
  ...over,
});
const box = { width: 128, height: 148, strip: 20 };

test("note names read as the rest of the app, and typed ones come back", () => {
  assert.equal(keyName(60), "C3");
  assert.equal(keyName(0), "C-2");
  for (const [t, n] of [["C3", 60], ["c#3", 61], ["Bb2", 58], ["C-2", 0], ["60", 60], ["F♯1", 42]] as const) assert.equal(parseKey(t), n, t);
  assert.equal(parseKey("H2"), null);
  assert.equal(parseKey(""), null);
});

test("the map's keys and velocities", () => {
  assert.equal(keyAt(box, 60.5), 60);
  assert.equal(keyAt(box, 500), 127);
  assert.equal(velAt(box, 0), 127);
  assert.equal(velAt(box, 128), 0);
  const r = zoneRect(box, zone({ key_lo: 10, key_hi: 19, vel_lo: 64, vel_hi: 127 }));
  assert.deepEqual([r.x, r.w], [10, 10]);
  assert.equal(r.y, 0);
  assert.equal(r.h, 64);
});

test("the topmost zone under the pointer answers", () => {
  const a = zone({ id: 1 });
  const b = zone({ id: 2, key_lo: 50, key_hi: 70 });
  assert.equal(zoneAt(box, [a, b], 60, 50)?.id, 2);
  assert.equal(zoneAt(box, [a, b], 20, 50)?.id, 1);
  assert.equal(zoneAt(box, [zone({ key_hi: 10 })], 60, 50), null);
});

test("edges resize and the middle moves, root along", () => {
  const z = zone({ key_lo: 40, key_hi: 80, vel_lo: 0, vel_hi: 127, root: 60 });
  assert.equal(grabOf(box, z, 40.5, 60), "key_lo");
  assert.equal(grabOf(box, z, 80.5, 60), "key_hi");
  assert.equal(grabOf(box, z, 60, 1), "vel_hi");
  assert.equal(grabOf(box, z, 60, 127), "vel_lo");
  assert.equal(grabOf(box, z, 60, 60), "move");
  const m = dragZone(z, "move", 5, 0);
  assert.deepEqual([m.key_lo, m.key_hi, m.root], [45, 85, 65]);
  // Stops at the keyboard's end without squashing.
  const far = dragZone(z, "move", 100, 0);
  assert.deepEqual([far.key_lo, far.key_hi], [87, 127]);
  assert.equal(dragZone(z, "key_lo", 100, 0).key_lo, 80);
  assert.equal(dragZone(z, "vel_hi", 0, -200).vel_hi, 0);
});

test("handles keep their order", () => {
  const z = zone({ loop_on: true, start: 100, end: 900, loop_start: 200, loop_end: 800 });
  assert.equal(moveHandle(z, "start", 950).start, 898);
  assert.equal(moveHandle(z, "start", 500).loop_start, 500);
  assert.equal(moveHandle(z, "end", 50).end, 102);
  assert.equal(moveHandle(z, "end", 500).loop_end, 500);
  assert.equal(moveHandle(z, "loop_start", 850).loop_start, 798);
  assert.equal(moveHandle(z, "loop_end", 100).loop_end, 202);
  assert.deepEqual(pointsOf(zone()), { start: 0, end: 1000, loop_start: 0, loop_end: 1000 });
});

test("zooming holds the frame under the pointer, within the file", () => {
  const [a, b] = zoomed(0, 1000, 1000, 500, 0.5);
  assert.deepEqual([a, b], [250, 750]);
  assert.deepEqual(zoomed(0, 1000, 1000, 0, 0.5), [0, 500]);
  assert.deepEqual(zoomed(250, 750, 1000, 500, 10), [0, 1000]);
  assert.equal(zoomed(0, 40, 1000, 10, 0.1)[1] - zoomed(0, 40, 1000, 10, 0.1)[0], 32);
  assert.ok(isSfzPath("/a/B.SFZ") && !isSfzPath("/a/b.wav"));
});
