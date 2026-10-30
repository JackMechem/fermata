// Unit tests for src/vca.ts (VCA faders in the Mixer). Run with `npm test`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { canAssignVca, canNest, effectiveDb, vcaChain, vcaOffset, vcaSlotLabel } from "../src/vca.ts";
import type { Vca } from "../src/api.ts";

const vca = (id: number, level_db: number, mute = false, parent: number | null = null): Vca => ({ id, name: `VCA ${id}`, color: "#fff", level_db, mute, parent });

test("levels add up along nested VCAs", () => {
  const vcas = [vca(1, -6), vca(2, -3, false, 1), vca(3, 2, false, 2)];
  assert.deepEqual(vcaOffset(vcas, null), { db: 0, mute: false });
  assert.deepEqual(vcaOffset(vcas, 1), { db: -6, mute: false });
  assert.deepEqual(vcaOffset(vcas, 3), { db: -7, mute: false });
  assert.deepEqual(vcaOffset(vcas, 42), { db: 0, mute: false }, "a deleted VCA offsets nothing");
});

test("a muted or bottomed-out VCA silences what is below it", () => {
  const vcas = [vca(1, 0, true), vca(2, 6, false, 1), vca(3, -96), vca(4, 6, false, 3)];
  assert.equal(vcaOffset(vcas, 2).mute, true);
  assert.equal(vcaOffset(vcas, 4).db, -Infinity);
  assert.equal(effectiveDb(6, vcaOffset(vcas, 4).db), -96);
  assert.equal(effectiveDb(-2, -6), -8);
});

test("loops are refused, and a loop in a file doesn't hang", () => {
  const vcas = [vca(1, -1), vca(2, -1, false, 1)];
  assert.equal(canNest(vcas, 2, 1), true);
  assert.equal(canNest(vcas, 1, 2), false);
  assert.equal(canNest(vcas, 1, 1), false);
  vcas[0].parent = 2;
  assert.equal(vcaChain(vcas, 1).length, 2);
});

test("folders get no VCA slot; the slot shows the VCA's name", () => {
  assert.equal(canAssignVca("folder"), false);
  assert.equal(canAssignVca("aux"), true);
  assert.equal(canAssignVca("summing"), true);
  assert.equal(vcaSlotLabel([vca(1, 0)], 1), "VCA 1");
  assert.equal(vcaSlotLabel([vca(1, 0)], null), "");
});
