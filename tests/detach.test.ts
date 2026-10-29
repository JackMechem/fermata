// Unit tests for src/detachModel.ts: panel keys and window labels, the detached list, the UI
// state the windows share, and fitting remembered windows onto the monitors there are now.
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  bottomAfterHide,
  cascade,
  cleanPatch,
  detachedKinds,
  fitToMonitors,
  keyOfLabel,
  panelKey,
  panelLabel,
  parseMemory,
  parsePanelKey,
  SYNCED_FIELDS,
  syncedState,
  syncPatch,
  withDetached,
  withoutDetached,
  type Rect,
} from "../src/detachModel.ts";

test("panel keys and window labels round-trip", () => {
  assert.equal(panelKey("mixer"), "mixer");
  assert.equal(panelKey("eq", 12), "eq:12");
  assert.equal(panelKey("eq", -1), "eq:-1");
  assert.deepEqual(parsePanelKey("mixer"), { kind: "mixer", arg: "" });
  assert.deepEqual(parsePanelKey("eq:12"), { kind: "eq", arg: "12" });
  // The effect panels' keys from fxDock.ts parse the same way.
  assert.deepEqual(parsePanelKey("fx:3051"), { kind: "fx", arg: "3051" });
  for (const key of ["mixer", "editor", "eq:12", "eq:-1", "fx:3051"]) {
    assert.equal(keyOfLabel(panelLabel(key)), key);
    const { kind, arg } = parsePanelKey(key);
    assert.equal(panelKey(kind, arg), key);
  }
  assert.equal(panelLabel("mixer"), "panel-mixer");
  assert.equal(keyOfLabel("main"), null);
  assert.equal(keyOfLabel("panel-"), null);
});

test("the detached list adds once and removes", () => {
  const a = withDetached([], "mixer");
  assert.deepEqual(a, ["mixer"]);
  assert.equal(withDetached(a, "mixer"), a);
  const b = withDetached(a, "eq:4");
  assert.deepEqual(b, ["mixer", "eq:4"]);
  assert.deepEqual(withoutDetached(b, "mixer"), ["eq:4"]);
  assert.equal(withoutDetached(b, "editor"), b);
  assert.deepEqual(detachedKinds(["eq:1", "eq:2", "mixer"]), ["eq", "mixer"]);
});

test("bottomAfterHide gives back what the section showed before", () => {
  // It was the panel itself, or nothing: the section closes.
  assert.equal(bottomAfterHide("mixer", "mixer", ["mixer"]), "none");
  assert.equal(bottomAfterHide("mixer", "none", ["mixer"]), "none");
  // What was there before is detached itself: it can't come back here.
  assert.equal(bottomAfterHide("mixer", "editor", ["mixer", "editor"]), "none");
  // Another pane: it comes back.
  assert.equal(bottomAfterHide("mixer", "editor", ["mixer"]), "editor");
  assert.equal(bottomAfterHide("editor", "fx", ["editor"]), "fx");
});

test("syncPatch has only the shared fields that changed", () => {
  const base = Object.fromEntries(SYNCED_FIELDS.map((f) => [f, [] as unknown])) as Record<(typeof SYNCED_FIELDS)[number], unknown>;
  assert.equal(syncPatch(base, { ...base }), null);
  const sel = [1, 2];
  const next = { ...base, selectedRegions: sel, themeId: "light" };
  assert.deepEqual(syncPatch(base, next), { selectedRegions: sel, themeId: "light" });
  // Fields that aren't shared don't count.
  assert.equal(syncPatch({ ...base, bottom: "mixer" }, { ...base, bottom: "editor" }), null);
  const all = syncedState({ ...base, bottom: "mixer" });
  assert.deepEqual(Object.keys(all).sort(), [...SYNCED_FIELDS].sort());
});

test("cleanPatch keeps only shared fields", () => {
  assert.deepEqual(cleanPatch({ selectedTracks: [3], bottom: "mixer", snap: null, dialog: "settings" }), { selectedTracks: [3] });
  assert.deepEqual(cleanPatch({}), {});
});

test("parseMemory ignores junk and keeps what is valid", () => {
  const empty = { open: [], geometry: {} };
  assert.deepEqual(parseMemory(null), empty);
  assert.deepEqual(parseMemory(""), empty);
  assert.deepEqual(parseMemory("{not json"), empty);
  assert.deepEqual(parseMemory("42"), empty);
  assert.deepEqual(parseMemory(JSON.stringify({ open: "mixer", geometry: [] })), empty);
  const mem = parseMemory(
    JSON.stringify({
      open: ["mixer", 3, "", "mixer", "editor"],
      geometry: {
        mixer: { x: 10.4, y: 20, w: 800.6, h: 500 },
        editor: { x: 0, y: 0, w: 0, h: 300 },
        eq: { x: "1", y: 0, w: 1, h: 1 },
        fx: null,
      },
    }),
  );
  assert.deepEqual(mem, { open: ["mixer", "editor"], geometry: { mixer: { x: 10, y: 20, w: 801, h: 500 } } });
  // Partial data: only the part that is there.
  assert.deepEqual(parseMemory(JSON.stringify({ open: ["editor"] })), { open: ["editor"], geometry: {} });
});

const main: Rect = { x: 0, y: 25, w: 1920, h: 1055 };
const right: Rect = { x: 1920, y: 0, w: 2560, h: 1440 };

test("fitToMonitors keeps a window that fits where it is", () => {
  const g = { x: 2100, y: 200, w: 1000, h: 600 };
  assert.deepEqual(fitToMonitors(g, [main, right]), g);
});

test("fitToMonitors moves a window off an unplugged monitor onto the nearest", () => {
  // It was on the right-hand monitor, which is gone.
  assert.deepEqual(fitToMonitors({ x: 2100, y: 200, w: 1000, h: 600 }, [main]), { x: 920, y: 200, w: 1000, h: 600 });
  // Far above everything: it lands at the top of the main monitor.
  assert.deepEqual(fitToMonitors({ x: 100, y: -3000, w: 400, h: 300 }, [main, right]), { x: 100, y: 25, w: 400, h: 300 });
});

test("fitToMonitors shrinks a window larger than its monitor", () => {
  assert.deepEqual(fitToMonitors({ x: -50, y: 0, w: 2500, h: 1500 }, [main]), { x: 0, y: 25, w: 1920, h: 1055 });
});

test("fitToMonitors puts a window spanning two monitors onto the one it covers most", () => {
  // Mostly on the right one.
  assert.deepEqual(fitToMonitors({ x: 1820, y: 100, w: 1000, h: 600 }, [main, right]), { x: 1920, y: 100, w: 1000, h: 600 });
  // Mostly on the main one.
  assert.deepEqual(fitToMonitors({ x: 1200, y: 100, w: 1000, h: 600 }, [main, right]), { x: 920, y: 100, w: 1000, h: 600 });
});

test("fitToMonitors with no monitors known leaves the window alone", () => {
  const g = { x: 5000, y: 5000, w: 10, h: 10 };
  assert.deepEqual(fitToMonitors(g, []), g);
  assert.deepEqual(fitToMonitors(g, [{ x: 0, y: 0, w: 0, h: 0 }]), g);
});

test("cascade offsets each further window of a kind", () => {
  const g = { x: 100, y: 100, w: 500, h: 400 };
  assert.equal(cascade(g, 0), g);
  assert.deepEqual(cascade(g, 2), { x: 156, y: 156, w: 500, h: 400 });
  assert.deepEqual(cascade(g, 1, 10), { x: 110, y: 110, w: 500, h: 400 });
});
