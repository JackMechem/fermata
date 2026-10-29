import { invoke } from "@tauri-apps/api/core";
import { setState } from "./store";
import type { SigEvent, TempoEvent } from "./tempo";

export type Id = number;
export type TrackKind = "audio" | "instrument" | "folder" | "summing" | "aux";

export const isStack = (k: TrackKind) => k === "folder" || k === "summing";
/** Tracks with an R button. */
export const canRecord = (k: TrackKind) => k === "audio" || k === "instrument";
export type InputSel = { type: "none" } | { type: "mono"; ch: number } | { type: "stereo"; ch: number };

export interface PluginSlot {
  uid: Id;
  plugin_id: string;
  name: string;
  bypass: boolean;
  /** A built-in effect's values: [parameter id, plain value]; missing ids are at their default. */
  params?: [number, number][];
  /** The track keying a built-in effect's side-chain. */
  sidechain?: Id | null;
  /** A VST3's state, only in what `copy_plugin` returns (for the clipboard). */
  state?: string | null;
}

export interface Note {
  start: number;
  length: number;
  pitch: number;
  velocity: number;
  /** Muted notes stay in the region but don't play. */
  muted?: boolean;
}

/** What a controller event drives: a CC number, pitch bend (0–16383) or channel pressure. */
export type CcKind = { type: "controller"; cc: number } | { type: "pitch_bend" } | { type: "aftertouch" };

export interface CcEvent {
  /** Offset from the region start, in samples. */
  start: number;
  kind: CcKind;
  value: number;
}

export type RegionContent =
  | { type: "audio"; clip: Id; offset: number; gain_db: number; fade_in: number; fade_out: number }
  | { type: "midi"; notes: Note[]; cc?: CcEvent[] };

export interface Region {
  id: Id;
  name: string;
  start: number;
  length: number;
  muted: boolean;
  /** Overrides the track colour. */
  color?: string | null;
  content: RegionContent;
  /** Timeline sample where the loop stops (absent / null = not looping). */
  loop_end?: number | null;
  /** Present on take folders (see TakeFolder). */
  takes?: TakeFolder | null;
}

/** A take folder. Takes and the comp are in take time: a timeline position `p` in the folder
 *  region is take time `p - region.start + content.offset`, and a take plays clip frame
 *  `take.offset + t` at take time `t`. */
export interface TakeFolder {
  /** Oldest first. */
  takes: Take[];
  /** Sorted, non-overlapping; gaps are silent. */
  comp: CompSegment[];
}

export interface Take {
  id: Id;
  name: string;
  clip: Id;
  offset: number;
  /** Take-time range with audio. */
  start: number;
  end: number;
  gain_db: number;
}

export interface CompSegment {
  start: number;
  end: number;
  take: Id;
}

export interface Marker {
  id: Id;
  pos: number;
  name: string;
  color?: string | null;
  length?: number | null;
}

export type BandKind = "low_cut" | "low_shelf" | "bell" | "high_shelf" | "high_cut";

export interface EqBand {
  kind: BandKind;
  on: boolean;
  freq: number;
  gain: number;
  q: number;
  slope: number;
}

export interface ChannelEq {
  on: boolean;
  bands: EqBand[];
  output_db: number;
}

export interface Track {
  id: Id;
  kind: TrackKind;
  name: string;
  color: string;
  volume_db: number;
  pan: number;
  mute: boolean;
  solo: boolean;
  armed: boolean;
  monitor: boolean;
  input: InputSel;
  instrument: PluginSlot | null;
  inserts: (PluginSlot | null)[];
  regions: Region[];
  eq: ChannelEq;
  /** The track stack this track is in. */
  parent: Id | null;
  /** For stack main tracks: subtracks hidden. */
  collapsed: boolean;
  group: Id | null;
  /** Hidden with Hide Track (still plays). */
  hidden: boolean;
  /** SEND_SLOTS send slots (older projects may have fewer until edited). */
  sends: (Send | null)[];
  /** An aux, MASTER_ID for Stereo Out, or null for the default route (the summing stack the
   *  track is in, else Stereo Out). */
  output: Id | null;
  /** For auxes: the bus feeding them ("Bus 3"). */
  bus: number;
  automation: AutoLane[];
  auto_mode: AutoMode;
  /** Set while frozen: the track plays this render, its plug-ins unloaded. */
  frozen?: FreezeInfo | null;
}

