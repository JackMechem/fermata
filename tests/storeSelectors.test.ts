// A lint-style check: a `useStore`, `useEd` or `useSettings` selector must not build a new
// array or object each call. They run inside useSyncExternalStore, which re-renders whenever the
// result is not `===` the last one, so a fresh `[]` or `{}` re-renders forever and blanks the
// window. Derive such values outside the selector (or with useMemo) from a stable selection.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";

const ROOT = new URL("..", import.meta.url).pathname;

/**
 * Selectors that match a risky pattern but return a stable value (a number, or an element of
 * the snapshot). Keyed by file and the start of the selector, so a new one still fails.
 */
const ALLOWED: { file: string; starts: string; why: string }[] = [
  { file: "src/components/Latency.tsx", starts: "(s) => { if (uid === undefined) return 0;", why: "returns a number" },
  { file: "src/components/StripSilence.tsx", starts: "(s) => s.snap?.project.tracks.flatMap(", why: "returns the found region" },
  { file: "src/components/Vca.tsx", starts: "(s) => s.snap?.project.tracks.filter(", why: "returns a count" },
];

/** Builds a new value on each call: array methods that copy, empty fallbacks, spreads, literals. */
const RISKY = [
  /\.(filter|map|flatMap|slice|concat|sort|toSorted|toReversed|keys|values|entries)\(/,
  /(\?\?|\|\|)\s*[[{]/,
  /\.\.\./,
  /=>\s*\(\s*\{/,
  /=>\s*\[/,
  /\b(Object|Array)\.\w+\(/,
  /\bnew\s+[A-Z]/,
];

function sources(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
    e.isDirectory() ? sources(join(dir, e.name)) : /\.tsx?$/.test(e.name) ? [join(dir, e.name)] : [],
  );
}

/** Every selector passed to a store hook, with where it is. */
function selectors(): { file: string; line: number; sel: string }[] {
  const out: { file: string; line: number; sel: string }[] = [];
  for (const path of sources(join(ROOT, "src"))) {
    const src = readFileSync(path, "utf8");
    const re = /\b(?:useStore|useEd|useSettings)(?:<[^>]*>)?\(/g;
    for (let m; (m = re.exec(src)); ) {
      if (/function\s+$/.test(src.slice(Math.max(0, m.index - 10), m.index))) continue; // the definition
      let i = m.index + m[0].length;
      for (let depth = 1; depth && i < src.length; i++) {
        const c = src[i];
        if ("([{".includes(c)) depth++;
        else if (")]}".includes(c)) depth--;
      }
      out.push({
        file: relative(ROOT, path),
        line: src.slice(0, m.index).split("\n").length,
        sel: src.slice(m.index + m[0].length, i - 1).replace(/\s+/g, " ").trim(),
      });
    }
  }
  return out;
}

test("the scan finds the store hooks' selectors", () => {
  const all = selectors();
  assert.ok(all.length > 150, `only ${all.length} selectors found`);
  assert.ok(all.some((s) => s.sel === "(s) => s.snap?.project"));
});

test("no store selector returns a new array or object", () => {
  const bad = selectors().filter(
    ({ file, sel }) => RISKY.some((r) => r.test(sel)) && !ALLOWED.some((a) => a.file === file && sel.startsWith(a.starts)),
  );
  assert.deepEqual(
    bad.map((b) => `${b.file}:${b.line}: ${b.sel}`),
    [],
    "select the stable value and derive from it outside the selector",
  );
});

test("the allow-list has no stale entries", () => {
  const all = selectors();
  for (const a of ALLOWED) assert.ok(all.some((s) => s.file === a.file && s.sel.startsWith(a.starts)), `${a.file}: ${a.starts}`);
});
