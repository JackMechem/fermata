// App-wide settings (everything in Settings except the audio devices), mirrored from
// src-tauri/src/settings.rs, plus the launch sequence and crash recovery they drive.
import { useSyncExternalStore } from "react";
import { act, call, type BitDepth, type ClickSettings, type PluginDesc, type Project, type RecentProject, type Snapshot } from "./api";
import { getState, setState, type Snap } from "./store";
import { tempoMap } from "./tempo";
import type { WheelScrolls } from "./wheel";

export type AudioOverlap = "on_top" | "new_track" | "replace";
export type MidiOverlap = "new_region" | "merge";
export type TakeNames = "track_number" | "track" | "project_track";
export type VelocityCurve = "linear" | "soft" | "hard" | "fixed";
export type Startup = "chooser" | "last_project" | "empty_project";
export type Nudge = "bar" | "beat" | "8" | "16" | "32" | "ms10" | "ms1";
export type PdcMode = "all" | "off";
export type ClickLocates = "anywhere" | "empty" | "off";

/** Session behaviour (crates/engine/src/prefs.rs). */
export interface Prefs {
  general: { project_folder: string | null; autosave_minutes: number; undo_limit: number; autosave_after_recording: boolean };
  recording: {
    bit_depth: BitDepth;
    count_in_bars: number;
    pre_roll_secs: number;
    audio_overlap: AudioOverlap;
    midi_overlap: MidiOverlap;
    take_names: TakeNames;
    folder: string | null;
  };
  midi: { disabled_inputs: string[]; thru: boolean; velocity: VelocityCurve; fixed_velocity: number };
  editing: { fade_ms: number };
  /** Metronome settings for new projects. */
  click: ClickSettings;
  /** Settings › Audio: plug-in delay compensation and Low Latency Mode. */
  latency: { compensation: PdcMode; low_latency: boolean; low_latency_limit_ms: number };
}

export interface UiPrefs {
  startup: Startup;
  confirm_unsaved: boolean;
  snap: Snap;
  nudge: Nudge;
  /** Key command overrides: command id -> keys (an empty list removes the default). */
  keymap: Record<string, string[]>;
  /** Settings › Appearance (see appearance.ts); null until first saved. */
  theme?: string | null;
  ui_scale?: number | null;
  wave_style?: string | null;
  /** Settings › Editing: what a click in the tracks area does to the playhead. */
  click_locates?: ClickLocates;
  /** Settings › Editing: what the mouse wheel scrolls in the Tracks area and Piano Roll. */
  wheel_scrolls?: WheelScrolls;
  /** Settings › General: where built-in effect panels open (fxDock.ts). */
  fx_panels?: "docked" | "floating" | "window";
}

export interface PluginPrefs {
  disabled: string[];
  folders: string[];
}

export interface AppSettings {
  prefs: Prefs;
  ui: UiPrefs;
  plugins: PluginPrefs;
}

export const DEFAULT_CLICK: ClickSettings = {
  sound: "classic",
  volume_db: 0,
  accent: true,
  bar_pitch: 0,
  beat_pitch: 0,
  division: 1,
  while_playing: true,
  while_recording: true,
  only_count_in: false,
};

/** Used until the real settings arrive (and if they can't be read). */
const DEFAULTS: AppSettings = {
  prefs: {
    general: { project_folder: null, autosave_minutes: 2, undo_limit: 100, autosave_after_recording: true },
    recording: {
      bit_depth: "float32",
      count_in_bars: 1,
      pre_roll_secs: 0,
      audio_overlap: "on_top",
      midi_overlap: "new_region",
      take_names: "track_number",
      folder: null,
    },
    midi: { disabled_inputs: [], thru: true, velocity: "linear", fixed_velocity: 100 },
    editing: { fade_ms: 0 },
    click: DEFAULT_CLICK,
    latency: { compensation: "all", low_latency: false, low_latency_limit_ms: 5 },
  },
  ui: { startup: "chooser", confirm_unsaved: true, snap: "smart", nudge: "16", keymap: {} },
  plugins: { disabled: [], folders: [] },
};

let settings: AppSettings = DEFAULTS;
const listeners = new Set<() => void>();

export function getSettings(): AppSettings {
  return settings;
}

export function useSettings<T>(sel: (s: AppSettings) => T): T {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => sel(settings),
  );
}

/** Calls `l` whenever the settings change (detach.ts passes them to the other windows). */
export function onSettingsChange(l: () => void): () => void {
  listeners.add(l);
  return () => {
    listeners.delete(l);
  };
}

/** Takes settings another window changed (and already saved). */
export function receiveSettings(s: AppSettings) {
  settings = s;
  listeners.forEach((l) => l());
}

export async function loadSettings() {
  const s = await call<AppSettings>("get_settings");
  if (s) settings = s;
  listeners.forEach((l) => l());
  setState({ snapMode: settings.ui.snap });
}

