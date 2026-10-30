import { test } from "node:test";
import assert from "node:assert/strict";
import { keyName, keySourceItems, keySources, sideChainMenu } from "../src/sideChain.ts";

const tracks = [
  { id: 1, name: "Bass", kind: "audio" as const },
  { id: 2, name: "Kick", kind: "audio" as const },
  { id: 3, name: "Drums", kind: "folder" as const },
  { id: 4, name: "Synth", kind: "instrument" as const },
];
const labels = (items: { label?: string; separator?: boolean }[]) => items.map((i) => (i.separator ? "-" : i.label));

test("a plug-in's own track and folder stacks can't key it", () => {
  assert.deepEqual(keySources(tracks, 1).map((t) => t.id), [2, 4]);
  assert.deepEqual(labels(keySourceItems(tracks, 1, null, () => {})), ["None", "-", "Kick", "Synth"]);
});

test("the current source is checked, None without one", () => {
  const none = keySourceItems(tracks, 1, null, () => {});
  assert.equal(none[0].checked, true);
  assert.equal(none[2].checked, false);
  const kick = keySourceItems(tracks, 1, 2, () => {});
  assert.equal(kick[0].checked, false);
  assert.equal(kick[2].checked, true);
});

test("picking an item sets its source", () => {
  const got: (number | null)[] = [];
  const items = keySourceItems(tracks, 1, 2, (s) => got.push(s));
  items[0].onClick!();
  items[3].onClick!();
  assert.deepEqual(got, [null, 4]);
});

test("the slot's item names the source and offers Pre and Post only with one", () => {
  const plain = sideChainMenu(tracks, 1, {}, () => {}, () => {});
  assert.equal(plain.label, "Side Chain");
  assert.deepEqual(labels(plain.children!), ["None", "-", "Kick", "Synth"]);
  const pres: boolean[] = [];
  const keyed = sideChainMenu(tracks, 1, { sidechain: 2, sidechain_pre: true }, () => {}, (p) => pres.push(p));
  assert.equal(keyed.label, "Side Chain: Kick");
  const items = keyed.children!;
  assert.deepEqual(labels(items), ["None", "-", "Kick", "Synth", "-", "Pre Fader", "Post Fader"]);
  assert.equal(items[5].checked, true);
  assert.equal(items[6].checked, false);
  items[6].onClick!();
  assert.deepEqual(pres, [false]);
});

test("a source that's gone reads as none, without a name", () => {
  assert.equal(keyName(tracks, { sidechain: 99 }), null);
  assert.equal(keyName(tracks, { sidechain: null }), null);
  assert.equal(keyName(tracks, {}), null);
  assert.equal(keyName(tracks, { sidechain: 4 }), "Synth");
});
