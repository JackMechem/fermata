// Unit tests for an instrument panel's section layout (src/instLayout.ts).
import { test } from "node:test";
import assert from "node:assert/strict";
import { defaultLayout, layoutFor, parseLayouts } from "../src/instLayout.ts";

test("docked panels start in a row, floating ones in a grid", () => {
  assert.equal(defaultLayout("docked"), "row");
  assert.equal(defaultLayout("floating"), "grid");
});

test("each place keeps its own choice", () => {
  const saved = parseLayouts('{"docked":"grid"}');
  assert.equal(layoutFor(saved, "docked"), "grid");
  assert.equal(layoutFor(saved, "floating"), "grid");
  assert.equal(layoutFor(parseLayouts('{"floating":"row"}'), "docked"), "row");
  assert.equal(layoutFor(parseLayouts('{"floating":"row"}'), "floating"), "row");
});

test("anything malformed is dropped", () => {
  for (const bad of [null, "", "not json", "[]", "3", '{"docked":"spiral"}']) assert.deepEqual(parseLayouts(bad), {});
});
