// Detached panel windows, the runtime: the Mixer, the Editors, the Channel EQ or an effect panel
// shown in a window of its own (for another monitor), labelled `panel-<key>`. Every window runs
// this file; the main window decides what is detached and remembers it, opens and closes the
// windows (src-tauri/src/windows.rs) and runs the commands the panels' keys ask for, and every
// window passes the UI state they share and its edits' snapshots to the others.
//
// Events meant for one window are listened for on that window (`getCurrentWebviewWindow()
// .listen`): a plain `listen()` also hears events sent to other windows. Only the engine's
// `tick` and `project`, which every window wants, use plain `listen`.
//
// The pure part (keys, the shared fields, geometry) is detachModel.ts. Panel kinds register
// from panelKinds.tsx, so this file imports no components.
import type { ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit, emitTo, listen } from "@tauri-apps/api/event";
import { availableMonitors, getCurrentWindow } from "@tauri-apps/api/window";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { call, type Snapshot, type Tick } from "./api";
import type { MenuItem } from "./components/Menu";
import {
  bottomAfterHide,
  cascade,
  cleanPatch,
  fitToMonitors,
  keyOfLabel,
  panelLabel,
  parseMemory,
  parsePanelKey,
  syncedState,
  syncPatch,
  withDetached,
  withoutDetached,
  type PanelMemory,
  type Rect,
} from "./detachModel";
import { dockedPanels } from "./fxDock";
import { IS_MAC } from "./keyFormat";
import { commandForEvent } from "./keymap";
import { getSettings, onSettingsChange, receiveSettings, type AppSettings } from "./prefs";
import { getState, pushTick, setState, subscribeState, useStore, type BottomPane, type State } from "./store";
import { applyTheme } from "./themes";
import { applyUiScale } from "./uiScale";

const LABEL = getCurrentWindow().label;

/** The panel this window shows, or null in the main window. */
export const ownPanel: string | null = keyOfLabel(LABEL);
export const isMain = ownPanel === null;

/**
 * Window positions and sizes are kept in physical pixels, except on macOS: there the screens
 * share one coordinate space in points, while a physical position is converted with the scale
 * of whichever screen the window is on now, so a Retina screen next to a standard one would put
 * a window on the wrong screen.
 */
const LOGICAL = IS_MAC;

// ---- panel kinds --------------------------------------------------------------------------------

/** One kind of detachable panel; `arg` is the part of the key after the kind ("12" in "eq:12"). */
export interface PanelKind {
  /** The window title. */
  title(arg: string, s: State): string;
  /** What the panel's window shows. */
  render(arg: string): ReactNode;
  /** Whether this window's state shows the panel (in the bottom section, a floating panel…). */
  shown(arg: string, s: State): boolean;
  /** The state change that takes the panel out of this window. */
  hide(arg: string, s: State): Partial<State>;
  /** The state change that shows it in this window (attached again, or in its own window). */
  show(arg: string, s: State): Partial<State>;
  /** False once what the panel shows is gone (its track, its effect): the main window closes it. */
  alive?(arg: string, s: State): boolean;
  /** In the panel's own window: a change that keeps its state fit to show it, or null. */
  own?(arg: string, s: State): Partial<State> | null;
  /** Reopened at launch if it was detached when Fermata quit. */
  restore?: boolean;
  /** A first window's size, in logical pixels. */
  size: { w: number; h: number };
}

const kinds = new Map<string, PanelKind>();
/** Kinds that are bottom-section panes ("mixer", "editor"). */
const bottomKinds = new Set<string>();

export function registerPanelKind(kind: string, k: PanelKind) {
  kinds.set(kind, k);
}

export function panelKind(kind: string): PanelKind | undefined {
  return kinds.get(kind);
}

/**
 * A bottom-section pane as a panel kind: shown while the section shows it, and when it leaves,
 * the section goes back to what it showed before. Its own window keeps `bottom` on it.
 */
