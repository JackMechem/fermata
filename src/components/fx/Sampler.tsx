// Fermata Sampler's panel, laid out left to right for the wide, short dock: the zones on a keyboard
// map, then the selected zone's waveform and settings, then the instrument's parameter sections in
// a row. It scrolls sideways when the dock is narrower than that, and works floated.
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { act, api, call, type Id, type InputSel, type Snapshot } from "../../api";
import type { FxPanelProps } from "../../fx";
import { findFxSlot } from "../../fxHost";
import { dropOnSampler } from "../../samplerDrop";
import { memorySize, dragZone, grabOf, keyAt, keyName, moveHandle, parseKey, pointsOf, toZone, zoneAt, zoneRect, zoomed, type MapBox, type SamplerView, type SamplerWave, type WaveHandle, type ZoneGrab, type ZoneView } from "../../samplerMap";
import { getState, setState, useStore } from "../../store";
import { AUDIO_EXTENSIONS } from "../../dnd";
import { FxControl, FxGroup } from "./common";
import { wheelAcross } from "./Instrument";

type Result<T> = { snapshot: Snapshot; result: T };

const toast = (text: string, ms = 5000) => setState({ toast: { text, at: Date.now(), ms } });

/** A command that returns a snapshot along with something to say. */
async function callSnap<T>(cmd: string, args: Record<string, unknown>): Promise<T | null> {
  const r = await call<Result<T>>(cmd, args);
  if (!r) return null;
  setState({ snap: r.snapshot });
  if (r.snapshot.warning) toast(r.snapshot.warning, 8000);
  return r.result;
}

const cssVar = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();

/** Sizes a canvas to its box and hands back its context in CSS pixels. */
function prepare(c: HTMLCanvasElement): { ctx: CanvasRenderingContext2D; w: number; h: number } | null {
  const ctx = c.getContext("2d");
  if (!ctx) return null;
  const { width: w, height: h } = c.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  if (c.width !== Math.round(w * dpr) || c.height !== Math.round(h * dpr)) {
    c.width = Math.round(w * dpr);
    c.height = Math.round(h * dpr);
  }
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, h);
  return { ctx, w, h };
}

const STRIP = 18;
const isBlack = (k: number) => [1, 3, 6, 8, 10].includes(k % 12);

/** Redraws when its box changes size (the dock being dragged, say). */
function useResize(ref: React.RefObject<HTMLElement | null>, draw: () => void) {
  useEffect(() => {
    if (!ref.current) return;
    const ob = new ResizeObserver(draw);
    ob.observe(ref.current);
    return () => ob.disconnect();
  }, [ref, draw]);
}

