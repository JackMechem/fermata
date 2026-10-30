// The Linear Phase EQ's parameters (crates/engine/src/fx/linear_phase_eq.rs) as Channel EQ
// settings, so its panel can reuse the Channel EQ editor: the same eight bands, each a set of
// plug-in parameters.
import type { BandKind, ChannelEq, EqBand } from "./api";

export const LINEAR_PHASE_EQ = "builtin:linear-phase-eq";

/** The band fields that are parameters. */
export type BandField = "on" | "freq" | "gain" | "q" | "slope";

const FIELD_OFFSET: Record<BandField, number> = { on: 0, freq: 1, gain: 2, q: 3, slope: 4 };
const KINDS: BandKind[] = ["low_cut", "low_shelf", "bell", "bell", "bell", "bell", "high_shelf", "high_cut"];
const SLOPES = [12, 24, 48];

/** The output level (dB) and the Quality (0 Low, 1 Medium, 2 High). */
export const OUTPUT = 64;
export const QUALITY = 65;
export const QUALITY_LABELS = ["Low", "Medium", "High"];

/** The parameter id of a band's field. */
export const paramId = (band: number, field: BandField) => band * 8 + FIELD_OFFSET[field];

export const isCut = (band: number) => band === 0 || band === KINDS.length - 1;

/** The fields a band has: the cut filters have a slope instead of a gain. */
export function bandFields(band: number): BandField[] {
  return isCut(band) ? ["on", "freq", "q", "slope"] : ["on", "freq", "gain", "q"];
}

/** The settings the parameter values describe (missing values read as 0). */
export function eqFromValues(values: Record<number, number>): ChannelEq {
  const v = (id: number) => values[id] ?? 0;
  const bands: EqBand[] = KINDS.map((kind, b) => ({
    kind,
    on: v(paramId(b, "on")) >= 0.5,
    freq: v(paramId(b, "freq")),
    gain: isCut(b) ? 0 : v(paramId(b, "gain")),
    q: v(paramId(b, "q")),
    slope: isCut(b) ? SLOPES[Math.min(2, Math.max(0, Math.round(v(paramId(b, "slope")))))] : 24,
  }));
  return { on: true, bands, output_db: v(OUTPUT) };
}

/** A band field's value as a parameter value. */
function fieldValue(band: EqBand, field: BandField): number {
  switch (field) {
    case "on":
      return band.on ? 1 : 0;
    case "slope": {
      const i = SLOPES.indexOf(band.slope);
      return i < 0 ? 1 : i;
    }
    default:
      return band[field];
  }
}

/** The parameters that differ between two settings, as [id, value] pairs. */
export function changedParams(from: ChannelEq, to: ChannelEq): [number, number][] {
  const out: [number, number][] = [];
  to.bands.forEach((band, b) => {
    for (const f of bandFields(b)) {
      const v = fieldValue(band, f);
      if (!from.bands[b] || fieldValue(from.bands[b], f) !== v) out.push([paramId(b, f), v]);
    }
  });
  if (to.output_db !== from.output_db) out.push([OUTPUT, to.output_db]);
  return out;
}

/** The kernel's latency in frames for a Quality at a sample rate (half the kernel). */
export function latencyFrames(quality: number, sampleRate: number): number {
  const base = [2048, 4096, 8192][Math.min(2, Math.max(0, Math.round(quality)))];
  const ratio = Math.min(4, Math.max(1, Math.round(sampleRate / 48000)));
  const scale = Math.min(4, 2 ** Math.ceil(Math.log2(ratio)));
  return (base * scale - 2) / 2;
}