export interface FreezeInfo {
  clip: Id;
  start: number;
}

/** What an automation lane drives. */
export type AutoParam =
  | { type: "volume" }
  | { type: "pan" }
  | { type: "mute" }
  | { type: "send"; value: number }
  | { type: "plugin"; value: { uid: Id; param: number; name: string } };

/** A breakpoint, in the parameter's units: dB, pan -1..1, mute 0/1, plug-in 0..1. */
export interface AutoPoint {
  pos: number;
  value: number;
}

export interface AutoLane {
  param: AutoParam;
  points: AutoPoint[];
}

export type AutoMode = "off" | "read" | "touch" | "latch" | "write";

/** A plug-in parameter, from `list_plugin_params`. */
export interface ParamInfo {
  id: number;
  name: string;
  units: string;
  default: number;
  steps: number;
}

/** A send slot: a copy of the strip's signal into an aux (`bus` is the aux's track id). */
export interface Send {
  bus: Id;
  level_db: number;
  /** Taps after the plug-ins, before the fader and pan. */
  pre_fader: boolean;
  muted: boolean;
}

export const SEND_SLOTS = 8;

export interface Group {
  id: Id;
  name: string;
  color: string;
  enabled: boolean;
  volume: boolean;
  pan: boolean;
  mute: boolean;
  solo: boolean;
  record: boolean;
  select: boolean;
  editing: boolean;
}

/** An edit's snapshot plus the ids of what it created. */
export interface Made {
  snapshot: Snapshot;
  ids: Id[];
}

export interface Project {
  name: string;
  sample_rate: number;
  tempo: number;
  sig_num: number;
  sig_den: number;
  key: string;
  tracks: Track[];
  master: { volume_db: number; inserts: (PluginSlot | null)[]; eq: ChannelEq };
  cycle: { enabled: boolean; start: number; end: number };
  metronome: boolean;
  count_in: boolean;
  groups: Group[];
  /** Metronome settings (Project Settings › Metronome). */
  click: ClickSettings;
  /** Overrides Settings › Recording › Bit Depth for this project. */
  bit_depth: BitDepth | null;
  notes: string;
  /** The Marker track, sorted by position. */
  markers: Marker[];
  /** The Autopunch range. */
  punch: { enabled: boolean; start: number; end: number };
  /** The Tempo track; empty means `tempo` throughout (see tempo.ts). */
  tempo_map: TempoEvent[];
  /** The Signature track; empty means `sig_num/sig_den` throughout. */
  signatures: SigEvent[];
}

export type BitDepth = "int24" | "float32";
export type ClickSound = "classic" | "click" | "woodblock" | "beep";

export interface ClickSettings {
  sound: ClickSound;
  volume_db: number;
  accent: boolean;
  bar_pitch: number;
  beat_pitch: number;
  /** Clicks per beat: 1, 2, 3 or 4. */
  division: number;
  while_playing: boolean;
  while_recording: boolean;
  only_count_in: boolean;
}

export interface AudioStatus {
  running: boolean;
  host: string;
  input_name: string | null;
  output_name: string | null;
  input_channels: number;
  output_channels: number;
  sample_rate: number;
  buffer_size: number;
  error: string | null;
}

export interface Snapshot {
  project: Project;
  selected: Id | null;
  path: string;
  saved: boolean;
  dirty: boolean;
  audio: AudioStatus;
  midi_inputs: string[];
  open_editors: Id[];
  /** Track that live MIDI (hardware or Musical Typing) plays. */
  midi_target: string | null;
  /** Labels of the next undo / redo steps. */
  undo: string | null;
  redo: string | null;
  /** Plug-in latency and delay compensation. */
  pdc?: PdcReport;
  /** Why the edit was refused and put back (it touched a frozen track). */
  refused?: string | null;
}

/** session::StripLatency: frames. */
export interface StripLatency {
  track: Id;
  /** The strip's plug-ins, bypassed ones included. */
  latency: number;
  /** Compensation delay on its output. */
  delay: number;
  /** [instance uid, latency] per latent plug-in. */
  slots: [Id, number][];
}

export interface PdcReport {
  enabled: boolean;
  low_latency: boolean;
  /** How far Stereo Out lags the timeline. */
  total: number;
  total_ms: number;
  strips: StripLatency[];
}

export interface PluginDesc {
  id: string;
  name: string;
  vendor: string;
  version: string;
  subcategories: string;
  is_instrument: boolean;
}

