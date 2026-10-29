// Unit tests for src/keyFormat.ts: shortcuts read like native apps on each platform.
import { test } from "node:test";
import assert from "node:assert/strict";
import { formatKeyFor } from "../src/keyFormat.ts";

const pc = (k: string) => formatKeyFor(k, false);
const mac = (k: string) => formatKeyFor(k, true);

test("Linux and Windows spell modifiers out, Ctrl Alt Shift then the key", () => {
  assert.equal(pc("Mod+D"), "Ctrl+D");
  assert.equal(pc("Mod+Shift+Z"), "Ctrl+Shift+Z");
  assert.equal(pc("Alt+K"), "Alt+K");
  assert.equal(pc("Mod+Alt+B"), "Ctrl+Alt+B");
  assert.equal(pc("Ctrl+Alt+B"), "Ctrl+Alt+B");
  assert.equal(pc("Shift+Space"), "Shift+Space");
  assert.equal(pc("Delete"), "Delete");
  assert.equal(pc("Backspace"), "Backspace");
  assert.equal(pc("Mod+Backspace"), "Ctrl+Backspace");
  assert.equal(pc("Mod+ArrowLeft"), "Ctrl+←");
  assert.equal(pc("Mod+="), "Ctrl+=");
  assert.equal(pc("Mod+-"), "Ctrl+-");
  assert.equal(pc("Mod++"), "Ctrl++");
  assert.equal(pc("Escape"), "Esc");
  // Mod and Ctrl are the same key outside macOS: shown once.
  assert.equal(pc("Mod+Ctrl+P"), "Ctrl+P");
});

test("macOS uses Logic's glyphs, ⌃⌥⇧⌘ then the key", () => {
  assert.equal(mac("Mod+D"), "⌘D");
  assert.equal(mac("Mod+Shift+Z"), "⇧⌘Z");
  assert.equal(mac("Alt+K"), "⌥K");
  assert.equal(mac("Ctrl+Alt+B"), "⌃⌥B");
  assert.equal(mac("Ctrl+B"), "⌃B");
  assert.equal(mac("Mod+Ctrl+P"), "⌃⌘P");
  assert.equal(mac("Backspace"), "⌫");
  assert.equal(mac("Mod+Backspace"), "⌘⌫");
  assert.equal(mac("Shift+Space"), "⇧Space");
  assert.equal(mac("Mod+ArrowRight"), "⌘→");
  assert.equal(mac("Mod+-"), "⌘−");
  assert.equal(mac("Alt+'"), "⌥'");
});

test("a bare modifier names the key, for gestures like Alt-drag", () => {
  assert.equal(pc("Alt"), "Alt");
  assert.equal(pc("Mod"), "Ctrl");
  assert.equal(mac("Alt"), "⌥");
  assert.equal(mac("Mod"), "⌘");
  // A plain letter key is not mistaken for a modifier.
  assert.equal(pc("K"), "K");
  assert.equal(mac("Space"), "Space");
});
