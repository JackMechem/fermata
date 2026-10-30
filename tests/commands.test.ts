// Checks that the key commands and the native menu's items all do something, and that no two
// key commands share a default key. keymap.ts and App.tsx import the Tauri API, so this reads
// their sources rather than importing them.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";

const read = (path: string) => readFileSync(new URL(`../${path}`, import.meta.url), "utf8");
const keymap = read("src/keymap.ts");
const app = read("src/App.tsx");
const arrange = read("src/arrangeCmds.ts");
const stepInput = read("src/components/StepInput.tsx");
const menuRs = read("src-tauri/src/menu.rs");

/** The text from `start` up to the first `end` after it. */
function slice(src: string, start: string, end: string): string {
  const i = src.indexOf(start);
  assert.ok(i >= 0, `missing ${start}`);
  const j = src.indexOf(end, i + start.length);
  assert.ok(j >= 0, `missing ${end} after ${start}`);
  return src.slice(i, j);
}

/** The keys of an object literal's entries: `name:` or `"name":`. */
const objectKeys = (body: string) => [...body.matchAll(/^\s+"?([a-z0-9-]+)"?: /gm)].map((m) => m[1]);

// ---- KEY_COMMANDS, with each command's default keys on macOS and elsewhere -------------------

interface Cmd {
  id: string;
  group: string;
  keys: { mac: string[]; other: string[] };
}

function commands(): Cmd[] {
  const body = slice(keymap, "export const KEY_COMMANDS", "\n];");
  const out: Cmd[] = [];
  for (const m of body.matchAll(/\{ id: "([^"]+)", label: "[^"]*", group: "([^"]+)", keys: \[(.*)\] \},?$/gm)) {
    const keys = { mac: [] as string[], other: [] as string[] };
    for (const k of m[3].matchAll(/IS_MAC \? "([^"]*)" : "([^"]*)"|"([^"]*)"/g)) {
      keys.mac.push(k[1] ?? k[3]);
      keys.other.push(k[2] ?? k[3]);
    }
    out.push({ id: m[1], group: m[2], keys });
  }
  // The Step Input note values: `...[1, 2, 4, …].map((v, i) => ({ id: `step-${v}`, … keys: [String(i + 1)] }))`.
  const values = body.match(/\.\.\.\[([\d, ]+)\]\.map\(\(v, i\) => \(\{ id: `step-\$\{v\}`/);
  assert.ok(values, "the Step Input note values");
  values[1].split(",").forEach((v, i) => out.push({ id: `step-${v.trim()}`, group: "Step Input", keys: { mac: [String(i + 1)], other: [String(i + 1)] } }));
  return out;
}

/** normalizeKey from keymap.ts, for either platform (Ctrl is ⌘'s key outside macOS). */
function normalize(k: string, mac: boolean): string {
  const parts = k.split("+");
  let key = parts.pop() ?? "";
  if (key === "" && parts[parts.length - 1] === "") {
    parts.pop();
    key = "+";
  }
  const mods = new Set(parts.map((p) => (p === "Ctrl" && !mac ? "Mod" : p)));
  const out = ["Mod", "Ctrl", "Alt", "Shift"].filter((m) => mods.has(m));
  out.push(key.length === 1 ? key.toUpperCase() : key);
  return out.join("+");
}

function menuShortcuts(): { label: string; mac: string; other: string }[] {
  const body = slice(keymap, "export const MENU_SHORTCUTS", "\n];");
  return [...body.matchAll(/\{ label: "([^"]+)", menu: "[^"]+", key: (?:IS_MAC \? "([^"]+)" : "([^"]+)"|"([^"]+)") \}/g)].map((m) => ({
    label: m[1],
    mac: m[2] ?? m[4],
    other: m[3] ?? m[4],
  }));
}