export function bottomPanel(pane: BottomPane, k: Pick<PanelKind, "title" | "render" | "size"> & Pick<PanelKind, "own">): PanelKind {
  bottomKinds.add(pane);
  return {
    ...k,
    restore: true,
    shown: (_, s) => s.bottom === pane,
    hide: (_, s) => {
      const detached = s.detached.filter((d) => bottomKinds.has(parsePanelKey(d).kind));
      let bottom = bottomAfterHide(pane, bottomBefore, detached) as BottomPane;
      // The docked effect panels' pane is only worth going back to while it has panels.
      if (bottom === "fx" && !dockedPanels(s.fxDock).length) bottom = "none";
      // Nor should closing the last docked panel bring this one back (fxDock.ts `ret`).
      return s.fxDock.ret === pane ? { bottom, fxDock: { ...s.fxDock, ret: "none" } } : { bottom };
    },
    show: () => ({ bottom: pane }),
    own: k.own ?? ((_, s) => (s.bottom === pane ? null : { bottom: pane })),
  };
}

const kindOf = (key: string) => {
  const { kind, arg } = parsePanelKey(key);
  return { k: kinds.get(kind), kind, arg };
};

// ---- shared state ---------------------------------------------------------------------------

/** Above zero while applying another window's change, which must not be sent back. */
let remote = 0;
let prev: State = getState();
/** What the bottom section showed before its latest change (where a leaving pane goes back to). */
let bottomBefore: BottomPane = getState().bottom;
let lastBottom: BottomPane = getState().bottom;
/** The panel window shows its panel; from now on, its panel going away closes the window. */
let booted = false;
let broadcastTimer = 0;
let lastBroadcast = 0;
let keyAction: (id: string) => void = () => {};

/** Whether there are other windows to tell. */
const hasPeers = (s: State) => !isMain || s.detached.length > 0;

function remotely(f: () => void) {
  remote++;
  try {
    f();
  } finally {
    remote--;
  }
}

/** Other windows get the session's snapshot after this one's edit, at most every 30 ms. */
function broadcastSoon() {
  if (broadcastTimer) return;
  broadcastTimer = window.setTimeout(
    () => {
      broadcastTimer = 0;
      lastBroadcast = performance.now();
      invoke("broadcast_snapshot").catch(() => {});
    },
    Math.max(0, lastBroadcast + 30 - performance.now()),
  );
}

function onStateChange() {
  const s = getState();
  const was = prev;
  prev = s;
  if (s.bottom !== lastBottom) {
    bottomBefore = lastBottom;
    lastBottom = s.bottom;
  }
  if (remote === 0 && hasPeers(s)) {
    const patch = syncPatch(was, s);
    // Only the main window decides what is detached.
    if (patch && !isMain) delete patch.detached;
    if (patch && Object.keys(patch).length) emit("ui-sync", { from: LABEL, patch }).catch(() => {});
    if (s.snap !== was.snap) broadcastSoon();
  }
  // A sheet opened from a panel window (its menu bar, a key command) opens here: show it.
  if (isMain && s.dialog && s.dialog !== was.dialog && s.detached.length) getCurrentWindow().setFocus().catch(() => {});
  // A track or effect went away: so does its window.
  if (isMain && s.snap && s.snap !== was.snap) {
    for (const key of s.detached) {
      const { k, arg } = kindOf(key);
      if (k?.alive && !k.alive(arg, s)) return closeDetached(key);
    }
  }
  if (!isMain && booted) {
    const { k, arg } = kindOf(ownPanel!);
    const fix = k?.own?.(arg, s);
    if (fix) return setState(fix);
  }
  // A detached panel is shown only in its own window: shown here (its key, its slot), it goes
  // back out and its window comes to the front. Each setState re-runs this with fresh state.
  for (const key of s.detached) {
    if (key === ownPanel) continue;
    const { k, arg } = kindOf(key);
    if (k?.shown(arg, s)) {
      setState(k.hide(arg, s));
      invoke("focus_panel_window", { label: panelLabel(key) }).catch(() => {});
      return;
    }
  }
  // The panel was closed in its own window (its ×, Esc): so is the window.
  if (!isMain && booted) {
    const { k, arg } = kindOf(ownPanel!);
    if (k && !k.shown(arg, getState())) {
      booted = false;
      send({ type: "close", key: ownPanel! });
    }
  }
}

