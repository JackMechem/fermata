// Displaying key specs the way the host platform writes them. Pure (no app imports), so the
// tests can run it for both platforms.
//
// A key spec is the canonical form of keymap.ts: modifiers then the key, joined with "+".
// "Mod" is ⌘ on macOS and Ctrl elsewhere; "Ctrl" is the Control key on macOS (the same as Mod
// elsewhere). A spec may also be a bare modifier ("Alt") for text like "Alt-drag".
//
// macOS uses Logic's glyphs in its order, ⌃⌥⇧⌘ then the key ("⇧⌘Z"). Linux and Windows spell
// the modifiers out the way their native apps do, Ctrl, Alt, Shift then the key, joined with
// "+" ("Ctrl+Shift+Z").

function detectMac(): boolean {
  if (typeof navigator === "undefined") return false;
  const nav = navigator as Navigator & { userAgentData?: { platform?: string } };
  const platform = nav.userAgentData?.platform || nav.platform || nav.userAgent || "";
  return /Mac|iPhone|iPad/i.test(platform);
}

/** Whether Fermata runs on macOS, detected once. */
export const IS_MAC = detectMac();

const MAC_MODS: Record<string, string> = { Ctrl: "⌃", Alt: "⌥", Shift: "⇧", Mod: "⌘" };
const PC_MODS: Record<string, string> = { Ctrl: "Ctrl", Alt: "Alt", Shift: "Shift", Mod: "Ctrl" };

const MAC_KEYS: Record<string, string> = {
  Enter: "↩",
  Backspace: "⌫",
  Delete: "⌦",
  Escape: "Esc",
  Tab: "⇥",
  Home: "↖",
  End: "↘",
  PageUp: "⇞",
  PageDown: "⇟",
  "-": "−",
};

const PC_KEYS: Record<string, string> = {
  Escape: "Esc",
  PageUp: "Page Up",
  PageDown: "Page Down",
};

const ARROWS: Record<string, string> = { ArrowUp: "↑", ArrowDown: "↓", ArrowLeft: "←", ArrowRight: "→" };

/**
 * Formats a key spec for display. The spec should already be normalized (keymap.ts
 * `normalizeKey`), though any modifier order is accepted; outside macOS "Ctrl" and "Mod" are the
 * same key and show once.
 */
export function formatKeyFor(spec: string, mac: boolean): string {
  const parts = spec.split("+");
  let key = parts.pop() ?? "";
  // "Mod++": a trailing empty part is the "+" key itself.
  if (key === "" && parts[parts.length - 1] === "") {
    parts.pop();
    key = "+";
  }
  const mods = new Set(parts);
  // A bare modifier names the key on its own ("Alt" for "Alt-drag").
  if (!mods.size && key in MAC_MODS) return mac ? MAC_MODS[key] : PC_MODS[key];
  const name = ARROWS[key] ?? (mac ? MAC_KEYS[key] : PC_KEYS[key]) ?? key;
  if (mac) return ["Ctrl", "Alt", "Shift", "Mod"].filter((m) => mods.has(m)).map((m) => MAC_MODS[m]).join("") + name;
  const pc = ["Ctrl", "Alt", "Shift"].filter((m) => mods.has(m) || (m === "Ctrl" && mods.has("Mod")));
  return [...pc.map((m) => PC_MODS[m]), name].join("+");
}