/** Changes settings, applies them and saves them. */
export async function updateSettings(f: (s: AppSettings) => AppSettings) {
  const before = settings;
  settings = f(settings);
  listeners.forEach((l) => l());
  await act("set_settings", { settings });
  if (JSON.stringify(before.plugins.disabled) !== JSON.stringify(settings.plugins.disabled)) await refreshPlugins();
}

/** Reloads the plug-ins offered in the Library and slot menus. */
export async function refreshPlugins() {
  const p = await call<PluginDesc[]>("list_plugins");
  if (p) setState({ plugins: p });
}

/** The Nudge step in samples for the current project, at the tempo and signature at `pos`. */
export function nudgeSamples(p: Project, pos = 0): number {
  const n = settings.ui.nudge;
  const m = tempoMap(p);
  if (n === "bar") return m.samplesPerBarAt(pos);
  if (n === "beat") return m.samplesPerBeatAt(pos);
  if (n === "ms10") return p.sample_rate / 100;
  if (n === "ms1") return p.sample_rate / 1000;
  // Note values: 8 = eighth note, relative to a quarter.
  return m.samplesPerQuarterAt(pos) * (4 / Number(n));
}

// ---- launch and crash recovery ------------------------------------------------------------

/** Audio a recording left behind when Fermata quit mid-take. */
export interface RecordingRecovery {
  started: number;
  takes: number;
  /** The longest take, in seconds. */
  seconds: number;
  tracks: string[];
}

/** A project folder with unsaved work: an autosave newer than its saved document, or audio
 * from a recording that never finished. */
export interface Recovery {
  path: string;
  name: string;
  /** The autosave's time, or the interrupted recording's start when there's only that. */
  autosaved: number;
  saved: number | null;
  has_autosave: boolean;
  recording: RecordingRecovery | null;
}

let recoveries: Recovery[] = [];
let recoveryAtLaunch = false;
/** Projects whose recovery was put off ("Later") during this launch: not offered again. */
let putOff: string[] = [];
const recoveryListeners = new Set<() => void>();

export function useRecoveries(): Recovery[] {
  return useSyncExternalStore(
    (l) => {
      recoveryListeners.add(l);
      return () => recoveryListeners.delete(l);
    },
    () => recoveries,
  );
}

function showRecovery(list: Recovery[], atLaunch: boolean) {
  recoveries = list;
  recoveryAtLaunch = atLaunch;
  recoveryListeners.forEach((l) => l());
  setState({ dialog: "recovery" });
}

/**
 * Called when the recovery sheet closes. If nothing was opened from it at launch, carries on
 * with the usual startup.
 */
export function recoveryDone(opened: boolean) {
  const atLaunch = recoveryAtLaunch;
  if (atLaunch && !opened) putOff = [...putOff, ...recoveries.map((r) => r.path)];
  recoveries = [];
  recoveryAtLaunch = false;
  recoveryListeners.forEach((l) => l());
  setState({ dialog: null });
  if (atLaunch && !opened) runStartup(false);
}

/**
 * What happens at launch without a project on the command line: offer to recover unsaved
 * work first, then open the last project or show the Project Chooser, as set in
 * Settings › General.
 */
export async function runStartup(checkRecovery = true) {
  if (checkRecovery) {
    const lost = await call<Recovery[]>("recoverable_projects");
    if (lost?.length) return showRecovery(lost, true);
  }
  const startup = settings.ui.startup;
  if (startup === "empty_project") return;
  const recent = await call<RecentProject[]>("recent_projects");
  const last = recent?.find((p) => p.exists);
  if (startup === "last_project" && last && !putOff.includes(last.path)) {
    if (await openProjectPath(last.path)) return;
    // It has newer autosaved changes, so the recovery sheet is up instead. Leave it there; if
    // it's closed without opening anything, startup carries on with the Chooser.
    if (getState().dialog === "recovery") {
      recoveryAtLaunch = true;
      return;
    }
  }
  setState({ dialog: "chooser", chooserTab: last ? "recent" : "new" });
}

/** Offers the autosave of a project that is already open (one given on the command line). */
export async function offerRecoveryFor(path: string) {
  const lost = await call<Recovery | null>("recovery_info", { path });
  if (lost) showRecovery([lost], false);
}

/**
 * Opens a project, offering its autosave first when one is newer than the saved file.
 * Resolves to the snapshot, or null if it didn't open (or the recovery sheet took over).
 */
export async function openProjectPath(path: string): Promise<Snapshot | null> {
  const lost = await call<Recovery | null>("recovery_info", { path });
  if (lost) {
    showRecovery([lost], false);
    return null;
  }
  const snap = await act("open_project", { path });
  if (snap) setState({ selectedRegions: [] });
  return snap;
}