function ZoneMap({ zones, selected, playing, track, onSelect, onChange }: { zones: ZoneView[]; selected: number | null; playing: number; track: Id; onSelect: (id: number) => void; onChange: (z: ZoneView, final: boolean) => void }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const drag = useRef<{ zone: ZoneView; grab: ZoneGrab; x: number; y: number; moved: boolean } | null>(null);
  const held = useRef<number | null>(null);
  const [draft, setDraft] = useState<ZoneView | null>(null);

  const box = (c: HTMLCanvasElement): MapBox => {
    const r = c.getBoundingClientRect();
    return { width: r.width, height: r.height, strip: STRIP };
  };
  const draw = useCallback(() => {
    const c = ref.current;
    const p = c && prepare(c);
    if (!c || !p) return;
    const { ctx, w, h } = p;
    const b = box(c);
    const px = w / 128;
    const accent = cssVar("--accent") || "#4c9eff";
    ctx.fillStyle = cssVar("--panel-2") || "#2c2c2f";
    ctx.fillRect(0, 0, w, h - STRIP);
    // Octave lines.
    ctx.fillStyle = "rgba(128,128,128,0.25)";
    for (let k = 0; k < 128; k += 12) ctx.fillRect(k * px, 0, 1, h - STRIP);
    for (const z of zones.map((x) => (draft && x.id === draft.id ? draft : x))) {
      const r = zoneRect(b, z);
      const on = z.id === selected;
      ctx.globalAlpha = z.id === playing ? 0.95 : on ? 0.8 : 0.45;
      ctx.fillStyle = z.error ? "#c8483d" : on ? accent : "#7f8a99";
      ctx.fillRect(r.x, r.y, r.w, r.h);
      ctx.globalAlpha = 1;
      ctx.strokeStyle = on ? "#fff" : "rgba(0,0,0,0.5)";
      ctx.strokeRect(r.x + 0.5, r.y + 0.5, Math.max(1, r.w - 1), Math.max(1, r.h - 1));
      // The root.
      ctx.fillStyle = "#fff";
      ctx.fillRect(z.root * px, r.y, Math.max(1, px), Math.min(4, r.h));
      if (r.w > 40 && r.h > 14) {
        ctx.fillStyle = "#fff";
        ctx.font = "10px system-ui, sans-serif";
        ctx.fillText(z.name, r.x + 4, r.y + 11, r.w - 8);
      }
    }
    // The keys below. A piano's colours in every theme, as the instrument keyboard.
    ctx.fillStyle = "#e4e4e8";
    ctx.fillRect(0, h - STRIP, w, STRIP);
    for (let k = 0; k < 128; k++) {
      if (isBlack(k)) {
        ctx.fillStyle = "#26272b";
        ctx.fillRect(k * px, h - STRIP, Math.max(1, px), STRIP * 0.62);
      }
      if (k % 12 === 0) {
        ctx.fillStyle = "#26272b";
        ctx.font = "8px system-ui, sans-serif";
        ctx.fillRect(k * px, h - STRIP, 1, STRIP);
        if (k % 24 === 0) ctx.fillText(keyName(k).replace("♯", "#"), k * px + 2, h - 2);
      }
    }
  }, [zones, selected, playing, draft]);
  useEffect(draw, [draw]);
  useResize(ref, draw);

  const local = (e: React.PointerEvent) => {
    const r = ref.current!.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  };
  const midi = (status: number, note: number) => call("midi_event", { data: [status, note, status === 0x90 ? 100 : 0] });
  const noteOff = () => {
    if (held.current !== null) midi(0x80, held.current);
    held.current = null;
  };
  useEffect(() => noteOff, []); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <canvas
      ref={ref}
      className="smp-map"
      title="Drop files on a key to place them there. Drag a zone to move it, its edges to change its keys and velocities. Play the keys below."
      onPointerDown={(e) => {
        const c = ref.current!;
        const b = box(c);
        const { x, y } = local(e);
        c.setPointerCapture(e.pointerId);
        if (y >= b.height - STRIP) {
          const k = keyAt(b, x);
          held.current = k;
          if (getState().snap?.selected !== track) api.selectTrack(track);
          midi(0x90, k);
          return;
        }
        const z = zoneAt(b, zones, x, y);
        if (!z) return;
        onSelect(z.id);
        drag.current = { zone: z, grab: grabOf(b, z, x, y), x, y, moved: false };
      }}
      onPointerMove={(e) => {
        const c = ref.current!;
        const b = box(c);
        const { x, y } = local(e);
        if (held.current !== null && e.buttons & 1 && y >= b.height - STRIP) {
          const k = keyAt(b, x);
          if (k !== held.current) {
            midi(0x90, k);
            midi(0x80, held.current);
            held.current = k;
          }
          return;
        }
        const d = drag.current;
        if (!d) return;
        const dk = Math.round(((x - d.x) / b.width) * 128);
        const dv = Math.round(((d.y - y) / (b.height - STRIP)) * 127);
        if (dk === 0 && dv === 0 && !d.moved) return;
        d.moved = true;
        setDraft(dragZone(d.zone, d.grab, dk, dv));
      }}
      onPointerUp={() => {
        noteOff();
        if (drag.current?.moved && draft) onChange(draft, true);
        drag.current = null;
        setDraft(null);
      }}
      onPointerCancel={() => {
        noteOff();
        drag.current = null;
        setDraft(null);
      }}
    />
  );
}

