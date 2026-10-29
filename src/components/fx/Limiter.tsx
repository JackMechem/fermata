// The Limiter's panel, after Logic's Limiter: input, reduction and output meters on the left,
// Gain and Out Ceiling as the two big controls, then Release, Lookahead, the mode and True
// Peak. Parameter ids mirror crates/engine/src/fx/limiter.rs.
import type { FxPanelProps } from "../../fx";
import { GrBar, LevelBar, ParamChoice, ParamKnob, ParamToggle } from "./Compressor";

const P = { gain: 0, ceiling: 1, release: 2, lookahead: 3, mode: 4, truePeak: 5 } as const;

const MODE_NOTES = [
  "Transparent brickwall: nothing passes the ceiling",
  "Soft knee from 6 dB under the ceiling, program-dependent release",
  "Rounds peaks with a soft saturator before limiting, for loudness",
];

export function LimiterPanel(props: FxPanelProps) {
  const { info, values, meters } = props;
  const v = (id: number) => values[id] ?? info.params.find((p) => p.id === id)?.default ?? 0;
  const [mIn, mOut, mGr] = [meters[0] ?? -120, meters[1] ?? -120, meters[2] ?? 0];
  const tp = v(P.truePeak) >= 0.5;
  const mode = Math.round(v(P.mode));
  const modeSpec = info.params.find((p) => p.id === P.mode);
  const modeName = modeSpec?.kind.type === "choice" ? modeSpec.kind.labels[mode] : "";
  return (
    <div className="dyn-panel dyn-lim">
      <div className="dyn-lim-body">
        <div className="dyn-lim-meters">
          <LevelBar db={mIn} label="In" length={150} grow />
          <LevelBar db={mOut} label={tp ? "Out TP" : "Out"} length={150} grow />
        </div>
        <div className="dyn-lim-controls">
          <div className="dyn-lim-gr">
            <span className="dyn-field-label">Reduction</span>
            <GrBar gr={mGr} top={20} marks={[0, 1, 2, 3, 6, 10, 20]} />
          </div>
          <div className="dyn-big">
            <ParamKnob props={props} id={P.gain} size={74} />
            <ParamKnob props={props} id={P.ceiling} size={74} title="The highest level the output reaches (dBTP with True Peak on)" />
          </div>
          <div className="dyn-small">
            <ParamKnob props={props} id={P.release} />
            <ParamKnob props={props} id={P.lookahead} title="Longer lookahead ramps the gain down more gently; it adds this much latency" />
            <div className="dyn-field">
              <span className="dyn-field-label">Mode</span>
              <ParamChoice props={props} id={P.mode} vertical />
            </div>
            <div className="dyn-field">
              <span className="dyn-field-label">Detection</span>
              <ParamToggle props={props} id={P.truePeak} label="True Peak" title="4× oversampled peak detection, so peaks between samples stay under the ceiling too" />
            </div>
          </div>
        </div>
      </div>
      <div className="dyn-footer">
        <span className="dyn-circuit-name">{modeName}</span>
        <span className="dyn-circuit-about">{MODE_NOTES[mode] ?? ""}</span>
        <span className="dyn-footer-gr" title="Gain reduction">
          {mGr > 0.05 ? `−${mGr.toFixed(1)} dB` : "0.0 dB"}
        </span>
      </div>
    </div>
  );
}

export default LimiterPanel;
