// The Delay panel (builtin:delay): mode, left/right times (ms or synced note values), the
// feedback loop, Tape-only controls, and a picture of the repeats the settings produce.
import type { FxPanelProps } from "../../fx";
import { useStore } from "../../store";
import { cssVar, fitCanvas, formatMs, noteMs, SpKnob, SpNote, SpSegments, SpToggle, useCanvas, val } from "./Reverb";

/** Parameter ids (crates/engine/src/fx/delay.rs). */
const D = {
  mode: 0,
  sync: 1,
  timeL: 2,
  timeR: 3,
  noteL: 4,
  noteR: 5,
  link: 6,
  feedbackL: 7,
  feedbackR: 8,
  cross: 9,
  lowCut: 10,
  highCut: 11,
  modRate: 12,
  modDepth: 13,
  wow: 14,
  flutter: 15,
  saturation: 16,
  smooth: 17,
  width: 18,
  dry: 19,
  wet: 20,
  freeze: 21,
};

// Modes: 0 Stereo, 1 Ping-Pong, 2 Tape, 3 Digital.
const PING_PONG = 1;
const TAPE = 2;
const DIGITAL = 3;

interface Echo {
  t: number;
  ch: number;
  a: number;
  /** How many trips round the loop it took (for Tape's darkening). */
  n: number;
}

const FLOOR_DB = -48;

/** Follows an impulse round the loop, the way the engine routes it, until it fades out. */
function echoes(mode: number, times: [number, number], g: [number, number], cross: number, span: number): Echo[] {
  const floor = Math.pow(10, FLOOR_DB / 20);
  const queue: Echo[] = mode === PING_PONG ? [{ t: 0, ch: 0, a: 1, n: 0 }] : [{ t: 0, ch: 0, a: 1, n: 0 }, { t: 0, ch: 1, a: 1, n: 0 }];
  const out = new Map<string, Echo>();
  let guard = 0;
  while (queue.length && guard++ < 600) {
    // Earliest first, so echoes landing together merge before they feed back.
    let k = 0;
    for (let i = 1; i < queue.length; i++) if (queue[i].t < queue[k].t) k = i;
    const e = queue.splice(k, 1)[0];
    const t = e.t + times[e.ch];
    // Saturation keeps runaway feedback at full scale.
    const a = Math.min(1, e.a);
    if (t > span || Math.abs(a) < floor) continue;
    const key = `${e.ch}:${Math.round(t * 1e5)}`;
    const hit = out.get(key);
    if (hit) hit.a = Math.min(1, hit.a + a);
    else out.set(key, { t, ch: e.ch, a, n: e.n });
    if (mode === PING_PONG) {
      queue.push({ t, ch: 1 - e.ch, a: a * g[0], n: e.n + 1 });
    } else {
      const own = a * g[e.ch] * (1 - cross);
      const other = a * g[e.ch] * cross;
      if (own > floor) queue.push({ t, ch: e.ch, a: own, n: e.n + 1 });
      if (other > floor) queue.push({ t, ch: 1 - e.ch, a: other, n: e.n + 1 });
    }
  }
  return [...out.values()];
}

function DelayGraph({ mode, times, gains, cross, freeze, beatMs }: { mode: number; times: [number, number]; gains: [number, number]; cross: number; freeze: boolean; beatMs: number | null }) {
  const ref = useCanvas(
    (cv) => {
      const { ctx, w, h } = fitCanvas(cv);
      const pad = 8;
      const mid = Math.round(h / 2) + 0.5;
      const half = h / 2 - 14;
      const longest = Math.max(times[0], times[1]) / 1000;
      const g = Math.max(gains[0], gains[1]);
      // Show enough repeats to read the decay: until −48 dB, at least two, at most 8 s.
      const fade = freeze || g >= 0.999 ? 8 : g < 0.01 ? 1 : Math.ceil(Math.log(Math.pow(10, FLOOR_DB / 20)) / Math.log(g)) + 1;
      const span = Math.min(8, Math.max(2.2 * longest, Math.min(fade, 12) * longest * 1.05, 0.12));
      const x = (t: number) => pad + ((w - 2 * pad) * t) / span;
      const line = cssVar("--line");
      const faint = cssVar("--faint");
      const accent = cssVar("--accent", "#4c9eff");
      const text = cssVar("--text");
      ctx.font = "9.5px Inter, system-ui, sans-serif";

      // Beat grid when synced, otherwise a plain time grid.
      const step = beatMs ? beatMs / 1000 : [0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1].find((s) => span / s <= 10) ?? 1;
      for (let i = 1; i * step < span; i++) {
        const gx = Math.round(x(i * step)) + 0.5;
        ctx.fillStyle = line;
        ctx.globalAlpha = beatMs && i % 4 === 0 ? 0.9 : 0.45;
        ctx.fillRect(gx, 8, 1, h - 16);
      }
      ctx.globalAlpha = 1;
      ctx.fillStyle = line;
      ctx.fillRect(pad, mid, w - 2 * pad, 1);
      ctx.fillStyle = faint;
      ctx.fillText("L", pad, 16);
      ctx.fillText("R", pad, h - 8);
      ctx.textAlign = "right";
      ctx.fillText(beatMs ? "beats" : formatMs(span * 1000), w - pad, h - 8);
      ctx.textAlign = "left";

      // The dry hit at zero.
      ctx.fillStyle = text;
      ctx.globalAlpha = 0.35;
      ctx.fillRect(Math.round(x(0)), mid - half, 2, half * 2);
      ctx.globalAlpha = 1;

      const gainsShown: [number, number] = freeze ? [1, 1] : gains;
      for (const e of echoes(mode, times.map((t) => t / 1000) as [number, number], gainsShown, cross, span)) {
        const db = Math.max(FLOOR_DB, 20 * Math.log10(Math.max(1e-6, e.a)));
        const len = half * (1 - db / FLOOR_DB);
        const ex = Math.round(x(e.t));
        // Tape darkens each trip round the loop; the bars fade to show it.
        ctx.globalAlpha = mode === TAPE && !freeze ? Math.max(0.3, 1 - 0.09 * e.n) : 1;
        ctx.fillStyle = accent;
        if (e.ch === 0) ctx.fillRect(ex - 1, mid - len, 3, len);
        else ctx.fillRect(ex - 1, mid + 1, 3, len);
      }
      ctx.globalAlpha = 1;
      if (freeze) {
        ctx.fillStyle = text;
        ctx.textAlign = "right";
        ctx.fillText("FREEZE", w - pad, 16);
        ctx.textAlign = "left";
      }
    },
    [mode, times[0], times[1], gains[0], gains[1], cross, freeze, beatMs],
  );
  return (
    <div className="sp-graph">
      <canvas ref={ref} />
    </div>
  );
}

