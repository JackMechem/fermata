// The modulation effects' panels: Chorus, Flanger, Phaser and Tremolo
// (crates/engine/src/fx/{chorus,flanger,phaser,tremolo}.rs). Each shows what its LFO does over
// two cycles (left solid, right faded when Spread or Phase sets them apart, a dot where the LFO
// is now) above its controls, grouped as in Logic.
import type { FxPanelProps, FxParamSpec } from "../../fx";
import { phaserHz, sweptMs, tremoloEdge, tremoloGain, tremoloShape, unipolarSine, CHORUS_SWEEP_MS, FLANGE_OCTAVES, PHASER_OCTAVES, SQUARE } from "../../fxShapes";
import { useStore } from "../../store";
import { FxChoice, FxGroup, FxKnob, FxToggle } from "./common";
import { cssVar, fitCanvas, formatMs, noteMs, useCanvas } from "./Reverb";

const spec = (p: FxPanelProps, id: number): FxParamSpec => p.info.params.find((s) => s.id === id)!;

function knob(p: FxPanelProps, id: number, size?: number, label?: string) {
  return <FxKnob spec={spec(p, id)} value={p.values[id]} onChange={(v) => p.set(id, v)} onTouch={(on) => p.touch(id, on)} size={size} label={label} />;
}

function choice(p: FxPanelProps, id: number, label?: string) {
  return <FxChoice spec={spec(p, id)} value={p.values[id]} onChange={(v) => p.set(id, v)} label={label} />;
}

function formatHz(v: number): string {
  return v >= 1000 ? `${(v / 1000).toFixed(v >= 10000 ? 0 : 1)} kHz` : `${Math.round(v)} Hz`;
}

const clamp01 = (x: number) => Math.min(1, Math.max(0, x));

/**
 * What an LFO sweeps, over two cycles. `y` gives the swept value at a phase (cycles); the axis
 * runs from `lo` to `hi` (logarithmic with `log`), the full range the effect can sweep, so the
 * curve's height shows the depth.
 */
function SweepGraph({
  title,
  y,
  lo,
  hi,
  log,
  fmt,
  offset,
  phase,
  deps,
}: {
  title: string;
  y: (phase: number) => number;
  lo: number;
  hi: number;
  log?: boolean;
  fmt: (v: number) => string;
  /** The right channel's phase offset, in cycles. */
  offset: number;
  /** Where the LFO is now (0..1), when the effect reports it. */
  phase?: number;
  deps: unknown[];
}) {
  const ref = useCanvas(
    (cv) => {
      const { ctx, w, h } = fitCanvas(cv);
      const cycles = 2;
      const [left, right, top, bottom] = [10, 10, 22, 10];
      const gw = w - left - right;
      const gh = h - top - bottom;
      const norm = (v: number) => (log ? Math.log(v / lo) / Math.log(hi / lo) : (v - lo) / (hi - lo || 1));
      const toY = (v: number) => top + gh * (1 - clamp01(norm(v)));
      const toX = (ph: number) => left + (gw * ph) / cycles;
      const line = cssVar("--line");
      const faint = cssVar("--faint");
      const accent = cssVar("--accent", "#4c9eff");
      ctx.font = "9.5px Inter, system-ui, sans-serif";

      // A line per quarter of the range, and one per half cycle.
      ctx.fillStyle = line;
      for (let i = 0; i <= 4; i++) {
        ctx.globalAlpha = i === 0 || i === 4 ? 0.9 : 0.4;
        ctx.fillRect(left, Math.round(top + (gh * i) / 4), gw, 1);
      }
      for (let i = 1; i < cycles * 2; i++) {
        ctx.globalAlpha = i % 2 === 0 ? 0.9 : 0.4;
        ctx.fillRect(Math.round(toX(i / 2)), top, 1, gh);
      }
      ctx.globalAlpha = 1;
      ctx.fillStyle = faint;
      ctx.fillText(title, left, 14);
      ctx.textAlign = "right";
      ctx.fillText(`${fmt(lo)} – ${fmt(hi)}`, w - right, 14);
      ctx.textAlign = "left";

      const trace = (off: number, alpha: number, width: number) => {
        ctx.beginPath();
        const n = Math.max(64, Math.round(gw));
        for (let i = 0; i <= n; i++) {
          const ph = (i / n) * cycles;
          const py = toY(y(ph + off));
          if (i === 0) ctx.moveTo(toX(ph), py);
          else ctx.lineTo(toX(ph), py);
        }
        ctx.globalAlpha = alpha;
        ctx.strokeStyle = accent;
        ctx.lineWidth = width;
        ctx.lineJoin = "round";
        ctx.stroke();
        ctx.globalAlpha = 1;
      };
      const apart = Math.abs(offset) > 1e-3;
      if (apart) trace(offset, 0.4, 1.5);
      trace(0, 1, 2);
      if (phase !== undefined && Number.isFinite(phase)) {
        for (const [off, alpha] of apart ? [[offset, 0.5], [0, 1]] : [[0, 1]]) {
          ctx.globalAlpha = alpha;
          ctx.fillStyle = accent;
          ctx.beginPath();
          ctx.arc(toX(phase), toY(y(phase + off)), 3.5, 0, 2 * Math.PI);
          ctx.fill();
        }
        ctx.globalAlpha = 1;
      }
      if (apart) {
        // A key after the title: a short stroke of each trace, then its side.
        let x = left + ctx.measureText(title).width + 14;
        for (const [side, alpha] of [["L", 1], ["R", 0.4]] as const) {
          ctx.globalAlpha = alpha;
          ctx.fillStyle = accent;
          ctx.fillRect(x, 10, 12, 2);
          ctx.globalAlpha = 1;
          ctx.fillStyle = faint;
          ctx.fillText(side, x + 16, 14);
          x += 32;
        }
      }
    },
    [...deps, offset, phase, lo, hi, title],
  );
  return (
    <div className="fxm-graph">
      <canvas ref={ref} />
    </div>
  );
}

