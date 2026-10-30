// Value formatting and control mapping for built-in effect parameters (pure, tested in
// tests/fxFormat.test.ts). Mirrors the engine's normalization in crates/engine/src/dsp/builtin.rs,
// which automation lanes use.
import type { FxParamSpec } from "./fx";

const trim = (s: string) => (s.includes(".") ? s.replace(/\.?0+$/, "") : s);

function fixed(v: number, digits: number): string {
  return trim(v.toFixed(digits));
}

/** A parameter value as the panel shows it, with its unit: "-12.5 dB", "4:1", "2.40 kHz". */
export function formatValue(spec: FxParamSpec, v: number): string {
  if (spec.kind.type === "choice") return spec.kind.labels[Math.round(v)] ?? String(Math.round(v));
  if (spec.kind.type === "toggle") return v >= 0.5 ? "On" : "Off";
  switch (spec.unit) {
    case "db":
      if (v <= -100) return "−∞ dB";
      return `${v > 0.05 ? "+" : ""}${v.toFixed(1)} dB`.replace("-", "−");
    case "ms":
      if (v >= 1000) return `${fixed(v / 1000, 2)} s`;
      return `${v < 1 ? v.toFixed(2) : v < 10 ? v.toFixed(1) : Math.round(v)} ms`;
    case "hz":
      if (v >= 1000) return `${v >= 10000 ? (v / 1000).toFixed(1) : (v / 1000).toFixed(2)} kHz`;
      return `${Math.round(v)} Hz`;
    case "ratio":
      return v >= 100 ? "∞:1" : `${fixed(v, v < 10 ? 1 : 0)}:1`;
    case "percent":
      return `${Math.round(v * 100)} %`;
    case "semitones":
      return `${v > 0.05 ? "+" : ""}${v.toFixed(1)} st`.replace("-", "−");
    case "deg":
      return `${Math.round(v)}°`;
    default:
      return fixed(v, Math.abs(v) < 10 ? 2 : 1);
  }
}

const isLog = (spec: FxParamSpec) => spec.log && spec.min > 0 && spec.max > spec.min;

/** Clamps to the range; toggles and choices land on a whole step (as the engine does). */
export function clampValue(spec: FxParamSpec, v: number): number {
  if (!Number.isFinite(v)) return spec.default;
  if (spec.kind.type === "toggle") return v >= 0.5 ? 1 : 0;
  if (spec.kind.type === "choice") return Math.min(spec.kind.labels.length - 1, Math.max(0, Math.round(v)));
  return Math.min(Math.max(spec.min, spec.max), Math.max(Math.min(spec.min, spec.max), v));
}

/** The control position (0..1) of a value: logarithmic for frequencies, times and ratios. */
export function toNorm(spec: FxParamSpec, v: number): number {
  const x = clampValue(spec, v);
  if (spec.kind.type === "choice") return x / Math.max(1, spec.kind.labels.length - 1);
  if (isLog(spec)) return Math.log(x / spec.min) / Math.log(spec.max / spec.min);
  return spec.max > spec.min ? (x - spec.min) / (spec.max - spec.min) : 0;
}

/** The value at a control position (0..1). */
export function fromNorm(spec: FxParamSpec, n: number): number {
  const u = Math.min(1, Math.max(0, n));
  if (spec.kind.type === "toggle") return u >= 0.5 ? 1 : 0;
  if (spec.kind.type === "choice") return Math.round(u * Math.max(0, spec.kind.labels.length - 1));
  if (isLog(spec)) return spec.min * Math.pow(spec.max / spec.min, u);
  return spec.min + u * (spec.max - spec.min);
}

/** Rounds a dragged value to what its readout can show, so saved values stay tidy. */
export function tidy(spec: FxParamSpec, v: number): number {
  if (spec.kind.type !== "continuous") return clampValue(spec, v);
  const a = Math.abs(v);
  let step: number;
  switch (spec.unit) {
    case "db":
    case "semitones":
      step = 0.1;
      break;
    case "percent":
      step = 0.001;
      break;
    case "hz":
      step = a >= 10000 ? 100 : a >= 1000 ? 10 : 1;
      break;
    case "ms":
      step = a >= 1000 ? 10 : a >= 10 ? 1 : a >= 1 ? 0.1 : 0.01;
      break;
    case "ratio":
      step = a >= 10 ? 1 : 0.1;
      break;
    case "deg":
      step = 1;
      break;
    default:
      step = a >= 10 ? 0.1 : 0.01;
  }
  return clampValue(spec, Math.round(v / step) * step);
}

/**
 * Reads a typed value back: "250", "2.5k", "2.5 kHz", "-6 dB", "1.2 s", "4:1", "50 %". Returns
 * null when it doesn't parse.
 */
export function parseValue(spec: FxParamSpec, text: string): number | null {
  const t = text.trim().toLowerCase().replace("−", "-");
  if (spec.kind.type === "choice") {
    const i = spec.kind.labels.findIndex((l) => l.toLowerCase() === t);
    if (i >= 0) return i;
  }
  if (spec.kind.type === "toggle") {
    if (t === "on") return 1;
    if (t === "off") return 0;
  }
  const m = t.match(/^(-?\d*\.?\d+)\s*(k|khz|hz|ms|s|db|%|:1|st|°|deg)?$/);
  if (!m) return null;
  let v = parseFloat(m[1]);
  const suffix = m[2] ?? "";
  if (suffix === "k" || suffix === "khz") v *= 1000;
  if (suffix === "s" && spec.unit === "ms") v *= 1000;
  if (spec.unit === "percent") v /= 100;
  return clampValue(spec, v);
}

/** Every parameter's value: the saved ones, defaults for the rest. */
export function valuesOf(params: FxParamSpec[], saved: [number, number][] | undefined): Record<number, number> {
  const out: Record<number, number> = {};
  for (const p of params) out[p.id] = p.default;
  for (const [id, v] of saved ?? []) if (id in out) out[id] = v;
  return out;
}
