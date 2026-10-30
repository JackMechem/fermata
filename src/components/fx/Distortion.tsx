// The Distortion and Overdrive panels (crates/engine/src/fx/{distortion,overdrive}.rs): the
// transfer curve at the current drive above the controls. The curve is drawn scaled to the box,
// since Drive is level-compensated: it shows how hard the signal clips, and how asymmetrically,
// rather than how loud it comes out.
import type { FxPanelProps, FxParamSpec } from "../../fx";
import { distortionCurve, overdriveCurve } from "../../fxShapes";
import { FxChoice, FxGroup, FxKnob } from "./common";
import { cssVar, fitCanvas, useCanvas } from "./Reverb";

const spec = (p: FxPanelProps, id: number): FxParamSpec => p.info.params.find((s) => s.id === id)!;

function knob(p: FxPanelProps, id: number, size?: number) {
  return <FxKnob spec={spec(p, id)} value={p.values[id]} onChange={(v) => p.set(id, v)} onTouch={(on) => p.touch(id, on)} size={size} />;
}

/** The waveshaper's curve for full-scale input at `driveDb`, with the straight line for reference. */
function CurveGraph({ shape, driveDb, deps }: { shape: (x: number) => number; driveDb: number; deps: unknown[] }) {
  const ref = useCanvas(
    (cv) => {
      const { ctx, w, h } = fitCanvas(cv);
      // A square plot, centred.
      const size = Math.max(20, Math.min(w, h) - 16);
      const x0 = (w - size) / 2;
      const y0 = (h - size) / 2;
      const line = cssVar("--line");
      const faint = cssVar("--faint");
      const accent = cssVar("--accent", "#4c9eff");
      const g = Math.pow(10, driveDb / 20);
      const n = Math.max(64, Math.round(size));
      const ys: number[] = [];
      for (let i = 0; i <= n; i++) ys.push(shape(g * (2 * (i / n) - 1)));
      const peak = Math.max(1e-6, ...ys.map(Math.abs));
      const px = (u: number) => x0 + ((u + 1) / 2) * size;
      const py = (v: number) => y0 + ((1 - v) / 2) * size;

      ctx.fillStyle = line;
      ctx.globalAlpha = 0.9;
      ctx.fillRect(x0, Math.round(py(0)), size, 1);
      ctx.fillRect(Math.round(px(0)), y0, 1, size);
      ctx.globalAlpha = 0.4;
      for (const v of [-1, -0.5, 0.5, 1]) {
        ctx.fillRect(x0, Math.round(py(v)), size, 1);
        ctx.fillRect(Math.round(px(v)), y0, 1, size);
      }
      // The straight line: no distortion.
      ctx.globalAlpha = 0.6;
      ctx.strokeStyle = faint;
      ctx.lineWidth = 1;
      ctx.setLineDash([3, 3]);
      ctx.beginPath();
      ctx.moveTo(px(-1), py(-1));
      ctx.lineTo(px(1), py(1));
      ctx.stroke();
      ctx.setLineDash([]);
      ctx.globalAlpha = 1;

      ctx.beginPath();
      ys.forEach((v, i) => {
        const x = px(2 * (i / n) - 1);
        const y = py(v / peak);
        if (i === 0) ctx.moveTo(x, y);
        else ctx.lineTo(x, y);
      });
      ctx.strokeStyle = accent;
      ctx.lineWidth = 2;
      ctx.lineJoin = "round";
      ctx.stroke();

      ctx.font = "9.5px Inter, system-ui, sans-serif";
      ctx.fillStyle = faint;
      ctx.fillText("In", x0 + size - 12, py(0) + 12);
      ctx.fillText("Out", px(0) + 5, y0 + 10);
    },
    [...deps, driveDb],
  );
  return (
    <div className="fxm-graph fxm-curve">
      <canvas ref={ref} />
    </div>
  );
}

/** Parameter ids (distortion.rs). */
const DI = { drive: 0, tone: 1, type: 2, output: 3, mix: 4 };

export function DistortionPanel(p: FxPanelProps) {
  const kind = Math.round(p.values[DI.type]);
  return (
    <div className="fx-panel fxm">
      <CurveGraph shape={(x) => distortionCurve(kind, x)} driveDb={p.values[DI.drive]} deps={[kind]} />
      <div className="fxm-row">
        <FxGroup title="Drive">
          {knob(p, DI.drive, 64)}
          <div className="fx-stack">
            <FxChoice spec={spec(p, DI.type)} value={p.values[DI.type]} onChange={(v) => p.set(DI.type, v)} />
          </div>
        </FxGroup>
        <FxGroup title="Tone">{knob(p, DI.tone)}</FxGroup>
        <FxGroup title="Output">
          {knob(p, DI.output)}
          {knob(p, DI.mix)}
        </FxGroup>
      </div>
    </div>
  );
}

/** Parameter ids (overdrive.rs). */
const OD = { drive: 0, tone: 1, output: 2 };

export function OverdrivePanel(p: FxPanelProps) {
  return (
    <div className="fx-panel fxm">
      <CurveGraph shape={overdriveCurve} driveDb={p.values[OD.drive]} deps={[]} />
      <div className="fxm-row">
        <FxGroup title="Overdrive" className="fxm-pedal">
          {knob(p, OD.drive, 64)}
          {knob(p, OD.tone, 64)}
          {knob(p, OD.output, 64)}
        </FxGroup>
      </div>
    </div>
  );
}
