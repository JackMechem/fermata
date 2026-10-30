// Unit tests for src/fades.ts, which mirrors crates/engine/src/fades.rs for drawing.
import { test } from "node:test";
import assert from "node:assert/strict";
import { crossfadeLaws, crossfadesByRegion, draggedFadeOut, fadeCurve, fadeHandles, followingRegion, resized, sCurve, shapeOf, type Crossfade } from "../src/fades.ts";
import type { Region } from "../src/api.ts";

const near = (a: number, b: number, eps = 1e-9) => assert.ok(Math.abs(a - b) < eps, `${a} vs ${b}`);

test("curves run from silence to full level and rise", () => {
  for (const c of [-1, -0.5, 0, 0.3, 0.5, 0.8, 1]) {
    near(fadeCurve(0, c), 0);
    near(fadeCurve(1, c), 1, 1e-6);
    for (let k = 1; k <= 100; k++) assert.ok(fadeCurve(k / 100, c) >= fadeCurve((k - 1) / 100, c) - 1e-12, `curve ${c} falls at ${k}`);
  }
});

test("curve 0 is linear, the default a quarter sine, negatives mirror positives", () => {
  for (let k = 0; k <= 20; k++) {
    const x = k / 20;
    near(fadeCurve(x, 0), x);
    near(fadeCurve(x, 0.5), Math.sin((x * Math.PI) / 2), 1e-12);
    near(fadeCurve(x, -0.7), 1 - fadeCurve(1 - x, 0.7), 1e-12);
  }
  // Same values as the engine's tests check.
  near(fadeCurve(0.5, -0.5), 1 - Math.cos(Math.PI / 4), 1e-12);
  near(fadeCurve(0.5, 0.9), 0.9846, 1e-4);
});

test("crossfade laws: X and X S sum to one, EqP to constant power", () => {
  const x = (kind: Crossfade["kind"], curve = 0) => crossfadeLaws({ kind, curve });
  for (let k = 0; k <= 50; k++) {
    const p = k / 50;
    for (const c of [0, 0.5, -0.8]) {
      const [o, i] = x("x", c);
      near(o(1 - p) + i(p), 1, 1e-12);
    }
    const [o, i] = x("eqp");
    near(o(1 - p) ** 2 + i(p) ** 2, 1, 1e-12);
    const [so, si] = x("xs");
    near(so(1 - p) + si(p), 1, 1e-12);
  }
  near(sCurve(0.25), 0.1464, 1e-4);
});

const audio = (id: number, start: number, length: number, extra: Partial<Region> = {}): Region => ({
  id,
  name: `R${id}`,
  start,
  length,
  muted: false,
  content: { type: "audio", clip: 1, offset: 0, gain_db: 0, fade_in: 0, fade_out: 0 },
  ...extra,
});

test("the following region is the next to start inside or at the end and run past it", () => {
  const a = audio(1, 0, 100);
  assert.equal(followingRegion([a, audio(2, 100, 50)], a)?.id, 2);
  assert.equal(followingRegion([a, audio(2, 80, 50), audio(3, 90, 50)], a)?.id, 2);
  assert.equal(followingRegion([a, audio(2, 101, 50)], a), undefined, "a gap");
  assert.equal(followingRegion([a, audio(2, 20, 50)], a), undefined, "inside");
  assert.equal(followingRegion([a, audio(2, 100, 50, { muted: true })], a), undefined, "muted");
  // Loops crossfade at their loop end; take folders like any region.
  const looped = { ...a, loop_end: 250 };
  assert.equal(followingRegion([looped, audio(2, 100, 50), audio(3, 250, 50)], looped)?.id, 3);
  const folder = { ...audio(4, 100, 50), takes: { takes: [], comp: [] } };
  assert.equal(followingRegion([a, folder], a)?.id, 4);
});

test("shape defaults, handles and crossfade drags", () => {
  assert.deepEqual(shapeOf(audio(1, 0, 10).content), { in_curve: 0.5, out_curve: 0.5, in_type: "fade", out_type: "out" });
  const xf: Crossfade = { out_region: 1, in_region: 2, kind: "eqp", curve: 0.5, fade_out: [900, 200], fade_in: [900, 200] };
  const by = crossfadesByRegion([xf]);
  assert.equal(by.get(1)?.out, xf);
  assert.equal(by.get(2)?.in, xf);
  // The outgoing region's handle is at the crossfade's start; the incoming one has no fade-in handle.
  assert.deepEqual(fadeHandles(0, 1000, 50, 200, by.get(1)), { fadeIn: 50, fadeOut: 900 });
  assert.deepEqual(fadeHandles(1000, 1000, 50, 0, by.get(2)), { fadeIn: null, fadeOut: 1000 });
  assert.deepEqual(fadeHandles(0, 1000, 50, 200, undefined), { fadeIn: 50, fadeOut: 800 });
  // Dragging a crossfade's handle 10 frames left lengthens it by 20, about its centre.
  assert.equal(draggedFadeOut(200, -10, true, 1000), 220);
  assert.equal(draggedFadeOut(200, -10, false, 1000), 210);
  assert.deepEqual(resized(xf, 220).fade_out, [890, 220]);
  const butt: Crossfade = { ...xf, fade_out: [950, 50], fade_in: [1000, 50] };
  assert.deepEqual([resized(butt, 200).fade_out, resized(butt, 200).fade_in], [[900, 100], [1000, 100]]);
});
