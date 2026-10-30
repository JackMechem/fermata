// Unit tests for src/regionParams.ts, the Region inspector's playback parameter values.
import { test } from "node:test";
import assert from "node:assert/strict";
import { DEFAULT_PARAMS, DYNAMICS_OPTIONS, GATE_FIXED, GATE_LEGATO, GATE_OPTIONS, isDefault, optionLabel, paramsOf, playedNotes, QUANTIZE_OPTIONS, shared } from "../src/regionParams.ts";
import type { Region, RegionParams } from "../src/api.ts";

const midi = (id: number, params?: Partial<RegionParams>): Region => ({
  id,
  name: "M",
  start: 0,
  length: 1000,
  muted: false,
  content: { type: "midi", notes: [], cc: [] },
  ...(params ? { params: { ...DEFAULT_PARAMS, ...params } } : {}),
});

test("a region without params has the defaults, which change nothing", () => {
  assert.deepEqual(paramsOf(midi(1)), DEFAULT_PARAMS);
  assert.ok(isDefault(paramsOf(midi(1))));
  assert.ok(!isDefault(paramsOf(midi(1, { swing: 60 }))));
  assert.ok(!isDefault(paramsOf(midi(1, { delay_ms: true }))));
});

test("choice lists match the engine's values", () => {
  // crates/engine/src/region_params.rs: QUANTIZE_VALUES and DYNAMICS.
  assert.deepEqual(
    QUANTIZE_OPTIONS.map((o) => o[0]),
    [0, 1, 2, 4, 8, 16, 32, 64, 3, 6, 12, 24, 48, 96],
  );
  assert.deepEqual(
    DYNAMICS_OPTIONS.map((o) => o[0]),
    [0, 300, 200, 150, 125, 100, 80, 67, 50, 33],
  );
  assert.equal(GATE_OPTIONS[0][0], GATE_FIXED);
  assert.equal(GATE_OPTIONS[GATE_OPTIONS.length - 1][0], GATE_LEGATO);
  assert.equal(GATE_OPTIONS.length, 22);
  assert.equal(optionLabel(QUANTIZE_OPTIONS, 12), "1/12 Note");
  assert.equal(optionLabel(GATE_OPTIONS, 100), "100 %");
  assert.equal(optionLabel(GATE_OPTIONS, 104), "100 %");
  assert.equal(optionLabel(DYNAMICS_OPTIONS, 100), "1:1");
});

test("a mixed selection shares no value", () => {
  const a = midi(1, { quantize: 16 });
  const b = midi(2, { quantize: 16, transpose: 3 });
  assert.equal(shared([a, b], "quantize"), 16);
  assert.equal(shared([a, b], "transpose"), null);
  assert.equal(shared([midi(3), midi(4)], "gate"), 100);
  assert.equal(shared([], "gate"), null);
});

test("played notes are looked up by region", () => {
  const notes = [{ start: 0, length: 10, pitch: 72, velocity: 90 }];
  const played = [{ region: 7, notes }];
  assert.equal(playedNotes(played, 7), notes);
  assert.equal(playedNotes(played, 8), undefined);
  assert.equal(playedNotes(undefined, 7), undefined);
  assert.equal(playedNotes([], 7), undefined);
});
