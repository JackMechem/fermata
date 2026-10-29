// Logic's Signature and Tempo global tracks, between the ruler and the tracks (View › Show
// Global Tracks, G). They scroll and zoom with the arrange area.
//
// Tempo lane: click to add a tempo change at the snapped position (dragging it right away),
// drag a point up or down to change its tempo (Shift: fine) or sideways to move it,
// double-click it or press Delete to remove it, right-click for Ramp to Next Tempo and Move
// MIDI Regions with Tempo. Signature lane: double-click to add or edit a signature in a
// small popover; right-click a signature to delete it.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { act, type Project } from "../api";
import { coalesced, framePos, onFrame } from "../frame";
import { getState, setState, useStore, type State } from "../store";
import { fmtBpm, MAX_BPM, MIN_BPM, SIG_DENOMINATORS, tempoMap, type SigEvent, type TempoEvent } from "../tempo";
import { samplesPerBeat, snapPos } from "../time";
import { mix, theme } from "../theme";
import { pixelRatio } from "../uiScale";
import { onViewport, scrollTo, viewport } from "../view";
import { NumberField } from "./Form";
import { openMenu, type MenuItem } from "./Menu";

/** Height of the Signature lane. */
export const SIG_H = 20;
export const TEMPO_MIN_H = 32;
export const TEMPO_MAX_H = 240;
/** Inset of the tempo graph from the lane's top and bottom. */
const PAD = 7;
/** The tempo graph's colour, Logic's green. */
const TEMPO_COLOR = "#8cc63f";

/** Rows the global tracks take in the tracks area (0 when hidden). */
export const globalTracksHeight = (s: State) => (s.showGlobalTracks ? SIG_H + s.tempoLaneH : 0);

const project = () => getState().snap?.project;

/** The tempo change selected by a click, for Delete. Cleared by clicks elsewhere. */
let selectedTempo: number | null = null;

function commitTempo(events: TempoEvent[]) {
  return act("set_tempo_map", { events, moveMidi: getState().moveMidiWithTempo });
}

function commitSigs(events: SigEvent[]) {
  return act("set_signatures", { events });
}

/** Delete with a tempo change selected removes it (the first one stays). */
export function deleteSelectedTempoEvent(): boolean {
  const p = project();
  const i = selectedTempo;
  selectedTempo = null;
  if (!p || i === null || i <= 0) return false;
  const ev = tempoMap(p).events;
  if (i >= ev.length) return false;
  commitTempo(ev.filter((_, k) => k !== i));
  return true;
}

/** The tempo axis: the events' range with some room, in steps of 5 BPM. */
function tempoRange(events: TempoEvent[]): [number, number] {
  const v = events.map((e) => e.bpm);
  const lo = Math.min(...v);
  const hi = Math.max(...v);
  const pad = Math.max(8, (hi - lo) * 0.2);
  return [Math.max(MIN_BPM, Math.floor((lo - pad) / 5) * 5), Math.min(MAX_BPM, Math.ceil((hi + pad) / 5) * 5)];
}

interface SigPop {
  bar: number;
  num: number;
  den: number;
  /** Screen position of the popover. */
  x: number;
  y: number;
  editing: boolean;
}

