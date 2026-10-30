import { test } from "node:test";
import assert from "node:assert/strict";
import { boxAt, curveThrough, laneBoxes, nextParam, segmentAt, segmentSamples, shape, subLaneHeight, usedLayout, valueAt, withCurve, MIN_SUB_LANE } from "../src/autoLanes.ts";
import type { AutoLane, AutoParam, AutoPoint, Track } from "../src/api.ts";

const VOL: AutoParam = { type: "volume" };
const PAN: AutoParam = { type: "pan" };
const MUTE: AutoParam = { type: "mute" };
const lane = (param: AutoParam, n = 1): AutoLane => ({ param, points: Array.from({ length: n }, (_, k) => ({ pos: k * 100, value: 0 })) });

test("a straight segment is the plain line, and curves meet their points exactly", () => {
  const pts: AutoPoint[] = [
    { pos: 0, value: -30 },
    { pos: 1000, value: 0 },
  ];
  assert.equal(valueAt(pts, 250), -22.5);
  for (const c of [-1, -0.4, 0.3, 1]) {
    const bent: AutoPoint[] = [{ ...pts[0], curve: c }, pts[1]];
    assert.equal(valueAt(bent, 0), -30);
    assert.equal(valueAt(bent, 1000), 0);
    assert.equal(valueAt(bent, -5), -30);
    assert.equal(valueAt(bent, 5000), 0);
    assert.equal(shape(0, c), 0);
    assert.equal(shape(1, c), 1);
  }
});

test("positive curves change early, negative ones late, monotonically", () => {
  for (const c of [-0.8, 0.8]) {
    let last = -1;
    for (let k = 0; k <= 100; k++) {
      const f = shape(k / 100, c);
      assert.ok(f >= last, `curve ${c} at ${k}`);
      last = f;
    }
  }
  assert.ok(shape(0.5, 0.5) > 0.7);
  assert.ok(shape(0.5, -0.5) < 0.3);
  // The same shape as the engine: t^(2^(-3c)).
  assert.ok(Math.abs(shape(0.25, 1) - Math.pow(0.25, 1 / 8)) < 1e-12);
});

test("a bend passes the curve through the pointer, and snaps back to straight", () => {
  for (const [t, f] of [
    [0.5, 0.8],
    [0.3, 0.1],
    [0.7, 0.95],
  ]) {
    const c = curveThrough(t, f);
    assert.ok(Math.abs(shape(t, c) - f) < 0.01, `t ${t} f ${f}: ${shape(t, c)}`);
  }
  assert.equal(curveThrough(0.5, 0.5), 0);
  assert.equal(curveThrough(0.5, 0.51), 0);
  assert.equal(curveThrough(0.5, 1), 1);
});

test("segments and their curves", () => {
  const pts: AutoPoint[] = [
    { pos: 0, value: 0 },
    { pos: 100, value: 1 },
    { pos: 100, value: 0 },
    { pos: 200, value: 1 },
  ];
  assert.equal(segmentAt(pts, 50), 0);
  assert.equal(segmentAt(pts, 100), 2);
  assert.equal(segmentAt(pts, 250), -1);
  const bent = withCurve(pts, 0, 0.5);
  assert.equal(bent[0].curve, 0.5);
  assert.equal(pts[0].curve, undefined, "the old points are left alone");
  assert.equal("curve" in withCurve(bent, 0, 0)[0], false, "straight again drops the field");
  assert.equal(segmentSamples(pts[0], 0, 100).length, 1);
  const s = segmentSamples({ pos: 0, value: 0, curve: 0.5 }, 0, 300);
  assert.ok(s.length >= 50);
  assert.equal(s[s.length - 1].x, 300);
  assert.equal(s[s.length - 1].f, 1);
});

test("Show Used Automation keeps a used main lane and shows the rest", () => {
  const lanes = [lane(VOL), lane(PAN, 0), lane(MUTE)];
  assert.deepEqual(usedLayout(lanes, VOL), { main: VOL, subs: [MUTE] });
  // The main lane shows an empty parameter: it takes the first used one.
  assert.deepEqual(usedLayout(lanes, PAN), { main: VOL, subs: [MUTE] });
  // Nothing used: nothing changes.
  assert.deepEqual(usedLayout([], PAN), { main: PAN, subs: [] });
});

test("a new lane shows the next used parameter, then the next free one", () => {
  const lanes = [lane(VOL), lane(MUTE)];
  assert.deepEqual(nextParam([VOL], lanes, [VOL, PAN, MUTE]), MUTE);
  assert.deepEqual(nextParam([VOL, MUTE], lanes, [VOL, PAN, MUTE]), PAN);
  assert.equal(nextParam([VOL, PAN, MUTE], lanes, [VOL, PAN, MUTE]), null);
});

test("lane boxes: the main lane, then each sub-lane below", () => {
  const track = { id: 3, auto_lanes: [PAN, MUTE] } as unknown as Track;
  const plain = { id: 4 } as unknown as Track;
  const rows = [
    { track, top: 0, main: 60, subsTop: 80, sub: 60 },
    { track: plain, top: 200, main: 40, subsTop: 240, sub: 40 },
  ];
  const boxes = laneBoxes(rows, () => VOL);
  assert.deepEqual(
    boxes.map((b) => [b.track.id, b.param.type, b.top, b.height, b.sub]),
    [
      [3, "volume", 0, 60, -1],
      [3, "pan", 80, 60, 0],
      [3, "mute", 140, 60, 1],
      [4, "volume", 200, 40, -1],
    ],
  );
  assert.equal(boxAt(boxes, 100)?.param.type, "pan");
  // Take rows (60..80) belong to no lane.
  assert.equal(boxAt(boxes, 70), null);
  assert.equal(boxAt(boxes, 239)?.track.id, 4);
  assert.equal(subLaneHeight(22), MIN_SUB_LANE);
  assert.equal(subLaneHeight(90), 90);
});