/** A snapshot from the engine or another window: stored, not passed on. */
export function receiveSnapshot(snap: Snapshot) {
  remotely(() => setState({ snap }));
}

interface SyncMsg {
  from: string;
  patch: Record<string, unknown>;
}

function applySync({ from, patch }: SyncMsg) {
  if (from === LABEL) return;
  const p = cleanPatch(patch);
  // Only the main window decides what is detached.
  if (isMain) delete p.detached;
  remotely(() => {
    if (typeof p.themeId === "string" && p.themeId !== getState().themeId) applyTheme(p.themeId, isMain);
    if (typeof p.uiScale === "number" && p.uiScale !== getState().uiScale) void applyUiScale(p.uiScale, isMain);
    delete p.themeId;
    delete p.uiScale;
    if (Object.keys(p).length) setState(p as Partial<State>);
  });
}

function onSettings() {
  if (remote === 0 && hasPeers(getState())) emit("ui-settings", { from: LABEL, settings: getSettings() }).catch(() => {});
}

function applySettings({ from, settings }: { from: string; settings: AppSettings }) {
  if (from !== LABEL) remotely(() => receiveSettings(settings));
}

// ---- messages to the main window ------------------------------------------------------------

type PanelMsg =
  | { type: "hello" }
  | { type: "attach" | "detach" | "close"; key: string }
  | { type: "key"; id: string }
  | { type: "geometry"; key: string; rect: Rect };

function send(m: PanelMsg): Promise<void> {
  return emitTo("main", "panel-msg", { from: LABEL, ...m }).catch(() => {});
}

function onPanelMsg(m: PanelMsg & { from: string }) {
  switch (m.type) {
    case "hello":
      emitTo(m.from, "ui-sync", { from: LABEL, patch: syncedState(getState()) }).catch(() => {});
      emitTo(m.from, "ui-settings", { from: LABEL, settings: getSettings() }).catch(() => {});
      break;
    case "attach":
      attachPanel(m.key);
      break;
    case "detach":
      detachPanel(m.key);
      break;
    case "close":
      closeDetached(m.key);
      break;
    case "key":
      keyAction(m.id);
      break;
    case "geometry":
      memory.geometry[parsePanelKey(m.key).kind] = m.rect;
      saveMemory();
      break;
  }
}

// ---- detaching and attaching ----------------------------------------------------------------

const MEMORY = "fermata.detachedPanels";

function loadMemory(): PanelMemory {
  try {
    return parseMemory(localStorage.getItem(MEMORY));
  } catch {
    return parseMemory(null);
  }
}

/** Only the main window writes it; panels send it their geometry. */
let memory = loadMemory();

function saveMemory() {
  if (!isMain) return;
  memory.open = getState().detached.filter((k) => kindOf(k).k?.restore);
  try {
    localStorage.setItem(MEMORY, JSON.stringify(memory));
  } catch {
    // Not critical: the panels open in the main window next time.
  }
}

/** The monitors' work areas, in the units geometry is kept in. */
async function monitorRects(): Promise<Rect[]> {
  try {
    return (await availableMonitors()).map((m) => {
      const f = LOGICAL ? m.scaleFactor : 1;
      const a = m.workArea;
      return { x: a.position.x / f, y: a.position.y / f, w: a.size.width / f, h: a.size.height / f };
    });
  } catch {
    return [];
  }
}

async function openWindow(key: string) {
  const { k, kind, arg } = kindOf(key);
  if (!k) return;
  const saved = memory.geometry[kind];
  let geometry: (Rect & { logical: boolean }) | null = null;
  if (saved) {
    // Another EQ or effect window of the same kind opens a little lower and to the right.
    const others = getState().detached.filter((d) => d !== key && parsePanelKey(d).kind === kind).length;
    geometry = { ...fitToMonitors(cascade(saved, others), await monitorRects()), logical: LOGICAL };
  }
  try {
    await invoke("open_panel_window", { label: panelLabel(key), title: k.title(arg, getState()), width: k.size.w, height: k.size.h, geometry });
  } catch (e) {
    const s = getState();
    setState({ detached: withoutDetached(s.detached, key), ...k.show(arg, s), toast: { text: `Couldn't open a window: ${e}`, at: Date.now() } });
    saveMemory();
  }
}

