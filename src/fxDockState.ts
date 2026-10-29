// The plug-in panels' state in the store: fxDock.ts's transitions applied to `State.fxDock` and
// `State.bottom`, with `fxPanels` and `eqEditor` kept in step for the code that reads them, plus
// the floating windows' remembered sizes.
import type { Id } from "./api";
import * as dock from "./fxDock";
import { eqKey, fxKey, parseKey, type DockState, type PanelKey, type PanelMode, type Size } from "./fxDock";
import { getSettings } from "./prefs";
import { getState, setState, useStore, type State } from "./store";

export { eqKey, fxKey, parseKey, type PanelKey };

const dockOf = (s: State): DockState => ({ ...s.fxDock, bottom: s.bottom });

/** Where new panels open (Settings › General › Open Effect Panels). */
export const panelMode = (): PanelMode => (getSettings().ui.fx_panels === "docked" ? "docked" : "floating");

/** The store change a dock transition makes to `s`, or null if it changes nothing (also used
 *  by panelKinds.tsx to show and hide detached panels). */
export function dockPatch(s: State, f: (s: DockState) => DockState): Partial<State> | null {
  const before = dockOf(s);
  const next = dock.checkBottom(f(before));
  if (JSON.stringify(next) === JSON.stringify(before)) return null;
  const { bottom, ...fxDock } = next;
  const refs = next.open.map(parseKey);
  return {
    fxDock,
    bottom,
    fxPanels: refs.flatMap((r) => (r?.kind === "fx" ? [r.uid] : [])),
    eqEditor: refs.find((r) => r?.kind === "eq")?.track ?? null,
  };
}

function apply(f: (s: DockState) => DockState) {
  const p = dockPatch(getState(), f);
  if (p) setState(p);
}

export const openPanel = (key: PanelKey, mode: PanelMode = panelMode()) => apply((s) => dock.openPanel(s, key, mode));
export const closePanel = (key: PanelKey) => apply((s) => dock.closePanel(s, key));
export const togglePanel = (key: PanelKey) => apply((s) => dock.togglePanel(s, key, panelMode()));
export const floatPanel = (key: PanelKey) => apply((s) => dock.floatPanel(s, key));
export const dockPanel = (key: PanelKey) => apply((s) => dock.dockPanel(s, key));
export const focusPanel = (key: PanelKey) => apply((s) => dock.focusPanel(s, key));
export const selectDocked = (key: PanelKey) => apply((s) => dock.selectDocked(s, key));
export const prunePanels = (exists: (key: PanelKey) => boolean) => apply((s) => dock.prunePanels(s, exists));

/** What Esc would close now. */
export const escTarget = (): PanelKey | null => dock.escTarget(dockOf(getState()));

/** Whether a panel is on screen (lights its slot or EQ thumbnail). */
export function usePanelShown(key: PanelKey | null): boolean {
  return useStore((s) => key !== null && dock.isShown(dockOf(s), key));
}

/** The Channel EQ thumbnail's click. */
export const toggleEqPanel = (track: Id) => togglePanel(eqKey(track));

// ---- remembered floating sizes ----------------------------------------------------------------

const SIZES = "fermata.fxPanelSizes";

/** The size a type of panel ("builtin:compressor", "builtin:channel-eq") was last given. */
export function savedPanelSize(type: string): Size | null {
  try {
    return dock.parseSizes(localStorage.getItem(SIZES))[type] ?? null;
  } catch {
    return null;
  }
}

export function savePanelSize(type: string, size: Size) {
  try {
    const all = dock.parseSizes(localStorage.getItem(SIZES));
    all[type] = { w: Math.round(size.w), h: Math.round(size.h) };
    localStorage.setItem(SIZES, JSON.stringify(all));
  } catch {
    // Not critical: the panel opens at its natural size next time.
  }
}