function Waveform({ track, zone, onChange }: { track: Id; zone: ZoneView; onChange: (z: ZoneView) => void }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const [range, setRange] = useState<[number, number]>([0, zone.frames]);
  const [wave, setWave] = useState<SamplerWave | null>(null);
  const [draft, setDraft] = useState<ZoneView | null>(null);
  const grab = useRef<WaveHandle | null>(null);
  const shown = draft ?? zone;

  useEffect(() => setRange([0, zone.frames]), [zone.id, zone.frames]);
  useEffect(() => {
    let live = true;
    const width = Math.max(64, Math.round(ref.current?.getBoundingClientRect().width ?? 500));
    if (zone.error || !zone.frames) {
      setWave(null);
      return;
    }
    call<SamplerWave>("sampler_waveform", { track, zone: zone.id, from: range[0], to: range[1], points: width }).then((w) => live && setWave(w));
    return () => {
      live = false;
    };
  }, [track, zone.id, zone.file, zone.error, zone.frames, range]);

  const draw = useCallback(() => {
    const c = ref.current;
    const p = c && prepare(c);
    if (!c || !p) return;
    const { ctx, w, h } = p;
    const accent = cssVar("--accent") || "#4c9eff";
    ctx.fillStyle = cssVar("--panel-2") || "#2c2c2f";
    ctx.fillRect(0, 0, w, h);
    if (shown.error) {
      ctx.fillStyle = "#e0695e";
      ctx.font = "12px system-ui, sans-serif";
      ctx.fillText(`${shown.file}: ${shown.error}`, 10, h / 2, w - 20);
      return;
    }
    if (!wave) return;
    const [from, to] = range;
    const xOf = (frame: number) => ((frame - from) / Math.max(1, to - from)) * w;
    const pts = pointsOf(shown);
    // The loop's region, and its crossfade going in.
    if (shown.loop_on) {
      const a = xOf(pts.loop_start);
      const b = xOf(pts.loop_end);
      ctx.fillStyle = "rgba(76,190,120,0.18)";
      ctx.fillRect(a, 0, b - a, h);
      const x = Math.min(shown.xfade, pts.loop_start, pts.loop_end - pts.loop_start);
      if (x > 0) {
        ctx.fillStyle = "rgba(76,190,120,0.35)";
        ctx.beginPath();
        ctx.moveTo(xOf(pts.loop_end - x), h);
        ctx.lineTo(xOf(pts.loop_end), 0);
        ctx.lineTo(xOf(pts.loop_end), h);
        ctx.fill();
      }
    }
    // The audio, dim outside what plays.
    const mid = h / 2;
    const n = wave.peaks.length;
    for (let i = 0; i < n; i++) {
      const [lo, hi] = wave.peaks[i];
      const x = (i / n) * w;
      const frame = from + (i / n) * (to - from);
      ctx.fillStyle = frame < pts.start || frame >= pts.end ? "rgba(150,150,155,0.35)" : accent;
      ctx.fillRect(x, mid - hi * mid, Math.max(1, w / n), Math.max(1, (hi - lo) * mid));
    }
    // Handles.
    const handle = (frame: number, label: string, color: string, top: boolean) => {
      const x = xOf(frame);
      if (x < -1 || x > w + 1) return;
      ctx.fillStyle = color;
      ctx.fillRect(x - 0.5, 0, 1.5, h);
      ctx.fillRect(x - 0.5, top ? 0 : h - 12, 12, 12);
      ctx.fillStyle = "#000";
      ctx.font = "bold 9px system-ui, sans-serif";
      ctx.fillText(label, x + 2, top ? 9 : h - 3);
    };
    handle(pts.start, "S", "#f0d264", true);
    handle(pts.end, "E", "#f0d264", true);
    if (shown.loop_on) {
      handle(pts.loop_start, "L", "#5fd38d", false);
      handle(pts.loop_end, "R", "#5fd38d", false);
    }
  }, [shown, wave, range]);
  useEffect(draw, [draw]);
  useResize(ref, draw);

  const frameAt = (e: React.PointerEvent | React.WheelEvent) => {
    const r = ref.current!.getBoundingClientRect();
    return range[0] + ((e.clientX - r.left) / r.width) * (range[1] - range[0]);
  };
  return (
    <canvas
      ref={ref}
      className="smp-wave-canvas"
      title="Drag S and E for where the zone starts and ends, L and R for its loop. Scroll to zoom, double-click to see it all."
      onWheel={(e) => {
        // It zooms the waveform; the row behind it stays where it is.
        e.stopPropagation();
        e.preventDefault();
        if (zone.frames) setRange(zoomed(range[0], range[1], zone.frames, frameAt(e), e.deltaY < 0 ? 0.75 : 1.33));
      }}
      onDoubleClick={() => setRange([0, zone.frames])}
      onPointerDown={(e) => {
        const c = ref.current!;
        const r = c.getBoundingClientRect();
        const x = e.clientX - r.left;
        const xOf = (f: number) => ((f - range[0]) / Math.max(1, range[1] - range[0])) * r.width;
        const pts = pointsOf(shown);
        const candidates: [WaveHandle, number][] = [["start", pts.start], ["end", pts.end]];
        if (shown.loop_on) candidates.push(["loop_start", pts.loop_start], ["loop_end", pts.loop_end]);
        let best: WaveHandle | null = null;
        let near = 10;
        for (const [h, f] of candidates) {
          const d = Math.abs(xOf(f) - x);
          if (d < near) {
            near = d;
            best = h;
          }
        }
        grab.current = best;
        if (best) c.setPointerCapture(e.pointerId);
      }}
      onPointerMove={(e) => {
        if (grab.current) setDraft(moveHandle(shown, grab.current, frameAt(e)));
      }}
      onPointerUp={() => {
        if (grab.current && draft) onChange(draft);
        grab.current = null;
        setDraft(null);
      }}
    />
  );
}

