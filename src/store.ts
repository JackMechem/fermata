import { useSyncExternalStore } from "react";
import type { AutoParam, Id, PluginDesc, Region, Snapshot, Tick } from "./api";
import type { FxInfo } from "./fx";
import type { DropTarget } from "./dnd";
import type { WaveStyle } from "./waveform";

export type Snap = "smart" | "bar" | "beat" | "off";
export type BottomPane = "none" | "mixer" | "editor";
export type SettingsTab = "general" | "audio" | "recording" | "metronome" | "midi" | "editing" | "plugins" | "keys" | "appearance";

/** Resizable pane sizes in CSS pixels (widths, except the bottom panes' heights). */
export interface PaneSizes {
  library: number;
  inspector: number;
  browser: number;
  mixer: number;
  editor: number;
  /** Width of the track header column. */
  headers: number;
}

export interface State {
  snap: Snapshot | null;
  plugins: PluginDesc[];
  showLibrary: boolean;
  showInspector: boolean;
  showBrowser: boolean;
  bottom: BottomPane;
  /** Horizontal zoom in pixels per beat. */
  pxPerBeat: number;
  /** Default track height; tracks resized individually live in trackHeights. */
  trackHeight: number;
  trackHeights: Record<Id, number>;
  snapMode: Snap;
  catchPlayhead: boolean;
  selectedRegions: Id[];
  dialog: null | "new-tracks" | "settings" | "chooser" | "bounce" | "bounce-in-place" | "project-settings" | "recovery";
  chooserTab: "new" | "recent";
  /** Where dragged audio files would land, while a drag is over the tracks. */
  dropTarget: DropTarget | null;
  showTyping: boolean;
  /** Selected tracks; the snapshot's `selected` is the one the inspector and MIDI follow. */
  selectedTracks: Id[];
  /** Where Duplicate, Delete and Copy act: the track list or the regions. */
  focus: "tracks" | "regions";
  /** Copied regions, each with the index of the track it came from. */
  clipboard: { index: number; region: Region }[];
  /** Group whose settings sheet is open. */
  groupSheet: Id | null;
  paneSizes: PaneSizes;
  /** Strip whose Channel EQ editor is open (MASTER_ID = Stereo Out). */
  eqEditor: Id | null;
  settingsTab: SettingsTab;
  themeId: string;
  /** Interface zoom (1 = 100%). */
  uiScale: number;
  waveStyle: WaveStyle;
  toast: { text: string; at: number; ms?: number } | null;
  /** Logic's hide mode (H): hidden tracks are shown, dimmed, and every header gets an H button. */
  showHidden: boolean;
  /** Track automation shown on the lanes (A). */
  showAutomation: boolean;
  /** The automation lane each track shows (Volume when unset). */
  autoLane: Record<Id, AutoParam>;
  /** Take folders shown open, with a row per take below their track. */
  openFolders: Id[];
  /** The Marker List sheet. */
  showMarkerList: boolean;
  /** The Tempo and Signature global tracks above the tracks (G). */
  showGlobalTracks: boolean;
  /** Height of the Tempo lane. */
  tempoLaneH: number;
  /** Tempo edits move MIDI regions to keep their bars and beats (the Tempo lane's menu). */
  moveMidiWithTempo: boolean;
  /** The built-in effects (`list_builtin_effects`). */
  fxList: FxInfo[];
  /** Built-in effect instances whose panel windows are open, the frontmost last. */
  fxPanels: Id[];
  /** The overview strip under the tracks, in place of the horizontal scroll bar (View menu). */
  showOverview: boolean;
}

let state: State = {
  snap: null,
  plugins: [],
  showLibrary: false,
  showInspector: true,
  showBrowser: false,
  bottom: "none",
  pxPerBeat: 22,
  trackHeight: 64,
  trackHeights: {},
  snapMode: "smart",
  catchPlayhead: true,
  selectedRegions: [],
  dialog: null,
  chooserTab: "new",
  eqEditor: null,
  dropTarget: null,
  showTyping: false,
  selectedTracks: [],
  focus: "regions",
  clipboard: [],
  groupSheet: null,
  paneSizes: { library: 230, inspector: 222, browser: 230, mixer: 400, editor: 300, headers: 296 },
  settingsTab: "audio",
  themeId: "studio",
  uiScale: 1,
  waveStyle: "smooth",
  toast: null,
  showHidden: false,
  showAutomation: false,
  autoLane: {},
  openFolders: [],
  showMarkerList: false,
  showGlobalTracks: false,
  tempoLaneH: 56,
  moveMidiWithTempo: false,
  fxList: [],
  fxPanels: [],
  showOverview: true,
};

const listeners = new Set<() => void>();

export function getState(): State {
  return state;
}

export function setState(patch: Partial<State> | ((s: State) => Partial<State>)) {
  const p = typeof patch === "function" ? patch(state) : patch;
  state = { ...state, ...p };
  listeners.forEach((l) => l());
}

function subscribe(l: () => void) {
  listeners.add(l);
  return () => listeners.delete(l);
}

export function useStore<T>(sel: (s: State) => T): T {
  return useSyncExternalStore(subscribe, () => sel(state));
}

// ---- 30 Hz engine ticks are kept outside React state; components subscribe directly. -----

let lastTick: Tick | null = null;
let lastArrival = 0;
let seq = 0;
let meterMap = new Map<Id, [number, number]>();
const tickListeners = new Set<(t: Tick) => void>();

export function pushTick(t: Tick) {
  if (t.warning) setState({ toast: { text: t.warning, at: Date.now(), ms: 8000 } });
  lastTick = t;
  lastArrival = performance.now();
  seq++;
  const m = new Map<Id, [number, number]>();
  for (const [id, l, r] of t.meters) m.set(id, [l, r]);
  m.set(-1, t.master);
  t.inputs.forEach((v, i) => m.set(-100 - i, [v, v]));
  meterMap = m;
  tickListeners.forEach((l) => l(t));
}

export function onTick(l: (t: Tick) => void): () => void {
  tickListeners.add(l);
  if (lastTick) l(lastTick);
  return () => tickListeners.delete(l);
}

export function currentTick(): Tick | null {
  return lastTick;
}

/** performance.now() when the latest tick arrived. */
export function tickArrival(): number {
  return lastArrival;
}

/** Increments with every tick, so frame-driven views can tell whether new data arrived. */
export function tickSeq(): number {
  return seq;
}

/** Latest peak levels for a meter key: track id, -1 for Stereo Out, -100-n for input n. */
export function meterLevels(key: Id): [number, number] {
  return meterMap.get(key) ?? [0, 0];
}

/** Live recording previews, keyed by track. */
export const liveTakes = new Map<Id, { start: number; frames: number; peaks: number[] }>();
