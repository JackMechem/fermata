// The Linear Phase EQ's panel (crates/engine/src/fx/linear_phase_eq.rs): the Channel EQ's graph
// and band controls working on the effect's parameters, plus the Quality pop-up, as in Logic.
import { useContext } from "react";
import type { ChannelEq } from "../../api";
import { defaultEq } from "../../eqMath";
import type { FxPanelProps } from "../../fx";
import { FxLearnContext, learnContext } from "../../learn";
import { changedParams, eqFromValues, latencyFrames, OUTPUT, paramId, QUALITY, QUALITY_LABELS } from "../../linearPhaseEq";
import { useStore } from "../../store";
import { EqEditor, type EqControl } from "../Eq";
import { openMenuAt } from "../Menu";

const idOf = (c: EqControl) => (c.band === null ? OUTPUT : paramId(c.band, c.field));

export function LinearPhaseEqPanel(p: FxPanelProps) {
  const sr = useStore((s) => s.snap?.project.sample_rate ?? 48000);
  const fx = useContext(FxLearnContext);
  const eq = eqFromValues(p.values);
  const quality = Math.min(2, Math.max(0, Math.round(p.values[QUALITY] ?? 1)));
  const ms = ((latencyFrames(quality, sr) / sr) * 1000).toFixed(1);

  // An edit can move several parameters at once (a point dragged in the graph moves frequency
  // and gain): they go together, as one undo step.
  const apply = (next: ChannelEq) => {
    const changes = changedParams(eq, next);
    if (changes.length === 1 || !p.setMany) changes.forEach(([id, v]) => p.set(id, v));
    else if (changes.length > 1) p.setMany(changes);
  };
  const learn = (id: number) => (fx ? learnContext({ type: "plugin", track: fx.track, uid: fx.uid, plugin: "", param: id }) : undefined);

  return (
    <EqEditor
      eq={eq}
      sr={sr}
      onChange={apply}
      tools={
        <button className="eq-toggle" onClick={() => apply(defaultEq())} title="Reset every band">
          Reset
        </button>
      }
      extra={
        <>
          <span className="dim">Quality</span>
          <button
            className="fx-popup lpeq-quality"
            title="Longer filters shape low frequencies more precisely, with more latency"
            onMouseDown={(e) => openMenuAt(e.currentTarget, QUALITY_LABELS.map((label, i) => ({ label, checked: i === quality, onClick: () => p.set(QUALITY, i) })))}
            onContextMenu={learn(QUALITY)}
          >
            {QUALITY_LABELS[quality]}
          </button>
          <span className="dim lpeq-latency" title="The latency delay compensation makes up for">
            {ms} ms
          </span>
        </>
      }
      learn={(c) => learn(idOf(c))}
      onGrab={(controls, on) => controls.forEach((c) => p.touch(idOf(c), on))}
    />
  );
}
