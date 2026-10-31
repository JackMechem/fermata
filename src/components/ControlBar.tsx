import { useEffect, useRef, useState } from "react";
import { act } from "../api";
import { Icon } from "../icons";
import { getState, onTick, setState, useStore } from "../store";
import { cpuTitle, shownPeak } from "../cpuMeter";
import { framePos, onFrame } from "../frame";
import { formatTime, toBbt } from "../time";
import { fmtBpm, tempoMap } from "../tempo";
import { HSlider } from "./Controls";
import { openMenuAt, type MenuItem } from "./Menu";
import { transport } from "../transport";
import { fmt, keyHint } from "../keymap";
import { updateSettings, useSettings } from "../prefs";

const KEYS = ["C", "C♯", "D", "E♭", "E", "F", "F♯", "G", "A♭", "A", "B♭", "B"];

function ViewButton({ on, title, onClick, children }: { on: boolean; title: string; onClick: () => void; children: React.ReactNode }) {
  return (
    <button className={"cb-btn" + (on ? " on" : "")} title={title} onClick={onClick}>
      {children}
    </button>
  );
}

/** Vertical drag on a number, like Logic's LCD fields. `onStep` gets the steps since the
 *  last call and the total since the mouse went down. */
function useScrub(onStep: (steps: number, total: number) => void, px = 6) {
  return (e: React.MouseEvent) => {
    // A text field in the field (the tempo being typed) keeps the mouse for its caret.
    if (e.detail > 1 || (e.target as HTMLElement).tagName === "INPUT") return;
    e.preventDefault();
    let acc = 0;
    let total = 0;
    let last = e.clientY;
    const move = (ev: MouseEvent) => {
      acc += last - ev.clientY;
      last = ev.clientY;
      const steps = Math.trunc(acc / px);
      if (steps !== 0) {
        acc -= steps * px;
        total += steps;
        onStep(steps, total);
      }
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };
}

function Lcd() {
  const project = useStore((s) => s.snap?.project);
  const [pos, setPos] = useState(0);
  const [mode, setMode] = useState<"beats" | "time">("beats");
  const modeRef = useRef(mode);
  modeRef.current = mode;
  const [editTempo, setEditTempo] = useState(false);
  const posRef = useRef(0);
  const shown = useRef("");
  // Follows the interpolated playhead every frame, re-rendering only when the digits change.
  useEffect(
    () =>
      onFrame((now) => {
        const p = getState().snap?.project;
        if (!p) return;
        const pos = framePos(now);
        posRef.current = pos;
        const b = toBbt(p, pos);
        // The tempo and signature at the playhead follow it too (tempo changes and ramps).
        const m = tempoMap(p);
        const [num, den] = m.sigAt(pos);
        const key = `${modeRef.current === "time" ? formatTime(p, pos) : `${b.bar}:${b.beat}`} ${fmtBpm(m.bpmAt(pos))} ${num}/${den}`;
        if (key !== shown.current) {
          shown.current = key;
          setPos(pos);
        }
      }),
    [],
  );
  const scrubBar = useScrub((n) => {
    const p = getState().snap?.project;
    if (!p) return;
    const m = tempoMap(p);
    act("locate", { pos: Math.max(0, Math.round(m.barStart(m.barAt(posRef.current) + n))) });
  }, 8);
  // The tempo in effect at the playhead (the project's when there are no changes). The drag
  // counts from the tempo at mouse-down.
  const moveMidi = () => getState().moveMidiWithTempo;
  const tempoNow = (p: typeof project) => (p ? tempoMap(p).events[tempoMap(p).eventIndexAt(posRef.current)].bpm : 120);
  const scrubTempo = useScrub((_, total) => project && act("set_tempo_at", { pos: Math.round(posRef.current), bpm: Math.round(tempoNow(project)) + total, moveMidi: moveMidi() }), 3);
  const cancelTempo = useRef(false);
  const commitTempo = (text: string) => {
    setEditTempo(false);
    const v = parseFloat(text);
    if (!cancelTempo.current && v > 0 && project && v !== tempoNow(project)) act("set_tempo_at", { pos: Math.round(posRef.current), bpm: v, moveMidi: moveMidi() });
    cancelTempo.current = false;
  };
  if (!project) return <div className="lcd" />;
  const bbt = toBbt(project, pos);
  const map = tempoMap(project);
  const bpmHere = map.bpmAt(pos);
  const [sigNum, sigDen] = map.sigAt(pos);
  const keyRoot = project.key.split(" ")[0];
  const keyMinor = project.key.includes("minor");

  const sigMenu = (e: React.MouseEvent) => {
    const items: MenuItem[] = [];
    // The signature in effect at the playhead's bar.
    const bar = map.barAt(posRef.current);
    for (const [n, d] of [[2, 4], [3, 4], [4, 4], [5, 4], [6, 8], [7, 8], [9, 8], [12, 8]] as const) {
      items.push({ label: `${n}/${d}`, checked: sigNum === n && sigDen === d, onClick: () => act("set_signature_at", { bar, num: n, den: d }) });
    }
    openMenuAt(e.currentTarget as HTMLElement, items);
  };
  const keyMenu = (e: React.MouseEvent) => {
    const mk = (minor: boolean): MenuItem[] =>
      KEYS.map((k) => ({
        label: `${k} ${minor ? "minor" : "major"}`,
        checked: keyRoot === k && keyMinor === minor,
        onClick: () => act("set_key", { key: `${k} ${minor ? "minor" : "major"}` }),
      }));
    openMenuAt(e.currentTarget as HTMLElement, [{ label: "Major", children: mk(false) }, { label: "Minor", children: mk(true) }]);
  };

  return (
    <div className="lcd">
      {mode === "beats" ? (
        <div className="lcd-field lcd-pos" onMouseDown={scrubBar} title="Drag to move the playhead by bars">
          <div className="lcd-digits">
            <span className="lcd-bar">{bbt.bar}</span>
            <span className="lcd-beat">{bbt.beat}</span>
          </div>
          <div className="lcd-labels">
            <span>BAR</span>
            <span>BEAT</span>
          </div>
        </div>
      ) : (
        <div className="lcd-field lcd-pos lcd-time">
          <div className="lcd-digits">{formatTime(project, pos)}</div>
          <div className="lcd-labels">
            <span>TIME</span>
          </div>
        </div>
      )}
      <div className="lcd-sep" />
      <div className="lcd-field lcd-tempo" onMouseDown={scrubTempo} onDoubleClick={() => setEditTempo(true)} title="Drag or double-click to change the tempo">
        {editTempo ? (
          <input
            className="lcd-input"
            autoFocus
            defaultValue={fmtBpm(tempoNow(project))}
            // Clicking away keeps what was typed.
            onBlur={(e) => commitTempo(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") e.currentTarget.blur();
              if (e.key === "Escape") {
                cancelTempo.current = true;
                e.currentTarget.blur();
              }
              e.stopPropagation();
            }}
          />
        ) : (
          <div className="lcd-digits">{Number.isInteger(Math.round(bpmHere * 100) / 100) ? Math.round(bpmHere) : bpmHere.toFixed(2)}</div>
        )}
        <div className="lcd-labels">
          <span>TEMPO</span>
        </div>
      </div>
      <div className="lcd-sep" />
      <div className="lcd-field lcd-stack">
        <div className="lcd-small" onMouseDown={sigMenu}>
          {sigNum}/{sigDen}
        </div>
        <div className="lcd-small" onMouseDown={keyMenu}>
          {keyRoot}
          {keyMinor ? "min" : "maj"}
        </div>
      </div>
      <button className="lcd-mode" title="Display mode" onClick={(e) => openMenuAt(e.currentTarget, [
        { label: "Beats & Project", checked: mode === "beats", onClick: () => setMode("beats") },
        { label: "Time", checked: mode === "time", onClick: () => setMode("time") },
        { separator: true },
        { label: "Project Settings…", hint: keyHint("project-settings"), onClick: () => setState({ dialog: "project-settings" }) },
      ])}>
        <Icon.chevron />
      </button>
    </div>
  );
}

function TransportButtons() {
  const [t, setT] = useState({ playing: false, recording: false });
  useEffect(() => onTick((tk) => setT((p) => (p.playing === tk.playing && p.recording === tk.recording ? p : { playing: tk.playing, recording: tk.recording }))), []);
  return (
    <div className="transport">
      <button className="tp-btn" title="Rewind (,)" onClick={() => transport.nudgeBars(-1)}>
        <Icon.rewind />
      </button>
      <button className="tp-btn" title="Forward (.)" onClick={() => transport.nudgeBars(1)}>
        <Icon.forward />
      </button>
      <button className="tp-btn" title="Stop" onClick={() => transport.stop()}>
        <Icon.stop />
      </button>
      <button className={"tp-btn play" + (t.playing && !t.recording ? " on" : "")} title={`Play (${keyHint("play")})`} onClick={() => act("play")}>
        <Icon.play />
      </button>
      <button className={"tp-btn rec" + (t.recording ? " on" : "")} title={`Record (${keyHint("record")})`} onClick={() => act("record")}>
        <Icon.record />
      </button>
    </div>
  );
}

function CpuMeter() {
  const [cpu, setCpu] = useState(0);
  const [blockPeak, setBlockPeak] = useState(0);
  const [lateBlocks, setLateBlocks] = useState(0);
  const [threads, setThreads] = useState(1);
  const [cleared, setCleared] = useState(0);
  useEffect(() => onTick((t) => {
    setCpu((c) => (Math.abs(c - t.cpu) > 0.01 ? t.cpu : c));
    setBlockPeak((p) => (Math.abs(p - t.cpu_peak) > 0.01 ? t.cpu_peak : p));
    setLateBlocks(t.late_blocks);
    setThreads(t.threads);
  }), []);
  const peak = shownPeak(cpu, blockPeak);
  const late = lateBlocks - cleared;
  return (
    <div className={"cpu" + (late > 0 ? " over" : "")} title={cpuTitle(cpu, peak, late, threads)} onClick={() => setCleared(lateBlocks)}>
      <span>CPU</span>
      <div className="cpu-bar">
        <div style={{ width: `${Math.min(100, cpu * 100)}%`, background: cpu > 0.8 ? "#f0524a" : cpu > 0.5 ? "#f0b43c" : "#8d9099" }} />
        <div className="cpu-peak" style={{ left: `${Math.min(100, peak * 100)}%` }} />
      </div>
    </div>
  );
}

export function ControlBar() {
  // Subscribe to exactly what the bar shows, so unrelated edits don't re-render it.
  const showLibrary = useStore((s) => s.showLibrary);
  const showInspector = useStore((s) => s.showInspector);
  const showBrowser = useStore((s) => s.showBrowser);
  const bottom = useStore((s) => s.bottom);
  const cycleOn = useStore((s) => s.snap?.project.cycle.enabled ?? false);
  const punchOn = useStore((s) => s.snap?.project.punch.enabled ?? false);
  const countIn = useStore((s) => s.snap?.project.count_in ?? false);
  const metronome = useStore((s) => s.snap?.project.metronome ?? false);
  const masterDb = useStore((s) => s.snap?.project.master.volume_db ?? 0);
  const showTyping = useStore((s) => s.showTyping);
  const replace = useSettings((s) => s.prefs.recording.replace ?? false);
  const s = { showLibrary, showInspector, showBrowser, bottom };
  const p = { cycle: { enabled: cycleOn }, count_in: countIn, metronome, master: { volume_db: masterDb } };
  return (
    <div className="control-bar">
      <div className="cb-group">
        <ViewButton on={s.showLibrary} title={`Library (${keyHint("library")})`} onClick={() => setState({ showLibrary: !s.showLibrary })}>
          <Icon.library />
        </ViewButton>
        <ViewButton on={s.showInspector} title={`Inspector (${keyHint("inspector")})`} onClick={() => setState({ showInspector: !s.showInspector })}>
          <Icon.inspector />
        </ViewButton>
        <ViewButton on={s.bottom === "mixer"} title={`Mixer (${keyHint("mixer")})`} onClick={() => setState({ bottom: s.bottom === "mixer" ? "none" : "mixer" })}>
          <Icon.mixer />
        </ViewButton>
        <ViewButton on={s.bottom === "editor"} title={`Editors (${keyHint("editor")})`} onClick={() => setState({ bottom: s.bottom === "editor" ? "none" : "editor" })}>
          <Icon.editors />
        </ViewButton>
      </div>

      <div className="cb-center">
        <TransportButtons />
        <Lcd />
        <div className="cb-modes">
          <button className={"mode-btn cycle" + (p?.cycle.enabled ? " on" : "")} title={`Cycle (${keyHint("cycle")})`} onClick={() => transport.toggleCycle()}>
            <Icon.cycle />
          </button>
          <button className={"mode-btn punch" + (punchOn ? " on" : "")} title={`Autopunch (${keyHint("autopunch")})`} onClick={() => transport.togglePunch()}>
            <svg width="18" height="16" viewBox="0 0 18 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round">
              <path d="M5 3.5H3v9h2M13 3.5h2v9h-2" />
              <path d="M6.5 8h5" strokeWidth="2.6" />
            </svg>
          </button>
          <button
            className={"mode-btn replace" + (replace ? " on" : "")}
            title="Replace: recordings replace what they overlap, audio and MIDI"
            onClick={() => updateSettings((st) => ({ ...st, prefs: { ...st.prefs, recording: { ...st.prefs.recording, replace: !replace } } }))}
          >
            <svg width="18" height="16" viewBox="0 0 18 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinejoin="round">
              <rect x="1.5" y="4" width="15" height="8" rx="1.5" strokeDasharray="2 1.6" />
              <rect x="5.5" y="4" width="7" height="8" fill="currentColor" stroke="none" />
            </svg>
          </button>
          <button className={"mode-btn purple countin" + (p?.count_in ? " on" : "")} title={`Count-in (${keyHint("count-in")})`} onClick={() => act("set_count_in", { on: !p?.count_in })}>
            1234
          </button>
          <button className={"mode-btn purple" + (p?.metronome ? " on" : "")} title={`Metronome (${keyHint("metronome")})`} onClick={() => act("set_metronome", { on: !p?.metronome })}>
            <Icon.metronome />
          </button>
        </div>
        <div className="cb-master" title="Master volume">
          <HSlider db={p?.master.volume_db ?? 0} onChange={(db) => act("set_master_volume", { db })} width={92} />
        </div>
        <CpuMeter />
      </div>

      <div className="cb-group right">
        <ViewButton on={showTyping} title={`Musical Typing (${fmt("Mod+K")})`} onClick={() => setState({ showTyping: !showTyping })}>
          <svg width="18" height="16" viewBox="0 0 18 16" fill="none" stroke="currentColor" strokeWidth="1.3">
            <rect x="1.5" y="3" width="15" height="10" rx="1.5" />
            <path d="M5 3v10M9 3v10M13 3v10" />
            <path d="M4 3v5.5h2V3M8 3v5.5h2V3M12 3v5.5h2V3" fill="currentColor" strokeWidth="0" />
          </svg>
        </ViewButton>
        <ViewButton on={s.bottom === "list"} title={`List Editors (${keyHint("list-editors")})`} onClick={() => setState({ bottom: s.bottom === "list" ? "none" : "list" })}>
          <Icon.lists />
        </ViewButton>
        <ViewButton on={s.showBrowser} title={`Browsers (${keyHint("browser")})`} onClick={() => setState({ showBrowser: !s.showBrowser })}>
          <Icon.browser />
        </ViewButton>
      </div>
    </div>
  );
}
