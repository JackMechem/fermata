// The Noise Gate's panel (crates/engine/src/fx/gate.rs): threshold and reduction up front, the
// envelope times beside them and the side-chain filter on the right, as in Logic.
import type { FxPanelProps } from "../../fx";
import { FxGroup, FxKnob, FxMeter, FxToggle } from "./common";

// Parameter ids, as in gate.rs.
const THRESHOLD = 0;
const REDUCTION = 1;
const ATTACK = 2;
const HOLD = 3;
const RELEASE = 4;
const HYSTERESIS = 5;
const LOOKAHEAD = 6;
const HIGH_CUT = 7;
const LOW_CUT = 8;
const MONITOR = 9;

export function GatePanel({ info, values, set, touch, meters }: FxPanelProps) {
  const spec = (id: number) => info.params.find((p) => p.id === id)!;
  const knob = (id: number, size?: number) => <FxKnob spec={spec(id)} value={values[id]} onChange={(v) => set(id, v)} onTouch={(on) => touch(id, on)} size={size} />;
  const [input, key, reduction] = [meters[0] ?? -100, meters[1] ?? -100, meters[2] ?? 0];
  // Open when the gain is within a decibel of unity (or there is nothing to reduce).
  const open = reduction < 1 || values[REDUCTION] > -1;
  return (
    <div className="fx-panel fx-gate">
      <FxGroup className="fx-main">
        {knob(THRESHOLD, 70)}
        {knob(REDUCTION, 70)}
      </FxGroup>
      <FxGroup title="Envelope">
        {knob(ATTACK)}
        {knob(HOLD)}
        {knob(RELEASE)}
        {knob(HYSTERESIS)}
        {knob(LOOKAHEAD)}
      </FxGroup>
      <FxGroup title="Side Chain Filter">
        {knob(HIGH_CUT)}
        {knob(LOW_CUT)}
        <FxToggle spec={spec(MONITOR)} value={values[MONITOR]} onChange={(v) => set(MONITOR, v)} />
      </FxGroup>
      <div className="fx-meters">
        <FxMeter label="Input" value={input} />
        <FxMeter label="Key" value={key} marker={values[THRESHOLD]} />
        <FxMeter label="Gain" value={reduction} kind="reduction" max={Math.max(12, Math.min(60, -values[REDUCTION]))} />
        <div className={"fx-led" + (open ? " on" : "")} title={open ? "Open" : "Closed"}>
          {open ? "Open" : "Closed"}
        </div>
      </div>
    </div>
  );
}
