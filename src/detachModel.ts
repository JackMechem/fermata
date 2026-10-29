// Detached panel windows, the pure part: panel keys and window labels, the bookkeeping of what
// is detached, which UI state the windows share, and fitting a remembered window to the
// monitors connected now. detach.ts does the wiring; tests/detach.test.ts covers this file.

/** Window labels of detached panels: `panel-<key>` (src-tauri/src/windows.rs). */
export const PANEL_PREFIX = "panel-";

/**
 * A panel key names one detachable panel: its kind ("mixer", "editor"), plus an argument for
 * kinds that have several instances ("eq:12" is the Channel EQ of track 12).
 */
export function panelKey(kind: string, arg: string | number = ""): string {
  return arg === "" ? kind : `${kind}:${arg}`;
}

export function parsePanelKey(key: string): { kind: string; arg: string } {
  const i = key.indexOf(":");
  return i < 0 ? { kind: key, arg: "" } : { kind: key.slice(0, i), arg: key.slice(i + 1) };
}

/** Tauri labels allow letters, digits and - / : _, so a key is its own label suffix. */
export function panelLabel(key: string): string {
  return PANEL_PREFIX + key;
}

/** The panel a window shows, from its label; null for the main window. */
export function keyOfLabel(label: string): string | null {
  return label.startsWith(PANEL_PREFIX) && label.length > PANEL_PREFIX.length ? label.slice(PANEL_PREFIX.length) : null;
}

/** Adds a panel to the detached list (once). */
export function withDetached(list: string[], key: string): string[] {
  return list.includes(key) ? list : [...list, key];
}

export function withoutDetached(list: string[], key: string): string[] {
  return list.includes(key) ? list.filter((k) => k !== key) : list;
}

/** Kinds with at least one detached panel. */
export function detachedKinds(list: string[]): string[] {
  return [...new Set(list.map((k) => parsePanelKey(k).kind))];
}

/**
 * What the bottom section shows once the panel `kind` leaves it (detached, or its toggle used
 * while it is detached): what was there before it, unless that is detached too.
 */
export function bottomAfterHide(kind: string, before: string, detached: string[]): string {
  if (before === kind || before === "none") return "none";
  return detachedKinds(detached).includes(before) ? "none" : before;
}

// ---- shared UI state ------------------------------------------------------------------------

/**
 * The UI-store fields every window keeps the same, so a detached panel behaves as it would in
 * the main window: the selection (the editor follows it, the keys act on it), the region
 * clipboard, the snap mode, what is shown of the tracks, the plug-in lists, the appearance, and
 * which panels are detached. Window layout (the bottom section, pane sizes, open sheets and
 * floating editors, zoom) stays per window. The snapshot and the settings travel separately.
 */
export const SYNCED_FIELDS = [
  "selectedRegions",
  "selectedTracks",
  "focus",
  "clipboard",
  "snapMode",
  "catchPlayhead",
  "showHidden",
  "openFolders",
  "autoLane",
  "plugins",
  "fxList",
  "themeId",
  "uiScale",
  "waveStyle",
  "detached",
] as const;

export type SyncedField = (typeof SYNCED_FIELDS)[number];

/** The shared fields that changed between two states (by identity), or null if none did. */
export function syncPatch<S extends Record<SyncedField, unknown>>(prev: S, next: S): Partial<S> | null {
  const patch: Partial<S> = {};
  let changed = false;
  for (const f of SYNCED_FIELDS) {
    if (prev[f] !== next[f]) {
      patch[f] = next[f];
      changed = true;
    }
  }
  return changed ? patch : null;
}

/** Every shared field, for a window that has just opened. */
export function syncedState<S extends Record<SyncedField, unknown>>(s: S): Partial<S> {
  const out: Partial<S> = {};
  for (const f of SYNCED_FIELDS) out[f] = s[f];
  return out;
}

/** Drops anything but shared fields from a patch another window sent. */
export function cleanPatch(patch: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const f of SYNCED_FIELDS) if (f in patch) out[f] = patch[f];
  return out;
}

// ---- window geometry --------------------------------------------------------------------------

/** A rectangle in physical pixels: a window's outer position and inner size, or a monitor's
 *  work area. */
export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Where detached panels were, kept across launches. */
export interface PanelMemory {
  /** Panels detached when the app last ran, reopened at launch. */
  open: string[];
  /** The last window geometry of each panel kind. */
  geometry: Record<string, Rect>;
}

const isRect = (r: unknown): r is Rect => {
  const o = r as Rect;
  return !!o && [o.x, o.y, o.w, o.h].every((v) => typeof v === "number" && Number.isFinite(v)) && o.w > 0 && o.h > 0;
};

/** Reads saved panel memory, ignoring anything malformed. */
export function parseMemory(raw: string | null): PanelMemory {
  const mem: PanelMemory = { open: [], geometry: {} };
  if (!raw) return mem;
  try {
    const v = JSON.parse(raw) as Partial<PanelMemory>;
    if (Array.isArray(v.open)) mem.open = [...new Set(v.open.filter((k): k is string => typeof k === "string" && k.length > 0))];
    if (v.geometry && typeof v.geometry === "object") {
      for (const [k, r] of Object.entries(v.geometry)) if (isRect(r)) mem.geometry[k] = { x: Math.round(r.x), y: Math.round(r.y), w: Math.round(r.w), h: Math.round(r.h) };
    }
  } catch {
    // A damaged entry just means the panels open docked.
  }
  return mem;
}

function overlap(a: Rect, b: Rect): number {
  const w = Math.min(a.x + a.w, b.x + b.w) - Math.max(a.x, b.x);
  const h = Math.min(a.y + a.h, b.y + b.h) - Math.max(a.y, b.y);
  return w > 0 && h > 0 ? w * h : 0;
}

const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));

/**
 * Fits a remembered window onto the monitors there are now: onto the one it overlaps most, or
 * if it is on none of them (that monitor was unplugged), onto the nearest. It is shrunk to fit
 * and moved fully inside, so its title bar can always be reached. No monitors known: unchanged.
 */
export function fitToMonitors(g: Rect, monitors: Rect[]): Rect {
  const mons = monitors.filter((m) => m.w > 0 && m.h > 0);
  if (!mons.length) return g;
  let best = mons[0];
  let bestArea = -1;
  for (const m of mons) {
    const a = overlap(g, m);
    if (a > bestArea) {
      best = m;
      bestArea = a;
    }
  }
  if (bestArea <= 0) {
    const cx = g.x + g.w / 2;
    const cy = g.y + g.h / 2;
    const dist = (m: Rect) => Math.hypot(clamp(cx, m.x, m.x + m.w) - cx, clamp(cy, m.y, m.y + m.h) - cy);
    best = mons.reduce((a, m) => (dist(m) < dist(a) ? m : a));
  }
  const w = Math.min(g.w, best.w);
  const h = Math.min(g.h, best.h);
  return {
    x: Math.round(clamp(g.x, best.x, best.x + best.w - w)),
    y: Math.round(clamp(g.y, best.y, best.y + best.h - h)),
    w: Math.round(w),
    h: Math.round(h),
  };
}

/** Offsets the n-th window of a kind (n = how many are open already) so they don't stack. */
export function cascade(g: Rect, n: number, step = 28): Rect {
  return n > 0 ? { ...g, x: g.x + n * step, y: g.y + n * step } : g;
}