/** A number typed in a small box, which lands on Enter or leaving the box. */
function Field({ label, value, parse, show, onCommit, title, width = 46 }: { label: string; value: number; parse?: (t: string) => number | null; show?: (v: number) => string; onCommit: (v: number) => void; title?: string; width?: number }) {
  const text = show ? show(value) : String(Math.round(value * 100) / 100);
  const [edit, setEdit] = useState<string | null>(null);
  const commit = () => {
    if (edit !== null) {
      const v = parse ? parse(edit) : Number(edit);
      if (v !== null && Number.isFinite(v) && v !== value) onCommit(v);
    }
    setEdit(null);
  };
  return (
    <label className="smp-field" title={title}>
      <span>{label}</span>
      <input
        style={{ width }}
        value={edit ?? text}
        onChange={(e) => setEdit(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") e.currentTarget.blur();
          if (e.key === "Escape") setEdit(null);
        }}
      />
    </label>
  );
}

function ZoneFields({ zone, update, remove, slice, replace }: { zone: ZoneView; update: (patch: Partial<ZoneView>) => void; remove: () => void; slice: (sensitivity: number) => void; replace: () => void }) {
  const [sens, setSens] = useState(50);
  const pts = pointsOf(zone);
  const ms = (f: number) => (zone.rate ? `${Math.round((f / zone.rate) * 10000) / 10}` : String(f));
  const key = (t: string) => parseKey(t);
  return (
    <div className="smp-fields">
      <Field label="Low Key" value={zone.key_lo} parse={key} show={keyName} onCommit={(v) => update({ key_lo: v })} />
      <Field label="High Key" value={zone.key_hi} parse={key} show={keyName} onCommit={(v) => update({ key_hi: v })} />
      <Field label="Root" value={zone.root} parse={key} show={keyName} onCommit={(v) => update({ root: v })} title="The key that plays the sample at its own pitch" />
      <Field label="Fine" value={zone.fine} onCommit={(v) => update({ fine: v })} title="Fine tune, in cents" />
      <Field label="Low Vel" value={zone.vel_lo} onCommit={(v) => update({ vel_lo: v })} />
      <Field label="High Vel" value={zone.vel_hi} onCommit={(v) => update({ vel_hi: v })} />
      <Field label="Gain" value={zone.gain_db} onCommit={(v) => update({ gain_db: v })} title="dB" />
      <Field label="Pan" value={zone.pan * 100} onCommit={(v) => update({ pan: v / 100 })} title="-100 left to 100 right" />
      <Field label="Group" value={zone.rr_group} onCommit={(v) => update({ rr_group: Math.round(v) })} title="Round robin: zones of the same group on the same key and velocity take turns. 0 is none." />
      <Field label="Fade ms" value={zone.xfade} parse={(t) => (zone.rate ? Math.round((Number(t) / 1000) * zone.rate) : Number(t))} show={ms} onCommit={(v) => update({ xfade: Math.max(0, v) })} title="The loop's crossfade, from the audio before its start. A loop at the very start of a sample has no room for one." />
      <label className="smp-check" title="Loop while the key's held">
        <input type="checkbox" checked={zone.loop_on} onChange={(e) => update(e.target.checked && zone.loop_start === 0 && zone.loop_end === 0 ? { loop_on: true, loop_start: pts.start, loop_end: pts.end } : { loop_on: e.target.checked })} /> Loop
      </label>
      <label className="smp-check" title="Crossfade at equal power, for loops whose two sides don't line up">
        <input type="checkbox" checked={zone.equal_power} onChange={(e) => update({ equal_power: e.target.checked })} /> Equal Power
      </label>
      <label className="smp-check" title="Plays to its end whatever the key does">
        <input type="checkbox" checked={zone.one_shot} onChange={(e) => update({ one_shot: e.target.checked })} /> One-Shot
      </label>
      <label className="smp-check">
        <input type="checkbox" checked={zone.reverse} onChange={(e) => update({ reverse: e.target.checked })} /> Reverse
      </label>
      <span className="smp-buttons">
        <button className="fx-toggle" title="Cut this zone at its transients, one slice per key from C1" onClick={() => slice(sens / 100)}>
          Slice
        </button>
        <input className="smp-sens" type="range" min={0} max={100} value={sens} title={`Slice sensitivity: ${sens} %`} onChange={(e) => setSens(Number(e.target.value))} />
        <button className="fx-toggle" title="Point this zone at another file" onClick={replace}>
          Replace…
        </button>
        <button className="fx-toggle" onClick={remove}>
          Delete
        </button>
      </span>
    </div>
  );
}

