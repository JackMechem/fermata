// Waveform overviews and how they're drawn. Each clip's overview is fetched once as signed
// (min, max) pairs per 32 frames and kept as a pyramid: each level above halves the resolution,
// so drawing reads about one value per pixel whatever the zoom.
import { invoke } from "@tauri-apps/api/core";
import type { Id } from "./api";
import { getState, setState } from "./store";

export type WaveStyle = "smooth" | "mirrored" | "rectified" | "blocks";

export const WAVE_STYLES: { id: WaveStyle; name: string; description: string }[] = [
  { id: "smooth", name: "Smooth", description: "The true shape, above and below the centre line" },
  { id: "mirrored", name: "Mirrored", description: "Loudness, mirrored around the centre line" },
  { id: "rectified", name: "Rectified", description: "Loudness, rising from the bottom of the region" },
  { id: "blocks", name: "Blocks", description: "One bar per bucket, the classic overview" },
];

export interface WaveLevel {
  /** Lowest sample per bucket, or null when only peak levels are known (shown as -max). */
  min: ArrayLike<number> | null;
  /** Highest sample per bucket (or the absolute peak when `min` is null). */
  max: ArrayLike<number>;
}

export interface Wave {
  frames: number;
  /** Frames per bucket at level 0; level i uses bucket * 2^i. */
  bucket: number;
  levels: WaveLevel[];
}

/** Waveform overviews by clip id. */
export const waveCache = new Map<Id, Wave>();

/** Clips on their way, with what to call when each arrives (several views may wait). */
const pending = new Map<Id, (() => void)[]>();

/** Fetches the clip's overview unless it's cached or on its way; `done` runs once it arrives. */
export function ensureWave(clip: Id, done: () => void) {
  if (waveCache.has(clip)) return;
  const waiting = pending.get(clip);
  if (waiting) {
    if (!waiting.includes(done)) waiting.push(done);
    return;
  }
  pending.set(clip, [done]);
  invoke<ArrayBuffer>("clip_waveform", { id: clip })
    .then((buf) => {
      if (buf.byteLength >= 8) {
        waveCache.set(clip, parseWave(buf));
        pending.get(clip)?.forEach((f) => f());
      }
    })
    .catch(() => {})
    .finally(() => pending.delete(clip));
}

function parseWave(buf: ArrayBuffer): Wave {
  const head = new DataView(buf, 0, 8);
  const frames = head.getUint32(0, true);
  const bucket = head.getUint32(4, true);
  const pairs = new Int16Array(buf.slice(8));
  const n = pairs.length >> 1;
  const min = new Float32Array(n);
  const max = new Float32Array(n);
  for (let i = 0; i < n; i++) {
    min[i] = pairs[2 * i] / 32767;
    max[i] = pairs[2 * i + 1] / 32767;
  }
  const levels: WaveLevel[] = [{ min, max }];
  while (levels[levels.length - 1].max.length > 64) {
    const prev = levels[levels.length - 1];
    const pmin = prev.min as Float32Array;
    const pmax = prev.max as Float32Array;
    const len = Math.ceil(pmax.length / 2);
    const nmin = new Float32Array(len);
    const nmax = new Float32Array(len);
    for (let i = 0; i < len; i++) {
      const j = Math.min(2 * i + 1, pmax.length - 1);
      nmin[i] = Math.min(pmin[2 * i], pmin[j]);
      nmax[i] = Math.max(pmax[2 * i], pmax[j]);
    }
    levels.push({ min: nmin, max: nmax });
  }
  return { frames, bucket, levels };
}

// ---- the style setting ----------------------------------------------------------------------

const KEY = "fermata.waveStyle";

export function savedWaveStyle(): WaveStyle {
  try {
    const v = localStorage.getItem(KEY);
    return WAVE_STYLES.some((s) => s.id === v) ? (v as WaveStyle) : "smooth";
  } catch {
    return "smooth";
  }
}

export function applyWaveStyle(style: WaveStyle) {
  setState({ waveStyle: style });
  try {
    localStorage.setItem(KEY, style);
  } catch {
    // Storage can be unavailable; the style still applies for this session.
  }
}

// ---- drawing ----------------------------------------------------------------------------------

/**
 * Draws `wave` from frame `offset` into the box at x (width w, from `top`, height h) at `pps`
 * pixels per frame, only between clipX0 and clipX1. `scale` multiplies the amplitude (region gain,
 * waveform zoom); what goes past full height is cut off at the box edge.
 */