export interface TakePreview {
  track: Id;
  start: number;
  frames: number;
  from: number;
  peaks: number[];
}

export interface Tick {
  /** Something went wrong out of sight (a recording couldn't be written); shown once. */
  warning?: string | null;
  pos: number;
  playing: boolean;
  recording: boolean;
  cpu: number;
  meters: [Id, number, number][];
  master: [number, number];
  inputs: number[];
  previews: TakePreview[];
  midi_activity: number;
  overruns: number;
  latency: { input_ms: number; output_ms: number; compensation_ms: number };
  spectrum: [Id, number[]] | null;
  /** Meters and running values of the built-in effects whose panels are open. */
  fx?: { uid: Id; meters: number[]; values: [number, number][] }[];
}

export interface DeviceInfo {
  id: string;
  name: string;
  channels: number;
  sample_rates: number[];
  is_default: boolean;
}

export interface HostInfo {
  name: string;
  devices: number;
}

export interface DeviceList {
  host: string;
  hosts: HostInfo[];
  inputs: DeviceInfo[];
  outputs: DeviceInfo[];
}

/** The sample rates `cfg`'s devices both support (the output's, less any the input lacks). */
export function supportedRates(cfg: AudioConfig, devices: DeviceList | null): number[] {
  const outDev = devices?.outputs.find((d) => d.id === cfg.output) ?? devices?.outputs.find((d) => d.is_default);
  const inDev = devices?.inputs.find((d) => d.id === cfg.input) ?? devices?.inputs.find((d) => d.is_default);
  return (outDev?.sample_rates ?? [44100, 48000]).filter((r) => !inDev || !cfg.input_enabled || inDev.sample_rates.includes(r));
}

export interface AudioConfig {
  host: string | null;
  input: string | null;
  output: string | null;
  sample_rate: number;
  buffer_size: number;
  input_enabled: boolean;
  auto_monitor: boolean;
  recording_delay_ms: number;
}

export const MASTER_ID = 0;
export const INSTRUMENT_SLOT = -1;
export const BUILTIN_SYNTH = "builtin:synth";
/** The Channel EQ in the preset and paste commands (which take it in place of a slot). */
export const EQ_SLOT = -2;

function report(e: unknown) {
  const msg = typeof e === "string" ? e : (e as Error)?.message ?? String(e);
  setState({ toast: { text: msg, at: Date.now() } });
}

/** Invokes a command that returns a fresh snapshot and stores it. */
export async function act(cmd: string, args?: Record<string, unknown>): Promise<Snapshot | null> {
  try {
    const res = await invoke<Snapshot | Made>(cmd, args);
    // Commands that report what they made wrap the snapshot.
    const snap = res && "snapshot" in res ? res.snapshot : res;
    if (snap) setState({ snap });
    if (snap?.refused) report(snap.refused);
    return snap;
  } catch (e) {
    report(e);
    return null;
  }
}

export async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T | null> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    report(e);
    return null;
  }
}

/** Invokes a command returning a `Made` and stores its snapshot. */
export async function actMade(cmd: string, args?: Record<string, unknown>): Promise<Id[] | null> {
  const res = await call<Made>(cmd, args);
  if (!res) return null;
  setState({ snap: res.snapshot });
  return res.ids;
}

export const api = {
  updateTrack: (id: Id, patch: Partial<Omit<Track, "id" | "kind" | "regions" | "inserts" | "instrument" | "parent" | "group" | "hidden" | "sends" | "output" | "bus">>) =>
    act("update_track", { id, patch }),
  selectTrack: (id: Id | null) => act("select_track", { id }),
  setPlugin: (track: Id, slot: number, pluginId: string, open = true) =>
    act("set_plugin", { track, slot, pluginId, open }),
  removePlugin: (track: Id, slot: number) => act("remove_plugin", { track, slot }),
  setBypass: (track: Id, slot: number, bypass: boolean) => act("set_bypass", { track, slot, bypass }),
  openEditor: (uid: Id, title: string) => act("open_editor", { uid, title }),
};

export interface InputLevel {
  device: string;
  volume: number;
  gain_db: number;
  muted: boolean;
  attenuating: boolean;
}

export interface RecentProject {
  path: string;
  name: string;
  exists: boolean;
  modified: number | null;
  tracks: number;
  tempo: number | null;
  sample_rate: number | null;
}
