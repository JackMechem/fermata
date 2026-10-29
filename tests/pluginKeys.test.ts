// Unit tests for src/pluginKeys.ts: keys forwarded from a plug-in's editor window become the
// KeyboardEvent a key pressed in the main window would be. (vst3-host's keys.rs tests the
// macOS key codes.)
import { test } from "node:test";
import assert from "node:assert/strict";
import { keyEventInit, type ForwardedKey } from "../src/pluginKeys.ts";

const key = (k: Partial<ForwardedKey>): ForwardedKey => ({ up: false, key: "", code: "", shiftKey: false, ctrlKey: false, altKey: false, metaKey: false, repeat: false, ...k });

test("a press is a keydown with the key's fields", () => {
  const [type, init] = keyEventInit(key({ key: " ", code: "Space" }));
  assert.equal(type, "keydown");
  assert.deepEqual(init, { key: " ", code: "Space", shiftKey: false, ctrlKey: false, altKey: false, metaKey: false, repeat: false, bubbles: true, cancelable: true });
});

test("modifiers and repeats carry over; a release is a keyup", () => {
  const [, init] = keyEventInit(key({ key: "R", code: "KeyR", shiftKey: true, metaKey: true, repeat: true }));
  assert.equal(init.shiftKey && init.metaKey && init.repeat, true);
  assert.equal(init.ctrlKey || init.altKey, false);
  assert.equal(keyEventInit(key({ up: true, key: "a", code: "KeyA" }))[0], "keyup");
});

test("events bubble from the focused element to the window's listeners", () => {
  const [, init] = keyEventInit(key({ key: "Enter", code: "Enter" }));
  assert.equal(init.bubbles, true);
  assert.equal(init.cancelable, true);
});
