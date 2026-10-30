// Key commands. The main window looks its shortcuts up here (see `onKey` in App.tsx), so
// they can be reassigned in Settings › Key Commands. Shortcuts with ⌘ that the native menu
// bar owns are listed separately (MENU_SHORTCUTS) and shown read-only.
//
// A key is written as modifiers then the key, joined with "+": "Mod" is ⌘ on macOS and Ctrl
// elsewhere, "Ctrl" is the Control key on macOS (the same as Mod elsewhere), then "Alt" and
// "Shift". Letters are upper case ("Shift+K"); other keys use their KeyboardEvent.key name
// ("Space", "ArrowUp", "Enter", ",").
import { getSettings } from "./prefs";
import { formatKeyFor, IS_MAC } from "./keyFormat";

export interface KeyCommand {
  id: string;
  label: string;
  group: string;
  /** Default keys. */
  keys: string[];
}

export const KEY_GROUPS = ["Transport", "Navigation", "View", "Tracks", "Editing", "Windows", "Control Surfaces", "Step Input"] as const;

export const KEY_COMMANDS: KeyCommand[] = [
  { id: "play", label: "Play / Stop", group: "Transport", keys: ["Space"] },
  { id: "record", label: "Record", group: "Transport", keys: ["R", "Shift+R"] },
  { id: "cycle", label: "Cycle Mode", group: "Transport", keys: ["C"] },
  { id: "metronome", label: "Metronome", group: "Transport", keys: ["K"] },
  { id: "count-in", label: "Count-in", group: "Transport", keys: ["Shift+K"] },
  { id: "to-start", label: "Go to Beginning", group: "Transport", keys: ["Enter"] },
  { id: "rewind", label: "Rewind One Bar", group: "Transport", keys: [","] },
  { id: "forward", label: "Forward One Bar", group: "Transport", keys: ["."] },
  { id: "catch-playhead", label: "Catch Playhead", group: "Transport", keys: ["`"] },
  { id: "show-playhead", label: "Scroll to Playhead", group: "Transport", keys: ["Alt+`"] },
  { id: "autopunch", label: "Autopunch Mode", group: "Transport", keys: ["Mod+Ctrl+P"] },

  { id: "create-marker", label: "Create Marker", group: "Navigation", keys: ["Alt+'"] },
  { id: "delete-marker", label: "Delete Marker", group: "Navigation", keys: [] },
  { id: "previous-marker", label: "Go to Previous Marker", group: "Navigation", keys: ["Alt+,"] },
  { id: "next-marker", label: "Go to Next Marker", group: "Navigation", keys: ["Alt+."] },
  { id: "marker-list", label: "Open Marker List", group: "Navigation", keys: ["Mod+Alt+K"] },

  { id: "mixer", label: "Show/Hide Mixer", group: "View", keys: ["X"] },
  { id: "inspector", label: "Show/Hide Inspector", group: "View", keys: ["I"] },
  { id: "library", label: "Show/Hide Library", group: "View", keys: ["Y"] },
  { id: "editor", label: "Show/Hide Editors", group: "View", keys: ["E", "P"] },
  { id: "list-editors", label: "Show/Hide List Editors", group: "View", keys: ["D"] },
  { id: "browser", label: "Show/Hide Browsers", group: "View", keys: ["F"] },
  { id: "automation", label: "Show/Hide Automation", group: "View", keys: ["A"] },
  { id: "used-automation", label: "Show Used Automation", group: "View", keys: ["Shift+A"] },
  { id: "global-tracks", label: "Show/Hide Global Tracks", group: "View", keys: ["G"] },
  { id: "show-hidden", label: "Show Hidden Tracks", group: "View", keys: ["H"] },
  { id: "zoom-in", label: "Zoom In Horizontally", group: "View", keys: ["Mod+ArrowRight"] },
  { id: "zoom-out", label: "Zoom Out Horizontally", group: "View", keys: ["Mod+ArrowLeft"] },
  { id: "taller-tracks", label: "Zoom In Vertically", group: "View", keys: ["Mod+ArrowDown"] },
  { id: "shorter-tracks", label: "Zoom Out Vertically", group: "View", keys: ["Mod+ArrowUp"] },

  { id: "track-mute", label: "Mute Track", group: "Tracks", keys: ["M"] },
  { id: "track-solo", label: "Solo Track", group: "Tracks", keys: ["S"] },
  { id: "track-arm", label: "Record Enable Track", group: "Tracks", keys: ["Ctrl+R"] },
  { id: "select-previous", label: "Select Previous Track", group: "Tracks", keys: ["ArrowUp"] },
  { id: "select-next", label: "Select Next Track", group: "Tracks", keys: ["ArrowDown"] },
  { id: "hide-tracks", label: "Hide Current Track and Select Next", group: "Tracks", keys: ["Ctrl+H"] },
  { id: "reset-clips", label: "Reset All Clip Indicators", group: "Tracks", keys: [] },

  { id: "copy", label: "Copy Regions", group: "Editing", keys: ["Mod+C"] },
  { id: "cut", label: "Cut Regions", group: "Editing", keys: ["Mod+X"] },
  { id: "paste", label: "Paste Regions", group: "Editing", keys: ["Mod+V"] },
  { id: "mute-regions", label: "Mute Regions", group: "Editing", keys: ["Ctrl+M"] },
  { id: "delete-regions", label: "Delete Regions", group: "Editing", keys: ["Backspace", "Delete"] },
  { id: "loop", label: "Loop Regions", group: "Editing", keys: ["L"] },
  { id: "set-locators", label: "Set Locators by Regions", group: "Editing", keys: ["U"] },
  { id: "zero-crossings", label: "Snap Edits to Zero Crossings", group: "Editing", keys: [] },
  { id: "nudge-left", label: "Nudge Regions Left", group: "Editing", keys: ["Alt+ArrowLeft"] },
  { id: "nudge-right", label: "Nudge Regions Right", group: "Editing", keys: ["Alt+ArrowRight"] },
  { id: "pack-takes", label: "Pack Take Folder", group: "Editing", keys: ["Mod+Ctrl+F"] },
  { id: "tool-menu", label: "Show Tool Menu", group: "Editing", keys: ["T"] },
  // Logic's arranging commands (arrangeCmds.ts). Logic's ⌃⌘ keys for the section edits are
  // ⌃⌥ outside macOS, where ⌃ and ⌘ are the same key.
  { id: "sel-following", label: "Select All Following", group: "Editing", keys: ["Shift+F"] },
  { id: "sel-following-track", label: "Select All Following of Same Track", group: "Editing", keys: [] },
  { id: "sel-inside-locators", label: "Select Inside Locators", group: "Editing", keys: ["Shift+L"] },
  { id: "sel-muted", label: "Select Muted Regions", group: "Editing", keys: ["Shift+M"] },
  { id: "sel-overlapped", label: "Select Overlapped Regions", group: "Editing", keys: [] },
  { id: "sel-same-color", label: "Select Same Colored Regions", group: "Editing", keys: ["Shift+C"] },
  { id: "sel-equal", label: "Select Equal Regions", group: "Editing", keys: ["Shift+E"] },
  { id: "sel-similar", label: "Select Similar Regions", group: "Editing", keys: ["Shift+S"] },
  { id: "sel-invert", label: "Invert Selection", group: "Editing", keys: ["Shift+I"] },
  { id: "section-insert-silence", label: "Insert Silence Between Locators", group: "Editing", keys: [IS_MAC ? "Mod+Ctrl+Z" : "Mod+Alt+Z"] },
  { id: "section-cut", label: "Cut Section Between Locators", group: "Editing", keys: [IS_MAC ? "Mod+Ctrl+X" : "Mod+Alt+X"] },
  { id: "section-repeat", label: "Repeat Section Between Locators", group: "Editing", keys: [IS_MAC ? "Mod+Ctrl+R" : "Mod+Alt+R"] },
  { id: "section-insert-cut", label: "Insert Cut Section at Left Locator", group: "Editing", keys: [IS_MAC ? "Mod+Ctrl+V" : "Mod+Alt+V"] },
  { id: "trim-start-playhead", label: "Trim Region Start to Playhead", group: "Editing", keys: ["Alt+["] },
  { id: "trim-end-playhead", label: "Trim Region End to Playhead", group: "Editing", keys: ["Alt+]"] },
  { id: "trim-end-next", label: "Trim Region End to Next Region", group: "Editing", keys: [] },
  { id: "fill-locators", label: "Fill Within Locators", group: "Editing", keys: [] },
  { id: "remove-overlaps", label: "Remove Overlaps", group: "Editing", keys: [] },
  { id: "same-length", label: "Resize to Same Length", group: "Editing", keys: [] },
  { id: "crop-outside", label: "Crop Regions Outside Locators or Marquee", group: "Editing", keys: [] },
  { id: "remove-gaps", label: "Remove Gaps", group: "Editing", keys: [] },
  { id: "move-to-playhead", label: "Move Region to Playhead", group: "Editing", keys: [";"] },
  { id: "paste-original", label: "Paste at Original Position", group: "Editing", keys: [] },
  { id: "paste-replace", label: "Paste Replace", group: "Editing", keys: ["Mod+Shift+V"] },
  { id: "delete-shuffle", label: "Delete and Shuffle", group: "Editing", keys: [] },
  { id: "join-per-track", label: "Join Regions per Track", group: "Editing", keys: [] },
  { id: "strip-silence", label: "Strip Silence…", group: "Editing", keys: [] },
  { id: "name-regions-by-track", label: "Name Regions by Track", group: "Editing", keys: [] },
  { id: "name-tracks-by-region", label: "Name Tracks by Region", group: "Editing", keys: [] },
  { id: "color-regions-by-track", label: "Color Regions by Track", group: "Editing", keys: ["Alt+Shift+C"] },
  { id: "color-tracks-by-region", label: "Color Tracks by Region", group: "Editing", keys: [] },

  { id: "project-settings", label: "Project Settings…", group: "Windows", keys: ["Alt+P"] },
  { id: "settings-keys", label: "Key Commands…", group: "Windows", keys: ["Alt+K"] },

  { id: "learn-assignment", label: "Learn Assignment for Last Touched Parameter", group: "Control Surfaces", keys: ["Mod+L"] },
  { id: "controller-assignments", label: "Controller Assignments…", group: "Control Surfaces", keys: ["Mod+Shift+L"] },
  // Logic's ⌥⌘K opens the Marker List here, so the keyboard takes ⌥⇧⌘K. The others work while
  // the Step Input Keyboard is open (StepInput.tsx).
  { id: "step-input", label: "Show/Hide Step Input Keyboard", group: "Step Input", keys: ["Mod+Alt+Shift+K"] },
  { id: "step-rest", label: "Rest", group: "Step Input", keys: ["N"] },
  { id: "step-back", label: "Back (Delete Last Step)", group: "Step Input", keys: ["Shift+Backspace"] },
  { id: "step-chord", label: "Chord", group: "Step Input", keys: [] },
  { id: "step-sustain", label: "Sustain Inserted Notes", group: "Step Input", keys: [] },
  { id: "step-quantize", label: "Quantize Step Input", group: "Step Input", keys: [] },
  ...[1, 2, 4, 8, 16, 32, 64, 128].map((v, i) => ({ id: `step-${v}`, label: `Note Value 1/${v}`, group: "Step Input", keys: [String(i + 1)] })),
  { id: "step-dotted", label: "Dotted Note", group: "Step Input", keys: ["9"] },
  { id: "step-triplet", label: "Triplet", group: "Step Input", keys: ["0"] },
];

