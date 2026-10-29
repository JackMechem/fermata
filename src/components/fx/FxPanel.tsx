// A plug-in panel's content and header controls, wherever the panel shows: docked in the bottom
// section (FxDock), floating in a window of its own (FxWindows), or anywhere else that renders
// it. Everything here works from the panel key alone (fxDock.ts), fills the box it is given and
// scrolls when the box is smaller than the panel.
import { useEffect, useRef, useState } from "react";
import { act, api, call, MASTER_ID, type Id, type Project } from "../../api";
import { FX_PANELS, type FxInfo, type FxPanelProps } from "../../fx";
import { parseKey, type PanelKey } from "../../fxDock";
import { valuesOf } from "../../fxFormat";
import { CHANNEL_EQ, findFxSlot, fxInfo } from "../../fxHost";
import { onTick, useStore } from "../../store";
import { EqPanel } from "../Eq";
import { FxControl, FxMeter, FxSideChain, isReductionMeter } from "./common";
import { PresetButton, type Preset } from "./Presets";

/** Values set from this panel within this long win over the tick's (which lag a little). */
const LOCAL_MS = 500;

const stripName = (project: Project | undefined, track: Id) => (track === MASTER_ID ? "Stereo Out" : project?.tracks.find((t) => t.id === track)?.name ?? "");

/**
 * What a panel is called: the effect (or "Channel EQ") and its strip, and the type its size is
 * remembered under. Null once the instance or strip is gone.
 */
export function usePanelTitle(key: PanelKey): { name: string; track: string; type: string } | null {
  const project = useStore((s) => s.snap?.project);
  useStore((s) => s.fxList);
  const ref = parseKey(key);
  if (!ref || !project) return null;
  if (ref.kind === "eq") {
    if (ref.track !== MASTER_ID && !project.tracks.some((t) => t.id === ref.track)) return null;
    return { name: "Channel EQ", track: stripName(project, ref.track), type: CHANNEL_EQ };
  }
  const at = findFxSlot(project, ref.uid);
  const info = fxInfo(at?.plugin.plugin_id);
  if (!at || !info) return null;
  return { name: info.name, track: stripName(project, at.track), type: info.id };
}

/** Whether a panel's instance or strip still exists in `project`. */
export function panelExists(project: Project, key: PanelKey): boolean {
  const ref = parseKey(key);
  if (!ref) return false;
  if (ref.kind === "eq") return ref.track === MASTER_ID || project.tracks.some((t) => t.id === ref.track);
  return !!findFxSlot(project, ref.uid);
}

/** The panel itself: an effect's controls and meters, or the Channel EQ editor. */
export function PanelContent({ panelKey }: { panelKey: PanelKey }) {
  const ref = parseKey(panelKey);
  if (ref?.kind === "fx") return <FxPanelContent uid={ref.uid} />;
  if (ref?.kind === "eq") return <EqPanel track={ref.track} />;
  return null;
}

/** The header controls before the name: the power button (effects only). */
export function PanelLead({ panelKey }: { panelKey: PanelKey }) {
  const ref = parseKey(panelKey);
  return ref?.kind === "fx" ? <FxPower uid={ref.uid} /> : null;
}

/** The header controls after the name: presets and the side chain (effects only; the Channel EQ has them in its toolbar). */
export function PanelTools({ panelKey }: { panelKey: PanelKey }) {
  const ref = parseKey(panelKey);
  return ref?.kind === "fx" ? <FxTools uid={ref.uid} /> : null;
}

function useFx(uid: Id): { at: NonNullable<ReturnType<typeof findFxSlot>>; info: FxInfo } | null {
  const project = useStore((s) => s.snap?.project);
  useStore((s) => s.fxList);
  const at = findFxSlot(project, uid);
  const info = fxInfo(at?.plugin.plugin_id);
  return at && info ? { at, info } : null;
}

/** The power (bypass) button. */
export function FxPower({ uid }: { uid: Id }) {
  const fx = useFx(uid);
  if (!fx) return null;
  const { at } = fx;
  return (
    <button className={"fx-power" + (at.plugin.bypass ? "" : " on")} title={at.plugin.bypass ? "Turn on" : "Bypass"} onClick={() => api.setBypass(at.track, at.slot, !at.plugin.bypass)}>
      ⏻
    </button>
  );
}

/** The preset pop-up and, for effects with a side-chain, the Side Chain pop-up. */
export function FxTools({ uid }: { uid: Id }) {
  const fx = useFx(uid);
  if (!fx) return null;
  const { at, info } = fx;
  const norm = (vals: Record<number, number>) => JSON.stringify(info.params.map((p) => vals[p.id]));
  return (
    <>
      <PresetButton
        effect={info.id}
        track={at.track}
        slot={at.slot}
        instance={String(uid)}
        current={norm(valuesOf(info.params, at.plugin.params))}
        normalize={(p: Preset) => norm(valuesOf(info.params, p.values))}
      />
      {info.sidechain && <FxSideChain uid={uid} />}
    </>
  );
}

/**
 * An effect's panel from FX_PANELS (or the generic one), fed with its parameter values and
 * meters while it is on screen.
 */
export function FxPanelContent({ uid }: { uid: Id }) {
  const fx = useFx(uid);
  const [meters, setMeters] = useState<number[]>([]);
  const [live, setLive] = useState<Record<number, number>>({});
  const local = useRef(new Map<number, { v: number; at: number }>());

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

  if (!fx) return null;
  const { at, info } = fx;
  const track = at.track;
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
  return (
    <div className={"fx-body" + (at.plugin.bypass ? " bypassed" : "")}>
      <Panel {...props} />
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

/** The header button that puts a floating panel in the bottom section: a window with its lower part filled. */
export function DockGlyph() {
  return (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3">
      <rect x="1.5" y="2" width="11" height="10" rx="1.5" />
      <rect x="1.5" y="8" width="11" height="4" rx="1" fill="currentColor" stroke="none" />
    </svg>
  );
}

/** The header button that takes a docked panel out into its own window: a window over another. */
export function FloatGlyph() {
  return (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3">
      <rect x="1.5" y="4.5" width="8" height="8" rx="1.3" />
      <path d="M5 3V2.8A1.3 1.3 0 0 1 6.3 1.5h5A1.3 1.3 0 0 1 12.5 2.8v5A1.3 1.3 0 0 1 11.2 9.1H11" />
    </svg>
  );
}
