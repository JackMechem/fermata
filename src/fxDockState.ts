// The plug-in panels' state in the store: fxDock.ts's transitions applied to `State.fxDock` and
// `State.bottom`, with `fxPanels` and `eqEditor` kept in step for the code that reads them, plus
// the floating windows' remembered sizes.
import type { Id } from "./api";
import * as dock from "./fxDock";
import { eqKey, fxKey, parseKey, type DockState, type PanelKey, type PanelMode, type Size } from "./fxDock";
import { closeDetached, detachPanel } from "./detach";
import { getSettings } from "./prefs";
import { getState, setState, useStore, type State } from "./store";

export { eqKey, fxKey, parseKey, type PanelKey };

const dockOf = (s: State): DockState => ({ ...s.fxDock, bottom: s.bottom });

/** Where new panels open in the main window (Settings › General › Open Effect Panels); with
 *  "Separate windows", a panel attached back to the main window floats there. */
export const panelMode = (): PanelMode => (getSettings().ui.fx_panels === "docked" ? "docked" : "floating");

/** Settings › General › Open Effect Panels is "Separate windows": panels open in OS windows (detach.ts). */
const inWindows = () => getSettings().ui.fx_panels === "window";

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

/** Opens a panel (or brings it forward) as Settings say, or with `mode` in the main window. */
export function openPanel(key: PanelKey, mode?: PanelMode) {
  if (!mode && inWindows()) detachPanel(key);
  else apply((s) => dock.openPanel(s, key, mode ?? panelMode()));
}
export const closePanel = (key: PanelKey) => apply((s) => dock.closePanel(s, key));
/** A slot or EQ thumbnail click. With "Separate windows", a panel in its own window closes
 *  (from a panel's window, which doesn't know what is detached, it comes to the front instead);
 *  otherwise a detached panel's window comes to the front (detach.ts). */
export function togglePanel(key: PanelKey) {
  const s = getState();
  if (inWindows() && s.detached.includes(key)) closeDetached(key);
  else if (inWindows() && !s.fxDock.open.includes(key)) detachPanel(key);
  else apply((d) => dock.togglePanel(d, key, panelMode()));
}
export const floatPanel = (key: PanelKey) => apply((s) => dock.floatPanel(s, key));
export const dockPanel = (key: PanelKey) => apply((s) => dock.dockPanel(s, key));
export const focusPanel = (key: PanelKey) => apply((s) => dock.focusPanel(s, key));
export const selectDocked = (key: PanelKey) => apply((s) => dock.selectDocked(s, key));
export const prunePanels = (exists: (key: PanelKey) => boolean) => apply((s) => dock.prunePanels(s, exists));

/** What Esc would close now. */
export const escTarget = (): PanelKey | null => dock.escTarget(dockOf(getState()));

/** Whether a panel is on screen, here or in its own window (lights its slot or EQ thumbnail). */
export function usePanelShown(key: PanelKey | null): boolean {
  return useStore((s) => key !== null && (dock.isShown(dockOf(s), key) || s.detached.includes(key)));
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
