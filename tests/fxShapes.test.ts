// Unit tests for src/fxShapes.ts (the curves the modulation, distortion and Gain panels draw).
// Run with `npm test`. The expected values mirror the engine's own tests in
// crates/engine/src/fx/{chorus,flanger,phaser,tremolo,distortion,overdrive,gain}.rs.
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  compensation,
  distortionCurve,
  formatBalance,
  overdriveCurve,
  phaserHz,
  sweptMs,
  transfer,
  tremoloEdge,
  tremoloGain,
  tremoloShape,
  unipolarSine,
  COMP_REF,
  SQUARE,
} from "../src/fxShapes.ts";

const near = (a: number, b: number, eps = 1e-6) => assert.ok(Math.abs(a - b) < eps, `${a} vs ${b}`);

test("LFO sweeps: chorus is linear, flanger by octaves, phaser either side of the centre", () => {
  near(unipolarSine(0), 1);
  near(unipolarSine(0.5), 0);
  assert.equal(sweptMs("linear", 12, 0.5, 0), 12);
  assert.equal(sweptMs("linear", 12, 0.5, 1), 15);
  assert.equal(sweptMs("octaves", 1, 1, 1), 8);
  assert.equal(sweptMs("octaves", 5, 1, 1), 40);
  near(phaserHz(700, 1, 1), 2800);
  near(phaserHz(700, 1, 0), 175);
  assert.equal(phaserHz(700, 0, 0.3), 700);
});

test("tremolo shapes peak at the start of the cycle and depth scales the dip", () => {
  for (const w of [0, 1, 2]) {
    near(tremoloShape(w, 0, 0.3), 1);
    near(tremoloShape(w, 0.5, 0.3), 0);
    near(tremoloShape(w, 1.25, 0.3), tremoloShape(w, 0.25, 0.3));
  }
  assert.equal(tremoloShape(SQUARE, 0.1, 0.2), 1);
  assert.equal(tremoloShape(SQUARE, 0.4, 0.2), 0);
  near(tremoloShape(SQUARE, 0.25, 0.2), 0.5);
  assert.equal(tremoloGain(0, 0), 1);
  assert.equal(tremoloGain(1, 0), 0);
  assert.equal(tremoloGain(0.5, 0), 0.5);
  // The edges never get shorter than 2 ms each: 8 % of a cycle at 20 Hz.
  near(tremoloEdge(0, 20), 0.08);
  assert.equal(tremoloEdge(0.5, 20), 0.5);
  assert.equal(tremoloEdge(2, 1), 1);
});

test("distortion curves stay within ±1 with unity slope, and compensation holds −18 dBFS", () => {
  for (const kind of [0, 1, 2, 3]) {
    const slope = (distortionCurve(kind, 1e-4) - distortionCurve(kind, -1e-4)) / 2e-4;
    near(slope, kind === 3 ? 2 : 1, 1e-2);
    for (let x = -100; x <= 100; x += 0.37) assert.ok(Math.abs(distortionCurve(kind, x)) <= 1);
    for (const db of [0, 24, 60]) {
      const f = transfer((x) => distortionCurve(kind, x), db, 0);
      near(0.5 * (f(COMP_REF) - f(-COMP_REF)), COMP_REF, 1e-4);
    }
  }
  assert.equal(distortionCurve(1, 0.5), 0.5);
  assert.equal(overdriveCurve(0), 0);
  assert.ok(overdriveCurve(5) + overdriveCurve(-5) < -0.2);
  const c = compensation(overdriveCurve, 100);
  assert.ok(c > 0.05 && c < 0.2);
  // Output gain adds on top.
  near(transfer(Math.tanh, 0, 6)(0.01) / transfer(Math.tanh, 0, 0)(0.01), Math.pow(10, 6 / 20));
});

test("balance reads like a pan", () => {
  assert.equal(formatBalance(0), "C");
  assert.equal(formatBalance(-0.35), "L 35");
  assert.equal(formatBalance(1), "R 100");
  assert.equal(formatBalance(0.004), "C");
  assert.equal(formatBalance(-3), "L 100");
});
