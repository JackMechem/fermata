// The De-Esser's panel (crates/engine/src/fx/deesser.rs), after Logic's DeEsser 2: the
// detector's frequency and threshold, how much it may take away, and Split or Wide.
import type { FxPanelProps } from "../../fx";
import { FxChoice, FxGroup, FxKnob, FxMeter, FxToggle } from "./common";

// Parameter ids, as in deesser.rs.
const FREQUENCY = 0;
const THRESHOLD = 1;
const RANGE = 2;
const MODE = 3;
const SMOOTHING = 4;
const LISTEN = 5;

export function DeEsserPanel({ info, values, set, touch, meters }: FxPanelProps) {
  const spec = (id: number) => info.params.find((p) => p.id === id)!;
  const knob = (id: number, size?: number) => <FxKnob spec={spec(id)} value={values[id]} onChange={(v) => set(id, v)} onTouch={(on) => touch(id, on)} size={size} />;
  const [reduction, sibilance] = [meters[0] ?? 0, meters[1] ?? -100];
  return (
    <div className="fx-panel fx-deesser">
      <FxGroup title="Detection" className="fx-main">
        {knob(FREQUENCY, 70)}
        {knob(THRESHOLD, 70)}
      </FxGroup>
      <FxGroup title="Reduction">
        {knob(RANGE)}
        {knob(SMOOTHING)}
        <div className="fx-stack">
          <FxChoice spec={spec(MODE)} value={values[MODE]} onChange={(v) => set(MODE, v)} />
          <FxToggle spec={spec(LISTEN)} value={values[LISTEN]} onChange={(v) => set(LISTEN, v)} label="Listen to Detector" />
        </div>
      </FxGroup>
      <div className="fx-meters">
        <FxMeter label="Sibilance" value={sibilance} marker={values[THRESHOLD]} />
        <FxMeter label="Reduction" value={reduction} kind="reduction" max={Math.max(6, values[RANGE])} />
      </div>
    </div>
  );
}
