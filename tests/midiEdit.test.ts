// Unit tests for src/midiEdit.ts. Run with `npm test` (Node 23.6+ runs TypeScript directly;
// on Node 22 add --experimental-strip-types). No test framework needed: node:test is built in.
import { test } from "node:test";
import assert from "node:assert/strict";
import * as m from "../src/midiEdit.ts";
import type { Region } from "../src/api.ts";

const tb: m.TimeBase = { sampleRate: 48000, tempo: 120, sigNum: 4, sigDen: 4 };
const Q = m.quarterLength(tb); // 24000
const BAR = m.barLength(tb); // 96000

function n(start: number, pitch = 60, extra: Partial<m.PNote> = {}): m.PNote {
  return { region: 1, start, length: Q / 2, pitch, velocity: 100, muted: false, sel: false, ...extra };
}

test("grid lengths", () => {
  assert.equal(m.gridLength(tb, "1/4"), Q);
  assert.equal(m.gridLength(tb, "1/16"), Q / 4);
  assert.equal(m.gridLength(tb, "1/8T"), Q / 3);
  assert.equal(m.barLength({ ...tb, sigNum: 6, sigDen: 8 }), Q * 3);
});

test("snap counts from the bar line in odd meters", () => {
  const bar = m.barLength({ ...tb, sigNum: 3, sigDen: 4 });
  // A half-note grid in 3/4 restarts at every bar.
  assert.equal(m.snapTime(bar + Q * 0.9, Q * 2, bar), bar);
  assert.equal(m.snapTime(bar + Q * 1.2, Q * 2, bar), bar + Q * 2);
});

test("quantize: full, strength, swing, ends, selection only", () => {
  const o: m.QuantizeOpts = { grid: Q / 4, bar: BAR, strength: 1, swing: 0, ends: false };
  const notes = [n(Q / 4 + 900), n(Q / 2 - 800)];
  assert.deepEqual(m.quantize(notes, o).map((x) => x.start), [Q / 4, Q / 2]);
  assert.deepEqual(m.quantize(notes, { ...o, strength: 0.5 }).map((x) => x.start), [Q / 4 + 450, Q / 2 - 400]);
  // Swing delays every second sixteenth.
  assert.deepEqual(m.quantize(notes, { ...o, swing: 0.5 }).map((x) => x.start), [Q / 4 + Q / 16, Q / 2]);
  // Lengths are kept unless ends are quantized too.
  const long = [n(10, 60, { length: Q * 0.6 })];
  assert.equal(m.quantize(long, o)[0].length, Q * 0.6);
  assert.equal(m.quantize(long, { ...o, ends: true })[0].length, Q / 2);
  // Only the selected note moves.
  const mixed = [n(900, 60, { sel: true }), n(Q + 900)];
  assert.deepEqual(m.quantize(mixed, o).map((x) => x.start), [0, Q + 900]);
});

test("transpose and nudge keep notes in range", () => {
  assert.equal(m.transpose([n(0, 120)], 12)[0].pitch, 127);
  const sel = [n(Q, 60, { sel: true }), n(2 * Q, 126, { sel: true }), n(0, 50)];
  const moved = m.nudge(sel, -5 * Q, 3);
  // Clamped as a group: the earliest selected note stops at 0, the highest at 127.
  assert.deepEqual(moved.map((x) => [x.start, x.pitch]), [[0, 61], [Q, 127], [0, 50]]);
});

test("velocity functions", () => {
  const notes = [n(0, 60, { velocity: 100 }), n(Q, 62, { velocity: 20 })];
  assert.deepEqual(m.setVelocity(notes, 200).map((x) => x.velocity), [127, 127]);
  assert.deepEqual(m.scaleVelocity(notes, 50).map((x) => x.velocity), [50, 10]);
  assert.deepEqual(m.offsetVelocity(notes, -30).map((x) => x.velocity), [70, 1]);
  assert.deepEqual(m.rampVelocity(notes, 0, 10, Q, 110).map((x) => x.velocity), [10, 110]);
});

test("legato extends to the next chord", () => {
  const notes = [n(0, 60), n(0, 64), n(Q, 67), n(3 * Q, 72)];
  assert.deepEqual(m.legato(notes).map((x) => x.length), [Q, Q, 2 * Q, Q / 2]);
});

