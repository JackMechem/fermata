// The Step Sequencer (Logic 10.5+): edits the selected pattern region as a grid of rows and
// steps in the bottom section, beside the Piano Roll and the Event List. Clicking or dragging
// over steps changes them as the edit mode says (a drag paints, one undo step per drag); the
// row headers hold each row's pitch, mute, solo, step rate and length. The maths is in
// stepSeq.ts; every edit sends the whole pattern back with `set_pattern`.
import { useEffect, useReducer, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { act, type Id, type Pattern, type Project, type Region, type Track } from "../api";
import { ownPanel } from "../detach";
import { onFrame, framePos } from "../frame";
import { Icon } from "../icons";
import { regionEnd } from "../loops";
import { createPatternRegion, trackOf } from "../ops";
import * as ss from "../stepSeq";
import { onLocalMidi } from "../stepInput";
import { getState, onTick, setState, useStore, type State } from "../store";
import { tempoMap } from "../tempo";
import { DetachButton } from "./DetachButton";
import { EditorTabs } from "./EventList";
import { NumberField } from "./Form";
import { openMenu, openMenuAt, type MenuItem } from "./Menu";

/** The editor's settings outlive it, as Logic's editor keeps them. */
let saved: { mode: ss.EditMode } = { mode: "on" };

/** The pattern region the Step Sequencer edits: the first selected one. */
function patternRegion(project: Project | undefined, selected: Id[]): { track: Track; region: Region } | null {
  for (const t of project?.tracks ?? []) {
    const region = t.regions.find((r) => r.pattern && selected.includes(r.id));
    if (region) return { track: t, region };
  }
  return null;
}

/** What the Step Sequencer shows, for its window's title. */
export function stepSequencerTitle(s: State): string {
  const at = patternRegion(s.snap?.project, s.selectedRegions);
  return at ? `Step Sequencer — ${at.region.name}` : "Step Sequencer";
}

/**
 * Moves the bottom section between the Piano Roll and the Step Sequencer when the selection
 * moves to a region the other one edits, as Logic's editor area shows the editor that fits the
 * region. Only on a change of selection, so either tab can still be chosen by hand, and only
 * in the main window (a detached editor keeps its pane).
 */
export function useEditorFollow(region: Region | null, here: "editor" | "steps") {
  const last = useRef(region?.id);
  useEffect(() => {
    if (region?.id === last.current) return;
    last.current = region?.id;
    if (!region || ownPanel || getState().bottom !== here) return;
    const want = region.pattern ? "steps" : region.content.type === "midi" ? "editor" : null;
    if (want && want !== here) setState({ bottom: want });
  });
}

const lengthOptions = () => [...Array.from({ length: 16 }, (_, k) => k + 1), 20, 24, 32, 48, 64];

/** Plays a row's note briefly on the selected track, as Logic does while editing. */
function audition(pitch: number, velocity = 100) {
  invoke("midi_event", { data: [0x90, pitch, velocity] }).catch(() => {});
  setTimeout(() => invoke("midi_event", { data: [0x80, pitch, 0] }).catch(() => {}), 160);
}

export function StepSequencer() {
  const project = useStore((s) => s.snap?.project);
  const selected = useStore((s) => s.selectedRegions);
  const selectedTrack = useStore((s) => s.snap?.selected ?? null);
  const [mode, setModeState] = useState<ss.EditMode>(saved.mode);
  const [learnRow, setLearnRow] = useState<number | null>(null);
  const [, bump] = useReducer((x: number) => x + 1, 0);
  const at = patternRegion(project, selected);
  const region = at?.region ?? null;
  useEditorFollow(project?.tracks.flatMap((t) => t.regions).find((r) => r.id === selected[0]) ?? null, "steps");

  // Edits are shown at once and sent in the background. While any are on their way (or a drag
  // is painting), the editor works on its own copy, so a quick run of edits never builds on a
  // snapshot that is behind.
  const work = useRef<{ id: Id; pattern: Pattern; pending: number; dragging: boolean } | null>(null);
  const live = work.current && region && work.current.id === region.id && (work.current.pending > 0 || work.current.dragging) ? work.current.pattern : null;
  const pattern = live ?? region?.pattern ?? null;
  const current = useRef({ region, pattern });
  current.current = { region, pattern };

  const setMode = (m: ss.EditMode) => {
    saved = { mode: m };
    setModeState(m);
  };

  /** Applies `f` to the pattern and sends it; `key` joins an undo step (a drag's steps). */
  const edit = (label: string, f: (p: Pattern) => Pattern, key: string | null = null) => {
    const { region, pattern } = current.current;
    if (!region || !pattern) return;
    const next = f(pattern);
    if (next === pattern) return;
    const w = work.current && work.current.id === region.id ? work.current : { id: region.id, pattern, pending: 0, dragging: false };
    work.current = { ...w, pattern: next, pending: w.pending + 1 };
    current.current = { region, pattern: next };
    bump();
    act("set_pattern", { id: region.id, pattern: next, label, key }).finally(() => {
      if (work.current?.id === region.id) work.current.pending--;
      bump();
    });
  };

  // ---- Learn: the next note played sets the row's pitch ----------------------------------
  const learning = useRef(learnRow);
  learning.current = learnRow;
  useEffect(() => {
    const heard = (d: [number, number, number]) => {
      const r = learning.current;
      if (r === null || (d[0] & 0xf0) !== 0x90 || d[2] === 0) return;
      setLearnRow(null);
      edit("Row Pitch", (p) => ss.setRow(p, r, { pitch: d[1] }));
    };
    let started = false;
    const offTick = onTick((t) => {
      if (started) for (const d of t.midi_in ?? []) heard(d);
    });
    started = true;
    const offLocal = onLocalMidi(heard);
    return () => {
      offTick();
      offLocal();
    };
  }, []);
  useEffect(() => setLearnRow(null), [region?.id]);

  // ---- the playhead: the playing step of each row, lit straight in the DOM ------------------
  const rowsRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    let lit: (HTMLElement | null)[] = [];
    const off = onFrame(() => {
      const { region, pattern } = current.current;
      const proj = getState().snap?.project;
      const box = rowsRef.current;
      if (!region || !pattern || !proj || !box) return;
      const map = tempoMap(proj);
      const pos = framePos();
      const end = regionEnd(region);
      const next: (HTMLElement | null)[] = pattern.rows.map((row, r) => {
        const k = ss.playingStep(map, region, pattern, row, pos, end);
        return k === null ? null : (box.querySelector(`[data-r="${r}"][data-i="${k}"]`) as HTMLElement | null);
      });
      lit.forEach((el) => el && !next.includes(el) && el.classList.remove("play"));
      next.forEach((el) => el?.classList.add("play"));
      lit = next;
    });
    return () => {
      off();
      lit.forEach((el) => el?.classList.remove("play"));
    };
  }, []);

  // ---- painting steps ---------------------------------------------------------------------
  const cellAt = (x: number, y: number) => {
    const el = document.elementFromPoint(x, y) as HTMLElement | null;
    const cell = el?.closest<HTMLElement>(".ss-cell");
    if (!cell || cell.classList.contains("out")) return null;
    return { r: Number(cell.dataset.r), i: Number(cell.dataset.i), el: cell };
  };

  const onStepsDown = (e: React.PointerEvent) => {
    if (e.button !== 0 || !region || !pattern) return;
    const hit = cellAt(e.clientX, e.clientY);
    if (!hit) return;
    e.preventDefault();
    const frac = (y: number, el: HTMLElement) => {
      const b = el.getBoundingClientRect();
      return 1 - (y - b.top) / b.height;
    };
    const m = mode;
    const row = hit.r;
    const stepsEl = hit.el.parentElement!;
    const key = `pattern:${region.id}:${Date.now()}`;
    const label = { on: "Toggle Steps", velocity: "Step Velocity", gate: "Step Gate", tie: "Tie Steps" }[m];
    const paint = ss.paintFor(m, pattern, hit.r, hit.i, frac(e.clientY, hit.el));
    if (work.current?.id === region.id) work.current.dragging = true;
    else work.current = { id: region.id, pattern, pending: 0, dragging: true };
    const done = new Set<string>();
    const apply = (r: number, i: number, patch: Partial<ss.FullStep>) => {
      done.add(`${r}:${i}`);
      edit(label, (p) => ss.setStep(p, r, i, patch), key);
    };
    apply(hit.r, hit.i, paint);
    if (paint.on) audition(pattern.rows[hit.r].pitch, ss.stepAt(pattern.rows[hit.r], hit.i).velocity);
    const move = (ev: PointerEvent) => {
      if (ss.isBarMode(m)) {
        // Bars: along the pressed row, each step set from the pointer's height.
        const b = stepsEl.getBoundingClientRect();
        const cells = stepsEl.children.length;
        const i = Math.floor(((ev.clientX - b.left) / b.width) * cells);
        const cell = stepsEl.children[i] as HTMLElement | undefined;
        if (!cell || cell.classList.contains("out")) return;
        apply(row, i, ss.paintFor(m, current.current.pattern!, row, i, frac(ev.clientY, cell)));
      } else {
        // Switches: every step passed over gets what the first one got.
        const h = cellAt(ev.clientX, ev.clientY);
        if (h && !done.has(`${h.r}:${h.i}`)) apply(h.r, h.i, paint);
      }
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      if (work.current) work.current.dragging = false;
      bump();
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  // ---- menus ------------------------------------------------------------------------------
  const pitchItems = (r: number): MenuItem[] => {
    const set = (pitch: number) => {
      edit("Row Pitch", (p) => ss.setRow(p, r, { pitch }));
      audition(pitch);
    };
    const cur = pattern?.rows[r]?.pitch;
    const octaves: MenuItem[] = Array.from({ length: 11 }, (_, o) => ({
      label: `${ss.noteName(o * 12)}–${ss.noteName(Math.min(127, o * 12 + 11))}`,
      children: Array.from({ length: 12 }, (_, k) => o * 12 + k)
        .filter((n) => n <= 127)
        .map((n) => ({ label: ss.noteName(n), checked: n === cur, onClick: () => set(n) })),
    }));
    return [
      { label: "Learn (play a note)", checked: learnRow === r, onClick: () => setLearnRow(learnRow === r ? null : r) },
      { separator: true },
      { label: "General MIDI Drums", header: true },
      ...Object.entries(ss.GM_DRUMS).map(([n, name]) => ({ label: `${name} (${ss.noteName(Number(n))})`, checked: Number(n) === cur, onClick: () => set(Number(n)) })),
      { separator: true },
      { label: "Notes", children: octaves },
    ];
  };

  const rateItems = (value: number | null | undefined, onPick: (v: number | null) => void, withDefault: string | null): MenuItem[] => [
    ...(withDefault ? [{ label: withDefault, checked: value == null, onClick: () => onPick(null) }, { separator: true } as MenuItem] : []),
    ...ss.RATES.map((v) => ({ label: ss.rateLabel(v), checked: v === value, onClick: () => onPick(v) })),
  ];

  const lengthItems = (value: number | null | undefined, onPick: (v: number | null) => void, withDefault: string | null): MenuItem[] => [
    ...(withDefault ? [{ label: withDefault, checked: value == null, onClick: () => onPick(null) }, { separator: true } as MenuItem] : []),
    ...lengthOptions().map((v) => ({ label: `${v} Steps`, checked: v === value, onClick: () => onPick(v) })),
  ];

  const rowMenu = (r: number): MenuItem[] => [
    { label: "Add Row Below", disabled: (pattern?.rows.length ?? 0) >= ss.MAX_ROWS, onClick: () => edit("Add Row", (p) => ss.addRow(p, r)) },
    { label: "Duplicate Row", disabled: (pattern?.rows.length ?? 0) >= ss.MAX_ROWS, onClick: () => edit("Duplicate Row", (p) => ({ ...p, rows: [...p.rows.slice(0, r + 1), p.rows[r], ...p.rows.slice(r + 1)] })) },
    { label: "Clear Row", onClick: () => edit("Clear Row", (p) => ss.clearSteps(p, r)) },
    { separator: true },
    { label: "Delete Row", onClick: () => edit("Delete Row", (p) => ss.removeRow(p, r)) },
  ];

  const functionsMenu = (): MenuItem[] => [
    { label: "Add Row", disabled: !pattern || pattern.rows.length >= ss.MAX_ROWS, onClick: () => edit("Add Row", (p) => ss.addRow(p)) },
    { label: "Clear Pattern", disabled: !pattern, onClick: () => edit("Clear Pattern", (p) => ss.clearSteps(p)) },
    { separator: true },
    { label: "Convert to MIDI Region", disabled: !region, onClick: () => region && act("convert_patterns_to_midi", { ids: [region.id] }).then(() => setState({ bottom: "editor" })) },
  ];

  // ---- the view ---------------------------------------------------------------------------
  const bar = (
    <div className="local-bar">
      <EditorTabs current="steps" />
      <div className="local-menus">
        <button className="local-menu" disabled={!region} onMouseDown={(e) => openMenuAt(e.currentTarget, functionsMenu())}>
          Functions
        </button>
        <span className="local-title">{region?.name ?? "Step Sequencer"}</span>
      </div>
      <div className="local-right">
        <DetachButton panel="steps" />
      </div>
    </div>
  );

  if (!region || !pattern) {
    const track = project?.tracks.find((t) => t.id === selectedTrack);
    const canCreate = track?.kind === "instrument" && !track.frozen;
    return (
      <div className="editor step-seq">
        {bar}
        <div className="editor-body">
          <div className="editor-empty ss-empty">
            <span>Select a pattern region to edit its steps.</span>
            {canCreate && (
              <button className="popup" onClick={() => createPatternRegion(track!.id, framePos())}>
                Create Pattern Region on {track!.name}
              </button>
            )}
          </div>
        </div>
      </div>
    );
  }

  const cols = ss.columns(pattern);
  const solo = pattern.rows.some((r) => r.solo);
  const frozen = !!trackOf(region.id)?.frozen;

  return (
    <div className="editor step-seq">
      {bar}
      <div className="ss-tools">
        <div className="editor-tabs ss-modes">
          {ss.EDIT_MODES.map((m) => (
            <button key={m.id} className={"filter" + (mode === m.id ? " on" : "")} title={m.title} onClick={() => setMode(m.id)}>
              {m.label}
            </button>
          ))}
        </div>
        <span className="lb-label">Length</span>
        <button className="popup" title="Pattern length in steps" onMouseDown={(e) => openMenuAt(e.currentTarget, lengthItems(pattern.length, (v) => v && edit("Pattern Length", (p) => ({ ...p, length: v })), null))}>
          {pattern.length} <Icon.chevron />
        </button>
        <span className="lb-label">Rate</span>
        <button className="popup" title="Step rate: how long each step is" onMouseDown={(e) => openMenuAt(e.currentTarget, rateItems(pattern.rate, (v) => v && edit("Step Rate", (p) => ({ ...p, rate: v })), null))}>
          {ss.rateLabel(pattern.rate)} <Icon.chevron />
        </button>
        <span className="lb-label">Swing</span>
        <NumberField value={pattern.swing} min={0} max={100} width={70} onChange={(swing) => edit("Swing", (p) => ({ ...p, swing }), `swing:${region.id}`)} />
        <button className={"filter" + (pattern.looped !== false ? " on" : "")} title="Loop: the pattern repeats to the end of the region" onClick={() => edit("Pattern Loop", (p) => ({ ...p, looped: p.looped === false }))}>
          Loop
        </button>
        <button className="lb-btn ss-add" title="Add Row" disabled={pattern.rows.length >= ss.MAX_ROWS} onClick={() => edit("Add Row", (p) => ss.addRow(p))}>
          <Icon.plus />
        </button>
        {learnRow !== null && <span className="ss-learn">Play a note for {ss.rowName(pattern.rows[learnRow]?.pitch ?? 0)}…</span>}
        {frozen && <span className="ss-learn">The track is frozen.</span>}
      </div>
      <div className="editor-body ss-body">
        <div className="ss-rows" ref={rowsRef} style={{ "--ss-cols": cols } as React.CSSProperties}>
          {pattern.rows.map((row, r) => {
            const len = ss.rowLength(pattern, row);
            const rate = ss.rowRate(pattern, row);
            const group = ss.isTriplet(rate) ? 3 : 4;
            const silent = row.muted || (solo && !row.solo);
            return (
              <div key={r} className={"ss-row" + (silent ? " silent" : "") + (learnRow === r ? " learning" : "")}>
                <div
                  className="ss-head"
                  onContextMenu={(e) => {
                    e.preventDefault();
                    openMenu(e.clientX, e.clientY, rowMenu(r));
                  }}
                >
                  <button className={"tbtn mute" + (row.muted ? " on" : "")} title="Mute Row" onClick={() => edit("Mute Row", (p) => ss.setRow(p, r, { muted: !row.muted }))}>
                    M
                  </button>
                  <button className={"tbtn solo" + (row.solo ? " on" : "")} title="Solo Row" onClick={() => edit("Solo Row", (p) => ss.setRow(p, r, { solo: !row.solo }))}>
                    S
                  </button>
                  <button className="ss-name" title="Row pitch: choose a drum or note, or Learn one from a MIDI keyboard" onMouseDown={(e) => openMenuAt(e.currentTarget, pitchItems(r))}>
                    <span className="ss-name-text">{ss.rowName(row.pitch)}</span>
                    <span className="ss-note">{ss.noteName(row.pitch)}</span>
                  </button>
                  <button className={"ss-opt" + (row.rate != null ? " set" : "")} title="Row step rate" onMouseDown={(e) => openMenuAt(e.currentTarget, rateItems(row.rate, (v) => edit("Row Rate", (p) => ss.setRow(p, r, { rate: v })), `Pattern (${ss.rateLabel(pattern.rate)})`))}>
                    {ss.rateLabel(rate)}
                  </button>
                  <button className={"ss-opt" + (row.length != null ? " set" : "")} title="Row length in steps (rows of different lengths repeat on their own)" onMouseDown={(e) => openMenuAt(e.currentTarget, lengthItems(row.length, (v) => edit("Row Length", (p) => ss.setRow(p, r, { length: v })), `Pattern (${pattern.length})`))}>
                    {len}
                  </button>
                </div>
                <div className={"ss-steps mode-" + mode} onPointerDown={onStepsDown}>
                  {Array.from({ length: cols }, (_, i) => {
                    const s = ss.stepAt(row, i);
                    const cls = ["ss-cell", Math.floor(i / group) % 2 ? "alt" : "", s.on ? "on" : "", s.tie ? "tie" : "", i >= len ? "out" : ""].filter(Boolean).join(" ");
                    return (
                      <div key={i} className={cls} data-r={r} data-i={i}>
                        {ss.isBarMode(mode) && i < len && <div className="ss-bar" style={{ height: `${Math.max(4, ss.barFraction(mode, s) * 100)}%` }} />}
                      </div>
                    );
                  })}
                </div>
              </div>
            );
          })}
          {!pattern.rows.length && <div className="editor-empty">No rows. Click + to add one.</div>}
        </div>
      </div>
    </div>
  );
}
