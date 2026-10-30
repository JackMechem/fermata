// The curves the modulation, distortion and Gain panels draw: LFO shapes, the delay and
// frequency sweeps, waveshaper curves and the balance readout. Each mirrors the engine code
// named beside it (crates/engine/src/fx/*.rs), so the pictures show what is heard. Pure, tested
// in tests/fxShapes.test.ts.

/** A sine LFO at `phase` (cycles), 0..1 and at its top at phase 0 (`unipolar_sine`, lfo.rs). */
export function unipolarSine(phase: number): number {
  return 0.5 + 0.5 * Math.cos(2 * Math.PI * phase);
}

// ---- chorus, flanger, phaser ----------------------------------------------------------------

/** How far Intensity at 100 % sweeps the Chorus's delay (SWEEP_MS, chorus.rs). */
export const CHORUS_SWEEP_MS = 6;
/** How many octaves Intensity at 100 % sweeps the Flanger's delay (FLANGE_OCTAVES). */
export const FLANGE_OCTAVES = 3;
/** How far Depth at 100 % sweeps the Phaser either side of its frequency (phaser.rs). */
export const PHASER_OCTAVES = 2;

/** The delay (ms) at LFO position `u` (0..1): `swept_ms` in chorus.rs. */
export function sweptMs(sweep: "linear" | "octaves", delayMs: number, depth: number, u: number): number {
  const d = sweep === "linear" ? delayMs + depth * CHORUS_SWEEP_MS * u : delayMs * Math.pow(2, FLANGE_OCTAVES * depth * u);
  return Math.min(d, 63);
}

/** The phaser's all-pass frequency at LFO position `u` (0..1): `swept_hz` in phaser.rs. */
export function phaserHz(center: number, depth: number, u: number): number {
  return center * Math.pow(2, PHASER_OCTAVES * depth * (2 * u - 1));
}

// ---- tremolo --------------------------------------------------------------------------------

export const SINE = 0;
export const TRIANGLE = 1;
export const SQUARE = 2;

/** The square's edges as a share of the cycle, never under 2 ms each (tremolo.rs). */
export function tremoloEdge(smoothing: number, rateHz: number): number {
  return Math.min(1, Math.max(smoothing, 2 * 0.002 * rateHz));
}

/** The LFO's level (0..1) at `phase`, peaking at phase 0: `shape` in tremolo.rs. */
export function tremoloShape(waveform: number, phase: number, edge: number): number {
  const p = phase - Math.floor(phase);
  const d = Math.min(p, 1 - p);
  if (waveform === TRIANGLE) return 1 - 2 * d;
  if (waveform === SQUARE) {
    const v = Math.min(1, Math.max(-1, (1 - 4 * d) / Math.max(edge, 1e-6)));
    return 0.5 + 0.5 * Math.sin((v * Math.PI) / 2);
  }
  return unipolarSine(p);
}

/** The gain at LFO level `u` for a depth (0..1). */
export function tremoloGain(depth: number, u: number): number {
  return 1 - depth * (1 - u);
}

// ---- distortion -----------------------------------------------------------------------------

/** The Distortion's curve for a Type index (Soft, Hard, Tube, Fuzz): `curve` in distortion.rs. */
export function distortionCurve(kind: number, x: number): number {
  switch (kind) {
    case 1: {
      const a = Math.abs(x);
      return a <= 0.8 ? x : Math.sign(x) * (0.8 + 0.2 * Math.tanh((a - 0.8) / 0.2));
    }
    case 2:
      return x >= 0 ? 1 - Math.exp(-x) : (Math.exp(1.5 * x) - 1) / 1.5;
    case 3: {
      const s = Math.tanh(2 * x);
      return Math.min(1, Math.max(-1, s + 0.4 * s * s));
    }
    default:
      return Math.tanh(x);
  }
}

/** The Overdrive's asymmetric tanh (overdrive.rs). */
export function overdriveCurve(x: number): number {
  return Math.tanh(x + 0.3) - Math.tanh(0.3);
}

/** The level Drive is compensated around, −18 dBFS (COMP_REF). */
export const COMP_REF = 0.1259;

/** The output gain that keeps a −18 dBFS signal at about its level (`compensation`). */
export function compensation(shape: (x: number) => number, drive: number): number {
  const x = COMP_REF * drive;
  const out = 0.5 * (shape(x) - shape(-x));
  return out > 1e-9 ? COMP_REF / out : 1;
}

/**
 * The whole transfer curve, input to output (before the tone filter and DC blocker), for
 * drive and output in dB.
 */
export function transfer(shape: (x: number) => number, driveDb: number, outputDb: number): (x: number) => number {
  const g = Math.pow(10, driveDb / 20);
  const out = Math.pow(10, outputDb / 20) * compensation(shape, g);
  return (x) => shape(x * g) * out;
}

// ---- gain -----------------------------------------------------------------------------------

/** The Gain plug-in's balance as Logic shows a pan: "C", "L 35", "R 100". */
export function formatBalance(v: number): string {
  const n = Math.round(Math.min(1, Math.max(-1, v)) * 100);
  return n === 0 ? "C" : n < 0 ? `L ${-n}` : `R ${n}`;
}