/** Shows a panel in its own window (or brings its window to the front). */
export function detachPanel(key: string) {
  const { k, arg } = kindOf(key);
  if (!k) return;
  const s = getState();
  if (!isMain) {
    if (k.shown(arg, s)) setState(k.hide(arg, s));
    send({ type: "detach", key });
    return;
  }
  if (s.detached.includes(key)) {
    invoke("focus_panel_window", { label: panelLabel(key) }).catch(() => {});
    return;
  }
  setState({ ...(k.shown(arg, s) ? k.hide(arg, s) : {}), detached: withDetached(s.detached, key) });
  saveMemory();
  void openWindow(key);
}

/** Puts a detached panel back in the main window and closes its window. */
export function attachPanel(key: string) {
  // From the panel's window, which is about to close: it doesn't hide the panel first, so the
  // main window's copy starts its meters after this one last stopped them.
  if (!isMain) {
    send({ type: "attach", key });
    return;
  }
  const s = getState();
  if (!s.detached.includes(key)) return;
  const { k, arg } = kindOf(key);
  const rest = { ...s, detached: withoutDetached(s.detached, key) };
  setState({ detached: rest.detached, ...(k && !k.shown(arg, rest) ? k.show(arg, rest) : {}) });
  invoke("close_panel_window", { label: panelLabel(key) }).catch(() => {});
  saveMemory();
}

/** Closes a detached panel's window without showing the panel in the main window. */
export function closeDetached(key: string) {
  if (!isMain) {
    send({ type: "close", key });
    return;
  }
  const s = getState();
  if (!s.detached.includes(key)) return;
  setState({ detached: withoutDetached(s.detached, key) });
  invoke("close_panel_window", { label: panelLabel(key) }).catch(() => {});
  saveMemory();
}

/** View › "Mixer in Separate Window" and the like. */
export function toggleDetached(key: string) {
  if (getState().detached.includes(key)) attachPanel(key);
  else detachPanel(key);
}

export function useDetached(key: string): boolean {
  return useStore((s) => s.detached.includes(key));
}

/** The end of a panel's View menu: open it in its own window, or attach it from there. */
export function detachMenuItems(key: string): MenuItem[] {
  return [
    { separator: true },
    ownPanel === key ? { label: "Attach to Main Window", onClick: () => attachPanel(key) } : { label: "Open in Separate Window", onClick: () => detachPanel(key) },
  ];
}

// ---- installing ---------------------------------------------------------------------------------

let restored = false;

/**
 * The main window's part (App, once): it hears the panels, and reopens the panels that were
 * detached when Fermata quit. `key` runs a key command a panel window passed on.
 */
export function installDetach(opts: { key: (id: string) => void }): () => void {
  keyAction = opts.key;
  prev = getState();
  const offs = [subscribeState(onStateChange), onSettingsChange(onSettings)];
  const wv = getCurrentWebviewWindow();
  const pending = [
    wv.listen<PanelMsg & { from: string }>("panel-msg", (e) => onPanelMsg(e.payload)),
    wv.listen<SyncMsg>("ui-sync", (e) => applySync(e.payload)),
    wv.listen<{ from: string; settings: AppSettings }>("ui-settings", (e) => applySettings(e.payload)),
    wv.listen<Snapshot>("project-sync", (e) => receiveSnapshot(e.payload)),
  ];
  let live = true;
  // Once the panels can be heard: their first message is "hello".
  Promise.all(pending).then(() => {
    if (!live) return;
    if (!restored) {
      restored = true;
      for (const key of memory.open) if (kindOf(key).k?.restore) detachPanel(key);
    }
    // Panels that outlived a reload of this window catch up.
    const s = getState();
    if (s.detached.length) emit("ui-sync", { from: LABEL, patch: syncedState(s) }).catch(() => {});
  });
  return () => {
    live = false;
    offs.forEach((off) => off());
    pending.forEach((p) => p.then((off) => off()));
  };
}

