// Bounce and export: the types mirroring the engine's options, and opening the dialogs in
// components/Bounce.tsx (kept apart so ops.ts can open them without importing components).
import { regionEnd } from "./loops";
import type { Id, Project } from "./api";
import { setState } from "./store";
import { fmt, MENU_SHORTCUTS } from "./keymap";
import type { AudioFormat, BitDepth } from "./bounceFormat";

export type { AudioFormat, BitDepth } from "./bounceFormat";

/** session::RenderOptions. Positions are project samples, the end exclusive. */
export interface RenderOptions {
  start: number;
  end: number;
  tail_secs: number;
  bit_depth: BitDepth;
  /** null keeps the project rate. */
  sample_rate: number | null;
  normalize: boolean;
  format: AudioFormat;
}

export type BipSource = "leave" | "mute" | "delete";

/** session::BipOptions. */
export interface BipOptions {
  effects: boolean;
  fader: boolean;
  source: BipSource;
  tail: boolean;
}

export interface BounceResult {
  paths: string[];
  peak_db: number;
  seconds: number;
}

/** Bounce Project or Section, Export All Tracks, Export Selected Tracks, or Export Region. */
export type BounceKind = "project" | "tracks" | "selected" | "region";

export let bounceRequest: { kind: BounceKind; region: Id | null; tracks: Id[] } = { kind: "project", region: null, tracks: [] };
export let inPlaceRequest: { tracks: Id[]; regions: Id[] } = { tracks: [], regions: [] };

export function openBounce(kind: BounceKind, region: Id | null = null, tracks: Id[] = []) {
  bounceRequest = { kind, region, tracks };
  setState({ dialog: "bounce" });
}

/** Bounce in Place for whole tracks, or (from the region menu) just some regions. */
export function openBounceInPlace(tracks: Id[], regions: Id[] = []) {
  if (!tracks.length && !regions.length) return;
  inPlaceRequest = { tracks, regions };
  setState({ dialog: "bounce-in-place" });
}

let busy = false;

/** True while a render holds the engine; other commands would wait for it. */
export const isBouncing = () => busy;
export const setBouncing = (b: boolean) => {
  busy = b;
};

/** Where the last region ends: the end of "Whole Project". */
export function projectEnd(p: Project): number {
  return Math.max(0, ...p.tracks.flatMap((t) => t.regions.map(regionEnd)));
}

/** Logic's ⌃B; elsewhere Ctrl+B is Bounce Project, so it gains Alt (see menu.rs). */
export const BOUNCE_IN_PLACE_HINT = fmt(MENU_SHORTCUTS.find((m) => m.label === "Bounce in Place…")!.key);