export function drawWave(
  ctx: CanvasRenderingContext2D,
  wave: Wave,
  offset: number,
  x: number,
  w: number,
  top: number,
  h: number,
  pps: number,
  color: string,
  clipX0: number,
  clipX1: number,
  style: WaveStyle = getState().waveStyle,
  scale = 1,
) {
  const framesPerPx = 1 / pps;
  // The coarsest level whose buckets are still at most one pixel wide.
  const li = Math.max(0, Math.min(wave.levels.length - 1, Math.floor(Math.log2(framesPerPx / wave.bucket))));
  const { min, max } = wave.levels[li];
  const bucket = wave.bucket * 2 ** li;
  const n = max.length;
  const x0 = Math.max(Math.floor(x), Math.floor(clipX0));
  const x1 = Math.min(Math.ceil(x + w), Math.ceil(clipX1));
  if (x1 <= x0 || n === 0) return;
  const mid = top + h / 2;
  const half = h / 2 - 1;
  ctx.fillStyle = color;

  if (style === "blocks") {
    // One rect per pixel column, each the peak of the buckets under it.
    ctx.beginPath();
    for (let px = x0; px < x1; px++) {
      const f0 = offset + (px - x) / pps;
      const f1 = offset + (px + 1 - x) / pps;
      let b = Math.floor(f0 / bucket);
      const b1 = Math.max(b + 1, Math.ceil(f1 / bucket));
      let m = 0;
      for (; b < b1 && b < n; b++) {
        if (b < 0) continue;
        m = Math.max(m, Math.abs(max[b]), min ? Math.abs(min[b]) : 0);
      }
      const a = Math.max(0.25, Math.min(1, m * scale) * half);
      ctx.rect(px, mid - a, 1, a * 2);
    }
    ctx.fill();
    return;
  }

  // The other styles trace an outline through one value per half pixel (finer on HiDPI it
  // wouldn't show) and fill it, so edges are smooth and antialiased.
  const step = 0.5;
  const count = Math.ceil((x1 - x0) / step) + 1;
  const hi = new Float32Array(count);
  const lo = new Float32Array(count);
  // When buckets are wider than a step (deep zoom), interpolate between bucket centres.
  const coarse = bucket * pps > step;
  for (let i = 0; i < count; i++) {
    const px = x0 + i * step;
    let top_ = -Infinity;
    let bot = Infinity;
    if (coarse) {
      const u = (offset + (px - x) / pps) / bucket - 0.5;
      const b = Math.floor(u);
      const t = u - b;
      const ba = Math.min(n - 1, Math.max(0, b));
      const bb = Math.min(n - 1, Math.max(0, b + 1));
      top_ = max[ba] + (max[bb] - max[ba]) * t;
      bot = min ? min[ba] + (min[bb] - min[ba]) * t : -top_;
    } else {
      const f0 = offset + (px - x) / pps;
      const f1 = f0 + step / pps;
      let b = Math.max(0, Math.floor(f0 / bucket));
      const b1 = Math.min(n, Math.max(b + 1, Math.ceil(f1 / bucket)));
      for (; b < b1; b++) {
        if (max[b] > top_) top_ = max[b];
        const m = min ? min[b] : -max[b];
        if (m < bot) bot = m;
      }
      if (top_ === -Infinity) top_ = bot = 0;
    }
    top_ *= scale;
    bot *= scale;
    if (style === "smooth") {
      hi[i] = top_;
      lo[i] = bot;
    } else {
      const a = Math.max(Math.abs(top_), Math.abs(bot));
      hi[i] = a;
      lo[i] = -a;
    }
  }

  ctx.beginPath();
  if (style === "rectified") {
    // Rising from the bottom edge of the box.
    const base = top + h;
    const full = h - 1;
    ctx.moveTo(x0, base);
    for (let i = 0; i < count; i++) ctx.lineTo(x0 + i * step, base - Math.max(0.5, Math.min(1, hi[i]) * full));
    ctx.lineTo(x0 + (count - 1) * step, base);
  } else {
    // At least a pixel thick everywhere, so silence still shows as a hairline.
    const clamp = (v: number) => Math.max(-1, Math.min(1, v));
    for (let i = 0; i < count; i++) {
      let a = mid - clamp(hi[i]) * half;
      let b = mid - clamp(lo[i]) * half;
      if (b - a < 1) {
        const c = (a + b) / 2;
        a = c - 0.5;
        b = c + 0.5;
      }
      hi[i] = a;
      lo[i] = b;
    }
    ctx.moveTo(x0, hi[0]);
    for (let i = 1; i < count; i++) ctx.lineTo(x0 + i * step, hi[i]);
    for (let i = count - 1; i >= 0; i--) ctx.lineTo(x0 + i * step, lo[i]);
  }
  ctx.closePath();
  ctx.fill();
}