function inputOptions(channels: number): { label: string; value: InputSel }[] {
  const out: { label: string; value: InputSel }[] = [];
  for (let c = 0; c < channels; c++) out.push({ label: `Input ${c + 1}`, value: { type: "mono", ch: c } });
  for (let c = 0; c + 1 < channels; c += 2) out.push({ label: `Input ${c + 1}-${c + 2}`, value: { type: "stereo", ch: c } });
  return out;
}

function RecordControls({ track, onDone }: { track: Id; onDone: (zone: number) => void }) {
  const channels = useStore((s) => s.snap?.audio.input_channels ?? 0);
  const options = inputOptions(channels);
  const [choice, setChoice] = useState(0);
  const [trim, setTrim] = useState(true);
  const [status, setStatus] = useState<{ secs: number; peak: number } | null>(null);
  useEffect(() => {
    if (!status) return;
    const t = window.setInterval(async () => {
      const r = await call<{ secs: number; peak: number } | null>("sampler_record_status");
      if (r) setStatus((old) => ({ secs: r.secs, peak: Math.max(r.peak, (old?.peak ?? 0) * 0.8) }));
    }, 100);
    return () => window.clearInterval(t);
  }, [!!status]); // eslint-disable-line react-hooks/exhaustive-deps

  const start = async () => {
    const input = options[choice]?.value;
    if (!input) return toast("No audio input is open. Choose one in Settings › Audio.");
    try {
      await invoke("sampler_record_start", { track, input });
      setStatus({ secs: 0, peak: 0 });
    } catch (e) {
      toast(String(e));
    }
  };
  const stop = async () => {
    setStatus(null);
    const zone = await callSnap<number>("sampler_record_stop", { trim, atKey: null });
    if (zone !== null) onDone(zone);
  };
  return (
    <>
      {status ? (
        <>
          <button className="fx-toggle smp-rec on" title="Stop, and make what came in playable" onClick={stop}>
            ■ Stop
          </button>
          <button
            className="fx-toggle"
            title="Stop and throw it away"
            onClick={() => {
              setStatus(null);
              call("sampler_record_cancel");
            }}
          >
            Cancel
          </button>
          <span className="smp-rec-time">{status.secs.toFixed(1)} s</span>
          <span className="smp-rec-level">
            <span style={{ width: `${Math.min(100, status.peak * 100)}%` }} className={status.peak >= 0.99 ? "over" : ""} />
          </span>
        </>
      ) : (
        <>
          <button className="fx-toggle smp-rec" title="Record from the audio input into this sampler. The transport stays as it is." onClick={start}>
            ● Record
          </button>
          <select className="smp-input" value={choice} onChange={(e) => setChoice(Number(e.target.value))} title="The input to record from">
            {options.map((o, i) => (
              <option key={o.label} value={i}>
                {o.label}
              </option>
            ))}
            {!options.length && <option>No input</option>}
          </select>
          <label className="smp-check" title="Move the start and end in past the silence. The file keeps everything.">
            <input type="checkbox" checked={trim} onChange={(e) => setTrim(e.target.checked)} /> Trim
          </label>
        </>
      )}
    </>
  );
}