export function GlobalTracks() {
  const proj = useStore((s) => s.snap?.project);
  const pxPerBeat = useStore((s) => s.pxPerBeat);
  const laneH = useStore((s) => s.tempoLaneH);
  useStore((s) => s.themeId);
  useStore((s) => s.uiScale);
  const live = useRef({ project: proj, pxPerBeat, laneH });
  live.current = { project: proj, pxPerBeat, laneH };
  const ref = useRef<HTMLCanvasElement>(null);
  const phRef = useRef<HTMLDivElement>(null);
  /** Events shown while a point is dragged (committed on release), the axis kept still. */
  const preview = useRef<{ events: TempoEvent[]; range: [number, number]; i: number; label: string } | null>(null);
  const [pop, setPop] = useState<SigPop | null>(null);

  const geometry = () => {
    const { project, pxPerBeat, laneH } = live.current;
    const pps = project ? pxPerBeat / samplesPerBeat(project) : 1;
    return { pps, laneH, left: viewport().left };
  };

  const draw = () => {
    const cv = ref.current;
    const { project } = live.current;
    if (!cv || !project) return;
    const { pps, laneH, left } = geometry();
    const W = viewport().width;
    const H = SIG_H + laneH;
    const dpr = pixelRatio();
    if (cv.width !== Math.round(W * dpr) || cv.height !== Math.round(H * dpr)) {
      cv.width = Math.round(W * dpr);
      cv.height = Math.round(H * dpr);
      cv.style.width = `${W}px`;
      cv.style.height = `${H}px`;
    }
    const ctx = cv.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.fillStyle = theme.rulerCycleZone;
    ctx.fillRect(0, 0, W, SIG_H);
    ctx.fillStyle = theme.arrangeBg;
    ctx.fillRect(0, SIG_H, W, laneH);

    const map = tempoMap(project);
    const xOf = (pos: number) => pos * pps - left;
    // Bar lines, for orientation.
    ctx.fillStyle = theme.barLine;
    for (const b of map.bars(Math.max(0, left / pps), (left + W) / pps)) ctx.fillRect(Math.round(xOf(b.start)) + 0.5, 0, 1, H);
    ctx.fillStyle = theme.rulerEdge;
    ctx.fillRect(0, SIG_H - 1, W, 1);
    ctx.fillRect(0, H - 1, W, 1);

    // Signatures at their bars.
    ctx.font = "600 10px Inter, 'SF Pro Text', system-ui, sans-serif";
    ctx.textBaseline = "middle";
    for (const s of map.sigs) {
      const x = Math.round(xOf(map.barStart(s.bar))) + 0.5;
      if (x > W + 2) break;
      const label = `${s.num}/${s.den}`;
      const tw = ctx.measureText(label).width;
      if (x + tw + 8 < 0) continue;
      ctx.fillStyle = theme.rulerTick;
      ctx.fillRect(x, 3, 1, SIG_H - 6);
      ctx.fillStyle = theme.rulerText;
      ctx.fillText(label, x + 4, SIG_H / 2);
    }

    // The tempo graph: holds, ramps and a point per change.
    const pv = preview.current;
    const events = pv?.events ?? map.events;
    const [lo, hi] = pv?.range ?? tempoRange(events);
    const yOf = (bpm: number) => SIG_H + PAD + ((hi - bpm) / Math.max(1e-6, hi - lo)) * (laneH - 2 * PAD);
    ctx.beginPath();
    events.forEach((e, i) => {
      const next = events[i + 1];
      const x0 = xOf(e.pos);
      const y0 = yOf(e.bpm);
      if (i === 0) ctx.moveTo(x0, y0);
      else ctx.lineTo(x0, y0);
      if (!next) ctx.lineTo(W + 4, y0);
      else if (!e.ramp) ctx.lineTo(xOf(next.pos), y0);
    });
    ctx.strokeStyle = TEMPO_COLOR;
    ctx.lineWidth = 1.5;
    ctx.stroke();
    ctx.lineTo(W + 4, H);
    ctx.lineTo(xOf(0), H);
    ctx.closePath();
    ctx.fillStyle = mix(TEMPO_COLOR, theme.arrangeBg, 0.86);
    ctx.fill();
    ctx.lineWidth = 1;
    events.forEach((e, i) => {
      const x = xOf(e.pos);
      if (x < -40 || x > W + 4) return;
      const y = yOf(e.bpm);
      const on = pv ? pv.i === i : selectedTempo === i;
      ctx.fillStyle = on ? "#ffffff" : TEMPO_COLOR;
      ctx.fillRect(Math.round(x) - 2.5, Math.round(y) - 2.5, 5, 5);
      ctx.fillStyle = on ? theme.text : mix(TEMPO_COLOR, theme.text, 0.35);
      ctx.fillText(pv && pv.i === i ? pv.label : fmtBpm(e.bpm), x + 5, y < SIG_H + 14 ? y + 8 : y - 7);
    });
  };

  const drawRef = useRef(draw);
  drawRef.current = draw;
  const [requestDraw] = useState(() => coalesced(() => drawRef.current()));
  useLayoutEffect(() => draw());
  useEffect(() => {
    const a = onViewport(() => requestDraw());
    let last = "";
    const b = onFrame((now) => {
      const el = phRef.current;
      const { project } = live.current;
      if (!el || !project) return;
      const { pps, left } = geometry();
      const x = framePos(now) * pps - left;
      const tf = `translate3d(${x.toFixed(1)}px,0,0)`;
      if (tf !== last) {
        last = tf;
        el.style.transform = tf;
        el.style.visibility = x < -2 || x > viewport().width + 2 ? "hidden" : "visible";
      }
    });
    // A click anywhere else drops the tempo selection, so Delete goes back to regions.
    const down = (e: MouseEvent) => {
      if (e.target !== ref.current && selectedTempo !== null) {
        selectedTempo = null;
        requestDraw();
      }
    };
    window.addEventListener("mousedown", down, true);
    return () => {
      a();
      b();
      window.removeEventListener("mousedown", down, true);
    };
  }, [requestDraw]);

  /** Where a client x lands on the timeline. */
  const sampleAt = (clientX: number) => {
    const rect = ref.current!.getBoundingClientRect();
    const { pps, left } = geometry();
    return (clientX - rect.left + left) / pps;
  };

  /** The tempo change drawn at (x, y) in canvas pixels, if any. */
  const pointAt = (p: Project, x: number, y: number): number | null => {
    const { pps, laneH, left } = geometry();
    const events = tempoMap(p).events;
    const [lo, hi] = tempoRange(events);
    let best: number | null = null;
    let bestD = 7;
    events.forEach((e, i) => {
      const px = e.pos * pps - left;
      const py = SIG_H + PAD + ((hi - e.bpm) / Math.max(1e-6, hi - lo)) * (laneH - 2 * PAD);
      const d = Math.max(Math.abs(px - x), Math.abs(py - y));
      if (d < bestD) {
        bestD = d;
        best = i;
      }
    });
    return best;
  };

  /** The signature label drawn at canvas x, if any. */
  const sigAt = (p: Project, x: number): SigEvent | null => {
    const { pps, left } = geometry();
    const map = tempoMap(p);
    return [...map.sigs].reverse().find((s) => {
      const sx = map.barStart(s.bar) * pps - left;
      return x >= sx - 3 && x <= sx + 34;
    }) ?? null;
  };

  const onMouseDown = (e: React.MouseEvent) => {
    const p = live.current.project;
    if (!p || e.button !== 0) return;
    const rect = ref.current!.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    if (y >= SIG_H + live.current.laneH - 4) return resizeDown(e);
    if (y < SIG_H) return sigDown(e, p, x);
    tempoDown(e, p, x, y);
  };

  const resizeDown = (e: React.MouseEvent) => {
    e.preventDefault();
    const startY = e.clientY;
    const startH = live.current.laneH;
    const move = (ev: MouseEvent) => setState({ tempoLaneH: Math.round(Math.min(TEMPO_MAX_H, Math.max(TEMPO_MIN_H, startH + ev.clientY - startY))) });
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  const sigDown = (e: React.MouseEvent, p: Project, x: number) => {
    if (e.detail !== 2) return;
    const map = tempoMap(p);
    const hit = sigAt(p, x);
    if (hit) {
      setPop({ ...hit, x: e.clientX, y: e.clientY, editing: true });
      return;
    }
    // A new signature starts at the nearest bar line.
    const bar = Math.max(1, Math.round(map.barPosition(sampleAt(e.clientX))) + 1);
    const [num, den] = map.sigAtBar(bar);
    setPop({ bar, num, den, x: e.clientX, y: e.clientY, editing: false });
  };

  const tempoDown = (e: React.MouseEvent, p: Project, x: number, y: number) => {
    e.preventDefault();
    const map = tempoMap(p);
    const events = map.events.map((ev) => ({ ...ev }));
    const range = tempoRange(events);
    let i = pointAt(p, x, y);
    const isNew = i === null;
    if (i !== null && e.detail === 2) {
      selectedTempo = null;
      if (i > 0) commitTempo(events.filter((_, k) => k !== i));
      return;
    }
    if (i === null) {
      // A new change at the snapped position, on the line (so a ramp it splits keeps its shape).
      const s = getState();
      const pos = Math.round(snapPos(p, sampleAt(e.clientX), s.snapMode, s.pxPerBeat));
      if (pos <= 0) return;
      const at = map.eventIndexAt(pos);
      if (events[at].pos === pos) {
        i = at;
      } else {
        events.splice(at + 1, 0, { pos, bpm: Math.round(map.bpmAt(pos) * 100) / 100, ramp: events[at].ramp });
        i = at + 1;
      }
    }
    const idx = i;
    selectedTempo = idx;
    const orig = { ...events[idx] };
    const perPx = (range[1] - range[0]) / Math.max(1, live.current.laneH - 2 * PAD);
    const { pps } = geometry();
    const startX = e.clientX;
    const startY = e.clientY;
    let axis: "x" | "y" | null = null;
    const show = (ev: TempoEvent) => (axis === "x" ? `Bar ${tempoMap(p).bbtAt(ev.pos).bar} ${tempoMap(p).bbtAt(ev.pos).beat}` : `${fmtBpm(ev.bpm)} BPM`);
    preview.current = { events, range, i: idx, label: `${fmtBpm(orig.bpm)} BPM` };
    draw();

    const move = (ev: MouseEvent) => {
      const dx = ev.clientX - startX;
      const dy = ev.clientY - startY;
      if (!axis) {
        if (Math.abs(dx) < 3 && Math.abs(dy) < 3) return;
        // The first change stays at the start of the project.
        axis = idx > 0 && Math.abs(dx) > Math.abs(dy) ? "x" : "y";
      }
      const cur = { ...orig };
      if (axis === "y") {
        const bpm = ev.shiftKey ? orig.bpm - dy * 0.05 : orig.bpm - dy * perPx;
        cur.bpm = Math.min(MAX_BPM, Math.max(MIN_BPM, ev.shiftKey ? Math.round(bpm * 100) / 100 : Math.round(bpm)));
      } else {
        const s = getState();
        const lo = events[idx - 1].pos + 1;
        const hi = events[idx + 1] ? events[idx + 1].pos - 1 : Infinity;
        cur.pos = Math.round(Math.min(hi, Math.max(lo, snapPos(p, orig.pos + dx / pps, s.snapMode, s.pxPerBeat))));
      }
      const next = events.map((x, k) => (k === idx ? cur : x));
      preview.current = { events: next, range, i: idx, label: show(cur) };
      draw();
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      const pv = preview.current;
      preview.current = null;
      if (pv && (axis || isNew)) commitTempo(pv.events);
      else draw();
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  const onContextMenu = (e: React.MouseEvent) => {
    const p = live.current.project;
    if (!p) return;
    e.preventDefault();
    const rect = ref.current!.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    const map = tempoMap(p);
    if (y < SIG_H) {
      // Right-click deletes a signature change (the first one is the project's own).
      const s = sigAt(p, x);
      if (s && s.bar > 1) commitSigs(map.sigs.filter((x) => x.bar !== s.bar));
      return;
    }
    const events = map.events.map((ev) => ({ ...ev }));
    const i = pointAt(p, x, y);
    const moveMidi = getState().moveMidiWithTempo;
    const items: MenuItem[] = [];
    if (i !== null) {
      selectedTempo = i;
      draw();
      items.push(
        {
          label: "Ramp to Next Tempo",
          checked: events[i].ramp,
          disabled: i === events.length - 1,
          onClick: () => commitTempo(events.map((ev, k) => (k === i ? { ...ev, ramp: !ev.ramp } : ev))),
        },
        { label: "Delete Tempo Change", disabled: i === 0, onClick: () => commitTempo(events.filter((_, k) => k !== i)) },
        { separator: true },
      );
    }
    items.push({
      label: "Move MIDI Regions with Tempo",
      checked: moveMidi,
      onClick: () => setState({ moveMidiWithTempo: !moveMidi }),
    });
    openMenu(e.clientX, e.clientY, items);
  };

  // Horizontal wheel scrolls the arrange area.
  const onWheel = (e: React.WheelEvent) => {
    const dx = e.shiftKey ? e.deltaY : e.deltaX;
    if (dx) scrollTo(Math.max(0, viewport().left + dx));
  };

  const commitPop = (num: number, den: number) => {
    const p = live.current.project;
    const at = pop;
    setPop(null);
    if (!p || !at) return;
    const sigs = tempoMap(p).sigs.filter((s) => s.bar !== at.bar);
    commitSigs([...sigs, { bar: at.bar, num, den }]);
  };

  return (
    <>
      <div className="global-headers">
        <div className="global-name" style={{ height: SIG_H }}>
          Signature
        </div>
        <div className="global-name" style={{ height: laneH }}>
          Tempo
          <div className="global-resize" onMouseDown={resizeDown} />
        </div>
      </div>
      <div className="global-lanes">
        <canvas ref={ref} onMouseDown={onMouseDown} onContextMenu={onContextMenu} onWheel={onWheel} />
        <div className="global-playhead" ref={phRef} style={{ ["--ph" as string]: theme.playhead }} />
      </div>
      {pop && <SignaturePopover pop={pop} onCancel={() => setPop(null)} onCommit={commitPop} />}
    </>
  );
}

/** The small popover for a signature: numerator and denominator. */
function SignaturePopover({ pop, onCancel, onCommit }: { pop: SigPop; onCancel: () => void; onCommit: (num: number, den: number) => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [num, setNum] = useState(pop.num);
  const [den, setDen] = useState(pop.den);
  useEffect(() => {
    const down = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) onCancel();
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") onCancel();
    };
    window.addEventListener("mousedown", down);
    window.addEventListener("keydown", key);
    return () => {
      window.removeEventListener("mousedown", down);
      window.removeEventListener("keydown", key);
    };
  }, [onCancel]);
  return (
    <div className="sig-pop" ref={ref} style={{ left: Math.max(4, Math.min(pop.x - 20, window.innerWidth - 200)), top: pop.y + 10 }}>
      <div className="pr-qtitle">{pop.editing ? "Signature" : "New Signature"} at Bar {pop.bar}</div>
      <div className="sig-pop-row">
        <NumberField value={num} min={1} max={32} step={1} width={72} onChange={(v) => setNum(Math.round(v))} />
        <span className="sig-pop-slash">/</span>
        {SIG_DENOMINATORS.map((d) => (
          <button key={d} className={"pr-qval" + (d === den ? " on" : "")} onClick={() => setDen(d)}>
            {d}
          </button>
        ))}
      </div>
      <div className="pr-qfoot">
        <button className="pr-qbtn" onClick={() => onCommit(num, den)}>
          {pop.editing ? "Change" : "Add"}
        </button>
      </div>
    </div>
  );
}
