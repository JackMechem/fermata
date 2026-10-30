// Checks the frontend's calls into Rust against src-tauri: every command named in an `act`,
// `actMade`, `call` or `invoke` is registered, and each call passes the command's required
// (non-Option) arguments, in camelCase, and nothing it doesn't take. A wrong name only fails
// at run time, when the command is used.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";

const ROOT = new URL("..", import.meta.url).pathname;

function files(dir: string, ext: RegExp): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? files(join(dir, e.name), ext) : ext.test(e.name) ? [join(dir, e.name)] : []));
}

/** Splits on commas outside brackets. */
function topLevel(s: string, open: string, close: string): string[] {
  const parts: string[] = [];
  let depth = 0;
  let cur = "";
  for (const c of s) {
    if (open.includes(c)) depth++;
    else if (close.includes(c)) depth--;
    if (c === "," && depth === 0) {
      parts.push(cur);
      cur = "";
    } else cur += c;
  }
  parts.push(cur);
  return parts.map((p) => p.trim()).filter(Boolean);
}

/** From `start` (just past an opening bracket) to its closing one. */
function balanced(s: string, start: number, open: string, close: string): string {
  let depth = 1;
  let i = start;
  for (; i < s.length && depth; i++) {
    if (open.includes(s[i])) depth++;
    else if (close.includes(s[i])) depth--;
  }
  return s.slice(start, i - 1);
}

/** The Tauri commands and the arguments JS passes them (State, AppHandle and windows aside). */
function commands(): Map<string, { name: string; optional: boolean }[]> {
  const out = new Map<string, { name: string; optional: boolean }[]>();
  for (const path of files(join(ROOT, "src-tauri/src"), /\.rs$/)) {
    const src = readFileSync(path, "utf8");
    for (const m of src.matchAll(/#\[tauri::command[^\]]*\]\s*(?:#\[[^\]]*\]\s*|\/\/\/[^\n]*\n\s*)*pub (?:async )?fn (\w+)(?:<[^>]*>)?\(/g)) {
      const params = topLevel(balanced(src, m.index + m[0].length, "(<[", ")>]"), "(<[", ")>]")
        .map((p) => ({ name: p.slice(0, p.indexOf(":")).trim().replace(/^mut /, ""), type: p.slice(p.indexOf(":") + 1).trim() }))
        .filter((p) => !/State<|AppHandle|Window|Webview/.test(p.type));
      out.set(
        m[1],
        params.map((p) => ({ name: p.name, optional: p.type.startsWith("Option<") })),
      );
    }
  }
  return out;
}

const registered = (() => {
  const main = readFileSync(join(ROOT, "src-tauri/src/main.rs"), "utf8");
  const list = main.slice(main.indexOf("generate_handler!["));
  return new Set([...balanced(list, list.indexOf("[") + 1, "[", "]").matchAll(/(\w+)\s*(?:,|$)/g)].map((m) => m[1]));
})();

interface Use {
  where: string;
  cmd: string;
  /** The argument names passed (snake_case), or null when they can't be read (a variable). */
  args: string[] | null;
  /** Some arguments come from a spread. */
  spread: boolean;
}

function uses(): Use[] {
  const out: Use[] = [];
  for (const path of files(join(ROOT, "src"), /\.tsx?$/)) {
    const src = readFileSync(path, "utf8");
    for (const m of src.matchAll(/\b(?:act|call|actMade|invoke)(?:<[^>]*>)?\(\s*"(\w+)"\s*(,\s*)?/g)) {
      const where = `${relative(ROOT, path)}:${src.slice(0, m.index).split("\n").length}`;
      const at = m.index + m[0].length;
      if (!m[2]) out.push({ where, cmd: m[1], args: [], spread: false });
      else if (src[at] !== "{") out.push({ where, cmd: m[1], args: null, spread: true });
      else {
        const args: string[] = [];
        let spread = false;
        for (const p of topLevel(balanced(src, at + 1, "([{", ")]}"), "([{", ")]}")) {
          const k = p.match(/^"?([A-Za-z_$][\w$]*)"?\s*(?::|$)/);
          if (k && !p.startsWith("...")) args.push(k[1].replace(/[A-Z]/g, (c) => "_" + c.toLowerCase()));
          else spread = true;
        }
        out.push({ where, cmd: m[1], args, spread });
      }
    }
  }
  return out;
}

test("the scan finds the commands and their uses", () => {
  assert.ok(commands().size > 150);
  assert.ok(registered.size > 150);
  assert.ok(uses().length > 200);
});

test("every command the frontend calls is registered", () => {
  const cmds = commands();
  const bad = uses().filter((u) => !cmds.has(u.cmd) || !registered.has(u.cmd));
  assert.deepEqual(
    bad.map((u) => `${u.where}: ${u.cmd}`),
    [],
  );
});

test("every call passes the command's arguments by their names", () => {
  const cmds = commands();
  const bad: string[] = [];
  for (const u of uses()) {
    const params = cmds.get(u.cmd);
    if (!params || !u.args) continue;
    const extra = u.args.filter((a) => !params.some((p) => p.name === a));
    const missing = u.spread ? [] : params.filter((p) => !p.optional && !u.args!.includes(p.name)).map((p) => p.name);
    if (extra.length || missing.length) bad.push(`${u.where}: ${u.cmd} ${extra.length ? `takes no ${extra.join(", ")}` : ""}${missing.length ? ` needs ${missing.join(", ")}` : ""}`);
  }
  assert.deepEqual(bad, []);
});