/** Accelerators of the native menu (src-tauri/src/menu.rs); they can't be changed here. */
export const MENU_SHORTCUTS: { label: string; menu: string; key: string }[] = [
  { label: "Settings…", menu: "Fermata", key: "Mod+," },
  { label: "New…", menu: "File", key: "Mod+N" },
  { label: "Open Recent…", menu: "File", key: "Mod+Shift+O" },
  { label: "Open…", menu: "File", key: "Mod+O" },
  { label: "Save", menu: "File", key: "Mod+S" },
  { label: "Save As…", menu: "File", key: "Mod+Shift+S" },
  { label: "Import Audio File…", menu: "File", key: "Mod+Shift+I" },
  { label: "Bounce Project or Section…", menu: "File", key: "Mod+B" },
  { label: "Export Selected Tracks as Audio Files…", menu: "File", key: "Mod+E" },
  { label: "Export All Tracks as Audio Files…", menu: "File", key: "Mod+Shift+E" },
  // BOUNCE_IN_PLACE_KEY in menu.rs: Logic's ⌃B on macOS; elsewhere Ctrl+B is Bounce Project.
  { label: "Bounce in Place…", menu: "Track", key: IS_MAC ? "Ctrl+B" : "Ctrl+Alt+B" },
  { label: "Undo", menu: "Edit", key: "Mod+Z" },
  { label: "Redo", menu: "Edit", key: "Mod+Shift+Z" },
  { label: "Duplicate", menu: "Edit", key: "Mod+D" },
  { label: "Split Regions at Playhead", menu: "Edit", key: "Mod+T" },
  { label: "Join Regions", menu: "Edit", key: "Mod+J" },
  { label: "Select All", menu: "Edit", key: "Mod+A" },
  { label: "New Tracks…", menu: "Track", key: "Mod+Alt+N" },
  { label: "Create Folder Stack", menu: "Track", key: "Mod+Shift+D" },
  { label: "Group Selected Tracks", menu: "Track", key: "Mod+G" },
  { label: "Delete Track", menu: "Track", key: "Mod+Backspace" },
  { label: "Musical Typing", menu: "View", key: "Mod+K" },
  { label: "Larger Interface", menu: "View", key: "Mod+=" },
  { label: "Smaller Interface", menu: "View", key: "Mod+-" },
  { label: "Actual Size", menu: "View", key: "Mod+0" },
];

