// The Gain utility's panel (crates/engine/src/fx/gain.rs): the level, then the polarity and
// channel switches in signal order, after Logic's Gain plug-in.
import type { FxPanelProps, FxParamSpec } from "../../fx";
import { formatBalance } from "../../fxShapes";
import { FxChoice, FxGroup, FxKnob, FxToggle } from "./common";

/** Parameter ids (gain.rs). */
const G = { gain: 0, invertL: 1, invertR: 2, swap: 3, mono: 4, balance: 5 };

export function GainPanel(p: FxPanelProps) {
  const spec = (id: number): FxParamSpec => p.info.params.find((s) => s.id === id)!;
  const toggle = (id: number, label: string) => <FxToggle spec={spec(id)} value={p.values[id]} onChange={(v) => p.set(id, v)} label={label} />;
  return (
    <div className="fx-panel fxm-gain">
      <FxGroup title="Gain" className="fx-main">
        <FxKnob spec={spec(G.gain)} value={p.values[G.gain]} onChange={(v) => p.set(G.gain, v)} onTouch={(on) => p.touch(G.gain, on)} size={70} />
      </FxGroup>
      <FxGroup title="Phase Invert">
        <div className="fx-stack">
          {toggle(G.invertL, "Left")}
          {toggle(G.invertR, "Right")}
        </div>
      </FxGroup>
      <FxGroup title="Channels">
        <div className="fx-stack">
          {toggle(G.swap, "Swap L/R")}
          <FxChoice spec={spec(G.mono)} value={p.values[G.mono]} onChange={(v) => p.set(G.mono, v)} />
        </div>
        <FxKnob
          spec={spec(G.balance)}
          value={p.values[G.balance]}
          onChange={(v) => p.set(G.balance, v)}
          onTouch={(on) => p.touch(G.balance, on)}
          format={(_, v) => formatBalance(v)}
        />
      </FxGroup>
    </div>
  );
}
