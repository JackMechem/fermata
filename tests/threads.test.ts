import { test } from "node:test";
import assert from "node:assert/strict";
import { AUTOMATIC_THREADS, threadOptions } from "../src/threads.ts";

test("Automatic comes first and says what it means on this machine", () => {
  const o = threadOptions({ automatic: 6, max: 16 });
  assert.equal(o[0][0], AUTOMATIC_THREADS);
  assert.match(o[0][1], /up to 6 threads/);
  assert.equal(threadOptions({ automatic: 1, max: 2 })[0][1], "Automatic (up to 1 thread)");
});

test("1 is the off switch, then every number up to the machine's", () => {
  const o = threadOptions({ automatic: 3, max: 4 });
  assert.deepEqual(o.map((x) => x[0]), [0, 1, 2, 3, 4]);
  assert.match(o[1][1], /audio thread alone/);
  assert.equal(o[4][1], "4 threads");
});

test("a one-core machine offers only the two", () => {
  assert.deepEqual(threadOptions({ automatic: 1, max: 1 }).map((x) => x[0]), [0, 1]);
});
