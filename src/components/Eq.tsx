import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { call, EQ_SLOT, MASTER_ID, type ChannelEq, type EqBand, type Id, type Project } from "../api";
import { BAND_COLORS, bandSections, defaultEq, eqSections, formatFreq, isFlat, responseDb } from "../eqMath";
import { getState, onTick, setState, useStore } from "../store";
import { eqKey, toggleEqPanel, usePanelShown } from "../fxDockState";
import { pixelRatio } from "../uiScale";
import { refuseFrozen } from "../freeze";
import { fmt } from "../keymap";
import { CHANNEL_EQ, copyEq, eqClipboard, eqMenu, pasteEq } from "../fxHost";
import { openMenu } from "./Menu";
import { PresetButton } from "./fx/Presets";
import { learnContext } from "../learn";

// ---- state helpers ------------------------------------------------------------------------

export function eqOf(project: Project | undefined, id: Id): ChannelEq | undefined {
  if (!project) return undefined;
  return id === MASTER_ID ? project.master.eq : project.tracks.find((t) => t.id === id)?.eq;
}

let pending: { track: Id; eq: ChannelEq } | null = null;
let scheduled = false;

/** Updates the strip's EQ in the UI immediately and sends it to the engine once per frame. */
export function updateEq(track: Id, eq: ChannelEq) {
  const snap = getState().snap;
  // A frozen track's EQ is baked into its freeze file.
  if (!snap || refuseFrozen([track])) return;
  const project =
    track === MASTER_ID
      ? { ...snap.project, master: { ...snap.project.master, eq } }
      : { ...snap.project, tracks: snap.project.tracks.map((t) => (t.id === track ? { ...t, eq } : t)) };
  setState({ snap: { ...snap, project, dirty: true } });
  pending = { track, eq };
  if (!scheduled) {
    scheduled = true;
    requestAnimationFrame(() => {
      scheduled = false;
      if (pending) call("set_eq", pending);
      pending = null;
    });
  }
}

function cssVar(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || "#4c9eff";
}

const MIN_F = 20;
const MAX_F = 20000;
const fx = (f: number, w: number) => ((Math.log(f) - Math.log(MIN_F)) / (Math.log(MAX_F) - Math.log(MIN_F))) * w;
const xf = (x: number, w: number) => MIN_F * Math.pow(MAX_F / MIN_F, Math.min(1, Math.max(0, x / w)));

// ---- channel strip thumbnail ------------------------------------------------------------

