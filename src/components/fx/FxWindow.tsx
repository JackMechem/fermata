// Panel windows for built-in effects: one floating window per open instance (like the Channel
// EQ window), hosting the effect's custom panel from FX_PANELS or the generic one.
import { useEffect, useRef, useState } from "react";
import { act, api, call, MASTER_ID, type Id } from "../../api";
import { FX_PANELS, type FxPanelProps } from "../../fx";
import { valuesOf } from "../../fxFormat";
import { closeFxPanel, findFxSlot, fxInfo, openFxPanel } from "../../fxHost";
import { getState, onTick, useStore } from "../../store";
import { FxControl, FxMeter, FxSideChain, isReductionMeter } from "./common";
import { PresetButton, type Preset } from "./Presets";

/** Every open panel window. */
export function FxWindows() {
  const open = useStore((s) => s.fxPanels);
  useEffect(() => {
    if (!open.length) return;
    // Esc closes the frontmost panel, unless it belongs to a sheet, a menu or a field.
    const k = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented || getState().dialog) return;
      const tgt = e.target as HTMLElement | null;
      if (tgt && ["INPUT", "SELECT", "TEXTAREA"].includes(tgt.tagName)) return;
      const panels = getState().fxPanels;
      if (panels.length) {
        e.preventDefault();
        closeFxPanel(panels[panels.length - 1]);
      }
    };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, [open.length]);
  return (
    <>
      {open.map((uid, i) => (
        <FxWindow key={uid} uid={uid} z={i} />
      ))}
    </>
  );
}

/** Values set from this panel within this long win over the tick's (which lag a little). */
const LOCAL_MS = 500;

function FxWindow({ uid, z }: { uid: Id; z: number }) {
  const project = useStore((s) => s.snap?.project);
  useStore((s) => s.fxList);
  const at = findFxSlot(project, uid);
  const info = fxInfo(at?.plugin.plugin_id);
  const [pos, setPos] = useState(() => ({ x: Math.max(20, window.innerWidth / 2 - 280 + z * 24), y: 120 + z * 24 }));
  const [meters, setMeters] = useState<number[]>([]);
  const [live, setLive] = useState<Record<number, number>>({});
  const local = useRef(new Map<number, { v: number; at: number }>());

  // The instance is gone (removed, undone, another project): the window goes with it.
  const gone = !at || !info;
  useEffect(() => {
    if (!at && project) closeFxPanel(uid);
  }, [at, project, uid]);

  useEffect(() => {
    call("watch_builtin", { uid, on: true });
    const off = onTick((t) => {
      const f = t.fx?.find((x) => x.uid === uid);
      if (!f) return;
      setMeters(f.meters);
      setLive(Object.fromEntries(f.values));
    });
    return () => {
      off();
      call("watch_builtin", { uid, on: false });
    };
  }, [uid]);

  if (gone) return null;
  const track = at.track;
  const trackName = track === MASTER_ID ? "Stereo Out" : project?.tracks.find((t) => t.id === track)?.name ?? "";
  const saved = valuesOf(info.params, at.plugin.params);
  // What the effect runs with (automation included), except what was just set here.
  const now = performance.now();
  const values: Record<number, number> = {};
  for (const p of info.params) {
    const l = local.current.get(p.id);
    values[p.id] = l && now - l.at < LOCAL_MS ? l.v : live[p.id] ?? saved[p.id];
  }

  const set = (id: number, value: number) => {
    local.current.set(id, { v: value, at: performance.now() });
    setLive((cur) => ({ ...cur, [id]: value }));
    act("set_builtin_param", { track, slot: at.slot, id, value });
  };
  const touch = (id: number, on: boolean) => {
    // Stereo Out isn't automated.
    if (track === MASTER_ID) return;
    const spec = info.params.find((p) => p.id === id);
    act("automation_touch", { track, param: { type: "plugin", value: { uid, param: id, name: `${at.plugin.name}: ${spec?.name ?? id}` } }, touching: on });
  };
  const props: FxPanelProps = { info, track, uid, values, set, touch, meters };
  const Panel = FX_PANELS[info.id] ?? GenericPanel;
  const norm = (vals: Record<number, number>) => JSON.stringify(info.params.map((p) => vals[p.id]));

  const startMove = (e: React.MouseEvent) => {
    if ((e.target as HTMLElement).closest("button, .fx-presets, .fx-sidechain")) return;
    const sx = e.clientX - pos.x;
    const sy = e.clientY - pos.y;
    const move = (ev: MouseEvent) => setPos({ x: Math.max(0, ev.clientX - sx), y: Math.max(0, ev.clientY - sy) });
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  return (
    <div
      className="eq-window fx-window"
      style={{ left: pos.x, top: pos.y, zIndex: 41 + z }}
      onMouseDown={(e) => {
        e.stopPropagation();
        if (getState().fxPanels[getState().fxPanels.length - 1] !== uid) openFxPanel(uid);
      }}
    >
      <div className="eq-titlebar" onMouseDown={startMove}>
        <button className={"fx-power" + (at.plugin.bypass ? "" : " on")} title={at.plugin.bypass ? "Turn on" : "Bypass"} onClick={() => api.setBypass(track, at.slot, !at.plugin.bypass)}>
          ⏻
        </button>
        <span className="eq-title">{info.name}</span>
        <span className="eq-track">{trackName}</span>
        <PresetButton
          effect={info.id}
          track={track}
          slot={at.slot}
          instance={String(uid)}
          current={norm(saved)}
          normalize={(p: Preset) => norm(valuesOf(info.params, p.values))}
        />
        {info.sidechain && <FxSideChain uid={uid} />}
        <button className="eq-close" title="Close (Esc)" onClick={() => closeFxPanel(uid)}>
          ×
        </button>
      </div>
      <div className={"fx-body" + (at.plugin.bypass ? " bypassed" : "")}>
        <Panel {...props} />
      </div>
    </div>
  );
}

/** The panel for effects without a custom one: a control per parameter and a bar per meter. */
export function GenericPanel({ info, values, set, touch, meters }: FxPanelProps) {
  const knobs = info.params.filter((p) => p.kind.type === "continuous");
  const switches = info.params.filter((p) => p.kind.type !== "continuous");
  return (
    <div className="fx-generic">
      <div className="fx-controls">
        <div className="fx-knobs">
          {knobs.map((p) => (
            <FxControl key={p.id} spec={p} value={values[p.id]} onChange={(v) => set(p.id, v)} onTouch={(on) => touch(p.id, on)} />
          ))}
        </div>
        {switches.length > 0 && (
          <div className="fx-switches">
            {switches.map((p) => (
              <FxControl key={p.id} spec={p} value={values[p.id]} onChange={(v) => set(p.id, v)} />
            ))}
          </div>
        )}
      </div>
      {info.meters.length > 0 && (
        <div className="fx-meters">
          {info.meters.map((m, i) => (
            <FxMeter key={m} label={m} value={meters[i] ?? NaN} kind={isReductionMeter(m) ? "reduction" : "level"} />
          ))}
        </div>
      )}
    </div>
  );
}