test("the key command list is read whole", () => {
  const cmds = commands();
  assert.ok(cmds.length > 80, `only ${cmds.length} commands`);
  assert.equal(cmds.length, (slice(keymap, "export const KEY_COMMANDS", "\n];").match(/\{ id: /g) ?? []).length + 7);
  assert.equal(new Set(cmds.map((c) => c.id)).size, cmds.length, "two commands share an id");
  assert.ok(menuShortcuts().length > 20);
});

// commandForEvent looks a key up in one map for every command, Step Input's included, and the
// first command listed wins: a key shared by two commands would never reach the second.
for (const mac of [false, true]) {
  test(`no two key commands or menu items share a default key (${mac ? "macOS" : "Linux and Windows"})`, () => {
    const owner = new Map<string, string>();
    const clashes: string[] = [];
    const claim = (k: string, who: string) => {
      const n = normalize(k, mac);
      const was = owner.get(n);
      if (was && was !== who) clashes.push(`${n}: ${was} and ${who}`);
      owner.set(n, who);
    };
    for (const m of menuShortcuts()) claim(mac ? m.mac : m.other, `menu ${m.label}`);
    for (const c of commands()) for (const k of mac ? c.keys.mac : c.keys.other) claim(k, c.id);
    assert.deepEqual(clashes, []);
  });
}

test("every key command does something", () => {
  const actions = new Set([
    ...objectKeys(slice(app, "const KEY_ACTIONS", "\n};")),
    ...objectKeys(slice(arrange, "export const ARRANGE_COMMANDS", "\n};")),
  ]);
  // Step Input's, while its panel is open; the note values are added from NOTE_VALUES.
  const step = slice(stepInput, "const commands: Record<string, () => void> = {", "\n  };");
  assert.match(step, /si\.NOTE_VALUES\.map\(\(v\) => \[`step-\$\{v\}`/);
  for (const id of objectKeys(step)) actions.add(id);
  const unhandled = commands().filter((c) => !actions.has(c.id) && !(c.group === "Step Input" && /^step-\d+$/.test(c.id)));
  assert.deepEqual(unhandled.map((c) => c.id), []);
  // And nothing handles a command that isn't there (a renamed id).
  const ids = new Set(commands().map((c) => c.id));
  assert.deepEqual([...actions].filter((a) => !ids.has(a)), []);
});

// ---- the native menu ------------------------------------------------------------------------

/** Every id the menu bar can send: its items, and the generated `alternative:`/`revert:` ones. */
function menuIds(): string[] {
  return [...menuRs.matchAll(/(?:item\(|MenuItem::with_id\(app, |CheckMenuItem::with_id\(app, )(?:"([^"]+)"|format!\("([a-z-]+:)\{)/g)].map((m) => m[1] ?? m[2]);
}

test("every native menu item is handled", () => {
  const ids = menuIds();
  assert.ok(ids.length > 100, `only ${ids.length} menu ids`);
  assert.ok(ids.includes("alternative:") && ids.includes("revert:"));
  const handler = slice(app, "function handleMenu", "\n/** What each key command does");
  const cases = new Set([...handler.matchAll(/case "([^"]+)":/g)].map((m) => m[1]));
  const prefixes = [...handler.matchAll(/id\.startsWith\("([^"]+)"\)/g)].map((m) => m[1]);
  assert.match(handler, /if \(runArrangeCommand\(id\)\) return;/);
  const arranged = new Set(objectKeys(slice(arrange, "export const ARRANGE_COMMANDS", "\n};")));
  // "No Backups" is a disabled placeholder.
  const unhandled = ids.filter((id) => id !== "revert-none" && !cases.has(id) && !arranged.has(id) && !prefixes.some((p) => id.startsWith(p)));
  assert.deepEqual(unhandled, []);
});

test("the key commands that go through handleMenu name one of its cases", () => {
  const handler = slice(app, "function handleMenu", "\n/** What each key command does");
  const cases = new Set([...handler.matchAll(/case "([^"]+)":/g)].map((m) => m[1]));
  const calls = [...slice(app, "const KEY_ACTIONS", "\n};").matchAll(/handleMenu\("([^"]+)"\)/g)].map((m) => m[1]);
  assert.ok(calls.length > 20);
  assert.deepEqual(calls.filter((c) => !cases.has(c)), []);
});

test("every key hint in a menu or tooltip names a key command", () => {
  const ids = new Set(commands().map((c) => c.id));
  const src = (dir: string): string[] =>
    readdirSync(new URL(`../${dir}`, import.meta.url), { withFileTypes: true }).flatMap((e) =>
      e.isDirectory() ? src(`${dir}/${e.name}`) : /\.tsx?$/.test(e.name) ? [read(`${dir}/${e.name}`)] : [],
    );
  const named = src("src").flatMap((s) => [...s.matchAll(/\b(?:keyHint|keysFor)\("([^"]+)"/g)].map((m) => m[1]));
  assert.ok(named.length > 20);
  assert.deepEqual([...new Set(named)].filter((id) => !ids.has(id)), []);
});
