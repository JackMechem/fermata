import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const ROOT = fileURLToPath(new URL("..", import.meta.url));
const read = (p: string) => readFileSync(ROOT + p, "utf8");

test("no trademark reaches the product", () => {
  // The docs say plainly what it's modelled on; panel, presets and menus don't.
  for (const f of ["crates/engine/src/instrument/tre/params.rs", "crates/engine/src/instrument/tre/presets.rs", "src/components/fx/Instrument.tsx", "src/pluginMenu.ts"]) {
    assert.ok(!/moog/i.test(read(f)), f);
  }
  const info = read("crates/engine/src/instrument/tre/mod.rs").split("pub fn info()")[1].split("\n}")[0];
  assert.ok(!/moog|model d/i.test(info));
});

// Each model's product-facing files and what mustn't appear in them. The docs may name what it's
// modelled on; the panel, presets and menus don't.
const MODELS: [string, RegExp][] = [
  ["sei", /roland|juno|\bjx\b/i],
  ["cinque", /sequential|prophet|\bdave smith\b/i],
  ["uno", /roland|\btb-?303\b|\b303\b|devil fish/i],
  ["sedici", /yamaha|\bdx-?7\b|\bdx\b/i],
  ["onda", /serum|xfer|massive|vital|ppg|waldorf/i],
];

for (const [model, mark] of MODELS) {
  test(`${model}: no trademark in its panel or presets`, () => {
    for (const f of ["params.rs", "presets.rs"]) {
      assert.ok(!mark.test(read(`crates/engine/src/instrument/${model}/${f}`)), f);
    }
    const info = read(`crates/engine/src/instrument/${model}/mod.rs`).split("pub fn info()")[1].split("\n}")[0];
    assert.ok(!mark.test(info), "info");
  });
}

test("the modern synth has a group of its own, the others are Vintage Synths", () => {
  for (const [model] of MODELS) {
    const info = read(`crates/engine/src/instrument/${model}/mod.rs`).split("pub fn info()")[1];
    assert.ok(info.includes(model === "onda" ? 'group: "Modern Synths"' : 'group: "Vintage Synths"'), model);
  }
});

test("the windows that show panels get the instrument list too", () => {
  assert.ok(read("src/detachModel.ts").includes('"instList"'));
  assert.ok(read("src-tauri/src/main.rs").includes("commands::list_builtin_instruments"));
});