/** The custom panel for builtin:delay. */
export function DelayPanel(p: FxPanelProps) {
  const tempo = useStore((s) => s.snap?.project.tempo ?? 120);
  const mode = Math.round(val(p, D.mode));
  const sync = val(p, D.sync) >= 0.5;
  const freeze = val(p, D.freeze) >= 0.5;
  const linked = mode === DIGITAL || val(p, D.link) >= 0.5;
  // Times and loop gains exactly as the engine works them out (times_ms, feedback).
  const time = (ms: number, note: number) => Math.min(4000, Math.max(1, sync ? noteMs(Math.round(val(p, note)), tempo) : val(p, ms)));
  const tl = time(D.timeL, D.noteL);
  const tr = linked ? tl : time(D.timeR, D.noteR);
  const fbl = val(p, D.feedbackL);
  const fbr = mode === DIGITAL || mode === PING_PONG || linked ? fbl : val(p, D.feedbackR);
  const loop = (fb: number) => (mode === DIGITAL ? Math.min(fb, 0.98) : mode === TAPE ? fb * 1.1 : fb);
  const oneFeedback = mode === DIGITAL || mode === PING_PONG || linked;

  const timeControl = (ms: number, note: number, label: string, disabled = false) =>
    sync && !disabled ? <SpNote p={p} id={note} label={label} /> : <SpKnob p={p} id={ms} label={label} size={56} disabled={disabled} />;

  return (
    <div className="sp-panel sp-delay">
      <div className="sp-bar">
        <SpSegments p={p} id={D.mode} />
        <div className="sp-spacer" />
        <SpToggle p={p} id={D.sync} label="Sync" title="Times in note values at the project tempo" />
        {mode !== DIGITAL && <SpToggle p={p} id={D.link} label="Link L/R" title="The right side follows the left's time and feedback" />}
        <SpToggle p={p} id={D.freeze} title="Hold the repeats indefinitely; new input is ignored" />
      </div>
      <div className="sp-main">
        <div className="sp-col">
          {timeControl(D.timeL, D.noteL, linked ? "Time" : "Left")}
          <SpKnob p={p} id={D.feedbackL} label={oneFeedback ? "Feedback" : "Feedback L"} size={48} />
        </div>
        <DelayGraph
          mode={mode}
          times={[tl, tr]}
          gains={[loop(fbl), loop(fbr)]}
          cross={mode === PING_PONG ? 0 : val(p, D.cross)}
          freeze={freeze}
          beatMs={sync ? 60000 / tempo : null}
        />
        <div className={"sp-col" + (linked ? " sp-linked" : "")}>
          {timeControl(D.timeR, D.noteR, "Right", linked)}
          {oneFeedback ? <div className="sp-col-note">{linked ? "linked to left" : "one loop"}</div> : <SpKnob p={p} id={D.feedbackR} label="Feedback R" size={48} />}
        </div>
      </div>
      <div className="sp-row">
        <div className="sp-group">
          <div className="sp-group-name">Loop</div>
          {mode !== PING_PONG && <SpKnob p={p} id={D.cross} />}
          <SpKnob p={p} id={D.lowCut} format={(v) => (v <= 20.5 ? "Off" : `${Math.round(v)} Hz`)} />
          <SpKnob p={p} id={D.highCut} format={(v) => (v >= 19999 ? "Off" : v >= 1000 ? `${(v / 1000).toFixed(1)} kHz` : `${Math.round(v)} Hz`)} />
        </div>
        <div className="sp-group">
          <div className="sp-group-name">Modulation</div>
          <SpKnob p={p} id={D.modRate} label="Rate" />
          <SpKnob p={p} id={D.modDepth} label="Depth" />
        </div>
        <div className="sp-group">
          <div className="sp-group-name">Output</div>
          <SpKnob p={p} id={D.width} />
          <SpKnob p={p} id={D.dry} />
          <SpKnob p={p} id={D.wet} />
        </div>
      </div>
      {mode === TAPE && (
        <div className="sp-row sp-tape">
          <div className="sp-group">
            <div className="sp-group-name">Tape</div>
            <SpKnob p={p} id={D.wow} />
            <SpKnob p={p} id={D.flutter} />
            <SpKnob p={p} id={D.saturation} />
            <SpKnob p={p} id={D.smooth} />
          </div>
        </div>
      )}
    </div>
  );
}