/** Puts a key string into canonical form (modifier order, and Ctrl = Mod outside macOS). */
export function normalizeKey(k: string): string {
  const parts = k.split("+");
  // "Mod++" style keys: a trailing empty part is the "+" key itself.
  let key = parts.pop() ?? "";
  if (key === "" && parts[parts.length - 1] === "") {
    parts.pop();
    key = "+";
  }
  const mods = new Set(parts.map((p) => (p === "Ctrl" && !IS_MAC ? "Mod" : p)));
  const out = ["Mod", "Ctrl", "Alt", "Shift"].filter((m) => mods.has(m));
  out.push(key.length === 1 ? key.toUpperCase() : key);
  return out.join("+");
}

const PUNCTUATION: Record<string, string> = { Quote: "'", Comma: ",", Period: ".", Semicolon: ";", Slash: "/", BracketLeft: "[", BracketRight: "]", Backquote: "`" };

/** The key string for a key press, or null for a bare modifier. */
export function eventKey(e: KeyboardEvent): string | null {
  if (["Shift", "Control", "Alt", "Meta", "AltGraph", "CapsLock"].includes(e.key)) return null;
  let key = e.key === " " ? "Space" : e.key;
  // ⌥ on macOS turns letters into symbols; name the physical letter instead.
  if ((e.altKey || e.metaKey || e.ctrlKey) && /^(Key[A-Z]|Digit[0-9])$/.test(e.code)) key = e.code.slice(-1);
  // Likewise the punctuation keys Logic uses with ⌥ (⌥' Create Marker, ⌥, ⌥. between markers).
  else if (e.altKey && !e.shiftKey && PUNCTUATION[e.code]) key = PUNCTUATION[e.code];
  const single = key.length === 1;
  const letter = single && /[a-z]/i.test(key);
  const parts: string[] = [];
  if (IS_MAC ? e.metaKey : e.metaKey || e.ctrlKey) parts.push("Mod");
  if (IS_MAC && e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  // Shift is part of the character for punctuation ("<" is Shift-comma already).
  if (e.shiftKey && (!single || letter)) parts.push("Shift");
  parts.push(single ? key.toUpperCase() : key);
  return parts.join("+");
}

/** Display form of a key, as the platform writes it: "⇧⌘Z" on macOS, "Ctrl+Shift+Z" elsewhere. */
export function formatKey(k: string): string {
  return formatKeyFor(normalizeKey(k), IS_MAC);
}

/**
 * A fixed shortcut (one that isn't in KEY_COMMANDS) for menu hints, tooltips and help text:
 * `fmt("Mod+T")`, or a bare modifier for gestures, `${fmt("Alt")}-drag`. Commands that can be
 * rebound use `keyHint(id)` instead so their hints follow the user's keymap.
 */
export const fmt = formatKey;

/** The keys currently assigned to a command (defaults unless overridden in Settings). */
export function keysFor(id: string, overrides = getSettings().ui.keymap): string[] {
  const o = overrides[id];
  return (o ?? KEY_COMMANDS.find((c) => c.id === id)?.keys ?? []).map(normalizeKey);
}

let cache: { overrides: Record<string, string[]>; map: Map<string, string> } | null = null;

/** Which command a key press runs, if any. */
export function commandForEvent(e: KeyboardEvent): string | null {
  const overrides = getSettings().ui.keymap;
  if (cache?.overrides !== overrides) {
    const map = new Map<string, string>();
    for (const c of KEY_COMMANDS) for (const k of keysFor(c.id, overrides)) if (!map.has(k)) map.set(k, c.id);
    cache = { overrides, map };
  }
  const k = eventKey(e);
  return k ? (cache.map.get(k) ?? null) : null;
}

/** The menu item or command already using `key`, other than `except`. */
export function keyConflict(key: string, except: string, overrides = getSettings().ui.keymap): { label: string; id?: string } | null {
  const k = normalizeKey(key);
  const menu = MENU_SHORTCUTS.find((m) => normalizeKey(m.key) === k);
  if (menu) return { label: `${menu.menu} › ${menu.label}` };
  const cmd = KEY_COMMANDS.find((c) => c.id !== except && keysFor(c.id, overrides).includes(k));
  return cmd ? { label: cmd.label, id: cmd.id } : null;
}

/** The first assigned key of a command, formatted for a tooltip or menu hint. */
export function keyHint(id: string): string {
  const k = keysFor(id)[0];
  return k ? formatKey(k) : "";
}
