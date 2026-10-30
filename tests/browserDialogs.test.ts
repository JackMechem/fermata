// macOS's web view (WKWebView, which Tauri uses there) doesn't implement window.prompt,
// window.confirm or window.alert: prompt returns null and the others show nothing, so a feature
// built on them silently does nothing on a Mac. The app asks with its own name sheet
// (components/NameSheet.tsx `askName`) and the dialog plug-in's native `ask`/`message`/`confirm`.
// This reads every source file and fails on a browser dialog.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

// fileURLToPath, not .pathname: on Windows that would be "/D:/…", which fs can't open.
const SRC = fileURLToPath(new URL("../src/", import.meta.url));

function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((f) => {
    const p = join(dir, f);
    if (statSync(p).isDirectory()) return sources(p);
    return /\.(ts|tsx)$/.test(f) ? [p] : [];
  });
}

/** The source without comments (a comment may name window.prompt to say why it isn't used). */
const code = (src: string) => src.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[^:"'`\\])\/\/.*$/gm, "$1");

/** The dialog plug-in's functions a file imports by these names (they open native dialogs). */
function pluginImports(src: string): Set<string> {
  const out = new Set<string>();
  for (const m of src.matchAll(/import\s*\{([^}]*)\}\s*from\s*["']@tauri-apps\/plugin-dialog["']/g)) {
    for (const part of m[1].split(",")) {
      const name = part.trim().split(/\s+as\s+/).pop()!.trim();
      if (name) out.add(name);
    }
  }
  return out;
}

/** Browser dialogs used in a source: `window.prompt` (called or not), and bare `prompt(`,
 *  `confirm(` and `alert(` calls unless the dialog plug-in's function of that name is imported. */
function browserDialogs(src: string): string[] {
  const body = code(src);
  const native = pluginImports(body);
  const found: string[] = [];
  for (const m of body.matchAll(/\b(?:window|globalThis|self)\s*\.\s*(prompt|confirm|alert)\b/g)) found.push(m[0]);
  for (const m of body.matchAll(/(?<![\w$.])(prompt|confirm|alert)\s*\(/g)) {
    // A declaration of that name (`function alert(`) isn't a call, nor is a method (`x.alert(`).
    const before = body.slice(Math.max(0, (m.index ?? 0) - 9), m.index);
    if (/function\s+$/.test(before) || native.has(m[1])) continue;
    found.push(m[0]);
  }
  return found;
}

test("the checker finds browser dialogs and lets the dialog plug-in's through", () => {
  assert.deepEqual(browserDialogs(`const n = window.prompt("Name", "x");`), ["window.prompt"]);
  assert.deepEqual(browserDialogs(`if (confirm("Sure?")) go();`), ["confirm("]);
  assert.deepEqual(browserDialogs(`alert ("hi")`), ["alert ("]);
  assert.deepEqual(browserDialogs(`const p = window.confirm;`), ["window.confirm"]);
  assert.deepEqual(browserDialogs(`import { confirm } from "@tauri-apps/plugin-dialog";\nawait confirm("Sure?");`), []);
  assert.deepEqual(browserDialogs(`import { ask, message } from "@tauri-apps/plugin-dialog";\nawait confirm("Sure?");`), ["confirm("]);
  assert.deepEqual(browserDialogs(`confirmDiscard("Quit"); askName({}); x.alert(1); const prompted = 1;`), []);
  assert.deepEqual(browserDialogs(`// window.prompt returns null on macOS\n/* alert( */ go();`), []);
});

test("no source uses window.prompt, confirm or alert", () => {
  const bad = sources(SRC).flatMap((p) => browserDialogs(readFileSync(p, "utf8")).map((d) => `${p.slice(SRC.length)}: ${d}`));
  assert.deepEqual(bad, [], "use askName (components/NameSheet.tsx) or @tauri-apps/plugin-dialog instead");
});