// ---- Chorus and Flanger ---------------------------------------------------------------------

/** Parameter ids (chorus.rs, flanger.rs). */
const CH = { rate: 0, intensity: 1, delay: 2, spread: 3, mix: 4 };
const FL = { rate: 0, intensity: 1, delay: 2, feedback: 3, spread: 4, mix: 5 };

export function ChorusPanel(p: FxPanelProps) {
  const [delay, depth] = [p.values[CH.delay], p.values[CH.intensity]];
  return (
    <div className="fx-panel fxm">
      <SweepGraph
        title="Delay"
        y={(ph) => sweptMs("linear", delay, depth, unipolarSine(ph))}
        lo={delay}
        hi={delay + CHORUS_SWEEP_MS}
        fmt={formatMs}
        offset={0.5 * p.values[CH.spread]}
        phase={p.meters[0]}
        deps={[delay, depth]}
      />
      <div className="fxm-row">
        <FxGroup title="Modulation">
          {knob(p, CH.rate, 56)}
          {knob(p, CH.intensity, 56)}
        </FxGroup>
        <FxGroup title="Delay">{knob(p, CH.delay)}</FxGroup>
        <FxGroup title="Output">
          {knob(p, CH.spread)}
          {knob(p, CH.mix)}
        </FxGroup>
      </div>
    </div>
  );
}

export function FlangerPanel(p: FxPanelProps) {
  const [delay, depth] = [p.values[FL.delay], p.values[FL.intensity]];
  return (
    <div className="fx-panel fxm">
      <SweepGraph
        title="Delay"
        y={(ph) => sweptMs("octaves", delay, depth, unipolarSine(ph))}
        lo={delay}
        hi={Math.min(63, delay * Math.pow(2, FLANGE_OCTAVES))}
        log
        fmt={formatMs}
        offset={0.5 * p.values[FL.spread]}
        phase={p.meters[0]}
        deps={[delay, depth]}
      />
      <div className="fxm-row">
        <FxGroup title="Modulation">
          {knob(p, FL.rate, 56)}
          {knob(p, FL.intensity, 56)}
        </FxGroup>
        <FxGroup title="Delay">
          {knob(p, FL.delay)}
          {knob(p, FL.feedback)}
        </FxGroup>
        <FxGroup title="Output">
          {knob(p, FL.spread)}
          {knob(p, FL.mix)}
        </FxGroup>
      </div>
    </div>
  );
}

