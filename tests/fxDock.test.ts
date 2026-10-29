// Unit tests for the plug-in panels' model (src/fxDock.ts): opening docked or floating, the
// slot toggle, closing back to the previous bottom pane, floating and docking, one Channel EQ at
// a time, what Esc closes, pruning, and the floating windows' resize maths and saved sizes.
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  NO_PANELS,
  checkBottom,
  clampSize,
  closePanel,
  dockPanel,
  dockedPanels,
  eqKey,
  escTarget,
  floatPanel,
  focusPanel,
  fxKey,
  isShown,
  openPanel,
  parseKey,
  parseSizes,
  prunePanels,
  resizeRect,
  selectDocked,
  togglePanel,
  type DockState,
} from "../src/fxDock.ts";

const A = fxKey(1);
const B = fxKey(2);
const C = fxKey(3);
const start = (bottom: DockState["bottom"] = "mixer"): DockState => ({ ...NO_PANELS, bottom });

test("keys round-trip and bad ones are refused", () => {
  assert.deepEqual(parseKey(fxKey(12)), { kind: "fx", uid: 12 });
  assert.deepEqual(parseKey(eqKey(-1)), { kind: "eq", track: -1 });
  assert.equal(parseKey("fx:"), null);
  assert.equal(parseKey("vst:3"), null);
});

test("a docked panel takes the bottom section and remembers what it showed", () => {
  const s = openPanel(start("mixer"), A, "docked");
  assert.equal(s.bottom, "fx");
  assert.equal(s.active, A);
  assert.equal(s.ret, "mixer");
  assert.deepEqual(s.open, [A]);
  assert.ok(isShown(s, A));
  // A second one is a new tab, shown; the section still goes back to the Mixer.
  const t = openPanel(s, B, "docked");
  assert.deepEqual(dockedPanels(t), [A, B]);
  assert.equal(t.active, B);
  assert.equal(t.ret, "mixer");
  assert.ok(!isShown(t, A), "the other tab is open but hidden");
});

test("a floating panel leaves the bottom section alone", () => {
  const s = openPanel(start("editor"), A, "floating");
  assert.equal(s.bottom, "editor");
  assert.deepEqual(s.floating, [A]);
  assert.deepEqual(dockedPanels(s), []);
  assert.ok(isShown(s, A));
});

test("an open panel reopens where it is, whatever the mode", () => {
  const s = openPanel(openPanel(start(), A, "floating"), B, "floating");
  const t = openPanel(s, A, "docked");
  assert.deepEqual(t.floating, [B, A], "brought to the front, not docked");
  assert.equal(t.bottom, "mixer");
  // A docked panel hidden behind the Mixer brings the dock back.
  const d = { ...openPanel(start(), C, "docked"), bottom: "mixer" as const };
  assert.ok(!isShown(d, C));
  const e = openPanel(d, C, "floating");
  assert.equal(e.bottom, "fx");
  assert.deepEqual(e.floating, []);
});

test("a slot click toggles: closes what is on screen, shows what is not", () => {
  const s = togglePanel(start("none"), A, "docked");
  assert.ok(isShown(s, A));
  const t = togglePanel(s, A, "docked");
  assert.deepEqual(t.open, []);
  assert.equal(t.bottom, "none");
  // Hidden behind the Editors: the click shows it again rather than closing it.
  const hidden = { ...s, bottom: "editor" as const };
  const u = togglePanel(hidden, A, "docked");
  assert.deepEqual(u.open, [A]);
  assert.equal(u.bottom, "fx");
  assert.equal(u.ret, "editor");
});

test("closing a tab shows the last opened other one; the last close gives the section back", () => {
  let s = start("mixer");
  for (const k of [A, B, C]) s = openPanel(s, k, "docked");
  s = selectDocked(s, A);
  assert.equal(s.active, A);
  s = closePanel(s, A);
  assert.equal(s.active, C, "the most recently opened of those left");
  assert.equal(s.bottom, "fx");
  s = closePanel(s, B);
  assert.equal(s.active, C, "closing a hidden tab keeps the shown one");
  s = closePanel(s, C);
  assert.equal(s.active, null);
  assert.equal(s.bottom, "mixer");
  assert.equal(s.ret, "none");
  assert.equal(closePanel(s, C), s, "closing a closed panel changes nothing");
});

test("float and dock move a panel between the dock and its own window", () => {
  let s = openPanel(openPanel(start("editor"), A, "docked"), B, "docked");
  s = floatPanel(s, B);
  assert.deepEqual(s.floating, [B]);
  assert.equal(s.active, A, "the dock shows the remaining tab");
  assert.equal(s.bottom, "fx");
  s = floatPanel(s, A);
  assert.deepEqual(s.floating, [B, A]);
  assert.equal(s.bottom, "editor", "nothing left docked: the Editors come back");
  assert.equal(floatPanel(s, A), s, "floating a floating panel changes nothing");
  s = dockPanel(s, B);
  assert.deepEqual(s.floating, [A]);
  assert.equal(s.active, B);
  assert.equal(s.bottom, "fx");
  assert.equal(s.ret, "editor");
  assert.deepEqual(s.open, [A, B], "tab order stays the opening order");
});

