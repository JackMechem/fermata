// Unit tests for src/bounceFormat.ts: the Bounce dialog's formats, resolutions and file names.
import { test } from "node:test";
import assert from "node:assert/strict";
import { depthFor, depthsFor, extensionOf, fileFilter, FORMATS, withExtension } from "../src/bounceFormat.ts";

test("FLAC offers 16 and 24-bit only; WAV and AIFF also 32-bit float", () => {
  assert.deepEqual(depthsFor("flac").map(([d]) => d), ["int16", "int24"]);
  for (const f of ["wav", "aiff"] as const) assert.deepEqual(depthsFor(f).map(([d]) => d), ["int16", "int24", "float32"]);
});

test("a remembered float resolution becomes 24-bit for FLAC and stays for the others", () => {
  assert.equal(depthFor("flac", "float32"), "int24");
  assert.equal(depthFor("flac", "int16"), "int16");
  assert.equal(depthFor("aiff", "float32"), "float32");
  assert.equal(depthFor("wav", "int16"), "int16");
});

test("extensions match the engine's", () => {
  assert.deepEqual(FORMATS.map(([f]) => extensionOf(f)), ["wav", "aif", "flac"]);
  assert.deepEqual(fileFilter("aiff"), { name: "AIFF audio", extensions: ["aif", "aiff", "aifc"] });
});

test("file names get the format's extension", () => {
  assert.equal(withExtension("/x/Mix", "flac"), "/x/Mix.flac");
  assert.equal(withExtension("/x/Mix.FLAC", "flac"), "/x/Mix.FLAC");
  assert.equal(withExtension("/x/Mix.aiff", "aiff"), "/x/Mix.aiff");
  assert.equal(withExtension("/x/Mix.wav", "aiff"), "/x/Mix.aif");
  assert.equal(withExtension("C:\\Songs\\Mix.aif", "wav"), "C:\\Songs\\Mix.wav");
  // A dot that isn't an audio extension is part of the name.
  assert.equal(withExtension("/x/Mix v1.2", "wav"), "/x/Mix v1.2.wav");
  assert.equal(withExtension("/x/my.song", "flac"), "/x/my.song.flac");
});
