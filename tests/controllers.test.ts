// Unit tests for src/controllers.ts, the controller assignment types and labels (MIDI Learn).
import { test } from "node:test";
import assert from "node:assert/strict";
import { autoParamTarget, channelLabel, controllerLabel, fromPercent, stripTarget, toPercent, type Controller } from "../src/controllers.ts";

test("controllers are labelled like Logic's assignment list", () => {
  const cc: Controller = { port: "nanoKONTROL2", channel: 0, kind: { type: "cc", number: 7 } };
  assert.equal(controllerLabel(cc), "CC 7");
  assert.equal(channelLabel(cc), "Ch 1");
  const bend: Controller = { port: "Keys", channel: 15, kind: { type: "pitch_bend" } };
  assert.equal(controllerLabel(bend), "Pitch Bend");
  assert.equal(channelLabel(bend), "Ch 16");
});

test("ranges show as whole percentages and come back clamped", () => {
  assert.equal(toPercent(0.254), 25);
  assert.equal(toPercent(1.2), 100);
  assert.equal(toPercent(-1), 0);
  assert.equal(fromPercent(75), 0.75);
  assert.equal(fromPercent(140), 1);
  assert.equal(fromPercent(-5), 0);
});

test("an automation lane's parameter becomes a target", () => {
  assert.deepEqual(autoParamTarget(3, { type: "volume" }), { type: "volume", track: 3 });
  assert.deepEqual(autoParamTarget(3, { type: "send", value: 2 }), { type: "send", track: 3, slot: 2 });
  assert.deepEqual(autoParamTarget(3, { type: "plugin", value: { uid: 9, param: 1234, name: "Synth: Cutoff" } }), {
    type: "plugin",
    track: 3,
    uid: 9,
    plugin: "",
    param: 1234,
  });
});

test("a right-click on a strip control finds its target", () => {
  const inside = (...sels: string[]) => (sel: string) => sels.includes(sel);
  assert.deepEqual(stripTarget(4, inside(".strip-fader")), { type: "volume", track: 4 });
  assert.deepEqual(stripTarget(4, inside(".strip-pan .knob")), { type: "pan", track: 4 });
  // The switches toggle on any button, so they aren't offered.
  assert.equal(stripTarget(4, inside(".tbtn.mute")), null);
  assert.equal(stripTarget(4, inside(".strip-name")), null);
  // Stereo Out: only its fader.
  assert.deepEqual(stripTarget(null, inside(".strip-fader")), { type: "master_volume" });
  assert.equal(stripTarget(null, inside(".strip-pan .knob")), null);
});
