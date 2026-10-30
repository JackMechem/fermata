// The Audio Track editor's state and operations: the time-range selection, its zoom, transient
// markers, and the actions on the selection shared by its local menus, right-click menu, the
// native menu bar and the keyboard. The canvas itself is components/AudioEditor.tsx.
import { useSyncExternalStore } from "react";
import { act, actMade, call, type Id, type Region, type Track } from "./api";
import type { MenuItem } from "./components/Menu";
import { fmt } from "./keymap";
import { framePos, onFrame } from "./frame";
import { undoRedo } from "./files";
import * as ops from "./ops";
import { currentTick, getState, setState } from "./store";
import { regionsIn, subRegion } from "./audioRange";
import { detachMenuItems } from "./detach";
import { zeroCrossingsItem } from "./zeroCrossings";
import { sheetOpen } from "./sheetKeys";

export { regionsIn };

/** A time range on a track, in timeline frames (start < end). */
export interface RangeSel {
  track: Id;
  start: number;
  end: number;
}

export type RangeAction = "split" | "trim" | "silence" | "delete" | "normalize" | "reverse" | "fade_in" | "fade_out";

export interface EdState {
  sel: RangeSel | null;
  /** Horizontal zoom in pixels per frame; 0 until the editor first lays out. */
  pps: number;
  /** Waveform (amplitude) zoom: 1 shows full scale at the full lane height. */
  amp: number;
  showTransients: boolean;
  /** Transient detection sensitivity, 0..1. */
  sensitivity: number;
}

export const MIN_AMP = 1;
export const MAX_AMP = 32;

let st: EdState = { sel: null, pps: 0, amp: 1, showTransients: false, sensitivity: 0.5 };
const subs = new Set<() => void>();

export const edState = () => st;

export function setEd(p: Partial<EdState>) {
  st = { ...st, ...p };
  subs.forEach((f) => f());
}

export function useEd<T>(sel: (s: EdState) => T): T {
  return useSyncExternalStore(
    (l) => {
      subs.add(l);
      return () => subs.delete(l);
    },
    () => sel(st),
  );
}

/** Set by the editor component: which track it shows, and its zoom and scroll controls. */
export interface EditorView {
  track: Id;
  /** The region info and gain act on: the selected one on the track, if any. */
  region: Id | null;
  zoomTo: (start: number, end: number) => void;
  zoomBy: (factor: number) => void;
  /** Current scroll and zoom, to come back to after Z. */
  saveView: () => { pps: number; left: number };
  restoreView: (v: { pps: number; left: number }) => void;
}

let view: EditorView | null = null;
/** True from a click in the editor until a click elsewhere, so its keys take precedence. */
let focused = false;

export function attachView(v: EditorView | null) {
  view = v;
  if (!v) focused = false;
}

export function setEditorFocus(on: boolean) {
  focused = on;
}

const project = () => getState().snap?.project;
const toast = (text: string) => setState({ toast: { text, at: Date.now(), ms: 2500 } });

function edTrack(): Track | undefined {
  return view ? project()?.tracks.find((t) => t.id === view!.track) : undefined;
}

/** The selection, if it's on the track the editor shows. */
export function rangeSel(): RangeSel | null {
  const s = st.sel;
  return s && view && s.track === view.track ? s : null;
}

export function clearSel() {
  if (st.sel) setEd({ sel: null });
}

function focusRegion(): Region | undefined {
  const t = edTrack();
  return t?.regions.find((r) => r.id === view?.region);
}

// ---- actions on the selection ---------------------------------------------------------------

/** Runs a range action (see `edit_range` in the engine) on the regions under the selection,
 *  then selects the regions that now hold the range. */
export async function rangeEdit(action: RangeAction) {
  const sel = rangeSel();
  const t = edTrack();
  if (!sel || !t) return;
  const ids = regionsIn(t, sel.start, sel.end).map((r) => r.id);
  if (!ids.length) {
    toast("The selection doesn't cover a region");
    return;
  }
  const made = await actMade("edit_range", { ids, start: Math.round(sel.start), end: Math.round(sel.end), action });
  if (made) setState({ selectedRegions: made, focus: "regions" });
}

/** Puts the selected part of each region under the selection on the clipboard. */
export function copySelection(): boolean {
  const sel = rangeSel();
  const t = edTrack();
  const p = project();
  if (!sel || !t || !p) return false;
  const index = p.tracks.indexOf(t);
  const clip = regionsIn(t, sel.start, sel.end).map((r) => ({ index, region: subRegion(r, sel.start, sel.end) }));
  if (!clip.length) return false;
  setState({ clipboard: clip, clipboardStart: null });
  return true;
}

