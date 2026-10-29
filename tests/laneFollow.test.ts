import { test } from "node:test";
import assert from "node:assert/strict";
import { followMovedPlugin } from "../src/laneFollow.ts";
import type { AutoParam } from "../src/api.ts";

const plug = (uid: number, param = 3): AutoParam => ({ type: "plugin", value: { uid, param, name: "Compressor: Make Up" } });

test("a shown lane of the moved plug-in moves to the new track, re-keyed", () => {
  const shown: Record<number, AutoParam> = { 1: plug(10), 2: { type: "pan" }, 5: { type: "mute" } };
  const out = followMovedPlugin(shown, 1, 10, 2, 40);
  assert.deepEqual(out, { 2: plug(40), 5: { type: "mute" } });
  assert.deepEqual(shown[1], plug(10), "the old map is left alone");
});

test("other lanes and other plug-ins are left as they are", () => {
  const vol: Record<number, AutoParam> = { 1: { type: "volume" } };
  assert.equal(followMovedPlugin(vol, 1, 10, 2, 40), vol);
  const other: Record<number, AutoParam> = { 1: plug(11) };
  assert.equal(followMovedPlugin(other, 1, 10, 2, 40), other);
});

test("a move within the track changes nothing", () => {
  const shown: Record<number, AutoParam> = { 1: plug(10) };
  assert.equal(followMovedPlugin(shown, 1, 10, 1, 10), shown);
});