test("focus brings a floating panel to the front", () => {
  let s = start();
  for (const k of [A, B, C]) s = openPanel(s, k, "floating");
  s = focusPanel(s, A);
  assert.deepEqual(s.floating, [B, C, A]);
  assert.equal(focusPanel(s, A), s, "already in front");
  assert.equal(focusPanel(s, fxKey(9)), s, "not floating");
});

test("only one Channel EQ is open at a time", () => {
  let s = openPanel(start(), eqKey(1), "docked");
  s = openPanel(s, A, "docked");
  s = openPanel(s, eqKey(2), "floating");
  assert.deepEqual(s.open, [A, eqKey(2)]);
  assert.deepEqual(s.floating, [eqKey(2)]);
  assert.equal(s.active, A);
});

test("Esc closes the frontmost floating panel first, then the docked one on screen", () => {
  let s = openPanel(start("mixer"), A, "docked");
  s = openPanel(s, B, "floating");
  s = openPanel(s, C, "floating");
  assert.equal(escTarget(s), C);
  s = closePanel(s, escTarget(s)!);
  assert.equal(escTarget(s), B);
  s = closePanel(s, escTarget(s)!);
  assert.equal(escTarget(s), A);
  assert.equal(escTarget({ ...s, bottom: "mixer" }), null, "a hidden dock isn't closed by Esc");
  s = closePanel(s, escTarget(s)!);
  assert.equal(escTarget(s), null);
});

test("prune closes the panels whose instance went away", () => {
  let s = start("none");
  for (const k of [A, B]) s = openPanel(s, k, "docked");
  s = openPanel(s, C, "floating");
  const t = prunePanels(s, (k) => k === A);
  assert.deepEqual(t.open, [A]);
  assert.deepEqual(t.floating, []);
  assert.equal(t.active, A);
  const u = prunePanels(s, () => false);
  assert.deepEqual(u.open, []);
  assert.equal(u.bottom, "none");
});

test("checkBottom hands back an empty dock", () => {
  const empty: DockState = { ...NO_PANELS, bottom: "fx", active: A };
  assert.deepEqual(checkBottom(empty), { ...NO_PANELS, bottom: "none", active: null });
  const docked = openPanel(start(), A, "docked");
  assert.equal(checkBottom(docked), docked);
  const mixer = start("mixer");
  assert.equal(checkBottom(mixer), mixer);
});

// ---- sizes ---------------------------------------------------------------------------------

const rect = { x: 100, y: 50, w: 400, h: 300 };
const min = { w: 300, h: 200 };
const max = { w: 800, h: 600 };

test("east and south edges grow the window from its top-left corner", () => {
  assert.deepEqual(resizeRect(rect, "e", 50, 99, min, max), { x: 100, y: 50, w: 450, h: 300 });
  assert.deepEqual(resizeRect(rect, "s", 99, 40, min, max), { x: 100, y: 50, w: 400, h: 340 });
  assert.deepEqual(resizeRect(rect, "se", 30, 20, min, max), { x: 100, y: 50, w: 430, h: 320 });
});

test("west edges keep the right side in place", () => {
  assert.deepEqual(resizeRect(rect, "w", -60, 0, min, max), { x: 40, y: 50, w: 460, h: 300 });
  assert.deepEqual(resizeRect(rect, "sw", 40, 10, min, max), { x: 140, y: 50, w: 360, h: 310 });
  // Past the minimum the window stops, its right side still where it was.
  const r = resizeRect(rect, "w", 300, 0, min, max);
  assert.equal(r.w, 300);
  assert.equal(r.x + r.w, rect.x + rect.w);
});

test("resizing stays within the limits", () => {
  assert.equal(resizeRect(rect, "e", -500, 0, min, max).w, 300);
  assert.equal(resizeRect(rect, "e", 900, 0, min, max).w, 800);
  assert.equal(resizeRect(rect, "s", 0, -500, min, max).h, 200);
  assert.equal(resizeRect(rect, "se", 0, 900, min, max).h, 600);
});

test("clampSize rounds, and the minimum wins when it is larger than the maximum", () => {
  assert.deepEqual(clampSize({ w: 450.4, h: 250.6 }, min, max), { w: 450, h: 251 });
  assert.deepEqual(clampSize({ w: 100, h: 900 }, min, max), { w: 300, h: 600 });
  assert.deepEqual(clampSize({ w: 500, h: 500 }, { w: 700, h: 400 }, { w: 600, h: 300 }), { w: 700, h: 400 });
});

test("parseSizes keeps good entries and drops the rest", () => {
  assert.deepEqual(parseSizes(null), {});
  assert.deepEqual(parseSizes(""), {});
  assert.deepEqual(parseSizes("not json"), {});
  assert.deepEqual(parseSizes("[1, 2]"), {});
  assert.deepEqual(parseSizes("42"), {});
  const text = JSON.stringify({
    "builtin:compressor": { w: 700.6, h: 420 },
    "builtin:limiter": { w: "wide", h: 300 },
    "builtin:gate": { w: 0, h: 300 },
    "builtin:tuner": null,
    "builtin:reverb": { w: 640 },
  });
  assert.deepEqual(parseSizes(text), { "builtin:compressor": { w: 701, h: 420 } });
});
