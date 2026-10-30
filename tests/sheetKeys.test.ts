// Unit tests for src/sheetKeysModel.ts: which keys reach the app while sheets are open. Also
// checks that every sheet registers with sheetKeys.ts, so none lets the app's keys through.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { heldBack, NON_MODAL_COMMANDS, outsideKey } from "../src/sheetKeysModel.ts";

const read = (path: string) => readFileSync(new URL(`../${path}`, import.meta.url), "utf8");

test("with no sheet open, every key reaches the app", () => {
  assert.equal(outsideKey([], "Backspace", "delete-regions"), "pass");
  assert.equal(outsideKey([], "Escape", null), "pass");
  assert.equal(outsideKey([], "Enter", "to-start"), "pass");
  assert.equal(heldBack([], "delete-regions"), false);
});

test("a modal sheet holds back every key command and the focused editor's keys", () => {
  for (const [key, cmd] of [
    ["Backspace", "delete-regions"],
    [" ", "play"],
    ["l", "learn-assignment"],
    ["ArrowLeft", null],
    ["x", "cut"],
  ] as const) {
    assert.equal(outsideKey(["modal"], key, cmd), "block", key);
    assert.equal(heldBack(["modal"], cmd), true, key);
  }
  // A non-modal sheet under a modal one doesn't let anything more through.
  assert.equal(outsideKey(["non-modal", "modal"], " ", "play"), "block");
});

test("Esc closes the top sheet, Return presses its default button, Tab still moves the focus", () => {
  for (const modes of [["modal"], ["non-modal"]] as const) {
    assert.equal(outsideKey([...modes], "Escape", null), "close");
    assert.equal(outsideKey([...modes], "Enter", "to-start"), "default");
    assert.equal(outsideKey([...modes], "Tab", null), "pass");
  }
});

test("Controller Assignments (non-modal) lets Learn and the transport through, not editing", () => {
  for (const cmd of ["learn-assignment", "controller-assignments", "play", "record", "cycle", "rewind", "forward"]) {
    assert.ok(NON_MODAL_COMMANDS.has(cmd));
    assert.equal(outsideKey(["non-modal"], "k", cmd), "pass", cmd);
    assert.equal(heldBack(["non-modal"], cmd), false, cmd);
  }
  for (const cmd of ["delete-regions", "cut", "paste", "copy", "paste-replace", "section-cut", "nudge-left", "mute-regions", null]) {
    assert.equal(outsideKey(["non-modal"], "Backspace", cmd), "block", String(cmd));
    assert.equal(heldBack(["non-modal"], cmd), true, String(cmd));
  }
});

test("the non-modal commands are real key commands", () => {
  const keymap = read("src/keymap.ts");
  for (const id of NON_MODAL_COMMANDS) assert.ok(keymap.includes(`{ id: "${id}",`), id);
});

test("every sheet registers its keys", () => {
  // The shared frames, and the sheets with their own markup.
  assert.match(read("src/components/Dialogs.tsx"), /export function Sheet\([^]*?useSheetKeys\(ref, onClose, mode\)/);
  assert.match(read("src/components/Settings.tsx"), /function SheetFrame\([^]*?useSheetKeys\(ref, onClose\)/);
  assert.match(read("src/components/NameSheet.tsx"), /useSheetKeys\(box,/);
  assert.match(read("src/components/ProjectChooser.tsx"), /useSheetKeys\(box, close\)/);
  assert.match(read("src/components/ControllerAssignments.tsx"), /<Sheet title="Controller Assignments"[^>]*mode="non-modal"/);
  // Any other file drawing a sheet's box registers it too.
  const dir = new URL("../src/components/", import.meta.url);
  for (const f of readdirSync(dir).filter((f) => f.endsWith(".tsx"))) {
    const src = readFileSync(new URL(f, dir), "utf8");
    if (/className="(sheet|chooser)"/.test(src)) assert.ok(src.includes("useSheetKeys("), `${f} draws a sheet without useSheetKeys`);
  }
  assert.match(read("src/main.tsx"), /installSheetKeys\(\);/);
});