export function SamplerPanel({ info, track, uid, values, set, touch, meters }: FxPanelProps) {
  const fingerprint = useStore((s) => findFxSlot(s.snap?.project, uid)?.plugin.state);
  const [view, setView] = useState<SamplerView | null>(null);
  const [selected, setSelected] = useState<number | null>(null);
  const [notes, setNotes] = useState<string | null>(null);
  const byId = new Map(info.params.map((p) => [p.id, p]));

  useEffect(() => {
    let live = true;
    call<SamplerView>("sampler_view", { track }).then((v) => {
      if (!live || !v) return;
      setView(v);
      setSelected((cur) => (v.zones.some((z) => z.id === cur) ? cur : v.zones[0]?.id ?? null));
    });
    return () => {
      live = false;
    };
  }, [track, fingerprint]);

  const zones = view?.zones ?? [];
  const zone = zones.find((z) => z.id === selected) ?? null;
  const update = (z: ZoneView, merge?: string) => {
    setView((v) => v && { ...v, zones: v.zones.map((x) => (x.id === z.id ? z : x)) });
    act("sampler_update_zones", { track, zones: [toZone(z)], merge: merge ?? null });
  };
  const addFiles = async () => {
    const picked = await open({ title: "Add Samples", multiple: true, filters: [{ name: "Audio or SFZ", extensions: [...AUDIO_EXTENSIONS, "sfz"] }] });
    if (!picked) return;
    const paths = Array.isArray(picked) ? picked : [picked];
    const report = await dropOnSampler(track, paths, null);
    if (report) setNotes(report);
  };
  const replace = async () => {
    if (!zone) return;
    const picked = await open({ title: `Replace ${zone.name}`, filters: [{ name: "Audio", extensions: AUDIO_EXTENSIONS }] });
    if (typeof picked === "string") act("sampler_replace_file", { track, zone: zone.id, path: picked });
  };
  const slice = async (sensitivity: number) => {
    if (!zone) return;
    const n = await callSnap<number>("sampler_slice", { track, zone: zone.id, sensitivity });
    if (n !== null) toast(`${n} slice${n === 1 ? "" : "s"}, from C1 up`, 3000);
  };
  const broken = zones.filter((z) => z.error);
  const gb = (b: number) => (b / (1 << 30)).toFixed(b < 1 << 30 ? 2 : 1);
  return (
    <div className="fx-panel smp-panel" data-sampler-track={track}>
      <div className="smp-row" onWheel={wheelAcross}>
        <FxGroup title="Zones" className="smp-col smp-zones">
          <div className="smp-tools">
            <button className="fx-toggle" title="Add audio files, or load an SFZ in place of the zones. You can drop them here too." onClick={addFiles}>
              Add…
            </button>
            <RecordControls track={track} onDone={setSelected} />
          </div>
          <div className="smp-map-box" data-sampler-map="1">
            <ZoneMap zones={zones} selected={selected} playing={meters[2] ?? -1} track={track} onSelect={setSelected} onChange={(z) => update(z)} />
            {!zones.length && <div className="smp-empty">Drop audio or SFZ files here, choose Add…, or Record</div>}
          </div>
          {broken.length > 0 ? (
            <div className="smp-status bad" title={broken.map((z) => `${z.file}: ${z.error}`).join("\n")}>
              {broken.length} sample{broken.length > 1 ? "s" : ""} can't play: {broken[0].file} ({broken[0].error}){broken.length > 1 ? " and more" : ""}. The rest play.
            </div>
          ) : notes ? (
            <div className="smp-status" title={notes} onClick={() => setNotes(null)}>
              {notes}
            </div>
          ) : (
            <div className="smp-status">
              {zones.length} zone{zones.length === 1 ? "" : "s"}, {memorySize(view?.memory ?? 0)} of {gb(view?.limit ?? 0)} GB in memory, {meters[0] ?? 0} voices
            </div>
          )}
        </FxGroup>
        <FxGroup title={zone ? `Zone: ${zone.name}` : "Zone"} className="smp-col smp-wave">
          {zones.length > 1 && (
            <select className="smp-zone-pick" value={selected ?? ""} onChange={(e) => setSelected(Number(e.target.value))} title="Choose a zone, for those behind others on the map">
              {zones.map((z) => (
                <option key={z.id} value={z.id}>
                  {z.error ? `(can't play) ${z.name}` : z.name} {keyName(z.key_lo)}–{keyName(z.key_hi)}
                </option>
              ))}
            </select>
          )}
          {zone ? (
            <>
              <Waveform track={track} zone={zone} onChange={(z) => update(z)} />
              <ZoneFields
                zone={zone}
                update={(patch) => update({ ...zone, ...patch })}
                remove={() => act("sampler_delete_zones", { track, ids: [zone.id] })}
                slice={slice}
                replace={replace}
              />
            </>
          ) : (
            <div className="smp-empty">No zone selected</div>
          )}
        </FxGroup>
        {(info.sections ?? []).map((s) => (
          <FxGroup key={s.title} title={s.title} className="smp-col smp-section">
            {s.params.map((id) => {
              const p = byId.get(id);
              return p ? <FxControl key={id} spec={p} value={values[id]} onChange={(v) => set(id, v)} onTouch={(on) => touch(id, on)} /> : null;
            })}
          </FxGroup>
        ))}
      </div>
    </div>
  );
}

