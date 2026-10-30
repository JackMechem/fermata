import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { runsOnKey } from "../src/keyRepeat.ts";

test("a first press always runs", () => {
  assert.equal(runsOnKey("play", false), true);
  assert.equal(runsOnKey("nudge-left", false), true);
});

test("holding a toggle or one-shot doesn't run it again", () => {
  for (const id of ["play", "record", "cycle", "metronome", "mixer", "track-mute", "track-on-off", "delete-regions", "paste"]) assert.equal(runsOnKey(id, true), false, id);
});

test("holding a move or a zoom repeats", () => {
  for (const id of ["rewind", "forward", "zoom-in", "zoom-out", "select-previous", "select-next", "nudge-left", "nudge-right"]) assert.equal(runsOnKey(id, true), true, id);
});

test("every repeating command exists in the keymap", () => {
  const read = (p: string) => readFileSync(fileURLToPath(new URL(p, import.meta.url)), "utf8");
  const ids = [...read("../src/keyRepeat.ts").matchAll(/"([a-z-]+)"/g)].map((m) => m[1]);
  const keymap = read("../src/keymap.ts");
  assert.ok(ids.length > 0);
  for (const id of ids) assert.ok(keymap.includes(`id: "${id}"`), id);
});
