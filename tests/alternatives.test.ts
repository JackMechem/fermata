// Unit tests for src/alternativesModel.ts (backup labels, names, track alternatives). Run with
// `npm test`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { activeTrackAlt, backupLabel, freeName, nameProblem, showsTrackAlts, trackAltNames } from "../src/alternativesModel.ts";

const at = (y: number, mo: number, d: number, h: number, mi: number) => new Date(y, mo - 1, d, h, mi).getTime();

test("backup labels say today, yesterday or the date", () => {
  const now = at(2026, 10, 29, 16, 5);
  assert.equal(backupLabel(at(2026, 10, 29, 14, 32), now), "Today 14:32");
  assert.equal(backupLabel(at(2026, 10, 29, 0, 0), now), "Today 00:00");
  assert.equal(backupLabel(at(2026, 10, 28, 9, 10), now), "Yesterday 09:10");
  assert.equal(backupLabel(at(2026, 10, 28, 23, 59), at(2026, 10, 29, 0, 1)), "Yesterday 23:59");
  assert.equal(backupLabel(at(2026, 10, 27, 14, 32), now), "27 Oct 2026 14:32");
  assert.equal(backupLabel(at(2025, 1, 3, 8, 0), now), "3 Jan 2025 08:00");
});

test("names are checked like the Rust side", () => {
  assert.equal(nameProblem("Rough Mix"), null);
  assert.equal(nameProblem("  "), "The name can't be empty.");
  for (const bad of [".hidden", "a/b", "a\\b", "a:b", "what?", 'say "hi"', "a|b", "tab\there"]) assert.notEqual(nameProblem(bad), null, bad);
  assert.notEqual(nameProblem("x".repeat(101)), null);
  assert.equal(freeName("Song", ["Other"]), "Song");
  assert.equal(freeName("Song", ["song", "Song 2"]), "Song 3");
});

test("track alternatives list the active one with the rest", () => {
  const one = { alternatives: undefined, alternative: undefined };
  assert.equal(activeTrackAlt(one), "A");
  assert.deepEqual(trackAltNames(one), ["A"]);
  assert.equal(showsTrackAlts(one), false);
  const three = { alternatives: [{ name: "C" }, { name: "a" }], alternative: "B" };
  assert.equal(activeTrackAlt(three), "B");
  assert.deepEqual(trackAltNames(three), ["a", "B", "C"]);
  assert.equal(showsTrackAlts(three), true);
});