export function EqThumb({ trackId, name }: { trackId: Id; name: string }) {
  const eq = useStore((s) => eqOf(s.snap?.project, trackId));
  const sr = useStore((s) => s.snap?.project.sample_rate ?? 48000);
  const open = usePanelShown(eqKey(trackId));
  const themeId = useStore((s) => s.themeId);
  const uiScale = useStore((s) => s.uiScale);
  const ref = useRef<HTMLCanvasElement>(null);
  const [w, setW] = useState(90);
  useLayoutEffect(() => {
    const el = ref.current!.parentElement!;
    const ro = new ResizeObserver(([e]) => setW(Math.max(40, Math.floor(e.contentRect.width))));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  useLayoutEffect(() => {
    const cv = ref.current;
    if (!cv || !eq) return;
    const H = 30;
    const dpr = pixelRatio();
    cv.width = w * dpr;
    cv.height = H * dpr;
    cv.style.width = `${w}px`;
    cv.style.height = `${H}px`;
    const ctx = cv.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, H);
    const mid = H / 2;
    const scale = (H / 2 - 3) / 18;
    ctx.fillStyle = cssVar("--line");
    ctx.fillRect(0, Math.round(mid), w, 1);
    if (isFlat(eq)) return;
    const freqs = Array.from({ length: w }, (_, i) => xf(i, w));
    const db = responseDb(eqSections(eq, sr), freqs, sr).map((d) => d + eq.output_db);
    const accent = cssVar("--accent");
    ctx.beginPath();
    ctx.moveTo(0, mid);
    db.forEach((d, i) => ctx.lineTo(i, mid - Math.max(-18, Math.min(18, d)) * scale));
    ctx.lineTo(w, mid);
    ctx.closePath();
    ctx.globalAlpha = eq.on ? 0.28 : 0.1;
    ctx.fillStyle = accent;
    ctx.fill();
    ctx.globalAlpha = eq.on ? 1 : 0.4;
    ctx.beginPath();
    db.forEach((d, i) => (i ? ctx.lineTo : ctx.moveTo).call(ctx, i, mid - Math.max(-18, Math.min(18, d)) * scale));
    ctx.strokeStyle = accent;
    ctx.lineWidth = 1.25;
    ctx.stroke();
    ctx.globalAlpha = 1;
  }, [eq, w, sr, themeId, uiScale]);
  return (
    <div
      className={"eq-thumb" + (open ? " open" : "") + (eq && !eq.on ? " off" : "")}
      title={`Channel EQ — ${name}`}
      onMouseDown={(e) => {
        e.stopPropagation();
        if (e.button !== 0) return;
        toggleEqPanel(trackId);
      }}
      onContextMenu={(e) => {
        e.preventDefault();
        e.stopPropagation();
        openMenu(e.clientX, e.clientY, eqMenu(trackId, eq));
      }}
    >
      <canvas ref={ref} />
      {eq && isFlat(eq) && <span className="eq-thumb-label">EQ</span>}
    </div>
  );
}

// ---- editor --------------------------------------------------------------------------------

const BAND_NAMES = ["Low Cut", "Low Shelf", "Bell 1", "Bell 2", "Bell 3", "Bell 4", "High Shelf", "High Cut"];
const GAIN_RANGE = 24;
const SLOPES = [12, 24, 48];

function BandIcon({ kind }: { kind: EqBand["kind"] }) {
  const d = {
    low_cut: "M1 11 C4 11 4 3 7 3 H15",
    high_cut: "M1 3 H9 C12 3 12 11 15 11",
    low_shelf: "M1 3 H5 C8 3 8 8 11 8 H15",
    high_shelf: "M1 8 H5 C8 8 8 3 11 3 H15",
    bell: "M1 9 H4 C6 9 6 2 8 2 C10 2 10 9 12 9 H15",
  }[kind];
  return (
    <svg width="16" height="12" viewBox="0 0 16 12" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
      <path d={d} />
    </svg>
  );
}

/** Pixels of a Shift-drag count this much, for fine adjustment. */
const FINE = 0.2;

/**
 * A value changed by dragging vertically; double-click resets. `onScrub` gets the value at
 * mouse-down and the pixels dragged up since then (Shift-drag counting FINE), so the result is
 * worked out from the start of the drag: applying each move's few pixels to an already rounded
 * value would round most of them away.
 */
function Scrub({
  text,
  value,
  onScrub,
  onReset,
  title,
  onContextMenu,
  onGrab,
}: {
  text: string;
  value: number;
  onScrub: (start: number, px: number) => void;
  onReset: () => void;
  title?: string;
  onContextMenu?: (e: React.MouseEvent) => void;
  onGrab?: (on: boolean) => void;
}) {
  return (
    <div
      className="scrub"
      onContextMenu={onContextMenu}
      title={title ?? "Drag up/down · Shift for fine · double-click to reset"}
      onMouseDown={(e) => {
        e.preventDefault();
        const start = value;
        let last = e.clientY;
        let px = 0;
        onGrab?.(true);
        const move = (ev: MouseEvent) => {
          px += (last - ev.clientY) * (ev.shiftKey ? FINE : 1);
          last = ev.clientY;
          onScrub(start, px);
        };
        const up = () => {
          window.removeEventListener("mousemove", move);
          window.removeEventListener("mouseup", up);
          onGrab?.(false);
        };
        window.addEventListener("mousemove", move);
        window.addEventListener("mouseup", up);
      }}
      onDoubleClick={onReset}
    >
      {text}
    </div>
  );
}

/** A control of the editor, for Learn and Touch: a band's field, or the output level. */
export type EqControl = { band: number; field: "on" | "freq" | "gain" | "q" } | { band: null; field: "output" };

export interface EqEditorProps {
  eq: ChannelEq;
  sr: number;
  /** Every edit, as the whole new setting. */
  onChange: (eq: ChannelEq) => void;
  /** Toolbar controls before the Analyzer button, after it, and before the Output level. */
  lead?: ReactNode;
  tools?: ReactNode;
  extra?: ReactNode;
  /** The strip whose spectrum the analyzer shows; without one there is no analyzer. */
  analyzerTrack?: Id;
  /** A control's right-click handler (Learn MIDI Assignment). */
  learn?: (c: EqControl) => ((e: React.MouseEvent) => void) | undefined;
  /** A drag starts or ends, with the controls it moves (for Touch and Latch automation). */
  onGrab?: (controls: EqControl[], on: boolean) => void;
}

/**
 * The Channel EQ editor for one strip (MASTER_ID = Stereo Out): toolbar, graph and bands. It
 * fills the box it is given (docked in the bottom section or in a floating window, see
 * fxDock.ts); the graph takes the height the toolbar and bands leave.
 */
export function EqPanel({ track: trackId }: { track: Id }) {
  const project = useStore((s) => s.snap?.project);
  const eq = eqOf(project, trackId);
  const sr = project?.sample_rate ?? 48000;
  if (!eq) return null;
  return (
    <EqEditor
      eq={eq}
      sr={sr}
      onChange={(e) => updateEq(trackId, e)}
      analyzerTrack={trackId}
      lead={
        <button className={"eq-toggle" + (eq.on ? " on" : "")} onClick={() => updateEq(trackId, { ...eq, on: !eq.on })} title="Turn the EQ on or off">
          {eq.on ? "On" : "Bypassed"}
        </button>
      }
      tools={
        <>
          <button className="eq-toggle" onClick={() => updateEq(trackId, defaultEq())} title="Reset every band">
            Reset
          </button>
          <PresetButton effect={CHANNEL_EQ} track={trackId} slot={EQ_SLOT} instance={`eq:${trackId}`} current={JSON.stringify(eq)} normalize={(p) => JSON.stringify(p.eq)} defaultName="Flat" />
          <button className="eq-toggle" onClick={() => copyEq(eq)} title="Copy these EQ settings">
            Copy
          </button>
          <button
            className="eq-toggle"
            onClick={() => (eqClipboard() ? pasteEq([trackId]) : setState({ toast: { text: "Copy a Channel EQ first", at: Date.now() } }))}
            title="Paste copied EQ settings onto this strip"
          >
            Paste
          </button>
        </>
      }
      learn={(c) => learnContext(c.band === null ? { type: "eq_output", track: trackId } : { type: "eq", track: trackId, band: c.band, field: c.field })}
    />
  );
}

/**
 * The graph and band controls of the Channel EQ, for any eight-band setting: a strip's
 * Channel EQ (EqPanel) or the Linear Phase EQ's parameters (fx/LinearPhaseEq.tsx).
 */
export function EqEditor({ eq, sr, onChange, lead, tools, extra, analyzerTrack, learn, onGrab }: EqEditorProps) {
  const [sel, setSel] = useState<number | null>(null);
  const [analyzerOn, setAnalyzer] = useState(true);
  const analyzer = analyzerOn && analyzerTrack !== undefined;
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const wrapRef = useRef<HTMLDivElement>(null);
  const spectrum = useRef<Float32Array | null>(null);
  const live = useRef({ eq, sel, sr, analyzer });
  live.current = { eq, sel, sr, analyzer };
  useStore((s) => s.themeId);
  useStore((s) => s.uiScale);

  // Feed the analyzer from this strip while the editor is open.
  useEffect(() => {
    if (analyzerTrack === undefined) return;
    call("set_analyzer", { track: analyzer ? analyzerTrack : null });
    spectrum.current = null;
    return () => {
      call("set_analyzer", { track: null });
    };
  }, [analyzerTrack, analyzer]);

  const draw = () => {
    const cv = canvasRef.current;
    const wrap = wrapRef.current;
    const { eq, sel, sr, analyzer } = live.current;
    if (!cv || !wrap || !eq) return;
    const W = wrap.clientWidth;
    const H = wrap.clientHeight;
    const dpr = pixelRatio();
    if (cv.width !== Math.round(W * dpr) || cv.height !== Math.round(H * dpr)) {
      cv.width = Math.round(W * dpr);
      cv.height = Math.round(H * dpr);
      cv.style.width = `${W}px`;
      cv.style.height = `${H}px`;
    }
    const ctx = cv.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, W, H);
    const gy = (db: number) => H / 2 - (db / GAIN_RANGE) * (H / 2 - 12);
    const line = cssVar("--line");
    const faint = cssVar("--faint");
    const text = cssVar("--text");
    const accent = cssVar("--accent");

    // Grid
    ctx.font = "10px Inter, system-ui, sans-serif";
    ctx.textBaseline = "middle";
    for (const f of [20, 50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000]) {
      const x = Math.round(fx(f, W)) + 0.5;
      ctx.fillStyle = line;
      ctx.globalAlpha = 0.55;
      ctx.fillRect(x, 0, 1, H);
      ctx.globalAlpha = 1;
      ctx.fillStyle = faint;
      const label = f >= 1000 ? `${f / 1000}k` : `${f}`;
      ctx.fillText(label, Math.min(W - 18, x + 3), H - 8);
    }
    for (let db = -GAIN_RANGE; db <= GAIN_RANGE; db += 6) {
      const y = Math.round(gy(db)) + 0.5;
      ctx.fillStyle = line;
      ctx.globalAlpha = db === 0 ? 1 : 0.45;
      ctx.fillRect(0, y, W, 1);
      ctx.globalAlpha = 1;
      if (db !== -GAIN_RANGE && db !== GAIN_RANGE) {
        ctx.fillStyle = faint;
        ctx.fillText(`${db > 0 ? "+" : ""}${db}`, 4, y - 6);
      }
    }

    // Analyzer (-96..0 dBFS over the full height)
    const spec = spectrum.current;
    if (analyzer && spec) {
      ctx.beginPath();
      ctx.moveTo(0, H);
      const n = spec.length;
      for (let i = 0; i < n; i++) {
        const x = ((i + 0.5) / n) * W;
        const y = H - ((Math.max(-96, spec[i]) + 96) / 96) * H;
        ctx.lineTo(x, y);
      }
      ctx.lineTo(W, H);
      ctx.closePath();
      ctx.fillStyle = faint;
      ctx.globalAlpha = 0.28;
      ctx.fill();
      ctx.globalAlpha = 1;
    }

    const freqs = Array.from({ length: Math.ceil(W / 2) + 1 }, (_, i) => xf(i * 2, W));
    // The selected band's own contribution
    if (sel !== null && eq.bands[sel]?.on) {
      const db = responseDb(bandSections(eq.bands[sel], sr), freqs, sr);
      ctx.beginPath();
      ctx.moveTo(0, gy(0));
      db.forEach((d, i) => ctx.lineTo(i * 2, gy(Math.max(-GAIN_RANGE - 6, d))));
      ctx.lineTo(W, gy(0));
      ctx.closePath();
      ctx.fillStyle = BAND_COLORS[sel];
      ctx.globalAlpha = 0.22;
      ctx.fill();
      ctx.globalAlpha = 1;
    }
    // Composite curve
    const total = responseDb(eqSections(eq, sr), freqs, sr).map((d) => d + (eq.on ? eq.output_db : 0));
    ctx.beginPath();
    total.forEach((d, i) => {
      const y = gy(Math.max(-GAIN_RANGE - 6, Math.min(GAIN_RANGE + 6, d)));
      if (i) ctx.lineTo(i * 2, y);
      else ctx.moveTo(0, y);
    });
    ctx.strokeStyle = eq.on ? accent : faint;
    ctx.lineWidth = 2;
    ctx.stroke();

    // Handles
    eq.bands.forEach((b, i) => {
      const x = fx(b.freq, W);
      const isCut = b.kind === "low_cut" || b.kind === "high_cut";
      const y = gy(isCut ? 0 : b.gain);
      ctx.beginPath();
      ctx.arc(x, y, i === sel ? 7 : 5.5, 0, Math.PI * 2);
      if (b.on && eq.on) {
        ctx.fillStyle = BAND_COLORS[i];
        ctx.fill();
        if (i === sel) {
          ctx.strokeStyle = text;
          ctx.lineWidth = 2;
          ctx.stroke();
        }
      } else {
        ctx.strokeStyle = BAND_COLORS[i];
        ctx.lineWidth = 1.5;
        ctx.globalAlpha = 0.6;
        ctx.stroke();
        ctx.globalAlpha = 1;
      }
    });
  };

  useLayoutEffect(() => draw());
  useEffect(() => {
    const ro = new ResizeObserver(() => draw());
    if (wrapRef.current) ro.observe(wrapRef.current);
    const off = onTick((t) => {
      if (!t.spectrum || t.spectrum[0] !== analyzerTrack) return;
      const fresh = t.spectrum[1];
      const prev = spectrum.current;
      // Fast attack, slow release, like an analyzer's peak response.
      spectrum.current = Float32Array.from(fresh, (v, i) => (prev ? Math.max(v, prev[i] - 1.5) : v));
      draw();
    });
    return () => {
      ro.disconnect();
      off();
    };
  }, [analyzerTrack]);

  const setBand = (i: number, patch: Partial<EqBand>) => {
    const cur = live.current.eq;
    onChange({ ...cur, bands: cur.bands.map((b, j) => (j === i ? { ...b, ...patch } : b)) });
  };
  const grab = (band: number, fields: ("freq" | "gain" | "q")[]) => (on: boolean) => onGrab?.(fields.map((field) => ({ band, field })), on);

  const onCanvasDown = (e: React.MouseEvent) => {
    const rect = canvasRef.current!.getBoundingClientRect();
    const W = rect.width;
    const H = rect.height;
    const gy = (db: number) => H / 2 - (db / GAIN_RANGE) * (H / 2 - 12);
    const px = e.clientX - rect.left;
    const py = e.clientY - rect.top;
    let best = -1;
    let bestD = 14;
    eq.bands.forEach((b, i) => {
      const isCut = b.kind === "low_cut" || b.kind === "high_cut";
      const d = Math.hypot(fx(b.freq, W) - px, gy(isCut ? 0 : b.gain) - py);
      if (d < bestD) {
        bestD = d;
        best = i;
      }
    });
    if (best < 0) {
      setSel(null);
      return;
    }
    setSel(best);
    const band = eq.bands[best];
    if (e.altKey) {
      setBand(best, { gain: 0 });
      return;
    }
    if (e.detail === 2) {
      setBand(best, { on: !band.on });
      return;
    }
    const isCut = band.kind === "low_cut" || band.kind === "high_cut";
    const startX = e.clientX;
    const startY = e.clientY;
    const f0 = band.freq;
    const g0 = band.gain;
    const held = grab(best, isCut ? ["freq"] : ["freq", "gain"]);
    held(true);
    const move = (ev: MouseEvent) => {
      const fine = ev.shiftKey ? 0.2 : 1;
      const freq = xf(fx(f0, W) + (ev.clientX - startX) * fine, W);
      const gain = isCut ? band.gain : Math.max(-GAIN_RANGE, Math.min(GAIN_RANGE, g0 - ((ev.clientY - startY) * fine * GAIN_RANGE) / (H / 2 - 12)));
      setBand(best, { freq: Math.round(freq * 10) / 10, gain: Math.round(gain * 10) / 10, on: true });
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      held(false);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  const onWheel = (e: React.WheelEvent) => {
    if (sel === null) return;
    const b = eq.bands[sel];
    if ((b.kind === "low_cut" || b.kind === "high_cut") && b.slope > 12) return;
    const q = Math.min(18, Math.max(0.1, b.q * (e.deltaY < 0 ? 1.08 : 1 / 1.08)));
    setBand(sel, { q: Math.round(q * 100) / 100 });
  };

  const defaults = defaultEq();
  return (
    <div className="eq-panel">
      <div className="eq-toolbar">
        {lead}
        {analyzerTrack !== undefined && (
          <button className={"eq-toggle" + (analyzer ? " on" : "")} onClick={() => setAnalyzer(!analyzer)} title="Show the post-EQ spectrum">
            Analyzer
          </button>
        )}
        {tools}
        <span className="eq-spacer" />
        {extra}
        <span className="dim">Output</span>
        <Scrub
          text={`${eq.output_db > 0 ? "+" : ""}${eq.output_db.toFixed(1)} dB`}
          value={eq.output_db}
          // The current EQ, not this render's: bands may change while the drag runs.
          onScrub={(v0, px) => onChange({ ...live.current.eq, output_db: Math.round(Math.max(-24, Math.min(24, v0 + px * 0.1)) * 10) / 10 })}
          onReset={() => onChange({ ...live.current.eq, output_db: 0 })}
          onContextMenu={learn?.({ band: null, field: "output" })}
          onGrab={(on) => onGrab?.([{ band: null, field: "output" }], on)}
        />
      </div>
      <div className="eq-graph" ref={wrapRef} onMouseDown={onCanvasDown} onWheel={onWheel}>
        <canvas ref={canvasRef} />
      </div>
      <div className="eq-bands">
        {eq.bands.map((b, i) => {
          const isCut = b.kind === "low_cut" || b.kind === "high_cut";
          return (
            <div key={i} className={"eq-band" + (i === sel ? " sel" : "")} onMouseDown={() => setSel(i)}>
              <button
                className={"eq-band-toggle" + (b.on ? " on" : "")}
                style={{ ["--band" as string]: BAND_COLORS[i] }}
                onClick={() => setBand(i, { on: !b.on })}
                onContextMenu={learn?.({ band: i, field: "on" })}
                title={`${BAND_NAMES[i]} on/off`}
              >
                <BandIcon kind={b.kind} />
              </button>
              <div className="eq-band-name">{BAND_NAMES[i]}</div>
              <Scrub
                text={formatFreq(b.freq)}
                value={b.freq}
                onScrub={(v0, px) => setBand(i, { freq: Math.round(Math.min(20000, Math.max(20, v0 * Math.pow(1.01, px))) * 10) / 10 })}
                onReset={() => setBand(i, { freq: defaults.bands[i].freq })}
                onContextMenu={learn?.({ band: i, field: "freq" })}
                onGrab={grab(i, ["freq"])}
              />
              {isCut ? (
                <button
                  className="scrub"
                  title="Slope"
                  onClick={() => setBand(i, { slope: SLOPES[(SLOPES.indexOf(b.slope) + 1) % SLOPES.length] })}
                >
                  {b.slope} dB/Oct
                </button>
              ) : (
                <Scrub
                  text={`${b.gain > 0 ? "+" : ""}${b.gain.toFixed(1)} dB`}
                  value={b.gain}
                  onScrub={(v0, px) => setBand(i, { gain: Math.round(Math.max(-24, Math.min(24, v0 + px * 0.1)) * 10) / 10, on: true })}
                  onReset={() => setBand(i, { gain: 0 })}
                  onContextMenu={learn?.({ band: i, field: "gain" })}
                  onGrab={grab(i, ["gain"])}
                />
              )}
              {isCut && b.slope > 12 ? (
                <div className="scrub disabled">Q —</div>
              ) : (
                <Scrub
                  text={`Q ${b.q.toFixed(2)}`}
                  value={b.q}
                  onScrub={(v0, px) => setBand(i, { q: Math.round(Math.min(18, Math.max(0.1, v0 * Math.pow(1.01, px))) * 100) / 100 })}
                  onReset={() => setBand(i, { q: defaults.bands[i].q })}
                  onContextMenu={learn?.({ band: i, field: "q" })}
                  onGrab={grab(i, ["q"])}
                />
              )}
            </div>
          );
        })}
      </div>
      <div className="eq-hint">Drag a point to set frequency and gain · scroll to change Q · double-click a point to switch the band off · {fmt("Alt")}-click resets its gain</div>
    </div>
  );
}
