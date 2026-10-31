// The panel a built-in instrument gets without any custom code: its parameters in the sections
// its spec lists, and a small keyboard to try it with. A model can have a layout of its own by
// registering it in FX_PANELS (src/fx.ts), as the effects do.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { api, call, type Id } from "../../api";
import type { FxPanelProps } from "../../fx";
import { getState } from "../../store";
import { savedLayout, saveLayout, type PanelPlace, type SectionLayout } from "../../instLayout";
import { keyboardKeys } from "../../pianoKeys";
import { FxControl, FxGroup, FxMeter, isReductionMeter } from "./common";

export function InstrumentPanel({ info, track, values, set, touch, meters }: FxPanelProps) {
  const byId = new Map(info.params.map((p) => [p.id, p]));
  const ref = useRef<HTMLDivElement>(null);
  const [place, setPlace] = useState<PanelPlace>("docked");
  const [layout, setLayout] = useState<SectionLayout>(() => savedLayout("docked"));
  // Floating or docking mounts the panel afresh, so this only needs to look once.
  useLayoutEffect(() => {
    const here: PanelPlace = ref.current?.closest(".fx-window") ? "floating" : "docked";
    setPlace(here);
    setLayout(savedLayout(here));
  }, []);
  const choose = (l: SectionLayout) => {
    setLayout(l);
    saveLayout(place, l);
  };
  return (
    <div ref={ref} className={"fx-panel inst-panel " + layout}>
      <div className="inst-sections" onWheel={wheelAcross}>
        {(info.sections ?? []).map((s) => (
          <FxGroup key={s.title} title={s.title} className={"inst-section " + (s.params.length > 4 ? "two-rows" : "one-row")}>
            {s.params.map((id) => {
              const p = byId.get(id);
              return p ? <FxControl key={id} spec={p} value={values[id]} onChange={(v) => set(id, v)} onTouch={(on) => touch(id, on)} /> : null;
            })}
          </FxGroup>
        ))}
        {info.meters.length > 0 && (
          <FxGroup title="Meters" className="inst-section">
            {info.meters.map((m, i) => (
              <FxMeter key={m} label={m} value={meters[i] ?? NaN} kind={isReductionMeter(m) ? "reduction" : "level"} />
            ))}
          </FxGroup>
        )}
      </div>
      <div className="inst-footer">
        <div className="fx-segments inst-layout" title="How the sections are laid out">
          <button className={layout === "row" ? "on" : ""} onClick={() => choose("row")}>
            Row
          </button>
          <button className={layout === "grid" ? "on" : ""} onClick={() => choose("grid")}>
            Grid
          </button>
        </div>
        <PanelKeyboard track={track} />
      </div>
    </div>
  );
}

/** In a row wider than the panel, a wheel moves along the sections. */
function wheelAcross(e: React.WheelEvent<HTMLDivElement>) {
  const el = e.currentTarget;
  if (e.deltaX === 0 && el.scrollWidth > el.clientWidth) el.scrollLeft += e.deltaY;
}

/** Two octaves to try a sound with, played through live MIDI on the instrument's track. */
function PanelKeyboard({ track }: { track: Id }) {
  const [base, setBase] = useState(48);
  const [down, setDown] = useState<number | null>(null);
  const held = useRef<number | null>(null);
  const send = (status: number, note: number) => call("midi_event", { data: [status, note, status === 0x90 ? 100 : 0] });
  const off = () => {
    if (held.current !== null) send(0x80, held.current);
    held.current = null;
    setDown(null);
  };
  const on = (note: number) => {
    if (held.current === note) return;
    const previous = held.current;
    held.current = note;
    setDown(note);
    // Live MIDI plays the selected track.
    if (getState().snap?.selected !== track) api.selectTrack(track);
    send(0x90, note);
    // Note on first, so sliding across the keys plays legato.
    if (previous !== null) send(0x80, previous);
  };
  // A key mustn't stay down because the panel closed under the pointer.
  useEffect(() => off, []); // eslint-disable-line react-hooks/exhaustive-deps

  const keys = keyboardKeys(base, 25);
  return (
    <div className="inst-keyboard" onPointerUp={off} onPointerLeave={off} onPointerCancel={off}>
      <button className="fx-toggle" title="An octave down" disabled={base <= 12} onClick={() => setBase(base - 12)}>
        −
      </button>
      <div className="inst-keys" style={{ width: `${keys.whites * 22}px` }}>
        {keys.list.map((k) => (
          <div
            key={k.note}
            className={"inst-key " + (k.black ? "black" : "white") + (down === k.note ? " down" : "")}
            style={{ left: `${(k.at / keys.whites) * 100}%`, width: `${(k.black ? 0.62 : 1) * (100 / keys.whites)}%` }}
            onPointerDown={(e) => {
              e.preventDefault();
              // Pointer events go to the key under the pointer as it slides, with none captured.
              (e.target as Element).releasePointerCapture?.(e.pointerId);
              on(k.note);
            }}
            onPointerEnter={(e) => {
              if (e.buttons & 1) on(k.note);
            }}
            title={k.name}
          />
        ))}
      </div>
      <button className="fx-toggle" title="An octave up" disabled={base >= 84} onClick={() => setBase(base + 12)}>
        +
      </button>
    </div>
  );
}
