// Fermata Sampler's panel, the parts with no screen in them: zones as the engine sends them
// (crates/engine/src/instrument/sampler/state.rs), note names, and the sums behind the zone map
// and the waveform's handles. Tested in tests/samplerMap.test.ts.

export interface Zone {
  id: number;
  name: string;
  file: string;
  key_lo: number;
  key_hi: number;
  vel_lo: number;
  vel_hi: number;
  root: number;
  fine: number;
  gain_db: number;
  pan: number;
  start: number;
  /** 0 is the end of the file. */
  end: number;
  loop_on: boolean;
  loop_start: number;
  /** 0 is the end of the zone. */
  loop_end: number;
  xfade: number;
  equal_power: boolean;
  one_shot: boolean;
  reverse: boolean;
  rr_group: number;
  env?: [number, number, number, number] | null;
}

export interface ZoneView extends Zone {
  frames: number;
  rate: number;
  channels: number;
  /** Why it can't play. */
  error: string | null;
}

export interface SamplerView {
  zones: ZoneView[];
  memory: number;
  limit: number;
  recording: { track: number; frames: number; secs: number; peak: number } | null;
}

export interface SamplerWave {
  frames: number;
  from: number;
  to: number;
  peaks: [number, number][];
}

/** The settings of a zone alone, as the engine takes them back. */
export function toZone(v: ZoneView): Zone {
  const { frames: _f, rate: _r, channels: _c, error: _e, ...zone } = v;
  return zone;
}

const NAMES = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];

/** Logic's note names, as the rest of the app: middle C (60) is C3. */
export const keyName = (k: number) => `${NAMES[k % 12]}${Math.floor(k / 12) - 2}`;

/** A typed key: a name ("C3", "f#2", "Bb-1") or a number. Null if it's neither. */
export function parseKey(text: string): number | null {
  const t = text.trim();
  if (/^\d+$/.test(t)) return Math.min(127, Number(t));
  const m = /^([a-gA-G])([#♯b♭]?)(-?\d)$/.exec(t);
  if (!m) return null;
  const base = { c: 0, d: 2, e: 4, f: 5, g: 7, a: 9, b: 11 }[m[1].toLowerCase()] as number;
  const acc = m[2] === "#" || m[2] === "♯" ? 1 : m[2] === "b" || m[2] === "♭" ? -1 : 0;
  const n = (Number(m[3]) + 2) * 12 + base + acc;
  return n >= 0 && n <= 127 ? n : null;
}

/** The zone map's geometry: keys across, velocity up, with a strip of keys below. */
export interface MapBox {
  width: number;
  height: number;
  /** The key strip's height at the bottom. */
  strip: number;
}

export const keyAt = (b: MapBox, x: number) => Math.max(0, Math.min(127, Math.floor((x / b.width) * 128)));
export const velAt = (b: MapBox, y: number) => {
  const h = b.height - b.strip;
  return Math.max(0, Math.min(127, Math.round(((h - y) / h) * 127)));
};

export interface ZoneRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export function zoneRect(b: MapBox, z: Zone): ZoneRect {
  const h = b.height - b.strip;
  const px = b.width / 128;
  const top = h - ((z.vel_hi + 1) / 128) * h;
  const bottom = h - (z.vel_lo / 128) * h;
  return { x: z.key_lo * px, y: top, w: (z.key_hi - z.key_lo + 1) * px, h: bottom - top };
}

/** The topmost zone under a point (later zones draw over earlier ones). */
export function zoneAt<T extends Zone>(b: MapBox, zones: T[], x: number, y: number): T | null {
  for (let i = zones.length - 1; i >= 0; i--) {
    const r = zoneRect(b, zones[i]);
    if (x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h) return zones[i];
  }
  return null;
}

export type ZoneGrab = "move" | "key_lo" | "key_hi" | "vel_lo" | "vel_hi";

/** Which part of a zone a press takes hold of: an edge when near one, otherwise all of it. */
export function grabOf(b: MapBox, z: Zone, x: number, y: number): ZoneGrab {
  const r = zoneRect(b, z);
  const edge = Math.min(5, r.w / 3);
  if (x - r.x < edge) return "key_lo";
  if (r.x + r.w - x < edge) return "key_hi";
  const v = Math.min(5, r.h / 3);
  if (y - r.y < v) return "vel_hi";
  if (r.y + r.h - y < v) return "vel_lo";
  return "move";
}

/** A zone dragged by `dk` keys and `dv` velocity steps. Moving it brings its root along. */
export function dragZone<T extends Zone>(z: T, grab: ZoneGrab, dk: number, dv: number): T {
  const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));
  switch (grab) {
    case "key_lo":
      return { ...z, key_lo: clamp(z.key_lo + dk, 0, z.key_hi) };
    case "key_hi":
      return { ...z, key_hi: clamp(z.key_hi + dk, z.key_lo, 127) };
    case "vel_lo":
      return { ...z, vel_lo: clamp(z.vel_lo + dv, 0, z.vel_hi) };
    case "vel_hi":
      return { ...z, vel_hi: clamp(z.vel_hi + dv, z.vel_lo, 127) };
    default: {
      const k = clamp(dk, -z.key_lo, 127 - z.key_hi);
      const v = clamp(dv, -z.vel_lo, 127 - z.vel_hi);
      return { ...z, key_lo: z.key_lo + k, key_hi: z.key_hi + k, vel_lo: z.vel_lo + v, vel_hi: z.vel_hi + v, root: clamp(z.root + k, 0, 127) };
    }
  }
}