// ---- Phaser ---------------------------------------------------------------------------------

/** Parameter ids (phaser.rs). */
const PH = { rate: 0, depth: 1, frequency: 2, stages: 3, feedback: 4, spread: 5, mix: 6 };

export function PhaserPanel(p: FxPanelProps) {
  const [center, depth] = [p.values[PH.frequency], p.values[PH.depth]];
  const span = Math.pow(2, PHASER_OCTAVES);
  return (
    <div className="fx-panel fxm">
      <SweepGraph
        title="Frequency"
        y={(ph) => phaserHz(center, depth, unipolarSine(ph))}
        lo={center / span}
        hi={center * span}
        log
        fmt={formatHz}
        offset={0.5 * p.values[PH.spread]}
        phase={p.meters[0]}
        deps={[center, depth]}
      />
      <div className="fxm-row">
        <FxGroup title="Modulation">
          {knob(p, PH.rate, 56)}
          {knob(p, PH.depth, 56)}
        </FxGroup>
        <FxGroup title="Filter">
          {knob(p, PH.frequency)}
          {knob(p, PH.feedback)}
          <div className="fx-stack">{choice(p, PH.stages)}</div>
        </FxGroup>
        <FxGroup title="Output">
          {knob(p, PH.spread)}
          {knob(p, PH.mix)}
        </FxGroup>
      </div>
    </div>
  );
}

// ---- Tremolo --------------------------------------------------------------------------------

/** Parameter ids (tremolo.rs). */
const TR = { rate: 0, sync: 1, note: 2, depth: 3, waveform: 4, smoothing: 5, phase: 6 };

export function TremoloPanel(p: FxPanelProps) {
  const tempo = useStore((s) => s.snap?.project.tempo ?? 120);
  const sync = p.values[TR.sync] >= 0.5;
  const note = Math.round(p.values[TR.note]);
  const rate = sync ? 1000 / noteMs(note, tempo) : p.values[TR.rate];
  const wave = Math.round(p.values[TR.waveform]);
  const depth = p.values[TR.depth];
  const edge = tremoloEdge(p.values[TR.smoothing], rate);
  return (
    <div className="fx-panel fxm">
      <SweepGraph
        title="Level"
        y={(ph) => tremoloGain(depth, tremoloShape(wave, ph, edge))}
        lo={0}
        hi={1}
        fmt={(v) => `${Math.round(v * 100)} %`}
        offset={p.values[TR.phase] / 360}
        phase={p.meters[0]}
        deps={[wave, depth, edge]}
      />
      <div className="fxm-row">
        <FxGroup title="Rate">
          <div className="fx-stack fxm-rate">
            <FxToggle spec={spec(p, TR.sync)} value={p.values[TR.sync]} onChange={(v) => p.set(TR.sync, v)} />
            {sync && (
              <>
                {choice(p, TR.note, "Note")}
                <div className="fx-value fxm-note-ms" title="One cycle at the project tempo">
                  {formatMs(noteMs(note, tempo))}
                </div>
              </>
            )}
          </div>
          {!sync && knob(p, TR.rate, 56)}
        </FxGroup>
        <FxGroup title="Shape">
          <div className="fx-stack">{choice(p, TR.waveform)}</div>
          <div className={wave === SQUARE ? "" : "fxm-off"} title={wave === SQUARE ? undefined : "Smoothing rounds the Square wave's edges"}>
            {knob(p, TR.smoothing)}
          </div>
        </FxGroup>
        <FxGroup title="Level">
          {knob(p, TR.depth, 56)}
          {knob(p, TR.phase, undefined, "Stereo Phase")}
        </FxGroup>
      </div>
    </div>
  );
}
