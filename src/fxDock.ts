// Plug-in panels: which built-in effect panels (and the Channel EQ) are open, whether each is
// docked in the main window's bottom section or floating in a window of its own, which docked
// panel the bottom section shows, and the floating windows' sizes. Pure, so it can be unit
// tested; fxDockState.ts applies it to the store and the components draw it.
//
// The bottom section shows one thing at a time, as in Logic: the Mixer, the Editors, or the
// docked panels ("fx"). Docked panels stay open while the Mixer or an Editor takes the section,
// so opening one of them again brings the dock back with every tab.
import type { BottomPane } from "./store";

/** A panel: a built-in effect instance ("fx:<uid>") or a strip's Channel EQ ("eq:<track>"). */
export type PanelKey = string;

export const fxKey = (uid: number): PanelKey => `fx:${uid}`;
export const eqKey = (track: number): PanelKey => `eq:${track}`;

export type PanelRef = { kind: "fx"; uid: number } | { kind: "eq"; track: number };

export function parseKey(key: PanelKey): PanelRef | null {
  const m = /^(fx|eq):(-?\d+)$/.exec(key);
  if (!m) return null;
  const n = Number(m[2]);
  return m[1] === "fx" ? { kind: "fx", uid: n } : { kind: "eq", track: n };
}

/** Where a panel opens: Settings › General › Open Effect Panels. */
export type PanelMode = "docked" | "floating";

/** The panels' part of the store (`State.fxDock`). */
export interface DockPanels {
  /** Open panels in the order they were opened (the dock's tabs keep this order). */
  open: PanelKey[];
  /** Open panels floating in their own window, the frontmost last; the rest are docked. */
  floating: PanelKey[];
  /** The docked panel the bottom section shows. */
  active: PanelKey | null;
  /** What the bottom section showed before the panels took it, to go back to. */
  ret: BottomPane;
}

/** The panels plus the bottom section they share with the Mixer and the Editors. */
export interface DockState extends DockPanels {
  bottom: BottomPane;
}

export const NO_PANELS: DockPanels = { open: [], floating: [], active: null, ret: "none" };

/** The docked panels, in tab order. */
export const dockedPanels = (s: DockPanels): PanelKey[] => s.open.filter((k) => !s.floating.includes(k));

/** Whether a panel is on screen: floating, or the docked one the bottom section shows. */
export const isShown = (s: DockState, key: PanelKey): boolean =>
  s.floating.includes(key) || (s.bottom === "fx" && s.active === key && s.open.includes(key));

/** The bottom section now shows the panels, remembering what it showed before. */
function showDock(s: DockState, key: PanelKey): DockState {
  return { ...s, active: key, bottom: "fx", ret: s.bottom === "fx" ? s.ret : s.bottom };
}

/** `active` has gone: show the most recently opened other docked panel, or give the section back. */
function reactivate(s: DockState): DockState {
  if (s.active !== null && dockedPanels(s).includes(s.active)) return s;
  const left = dockedPanels(s);
  const active = left.length ? left[left.length - 1] : null;
  if (active !== null) return { ...s, active };
  return s.bottom === "fx" ? { ...s, active: null, bottom: s.ret, ret: "none" } : { ...s, active: null };
}

/**
 * Opens a panel (or brings it forward if it is open): a new one opens as `mode` says, an open
 * one where it already is. Only one Channel EQ is open at a time, since the analyzer follows
 * one strip.
 */
export function openPanel(s: DockState, key: PanelKey, mode: PanelMode): DockState {
  let st = s;
  if (parseKey(key)?.kind === "eq") {
    for (const k of st.open) if (k !== key && parseKey(k)?.kind === "eq") st = closePanel(st, k);
  }
  if (st.open.includes(key)) {
    if (st.floating.includes(key)) return focusPanel(st, key);
    return showDock(st, key);
  }
  st = { ...st, open: [...st.open, key] };
  if (mode === "floating") return { ...st, floating: [...st.floating, key] };
  return showDock(st, key);
}

export function closePanel(s: DockState, key: PanelKey): DockState {
  if (!s.open.includes(key)) return s;
  return reactivate({ ...s, open: s.open.filter((k) => k !== key), floating: s.floating.filter((k) => k !== key) });
}

/** A slot click: closes the panel if it is on screen, otherwise opens or shows it. */
export function togglePanel(s: DockState, key: PanelKey, mode: PanelMode): DockState {
  return isShown(s, key) ? closePanel(s, key) : openPanel(s, key, mode);
}

/** Takes a docked panel out into a floating window, in front of the others. */
export function floatPanel(s: DockState, key: PanelKey): DockState {
  if (!s.open.includes(key) || s.floating.includes(key)) return s;
  return reactivate({ ...s, floating: [...s.floating, key] });
}

