import { test } from "node:test";
import assert from "node:assert/strict";
import { auxInput, hasExtraOutputs, nextOutput } from "../src/multiOut.ts";

const bus = (index: number, name: string) => ({ index, name, channels: 2, active: false });
const outputs = [{ track: 1, outputs: [bus(0, "Stereo out"), bus(1, "Direct out 0"), bus(2, "Direct out 1")] }];
const inst = { id: 1, name: "Drums" };
const onAux = (b: number) => ({ id: 10 + b, instrument_output: { track: 1, bus: b } });

test("the + takes the next output no aux is on", () => {
  assert.equal(nextOutput(outputs, [], 1)?.index, 1);
  assert.equal(nextOutput(outputs, [onAux(1)], 1)?.index, 2);
  // A gap left by a deleted aux fills first.
  assert.equal(nextOutput(outputs, [onAux(2)], 1)?.index, 1);
  assert.equal(nextOutput(outputs, [onAux(1), onAux(2)], 1), null);
});

test("another instrument's aux on the same bus number doesn't count", () => {
  assert.equal(nextOutput(outputs, [{ id: 20, instrument_output: { track: 2, bus: 1 } }], 1)?.index, 1);
});

test("instruments with one output, or none loaded, have no +", () => {
  assert.equal(nextOutput(outputs, [], 2), null);
  assert.equal(nextOutput(undefined, [], 1), null);
  assert.equal(hasExtraOutputs(outputs, 1), true);
  assert.equal(hasExtraOutputs(outputs, 2), false);
  assert.equal(hasExtraOutputs([{ track: 3, outputs: [bus(0, "Main")] }], 3), false);
});

test("an ordinary aux shows its bus", () => {
  assert.deepEqual(auxInput({ bus: 3 }, [inst], outputs).label, "Bus 3");
  assert.equal(auxInput({ bus: 3, instrument_output: null }, [inst], outputs).silent, false);
});

test("an aux on an output shows the instrument and the output", () => {
  const got = auxInput({ bus: 3, instrument_output: { track: 1, bus: 2 } }, [inst], outputs);
  assert.equal(got.label, "Drums: Direct out 1");
  assert.equal(got.silent, false);
});

test("an aux whose output is gone says so", () => {
  // The instrument was replaced by one with fewer outputs, or the plug-in is missing.
  for (const all of [outputs, [], undefined]) {
    const got = auxInput({ bus: 3, instrument_output: { track: 1, bus: 7 } }, [inst], all);
    assert.equal(got.silent, true);
    assert.equal(got.label, "Drums: no output 8");
    assert.match(got.title, /Bus 3/);
  }
});