test("overlaps and duplicates", () => {
  const notes = [n(0, 60, { length: Q * 2 }), n(Q, 60), n(Q, 60, { velocity: 120 }), n(0, 62, { length: Q * 2 })];
  assert.deepEqual(m.removeOverlaps(notes).map((x) => x.length), [Q, Q / 2, Q / 2, Q * 2]);
  const dedup = m.deleteDuplicates(notes);
  assert.equal(dedup.length, 3);
  assert.equal(dedup.find((x) => x.start === Q)!.velocity, 120);
});

test("humanize is bounded and deterministic with a seeded source", () => {
  const notes = [n(Q), n(2 * Q)];
  const out = m.humanize(notes, { time: 100, velocity: 10, length: 0 }, () => 1);
  assert.deepEqual(out.map((x) => [x.start, x.velocity, x.length]), [[Q + 100, 110, Q / 2], [2 * Q + 100, 110, Q / 2]]);
  const low = m.humanize(notes, { time: 100, velocity: 10, length: 0 }, () => 0);
  assert.deepEqual(low.map((x) => x.start), [Q - 100, 2 * Q - 100]);
});

test("mute toggles, reverse mirrors", () => {
  const notes = [n(0, 60, { sel: true }), n(Q, 62, { sel: true, muted: true })];
  assert.deepEqual(m.toggleMute(notes).map((x) => x.muted), [true, true]);
  assert.deepEqual(m.toggleMute(m.toggleMute(notes)).map((x) => x.muted), [false, false]);
  assert.deepEqual(m.reverse(notes).map((x) => x.start), [Q, 0]);
});

test("scale quantize", () => {
  const scale = m.scaleOf("E♭ major");
  assert.deepEqual([...scale].sort((a, b) => a - b), [0, 2, 3, 5, 7, 8, 10]);
  assert.deepEqual([...m.scaleOf("A minor")].sort((a, b) => a - b), [0, 2, 4, 5, 7, 9, 11]);
  // E (64) isn't in E♭ major: down to E♭ (63).
  assert.equal(m.scaleQuantize([n(0, 64)], scale)[0].pitch, 63);
});

test("split, glue, duplicate, paste", () => {
  const split = m.splitAt([n(0, 60, { length: Q })], Q / 4);
  assert.deepEqual(split.map((x) => [x.start, x.length]), [[0, Q / 4], [Q / 4, (3 * Q) / 4]]);
  const glued = m.glue(split.map((x) => ({ ...x, sel: true })));
  assert.deepEqual(glued.map((x) => [x.start, x.length]), [[0, Q]]);
  const dup = m.duplicate([n(0, 60, { sel: true, length: Q * 0.9 })], Q / 4);
  assert.deepEqual(dup.map((x) => [x.start, x.sel]), [[0, false], [Q, true]]);
  const clip = m.copyNotes([n(Q, 60, { sel: true }), n(2 * Q, 64, { sel: true })]);
  const pasted = m.pasteNotes([n(0)], clip, 4 * Q, 7);
  assert.deepEqual(pasted.map((x) => [x.region, x.start, x.pitch, x.sel]), [[1, 0, 60, false], [7, 4 * Q, 60, true], [7, 5 * Q, 64, true]]);
});

test("selection helpers", () => {
  const notes = [n(0, 60), n(0, 64), n(0, 67), n(Q, 62), n(Q, 65), n(2 * Q, 60)];
  assert.deepEqual(m.selectTop(notes, true).filter((x) => x.sel).map((x) => x.pitch), [67, 65, 60]);
  assert.deepEqual(m.selectTop(notes, false).filter((x) => x.sel).map((x) => x.pitch), [60, 62, 60]);
  const one = notes.map((x, i) => ({ ...x, sel: i === 0 }));
  assert.deepEqual(m.selectSamePitch(one).filter((x) => x.sel).map((x) => x.start), [0, 2 * Q]);
  assert.equal(m.invertSelection(one).filter((x) => x.sel).length, 5);
  assert.deepEqual(m.selectRect(notes, Q / 2, Q * 1.5, 61, 63, false).filter((x) => x.sel).map((x) => x.pitch), [62]);
});