/** Puts a floating panel back in the bottom section and shows it there. */
export function dockPanel(s: DockState, key: PanelKey): DockState {
  if (!s.open.includes(key)) return s;
  return showDock({ ...s, floating: s.floating.filter((k) => k !== key) }, key);
}

/** Brings a floating panel to the front (a click in it). */
export function focusPanel(s: DockState, key: PanelKey): DockState {
  const i = s.floating.indexOf(key);
  if (i < 0 || i === s.floating.length - 1) return s;
  return { ...s, floating: [...s.floating.filter((k) => k !== key), key] };
}

/** Shows another docked panel's tab. */
export function selectDocked(s: DockState, key: PanelKey): DockState {
  return dockedPanels(s).includes(key) ? showDock(s, key) : s;
}

/**
 * What Esc closes: the frontmost floating panel, otherwise the docked one on screen. Floating
 * windows sit above the main window, so they go first.
 */
export function escTarget(s: DockState): PanelKey | null {
  if (s.floating.length) return s.floating[s.floating.length - 1];
  return s.bottom === "fx" && s.active !== null ? s.active : null;
}

/** Closes every panel whose instance or strip no longer exists. */
export function prunePanels(s: DockState, exists: (key: PanelKey) => boolean): DockState {
  let st = s;
  for (const k of s.open) if (!exists(k)) st = closePanel(st, k);
  return st;
}

/** The section's content after the dock state changed elsewhere: "fx" with nothing docked is "none". */
export function checkBottom(s: DockState): DockState {
  return s.bottom === "fx" && !dockedPanels(s).length ? { ...s, bottom: "none", active: null } : s;
}

// ---- floating window sizes -------------------------------------------------------------------

export interface Size {
  w: number;
  h: number;
}

export interface Rect extends Size {
  x: number;
  y: number;
}

/** A window edge or corner being dragged. */
export type Edge = "e" | "w" | "s" | "se" | "sw";

const clamp = (v: number, lo: number, hi: number) => Math.round(Math.min(Math.max(lo, hi), Math.max(lo, v)));

/** A size within limits; the minimum wins if the two cross (a window larger than the screen). */
export function clampSize(s: Size, min: Size, max: Size): Size {
  return { w: clamp(s.w, min.w, max.w), h: clamp(s.h, min.h, max.h) };
}

/**
 * The window after dragging `edge` by (dx, dy) from `start`. West edges keep the right side in
 * place; the size stays within `min` and `max`.
 */
export function resizeRect(start: Rect, edge: Edge, dx: number, dy: number, min: Size, max: Size): Rect {
  const west = edge === "w" || edge === "sw";
  const east = edge === "e" || edge === "se";
  const south = edge === "s" || edge === "se" || edge === "sw";
  const w = west ? clamp(start.w - dx, min.w, max.w) : east ? clamp(start.w + dx, min.w, max.w) : start.w;
  const h = south ? clamp(start.h + dy, min.h, max.h) : start.h;
  return { x: west ? start.x + start.w - w : start.x, y: start.y, w, h };
}

/** Moves a window so its title bar stays reachable inside the viewport. */
export function keepOnScreen(r: Rect, view: Size): Rect {
  const x = Math.min(Math.max(0, r.x), Math.max(0, view.w - Math.min(r.w, 120)));
  const y = Math.min(Math.max(0, r.y), Math.max(0, view.h - 40));
  return { ...r, x, y };
}

/** Where a new floating window goes across: near the middle (each further one a step along), and
 *  with all of its width in view when it can be. */
export function openingX(viewW: number, w: number | undefined, z: number): number {
  const half = (w ?? 560) / 2;
  const x = viewW / 2 - half + z * 24;
  return Math.max(20, w === undefined ? x : Math.min(x, viewW - w - 8));
}

/** Reads the remembered sizes (by effect id), dropping anything malformed. */
export function parseSizes(text: string | null): Record<string, Size> {
  const out: Record<string, Size> = {};
  if (!text) return out;
  let v: unknown;
  try {
    v = JSON.parse(text);
  } catch {
    return out;
  }
  if (!v || typeof v !== "object" || Array.isArray(v)) return out;
  for (const [k, s] of Object.entries(v as Record<string, unknown>)) {
    const o = s as Partial<Size> | null;
    if (o && typeof o.w === "number" && typeof o.h === "number" && o.w > 0 && o.h > 0 && Number.isFinite(o.w + o.h)) {
      out[k] = { w: Math.round(o.w), h: Math.round(o.h) };
    }
  }
  return out;
}

/**
 * A floating window's width before it is first resized, for panels whose own width is a
 * minimum rather than a natural size (their rows would otherwise stretch the window).
 * Effects not listed size to their content.
 */
export const DEFAULT_WIDTH: Record<string, number> = {
  "builtin:compressor": 628,
  "builtin:limiter": 458,
  "builtin:channel-eq": 820,
  "builtin:linear-phase-eq": 820,
  "builtin:tre": 980,
};
