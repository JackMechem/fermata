// Unit tests for src/fxFormat.ts (built-in effect value readouts and control mapping). Run with
// `npm test`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { clampValue, formatValue, fromNorm, parseValue, tidy, toNorm, valuesOf } from "../src/fxFormat.ts";
import type { FxParamSpec } from "../src/fx.ts";

const spec = (p: Partial<FxParamSpec>): FxParamSpec => ({ id: 0, name: "x", min: 0, max: 1, default: 0, unit: "none", kind: { type: "continuous" }, log: false, ...p });

test("readouts carry their units", () => {
  assert.equal(formatValue(spec({ unit: "db", min: -60, max: 0 }), -12.34), "−12.3 dB");
  assert.equal(formatValue(spec({ unit: "db", min: -100, max: 0 }), -100), "−∞ dB");
  assert.equal(formatValue(spec({ unit: "db" }), 3), "+3.0 dB");
  assert.equal(formatValue(spec({ unit: "hz" }), 2400), "2.40 kHz");
  assert.equal(formatValue(spec({ unit: "hz" }), 12500), "12.5 kHz");
  assert.equal(formatValue(spec({ unit: "hz" }), 80.4), "80 Hz");
  assert.equal(formatValue(spec({ unit: "ms" }), 0.5), "0.50 ms");
  assert.equal(formatValue(spec({ unit: "ms" }), 1500), "1.5 s");
  assert.equal(formatValue(spec({ unit: "ratio" }), 4), "4:1");
  assert.equal(formatValue(spec({ unit: "percent" }), 0.25), "25 %");
  assert.equal(formatValue(spec({ kind: { type: "toggle" } }), 1), "On");
  assert.equal(formatValue(spec({ kind: { type: "choice", labels: ["Split", "Wide"] } }), 1), "Wide");
});

test("log controls map frequencies evenly by octave and match the engine", () => {
  const f = spec({ unit: "hz", min: 20, max: 20000, log: true });
  assert.ok(Math.abs(fromNorm(f, 0.5) - 632.4555) < 1e-3);
  assert.ok(Math.abs(toNorm(f, 2000) - 2 / 3) < 1e-9);
  const lin = spec({ unit: "db", min: -60, max: 0 });
  assert.equal(fromNorm(lin, 0.5), -30);
  assert.equal(toNorm(lin, 10), 1);
  const c = spec({ kind: { type: "choice", labels: ["a", "b", "c"] }, max: 2 });
  assert.equal(fromNorm(c, 0.6), 1);
  assert.equal(clampValue(c, 9), 2);
});

test("typed values parse with units and drags round tidily", () => {
  const hz = spec({ unit: "hz", min: 20, max: 20000, log: true });
  assert.equal(parseValue(hz, "2.5k"), 2500);
  assert.equal(parseValue(hz, "250 Hz"), 250);
  assert.equal(parseValue(hz, "abc"), null);
  assert.equal(parseValue(spec({ unit: "ms", max: 5000 }), "1.2 s"), 1200);
  assert.equal(parseValue(spec({ unit: "percent" }), "50 %"), 0.5);
  assert.equal(parseValue(spec({ unit: "db", min: -60, max: 0 }), "−6 dB"), -6);
  assert.equal(tidy(hz, 1234.56), 1230);
  assert.equal(tidy(spec({ unit: "db", min: -60, max: 0 }), -12.345), -12.3);
  assert.deepEqual(valuesOf([spec({ id: 3, default: 0.2 }), spec({ id: 5, default: 0.7 })], [[5, 0.1]]), { 3: 0.2, 5: 0.1 });
});