export function cutSelection() {
  if (copySelection()) rangeEdit("delete");
}

// Play Selection stops at the selection's end (unless it loops with Cycle).
let stopAt: { start: number; end: number; since: number } | null = null;
let offWatch: (() => void) | null = null;

function watchStop() {
  if (offWatch) return;
  offWatch = onFrame((now) => {
    const t = currentTick();
    const s = stopAt;
    const done = () => {
      stopAt = null;
      offWatch?.();
      offWatch = null;
    };
    if (!s) return done();
    if (!t?.playing) {
      // Give the engine a moment to start before deciding playback ended.
      if (now - s.since > 500) done();
      return;
    }
    const pos = framePos(now);
    // Located elsewhere: well before the selection (a tenth of a second, at any sample rate).
    if (pos < s.start - (project()?.sample_rate ?? 48000) / 10) return done();
    if (pos >= s.end) {
      done();
      act("stop").then(() => call("locate", { pos: s.start }));
    }
  });
}

/** Plays the selection from its start. With Cycle on, the cycle is set to the selection so it
 *  loops; otherwise playback stops at the selection's end and returns to its start. */
export async function playSelection() {
  const sel = rangeSel();
  const p = project();
  if (!sel || !p) return;
  const start = Math.round(sel.start);
  const end = Math.round(sel.end);
  if (p.cycle.enabled) {
    await act("set_cycle", { enabled: true, start, end });
    stopAt = null;
  } else {
    stopAt = { start, end, since: performance.now() };
    watchStop();
  }
  await call("locate", { pos: start });
  if (!currentTick()?.playing) await act("play");
}

export function setLocatorsBySelection() {
  const sel = rangeSel();
  if (sel) act("set_cycle", { enabled: true, start: Math.round(sel.start), end: Math.round(sel.end) });
}

// ---- transients ---------------------------------------------------------------------------

const transientCache = new Map<string, number[]>();
const transientPending = new Map<string, Promise<number[]>>();
const key = (clip: Id, sens: number) => `${clip}:${sens.toFixed(2)}`;

/** Onsets (clip frames) at the current sensitivity, fetching them if needed; `done` runs when
 *  they arrive. */
export function transientsFor(clip: Id, done?: () => void): number[] | null {
  const k = key(clip, st.sensitivity);
  const have = transientCache.get(k);
  if (have) return have;
  fetchTransients(clip).then(() => done?.());
  return null;
}

function fetchTransients(clip: Id): Promise<number[]> {
  const sens = st.sensitivity;
  const k = key(clip, sens);
  const have = transientCache.get(k);
  if (have) return Promise.resolve(have);
  let p = transientPending.get(k);
  if (!p) {
    p = call<number[]>("clip_transients", { clip, sensitivity: sens }).then((v) => {
      const list = v ?? [];
      transientCache.set(k, list);
      transientPending.delete(k);
      return list;
    });
    transientPending.set(k, p);
  }
  return p;
}

/** Splits at the transients inside the selection, or (without one) inside the selected
 *  regions on the track, as one undo step. */
export async function splitAtTransients() {
  const t = edTrack();
  if (!t) return;
  const sel = rangeSel();
  const selected = getState().selectedRegions;
  const regs = sel ? regionsIn(t, sel.start, sel.end) : t.regions.filter((r) => selected.includes(r.id));
  const audio = regs.filter((r) => r.content.type === "audio");
  if (!audio.length) {
    toast(sel ? "The selection doesn't cover a region" : "Select a region or a range first");
    return;
  }
  const lo = sel?.start ?? -Infinity;
  const hi = sel?.end ?? Infinity;
  const sr = project()?.sample_rate ?? 48000;
  // Don't leave slivers at the region edges.
  const margin = Math.round(sr * 0.005);
  const positions: number[] = [];
  for (const r of audio) {
    if (r.content.type !== "audio") continue;
    const c = r.content;
    for (const f of await fetchTransients(c.clip)) {
      const at = r.start + (f - c.offset);
      if (at > r.start + margin && at < r.start + r.length - margin && at >= lo && at <= hi) positions.push(at);
    }
  }
  if (!positions.length) {
    toast("No transients found there; try a higher sensitivity");
    return;
  }
  const made = await actMade("split_regions_at", { ids: audio.map((r) => r.id), positions });
  if (made) {
    setState({ selectedRegions: made, focus: "regions" });
    toast(`Split into ${made.length} regions`);
  }
}

// ---- zoom -------------------------------------------------------------------------------------

let zoomBack: { pps: number; left: number; to: { pps: number; left: number } } | null = null;

