import { test } from "node:test";
import assert from "node:assert/strict";
import { cpuTitle, shownPeak } from "../src/cpuMeter.ts";

test("the peak never shows under the average", () => {
  assert.equal(shownPeak(0.2, 0.9), 0.9);
  assert.equal(shownPeak(0.5, 0.3), 0.5);
});

test("the tooltip gives the average and the peak", () => {
  const title = cpuTitle(0.52, 1.304, 0);
  assert.match(title, /Average 52%, recent peak 130%\./);
  assert.match(title, /system monitor/);
});

test("late blocks are counted, and only mentioned when there are some", () => {
  assert.doesNotMatch(cpuTitle(0.1, 0.1, 0), /Click to clear/);
  assert.match(cpuTitle(0.1, 0.1, 1), /1 block over 100%\. Click to clear\./);
  assert.match(cpuTitle(0.1, 0.1, 3), /3 blocks over 100%\. Click to clear\./);
});
