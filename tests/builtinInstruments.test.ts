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

test("the windows that show panels get the instrument list too", () => {
  assert.ok(read("src/detachModel.ts").includes('"instList"'));
  assert.ok(read("src-tauri/src/main.rs").includes("commands::list_builtin_instruments"));
});