/** Z: zooms to the selection (or the region, or all the track's regions), and back again. */
export function toggleZoom() {
  if (!view) return;
  const now = view.saveView();
  if (zoomBack && Math.abs(zoomBack.to.pps - now.pps) < 1e-12 && Math.abs(zoomBack.to.left - now.left) < 2) {
    view.restoreView(zoomBack);
    zoomBack = null;
    return;
  }
  const t = edTrack();
  const sel = rangeSel();
  const r = focusRegion();
  let span: [number, number] | null = null;
  if (sel) span = [sel.start, sel.end];
  else if (r) span = [r.start, r.start + r.length];
  else if (t?.regions.length) span = [Math.min(...t.regions.map((x) => x.start)), Math.max(...t.regions.map((x) => x.start + x.length))];
  if (span) zoomToSpan(span[0], span[1]);
}

/** Zooms to timeline `a..b`, remembering the view so Z goes back to it. */
export function zoomToSpan(a: number, b: number) {
  if (!view) return;
  const now = view.saveView();
  view.zoomTo(a, b);
  zoomBack = { ...now, to: view.saveView() };
}

export function setAmp(a: number) {
  setEd({ amp: Math.min(MAX_AMP, Math.max(MIN_AMP, a)) });
}

// ---- menus ------------------------------------------------------------------------------------

const SENSITIVITIES: [string, number][] = [
  ["Low", 0.2],
  ["Medium", 0.5],
  ["High", 0.75],
  ["Very High", 0.95],
];

/** The Functions menu: actions on the selected range. */
export function functionsMenu(): MenuItem[] {
  const none = !rangeSel();
  return [
    { label: "Split at Selection", hint: fmt("Mod+T"), disabled: none, onClick: () => rangeEdit("split") },
    { label: "Split at Transients", hint: fmt("Mod+Shift+T"), onClick: splitAtTransients },
    { label: "Trim Region to Selection", hint: fmt("Alt+T"), disabled: none, onClick: () => rangeEdit("trim") },
    { separator: true },
    { label: "Silence Selection", hint: fmt("Alt+Backspace"), disabled: none, onClick: () => rangeEdit("silence") },
    { label: "Delete Selection", hint: fmt("Backspace"), disabled: none, onClick: () => rangeEdit("delete") },
    { separator: true },
    { label: "Normalize Selection", hint: fmt("Alt+N"), disabled: none, onClick: () => rangeEdit("normalize") },
    { label: "Reverse Selection", hint: fmt("Alt+R"), disabled: none, onClick: () => rangeEdit("reverse") },
    { label: "Fade In to Selection", hint: fmt("Alt+I"), disabled: none, onClick: () => rangeEdit("fade_in") },
    { label: "Fade Out to Selection", hint: fmt("Alt+O"), disabled: none, onClick: () => rangeEdit("fade_out") },
  ];
}

/** The local Edit menu. */
export function editMenu(): MenuItem[] {
  const snap = getState().snap;
  const sel = rangeSel();
  const r = focusRegion();
  return [
    { label: snap?.undo ? `Undo ${snap.undo}` : "Undo", hint: fmt("Mod+Z"), disabled: !snap?.undo, onClick: () => undoRedo("undo") },
    { label: snap?.redo ? `Redo ${snap.redo}` : "Redo", hint: fmt("Mod+Shift+Z"), disabled: !snap?.redo, onClick: () => undoRedo("redo") },
    { separator: true },
    { label: "Cut Selection", hint: fmt("Mod+X"), disabled: !sel, onClick: cutSelection },
    { label: "Copy Selection", hint: fmt("Mod+C"), disabled: !sel, onClick: copySelection },
    { label: "Paste at Playhead", hint: fmt("Mod+V"), disabled: !getState().clipboard.length, onClick: () => ops.pasteRegions() },
    { separator: true },
    { label: "Play Selection", hint: fmt("Shift+Space"), disabled: !sel, onClick: playSelection },
    { label: "Set Locators by Selection", disabled: !sel, onClick: setLocatorsBySelection },
    { separator: true },
    zeroCrossingsItem(),
    { separator: true },
    { label: "Select Region", hint: "Double-click", disabled: !r, onClick: () => r && view && setEd({ sel: { track: view.track, start: r.start, end: r.start + r.length } }) },
    { label: "Deselect", hint: "Esc", disabled: !sel, onClick: clearSel },
  ];
}

