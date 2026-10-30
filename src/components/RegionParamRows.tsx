// The Region inspector's playback parameters, as in Logic: Quantize, Q-Swing, Transpose,
// Velocity, Dynamics, Gate Time and Delay for MIDI regions; Delay alone for audio regions.
// A value set here goes to every selected region (the MIDI ones to every MIDI region).
import { act, type Region, type RegionParams } from "../api";
import { Icon } from "../icons";
import { DELAY_MS_MAX, DELAY_TICKS_MAX, DYNAMICS_OPTIONS, GATE_OPTIONS, optionLabel, paramsOf, QUANTIZE_OPTIONS, shared } from "../regionParams";
import { NumberField } from "./Form";
import { openMenuAt } from "./Menu";

type Patch = Partial<RegionParams>;

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="insp-row rp-row">
      <span className="insp-label">{label}</span>
      <span className="insp-value">{children}</span>
    </div>
  );
}

/** A value chosen from a menu, drawn as text with a small chevron; "*" when the selected
 *  regions differ. */
function Choice({ value, options, onChange }: { value: number | null; options: [number, string][]; onChange: (v: number) => void }) {
  return (
    <button
      type="button"
      className="insp-popup"
      onMouseDown={(e) => {
        e.preventDefault();
        e.stopPropagation();
        openMenuAt(
          e.currentTarget,
          options.map(([v, label]) => ({ label, checked: v === value, onClick: () => onChange(v) })),
        );
      }}
    >
      {value === null ? "*" : optionLabel(options, value)}
      <Icon.chevron />
    </button>
  );
}

export function RegionParamRows({ regions }: { regions: Region[] }) {
  const ids = regions.map((r) => r.id);
  const midi = regions.filter((r) => r.content.type === "midi");
  if (!regions.length) return null;
  const set = (patch: Patch) => act("set_region_params", { ids, patch });
  // Numbers show the first region's value when the selection differs (a field can't show "*").
  const num = (rs: Region[], key: "swing" | "transpose" | "velocity" | "delay") => shared(rs, key) ?? paramsOf(rs[0])[key];
  const ms = shared(regions, "delay_ms") ?? false;
  return (
    <>
      {midi.length > 0 && (
        <>
          <Row label="Quantize">
            <Choice value={shared(midi, "quantize")} options={QUANTIZE_OPTIONS} onChange={(quantize) => set({ quantize })} />
          </Row>
          <Row label="Q-Swing">
            <NumberField value={num(midi, "swing")} step={1} min={0} max={100} width={70} onChange={(swing) => set({ swing })} />
            <span className="dim">%</span>
          </Row>
          <Row label="Transpose">
            <NumberField value={num(midi, "transpose")} step={1} min={-96} max={96} width={70} onChange={(transpose) => set({ transpose })} />
          </Row>
          <Row label="Velocity">
            <NumberField value={num(midi, "velocity")} step={1} min={-99} max={127} width={70} onChange={(velocity) => set({ velocity })} />
          </Row>
          <Row label="Dynamics">
            <Choice value={shared(midi, "dynamics")} options={DYNAMICS_OPTIONS} onChange={(dynamics) => set({ dynamics })} />
          </Row>
          <Row label="Gate Time">
            <Choice value={shared(midi, "gate")} options={GATE_OPTIONS} onChange={(gate) => set({ gate })} />
          </Row>
        </>
      )}
      <Row label="Delay">
        <NumberField
          value={num(regions, "delay")}
          step={ms ? 1 : 10}
          resolution={ms ? 0.1 : 1}
          min={ms ? -DELAY_MS_MAX : -DELAY_TICKS_MAX}
          max={ms ? DELAY_MS_MAX : DELAY_TICKS_MAX}
          width={70}
          onChange={(delay) => set({ delay })}
        />
        <Choice
          value={shared(regions, "delay_ms") === null ? null : ms ? 1 : 0}
          options={[
            [0, "ticks"],
            [1, "ms"],
          ]}
          onChange={(v) => set({ delay_ms: v === 1 })}
        />
      </Row>
    </>
  );
}
