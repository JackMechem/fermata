import type { Project } from "./api";

export const PEAK_BUCKET = 256;

export function samplesPerBeat(p: Project): number {
  return ((p.sample_rate * 60) / p.tempo) * (4 / p.sig_den);
}

export function samplesPerBar(p: Project): number {
  return samplesPerBeat(p) * p.sig_num;
}

export interface Bbt {
  bar: number;
  beat: number;
  div: number;
  tick: number;
}

/** Position in bars/beats/divisions(16ths)/ticks, 1-based like Logic's LCD. */
export function toBbt(p: Project, pos: number): Bbt {
  const spb = samplesPerBeat(p);
  const beats = pos / spb;
  const bar = Math.floor(beats / p.sig_num);
  const inBar = beats - bar * p.sig_num;
  const beat = Math.floor(inBar);
  const frac = inBar - beat;
  const div = Math.floor(frac * 4);
  const tick = Math.floor((frac * 4 - div) * 240);
  return { bar: bar + 1, beat: beat + 1, div: div + 1, tick: tick + 1 };
}

export function formatTime(p: Project, pos: number): string {
  const secs = Math.max(0, pos) / p.sample_rate;
  const m = Math.floor(secs / 60);
  const s = secs - m * 60;
  return `${String(m).padStart(2, "0")}:${s.toFixed(3).padStart(6, "0")}`;
}

export function snapPos(p: Project, pos: number, mode: string, pxPerBeat: number): number {
  if (mode === "off") return Math.round(pos);
  const spb = samplesPerBeat(p);
  let grid: number;
  if (mode === "bar") grid = spb * p.sig_num;
  else if (mode === "beat") grid = spb;
  else {
    // Smart: follow the visible grid resolution.
    grid = pxPerBeat > 60 ? spb / 4 : pxPerBeat > 14 ? spb : spb * p.sig_num;
  }
  return Math.round(pos / grid) * grid;
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