/** The local View menu. */
export function viewMenu(): MenuItem[] {
  return [
    { label: "Zoom In", hint: fmt("Mod+ArrowRight"), onClick: () => view?.zoomBy(1.5) },
    { label: "Zoom Out", hint: fmt("Mod+ArrowLeft"), onClick: () => view?.zoomBy(1 / 1.5) },
    { label: rangeSel() ? "Zoom to Selection" : "Zoom to Region", hint: "Z", onClick: toggleZoom },
    { separator: true },
    { label: "Waveform Zoom In", hint: fmt("Mod+ArrowDown"), disabled: st.amp >= MAX_AMP, onClick: () => setAmp(st.amp * 2) },
    { label: "Waveform Zoom Out", hint: fmt("Mod+ArrowUp"), disabled: st.amp <= MIN_AMP, onClick: () => setAmp(st.amp / 2) },
    { label: "Reset Waveform Zoom", disabled: st.amp === 1, onClick: () => setAmp(1) },
    { separator: true },
    { label: "Show Transients", checked: st.showTransients, onClick: () => setEd({ showTransients: !st.showTransients }) },
    {
      label: "Transient Sensitivity",
      children: SENSITIVITIES.map(([label, v]) => ({ label, checked: Math.abs(st.sensitivity - v) < 0.01, onClick: () => setEd({ sensitivity: v, showTransients: true }) })),
    },
    ...detachMenuItems("editor"),
  ];
}

/** Right-clicking inside the selection. */
export function selectionMenu(): MenuItem[] {
  return [
    { label: "Play Selection", hint: fmt("Shift+Space"), onClick: playSelection },
    { separator: true },
    { label: "Cut", hint: fmt("Mod+X"), onClick: cutSelection },
    { label: "Copy", hint: fmt("Mod+C"), onClick: copySelection },
    { separator: true },
    ...functionsMenu(),
    { separator: true },
    { label: "Set Locators by Selection", onClick: setLocatorsBySelection },
    { label: "Zoom to Selection", hint: "Z", onClick: toggleZoom },
  ];
}

// ---- keys -------------------------------------------------------------------------------------

/** Native menu items the editor takes over while it has focus and a selection: Split acts at
 *  the selection's edges, Cut/Copy/Delete on the selected part. Returns true if handled. */
export function editorMenuCommand(id: string): boolean {
  if (!focused || !rangeSel()) return false;
  if (id === "split") rangeEdit("split");
  else if (id === "cut") cutSelection();
  else if (id === "copy") copySelection();
  else if (id === "delete") rangeEdit("delete");
  else return false;
  return true;
}

/** The editor's keys, checked before the window's own while the editor has focus. */
export function onEditorKey(e: KeyboardEvent) {
  if (!focused || !view) return;
  const tgt = e.target as HTMLElement;
  if (tgt && (tgt.tagName === "INPUT" || tgt.tagName === "SELECT" || tgt.tagName === "TEXTAREA")) return;
  // Dialogs and open menus keep their keys (Escape closes a menu, not the selection).
  if (getState().dialog || sheetOpen() || document.querySelector(".menu-layer")) return;
  const mod = e.metaKey || e.ctrlKey;
  const sel = rangeSel();
  const code = e.code;
  let handled = true;
  if (mod && !e.altKey) {
    if (e.key === "ArrowRight") view.zoomBy(1.5);
    else if (e.key === "ArrowLeft") view.zoomBy(1 / 1.5);
    else if (e.key === "ArrowDown") setAmp(st.amp * 2);
    else if (e.key === "ArrowUp") setAmp(st.amp / 2);
    else if (code === "KeyT" && e.shiftKey) splitAtTransients();
    else if (code === "KeyC" && !e.shiftKey && sel) copySelection();
    else if (code === "KeyX" && !e.shiftKey && sel) cutSelection();
    else handled = false;
  } else if (e.altKey && !mod) {
    const actions: Record<string, RangeAction> = { KeyT: "trim", KeyN: "normalize", KeyR: "reverse", KeyI: "fade_in", KeyO: "fade_out", Backspace: "silence", Delete: "silence" };
    if (sel && actions[code]) rangeEdit(actions[code]);
    else handled = false;
  } else if (!mod) {
    if (code === "KeyZ" && !e.shiftKey) toggleZoom();
    else if (e.key === "Escape" && sel) clearSel();
    else if ((e.key === "Backspace" || e.key === "Delete") && sel) rangeEdit("delete");
    else if (e.key === " " && e.shiftKey && sel) playSelection();
    else handled = false;
  } else handled = false;
  if (handled) {
    e.preventDefault();
    e.stopImmediatePropagation();
  }
}

/** Keeps `focused` in step with where the user last clicked. */
export function onWindowMouseDown(e: MouseEvent) {
  const el = e.target instanceof Element ? e.target : null;
  // Choosing from a menu the editor opened doesn't take focus away from it.
  if (el?.closest(".menu")) return;
  focused = !!el?.closest(".audio-editor");
}
