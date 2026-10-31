import { test } from "node:test";
import assert from "node:assert/strict";
import { keyboardKeys } from "../src/pianoKeys.ts";

test("two octaves and a note from C", () => {
  const { list, whites } = keyboardKeys(48, 25);
  assert.equal(list.length, 25);
  assert.equal(whites, 15);
  assert.equal(list.filter((k) => k.black).length, 10);
});

test("whites come first so blacks draw over them, and a black sits on the line between two", () => {
  const { list } = keyboardKeys(48, 3);
  assert.deepEqual(
    list.map((k) => k.note),
    [48, 50, 49],
  );
  assert.equal(list[1].at, 1);
  assert.ok(Math.abs(list[2].at - 0.69) < 1e-9);
});

test("names follow the same octave numbers as the rest of the app (C3 is 60)", () => {
  const { list } = keyboardKeys(60, 1);
  assert.equal(list[0].name, "C3");
});

test("a start on a black key still lays out", () => {
  const { list, whites } = keyboardKeys(49, 2);
  assert.equal(whites, 1);
  assert.equal(list.length, 2);
});
