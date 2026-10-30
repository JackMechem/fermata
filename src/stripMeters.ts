// The channel strip's clip indicator (the peak readout over the meter) and gain-reduction
// meter: pure helpers, tested in tests/stripMeters.test.ts. The engine holds the peaks
// (crates/engine/src/session/strip_meters.rs) and sends them with each tick.

/** A held peak (linear) over full scale, 0 dBFS, is a clip: the readout and the top of the
 *  meter go red until the indicator is reset. */
export function isClip(peak: number): boolean {
  return peak > 1;
}

/** The peak readout: the held peak in dBFS to a tenth, "-∞" before anything has played. */
export function peakText(peak: number): string {
  if (!(peak > 0) || !Number.isFinite(peak)) return "-∞";
  const db = 20 * Math.log10(peak);
  if (db <= -96) return "-∞";
  const r = Math.round(db * 10) / 10;
  return (r > 0 ? "+" : "") + (r === 0 ? 0 : r).toFixed(1);
}

/** What clicking a clip indicator resets: Alt resets every strip (null), as in Logic. */
export function clipResetTargets(id: number, alt: boolean): number[] | null {
  return alt ? null : [id];
}

/** The meter names under which built-in dynamics effects report their gain reduction (the
 *  engine picks them out the same way, in dsp/builtin.rs). */
const GR_METERS = ["Gain Reduction", "Reduction"];

/** Whether an effect with these meters reduces gain, and so shows on the strip's GR meter. */
export function reducesGain(meters: string[]): boolean {
  return meters.some((m) => GR_METERS.includes(m));
}

/** The gain-reduction meter's full length, in dB. */
export const GR_RANGE = 30;

/** Where a gain reduction (dB) reaches along the meter, 0..1. Square-root spaced, so the few
 *  dB of a gentle compressor still show. */
export function grFraction(db: number): number {
  if (!(db > 0)) return 0;
  return Math.min(1, Math.sqrt(db / GR_RANGE));
}

/** How fast the shown gain reduction falls back when the effect releases, in dB per second. */
export const GR_FALL = 40;

/** The gain reduction to show next: a new report as soon as it exceeds what is shown,
 *  otherwise falling back by `GR_FALL` per second, so a short peak stays visible. */
export function nextGr(shown: number, reported: number | undefined, dt: number): number {
  const r = reported !== undefined && reported > 0 ? reported : 0;
  return Math.max(r, shown - GR_FALL * dt, 0);
}
