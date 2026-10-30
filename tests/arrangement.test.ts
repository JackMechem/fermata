// Unit tests for src/arrangement.ts, the Arrangement track's drawing and drag helpers.
import { test } from "node:test";
import assert from "node:assert/strict";
import { dropSlot, regionsInSection, resizePreview, sectionAt, sectionColor, sections } from "../src/arrangement.ts";
import type { ArrangementMarker, Project } from "../src/api.ts";

const sec = (id: number, start: number, length: number, name = `S${id}`): ArrangementMarker => ({ id, name, start, length });
// 1 at 100..200, 2 at 200..250, 3 at 250..400.
const three = () => [sec(1, 100, 100), sec(2, 200, 50), sec(3, 250, 150)];

test("sections come in order and are found by position", () => {
  const list = sections({ arrangement: [sec(3, 250, 150), sec(1, 100, 100), sec(2, 200, 50)] });
  assert.deepEqual(list.map((s) => s.id), [1, 2, 3]);
  assert.equal(sectionAt(list, 199)?.id, 1);
  assert.equal(sectionAt(list, 200)?.id, 2);
  assert.equal(sectionAt(list, 400), null);
  assert.equal(sectionAt(list, 50), null);
});

test("resizing moves the later sections along", () => {
  const v = resizePreview(three(), 1, 40);
  assert.deepEqual(v.map((s) => [s.id, s.start, s.length]), [[1, 100, 40], [2, 140, 50], [3, 190, 150]]);
});

test("a dragged section drops at the nearest boundary", () => {
  // Section 3 dragged to start near 210: before section 2.
  assert.deepEqual(dropSlot(three(), 3, 210, false), { before: 2, at: 200 });
  // Section 1 dragged to the end.
  assert.deepEqual(dropSlot(three(), 1, 390, false), { before: null, at: 400 });
  // Back onto its own place, or just after itself: no move.
  assert.equal(dropSlot(three(), 2, 205, false), null);
  assert.equal(dropSlot(three(), 2, 245, false), null);
  // A copy can go right after itself.
  assert.deepEqual(dropSlot(three(), 2, 245, true), { before: 3, at: 250 });
  assert.equal(dropSlot([], 1, 0, false), null);
});

test("names have their own colours; a set colour wins", () => {
  assert.equal(sectionColor({ name: "Verse" }), sectionColor({ name: "Verse 2" }));
  assert.notEqual(sectionColor({ name: "Verse" }), sectionColor({ name: "Chorus" }));
  assert.equal(sectionColor({ name: "Chorus", color: "#123456" }), "#123456");
  assert.equal(sectionColor({ name: "Hook" }), sectionColor({ name: "Hook" }));
  assert.match(sectionColor({ name: "Hook" }), /^#[0-9a-f]{6}$/);
});

test("a section selects the regions that sound in it, on visible tracks", () => {
  const region = (id: number, start: number, length: number, loop_end: number | null = null) => ({ id, start, length, loop_end });
  const p = {
    tracks: [
      { hidden: false, regions: [region(1, 0, 100), region(2, 150, 10), region(3, 50, 20, 120)] },
      { hidden: false, regions: [region(4, 190, 50), region(5, 200, 10)] },
      { hidden: true, regions: [region(6, 150, 10)] },
    ],
  } as unknown as Pick<Project, "tracks">;
  assert.deepEqual(regionsInSection(p, { start: 100, length: 100 }), [2, 3, 4]);
});