export type WaveHandle = "start" | "end" | "loop_start" | "loop_end";

/** A zone's points with its "end of file" zeros filled in. */
export function pointsOf(z: ZoneView) {
  const end = z.end === 0 ? z.frames : Math.min(z.end, z.frames);
  return { start: Math.min(z.start, end), end, loop_start: Math.min(z.loop_start, end), loop_end: z.loop_end === 0 ? end : Math.min(z.loop_end, end) };
}

/** Moves a handle to a frame, keeping start before end and the loop inside them. */
export function moveHandle<T extends ZoneView>(z: T, h: WaveHandle, frame: number): T {
  const p = pointsOf(z);
  const f = Math.max(0, Math.min(z.frames, Math.round(frame)));
  const min = 2;
  switch (h) {
    case "start": {
      const start = Math.min(f, p.end - min);
      return { ...z, start, loop_start: z.loop_on ? Math.max(p.loop_start, start) : z.loop_start };
    }
    case "end": {
      const end = Math.max(f, p.start + min);
      return { ...z, end, loop_end: z.loop_on ? Math.min(p.loop_end, end) : z.loop_end };
    }
    case "loop_start":
      return { ...z, loop_start: Math.max(p.start, Math.min(f, p.loop_end - min)), loop_end: p.loop_end };
    default:
      return { ...z, loop_end: Math.min(p.end, Math.max(f, p.loop_start + min)), loop_start: p.loop_start };
  }
}

/** The view range after zooming by `factor` (under 1 is in) around a frame. */
export function zoomed(from: number, to: number, frames: number, at: number, factor: number): [number, number] {
  const span = Math.max(32, Math.min(frames, (to - from) * factor));
  const t = (at - from) / Math.max(1, to - from);
  let a = at - t * span;
  a = Math.max(0, Math.min(frames - span, a));
  return [Math.round(a), Math.round(a + span)];
}

/** What can be picked up by the sampler: audio files and SFZ files. */
export const isSfzPath = (p: string) => /\.sfz$/i.test(p);

/** Memory for the panel's readout: small amounts in MB, so one sample doesn't read as nothing. */
export function memorySize(bytes: number): string {
  const mb = bytes / (1 << 20);
  return mb < 1024 ? `${mb < 10 ? mb.toFixed(1) : mb.toFixed(0)} MB` : `${(mb / 1024).toFixed(2)} GB`;
}
