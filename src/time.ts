import type { Project } from "./api";
import { tempoMap, type Bbt } from "./tempo";

export const PEAK_BUCKET = 256;

/** Samples per beat at the tempo and signature at position 0: the horizontal scale of the
 *  editors (pixels per beat). Grids, snapping and bars go through the tempo map instead. */
export function samplesPerBeat(p: Project): number {
  return ((p.sample_rate * 60) / p.tempo) * (4 / p.sig_den);
}

/** Samples per bar at position 0 (see `samplesPerBeat`). */
export function samplesPerBar(p: Project): number {
  return samplesPerBeat(p) * p.sig_num;
}

export type { Bbt };

/** Position in bars/beats/divisions(16ths)/ticks, 1-based like Logic's LCD. */
export function toBbt(p: Project, pos: number): Bbt {
  return tempoMap(p).bbtAt(pos);
}

/** The bar line at or before `pos`. */
export function barFloor(p: Project, pos: number): number {
  const m = tempoMap(p);
  return m.barStart(m.barAt(pos));
}

/** The bar line `n` bars after the bar holding `pos` (n may be negative). */
export function barsFrom(p: Project, pos: number, n: number): number {
  const m = tempoMap(p);
  return m.barStart(m.barAt(pos) + n);
}

/** A length starting at `start`, in bars, beats, divisions and ticks (0-based, Logic's
 *  length display), counted through the tempo map. */
export function lengthBbt(p: Project, start: number, len: number): Bbt {
  const m = tempoMap(p);
  const [num, den] = m.sigAt(start);
  const beats = ((m.beatsAt(start + len) - m.beatsAt(start)) * den) / 4;
  const bar = Math.floor(beats / num + 1e-9);
  const rest = Math.max(0, beats - bar * num);
  const beat = Math.floor(rest + 1e-9);
  const div = Math.floor((rest - beat) * 4 + 1e-9);
  const tick = Math.round(((rest - beat) * 4 - div) * 240);
  return { bar, beat, div, tick };
}

export function formatTime(p: Project, pos: number): string {
  const secs = Math.max(0, pos) / p.sample_rate;
  const m = Math.floor(secs / 60);
  const s = secs - m * 60;
  return `${String(m).padStart(2, "0")}:${s.toFixed(3).padStart(6, "0")}`;
}

export function snapPos(p: Project, pos: number, mode: string, pxPerBeat: number): number {
  if (mode === "off") return Math.round(pos);
  const m = tempoMap(p);
  const [num, den] = m.sigAt(pos);
  const beat = 4 / den;
  let step: number;
  if (mode === "bar") step = beat * num;
  else if (mode === "beat") step = beat;
  else {
    // Smart: follow the visible grid resolution.
    step = pxPerBeat > 60 ? beat / 4 : pxPerBeat > 14 ? beat : beat * num;
  }
  // Counted from the bar line, so the grid follows tempo and signature changes.
  return m.sampleAt(m.snapQ(m.beatsAt(pos), step));
}

export function dbToFader(db: number): number {
  // Fader travel: 0..1 maps roughly like Logic's taper (+6 dB at top, -inf at bottom).
  if (db <= -96) return 0;
  if (db >= 6) return 1;
  if (db >= -12) return 0.62 + ((db + 12) / 18) * 0.38;
  if (db >= -40) return 0.22 + ((db + 40) / 28) * 0.4;
  return ((db + 96) / 56) * 0.22;
}

export function faderToDb(f: number): number {
  if (f <= 0.001) return -96;
  if (f >= 0.62) return -12 + ((f - 0.62) / 0.38) * 18;
  if (f >= 0.22) return -40 + ((f - 0.22) / 0.4) * 28;
  return -96 + (f / 0.22) * 56;
}

export function formatDb(db: number): string {
  if (db <= -96) return "-∞";
  const r = Math.round(db * 10) / 10;
  return (r > 0 ? "+" : "") + r.toFixed(1);
}

export function gainToDb(g: number): number {
  return g <= 0.0000158 ? -96 : 20 * Math.log10(g);
}

export function inputLabel(sel: { type: string; ch?: number }): string {
  if (sel.type === "mono") return `Input ${(sel.ch ?? 0) + 1}`;
  if (sel.type === "stereo") return `Input ${(sel.ch ?? 0) + 1}–${(sel.ch ?? 0) + 2}`;
  return "No Input";
}