/** Sends where a panel window is to the main window, which remembers it per kind. */
async function reportGeometry() {
  const w = getCurrentWindow();
  try {
    // A minimized, zoomed or full-screen window isn't where it should open next time.
    if ((await w.isMinimized()) || (await w.isMaximized()) || (await w.isFullscreen())) return;
    const [pos, size, f] = await Promise.all([w.outerPosition(), w.innerSize(), LOGICAL ? w.scaleFactor() : Promise.resolve(1)]);
    const rect = { x: Math.round(pos.x / f), y: Math.round(pos.y / f), w: Math.round(size.width / f), h: Math.round(size.height / f) };
    send({ type: "geometry", key: ownPanel!, rect });
  } catch {
    // The window is closing.
  }
}

/**
 * A panel window's part (DetachedWindow, once). `menu` handles a native menu item that belongs
 * to the panel (Select All in a Piano Roll) and returns false for the rest, which go to the
 * main window.
 */
export function installPanelWindow(opts: { menu: (id: string) => boolean }): () => void {
  const key = ownPanel!;
  const { k, arg } = kindOf(key);
  prev = getState();
  const offs = [subscribeState(onStateChange), onSettingsChange(onSettings)];
  const wv = getCurrentWebviewWindow();
  const win = getCurrentWindow();
  let geomTimer = 0;
  const geometrySoon = () => {
    window.clearTimeout(geomTimer);
    geomTimer = window.setTimeout(reportGeometry, 400);
  };
  const pending = [
    // The main window already shows the engine's warnings.
    listen<Tick>("tick", (e) => pushTick({ ...e.payload, warning: null })),
    listen<Snapshot>("project", (e) => receiveSnapshot(e.payload)),
    wv.listen<Snapshot>("project-sync", (e) => receiveSnapshot(e.payload)),
    wv.listen<SyncMsg>("ui-sync", (e) => applySync(e.payload)),
    wv.listen<{ from: string; settings: AppSettings }>("ui-settings", (e) => applySettings(e.payload)),
    wv.listen<string>("menu", (e) => {
      if (!opts.menu(e.payload)) emitTo("main", "menu", e.payload).catch(() => {});
    }),
    // The window's close button attaches the panel again (quitting closes it without asking).
    win.onCloseRequested(async () => {
      booted = false;
      await send({ type: "attach", key });
    }),
    win.onMoved(geometrySoon),
    win.onResized(geometrySoon),
  ];
  // Key commands run in the main window, as if pressed there; the panel's own keys (the Piano
  // Roll's) are handled first, in the capture phase, and prevent the default.
  const onKey = (e: KeyboardEvent) => {
    if (e.defaultPrevented || getState().dialog) return;
    const t = e.target as HTMLElement | null;
    if (t && (t.tagName === "INPUT" || t.tagName === "SELECT" || t.tagName === "TEXTAREA" || t.isContentEditable)) return;
    const id = commandForEvent(e);
    if (!id) return;
    e.preventDefault();
    send({ type: "key", id });
  };
  window.addEventListener("keydown", onKey);
  if (k) remotely(() => setState(k.show(arg, getState())));
  booted = !!k;
  let live = true;
  Promise.all(pending).then(() => {
    if (!live) return;
    send({ type: "hello" });
    // `call`, not `act`: a snapshot from `act` would be sent on to the other windows.
    call<Snapshot>("get_state").then((snap) => snap && live && receiveSnapshot(snap));
  });
  return () => {
    live = false;
    booted = false;
    window.clearTimeout(geomTimer);
    window.removeEventListener("keydown", onKey);
    offs.forEach((off) => off());
    pending.forEach((p) => p.then((off) => off()));
  };
}
