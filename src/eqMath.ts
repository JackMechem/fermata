// Frequency response of the Channel EQ, mirroring crates/engine/src/eq.rs so the curves on
// screen match what the engine actually does.
import type { ChannelEq, EqBand } from "./api";

type Sec = [number, number, number, number, number]; // b0 b1 b2 a1 a2

function norm(b0: number, b1: number, b2: number, a0: number, a1: number, a2: number): Sec {
  return [b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0];
}

function bell(sr: number, f: number, q: number, db: number): Sec {
  const a = Math.pow(10, db / 40);
  const w = (2 * Math.PI * f) / sr;
  const alpha = Math.sin(w) / (2 * q);
  const c = Math.cos(w);
  return norm(1 + alpha * a, -2 * c, 1 - alpha * a, 1 + alpha / a, -2 * c, 1 - alpha / a);
}

function shelf(sr: number, f: number, q: number, db: number, high: boolean): Sec {
  const a = Math.pow(10, db / 40);
  const w = (2 * Math.PI * f) / sr;
  const c = Math.cos(w);
  const sa = 2 * Math.sqrt(a) * (Math.sin(w) / (2 * q));
  return high
    ? norm(a * (a + 1 + (a - 1) * c + sa), -2 * a * (a - 1 + (a + 1) * c), a * (a + 1 + (a - 1) * c - sa), a + 1 - (a - 1) * c + sa, 2 * (a - 1 - (a + 1) * c), a + 1 - (a - 1) * c - sa)
    : norm(a * (a + 1 - (a - 1) * c + sa), 2 * a * (a - 1 - (a + 1) * c), a * (a + 1 - (a - 1) * c - sa), a + 1 + (a - 1) * c + sa, -2 * (a - 1 + (a + 1) * c), a + 1 + (a - 1) * c - sa);
}

function pass(sr: number, f: number, q: number, hp: boolean): Sec {
  const w = (2 * Math.PI * f) / sr;
  const c = Math.cos(w);
  const alpha = Math.sin(w) / (2 * q);
  return hp ? norm((1 + c) / 2, -(1 + c), (1 + c) / 2, 1 + alpha, -2 * c, 1 - alpha) : norm((1 - c) / 2, 1 - c, (1 - c) / 2, 1 + alpha, -2 * c, 1 - alpha);
}

export function bandSections(b: EqBand, sr: number): Sec[] {
  if (!b.on) return [];
  const f = Math.min(sr * 0.49, Math.max(10, b.freq));
  const q = Math.min(18, Math.max(0.1, b.q));
  switch (b.kind) {
    case "bell":
      return Math.abs(b.gain) >= 0.01 ? [bell(sr, f, q, b.gain)] : [];
    case "low_shelf":
      return Math.abs(b.gain) >= 0.01 ? [shelf(sr, f, q, b.gain, false)] : [];
    case "high_shelf":
      return Math.abs(b.gain) >= 0.01 ? [shelf(sr, f, q, b.gain, true)] : [];
    default: {
      const order = b.slope <= 12 ? 2 : b.slope <= 24 ? 4 : 8;
      const out: Sec[] = [];
      for (let k = 1; k <= order / 2; k++) {
        const bq = 1 / (2 * Math.cos((Math.PI * (2 * k - 1)) / (2 * order)));
        out.push(pass(sr, f, order === 2 ? q : bq, b.kind === "low_cut"));
      }
      return out;
    }
  }
}

function secDb(s: Sec, w: number): number {
  const [b0, b1, b2, a1, a2] = s;
  const c1 = Math.cos(w), s1 = Math.sin(w), c2 = Math.cos(2 * w), s2 = Math.sin(2 * w);
  const nr = b0 + b1 * c1 + b2 * c2, ni = -(b1 * s1 + b2 * s2);
  const dr = 1 + a1 * c1 + a2 * c2, di = -(a1 * s1 + a2 * s2);
  return 10 * Math.log10((nr * nr + ni * ni) / (dr * dr + di * di) || 1e-12);
}

/** Response in dB of the given sections at each frequency. */
export function responseDb(secs: Sec[], freqs: number[], sr: number): number[] {
  return freqs.map((f) => {
    const w = (2 * Math.PI * f) / sr;
    return secs.reduce((acc, s) => acc + secDb(s, w), 0);
  });
}

export function eqSections(eq: ChannelEq, sr: number): Sec[] {
  return eq.on ? eq.bands.flatMap((b) => bandSections(b, sr)) : [];
}

export function isFlat(eq: ChannelEq): boolean {
  return eqSections(eq, 48000).length === 0 && Math.abs(eq.output_db) < 0.01;
}

/** Logarithmically spaced frequencies between 20 Hz and 20 kHz. */
export function logFreqs(n: number): number[] {
  const lo = Math.log(20), hi = Math.log(20000);
  return Array.from({ length: n }, (_, i) => Math.exp(lo + ((hi - lo) * i) / (n - 1)));
}

export const BAND_COLORS = ["#e5483a", "#ef8a2f", "#f2c230", "#7cc84a", "#35b8a8", "#3d8fe0", "#8a63e0", "#d24ca3"];

export function formatFreq(f: number): string {
  return f >= 1000 ? `${(f / 1000).toFixed(f >= 10000 ? 1 : 2)} kHz` : `${Math.round(f)} Hz`;
}

export function defaultEq(): ChannelEq {
  const b = (kind: EqBand["kind"], on: boolean, freq: number, q: number): EqBand => ({ kind, on, freq, gain: 0, q, slope: 24 });
  return {
    on: true,
    output_db: 0,
    bands: [
      b("low_cut", false, 30, 0.71),
      b("low_shelf", true, 80, 0.71),
      b("bell", true, 160, 1.1),
      b("bell", true, 500, 1.1),
      b("bell", true, 1500, 1.1),
      b("bell", true, 5000, 1.1),
      b("high_shelf", true, 10000, 0.71),
      b("high_cut", false, 18000, 0.71),
    ],
  };
}
