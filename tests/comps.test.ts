// Unit tests for src/comps.ts: take folder comp names and the folder's name bar label.
import { test } from "node:test";
import assert from "node:assert/strict";
import { compNames, currentComp, folderLabel, isMidiFolder } from "../src/comps.ts";
import type { Region, TakeFolder } from "../src/api.ts";

const take = (id: number, name: string) => ({ id, name, clip: id, offset: 0, start: 0, end: 1000, gain_db: 0 });

function folder(f: Partial<TakeFolder>, type: "audio" | "midi" = "audio"): Region {
  const content =
    type === "audio" ? { type: "audio" as const, clip: 1, offset: 0, gain_db: 0, fade_in: 0, fade_out: 0 } : { type: "midi" as const, notes: [], cc: [] };
  return { id: 9, name: "Vocal", start: 0, length: 1000, muted: false, content, takes: { takes: [take(1, "Take 1"), take(2, "Take 2")], comp: [], ...f } } as Region;
}

test("a folder without saved comps has Comp 1", () => {
  const f = folder({}).takes!;
  assert.deepEqual(compNames(f), ["Comp 1"]);
  assert.equal(currentComp(f), 0);
});

test("the label names the whole take heard, or the comp", () => {
  assert.equal(folderLabel(folder({ comp: [{ start: 0, end: 1000, take: 2 }] })), "Take 2");
  const comps = [
    { name: "Comp 1", comp: [] },
    { name: "Lead", comp: [] },
  ];
  const r = folder({ comp: [{ start: 0, end: 500, take: 1 }, { start: 500, end: 1000, take: 2 }], comps, current: 1 });
  assert.equal(folderLabel(r), "Lead");
  assert.equal(currentComp({ ...r.takes!, current: 7 }), 1, "clamped");
  assert.equal(folderLabel({ ...r, takes: null }), null);
});

test("a MIDI folder names its take, the newest when none is chosen", () => {
  const r = folder({}, "midi");
  assert.ok(isMidiFolder(r) && !isMidiFolder(folder({})));
  assert.equal(folderLabel(r), "Take 2");
  assert.equal(folderLabel(folder({ comp: [{ start: 0, end: 1000, take: 1 }] }, "midi")), "Take 1");
});