test("controller lines and erasing", () => {
  const mod = { type: "controller", cc: 1 } as const;
  const bend = { type: "pitch_bend" } as const;
  let cc: m.PCc[] = [{ region: 1, start: 50, kind: bend, value: 9000, sel: false }];
  cc = m.drawCcLine(cc, 1, mod, 0, 0, 100, 127, 10);
  const mods = cc.filter((c) => m.sameKind(c.kind, mod));
  assert.equal(mods[0].value, 0);
  assert.equal(mods[mods.length - 1].value, 127);
  assert.equal(mods.length, 11);
  assert.equal(m.ccValueAt(cc, mod, 55), 64);
  assert.equal(m.ccValueAt(cc, bend, 49), null);
  // Limited to a region, the eraser leaves other regions' events alone.
  const other: m.PCc = { region: 2, start: 60, kind: mod, value: 5, sel: false };
  assert.deepEqual(m.eraseCc([...cc, other], mod, 0, 100, 1).filter((c) => m.sameKind(c.kind, mod)), [other]);
  cc = m.eraseCc(cc, mod, 0, 100);
  assert.deepEqual(cc.map((c) => c.kind.type), ["pitch_bend"]);
});

test("controller events copy and paste relative to the earliest", () => {
  const mod = { type: "controller", cc: 1 } as const;
  const cc: m.PCc[] = [
    { region: 1, start: 300, kind: mod, value: 10, sel: true },
    { region: 1, start: 100, kind: mod, value: 20, sel: true },
    { region: 1, start: 200, kind: mod, value: 30, sel: false },
  ];
  const clip = m.copyCc(cc);
  assert.deepEqual(clip.map((c) => [c.offset, c.value]), [[200, 10], [0, 20]]);
  assert.deepEqual(m.copyCc(cc.map((c) => ({ ...c, sel: false }))), []);
  const pasted = m.pasteCc(cc, clip, 1000, 2);
  // Sorted by time; only the pasted events are selected, in the target region.
  assert.deepEqual(pasted.map((c) => [c.start, c.region, c.sel]), [[100, 1, false], [200, 1, false], [300, 1, false], [1000, 2, true], [1200, 2, true]]);
});

test("round trip through region content keeps selection keys in engine order", () => {
  const region: Region = {
    id: 1,
    name: "R",
    start: BAR,
    length: BAR,
    muted: false,
    content: {
      type: "midi",
      notes: [
        { start: Q, length: 10, pitch: 64, velocity: 90 },
        { start: 0, length: 10, pitch: 60, velocity: 80 },
      ],
      cc: [{ start: 5, kind: { type: "controller", cc: 64 }, value: 127 }],
    },
  };
  const { notes, cc } = m.fromRegions([region], new Set(["1:0"]));
  assert.deepEqual(notes.map((x) => [x.start, x.sel]), [[BAR + Q, true], [BAR, false]]);
  // Move the selected note past the region end: the region grows to the next bar.
  const moved = notes.map((x) => (x.sel ? { ...x, start: 2 * BAR + Q } : x));
  const { edits, selNotes } = m.toEdits(moved, cc, [region], BAR);
  assert.equal(edits.length, 1);
  assert.equal(edits[0].length, 2 * BAR);
  assert.equal(edits[0].start, undefined);
  assert.deepEqual(edits[0].notes.map((x) => x.start), [0, BAR + Q]);
  assert.deepEqual([...selNotes], ["1:1"]);
  // An unsorted region comes back sorted, which is an edit; a sorted one, unchanged, makes none.
  const same = m.fromRegions([region]);
  assert.equal(m.toEdits(same.notes, same.cc, [region], BAR).edits.length, 1, "unsorted input is re-sorted, which counts as a change");
  const sorted: Region = { ...region, content: { ...region.content, notes: [...(region.content as { notes: [] }).notes].reverse() } as Region["content"] };
  const s2 = m.fromRegions([sorted]);
  assert.equal(m.toEdits(s2.notes, s2.cc, [sorted], BAR).edits.length, 0);
});
