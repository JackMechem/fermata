// The Settings window (Fermata › Settings…, ⌘,), Project Settings (File › Project
// Settings…, ⌥P) and the crash-recovery sheet. Modelled on Logic Pro's Settings: a row of
// icon tabs, each pane a form whose changes apply at once.
import { useEffect, useMemo, useRef, useState } from "react";
import { ask, open } from "@tauri-apps/plugin-dialog";
import { act, call, supportedRates, type AudioConfig, type BitDepth, type DeviceList, type ClickSettings, type ClickSound, type PluginDesc } from "../api";
import { Icon } from "../icons";
import { fmt, formatKey, KEY_COMMANDS, KEY_GROUPS, eventKey, keyConflict, keysFor, keyHint, MENU_SHORTCUTS, normalizeKey } from "../keymap";
import {
  DEFAULT_CLICK,
  getSettings,
  recoveryDone,
  updateSettings,
  useRecoveries,
  useSettings,
  type ClickLocates,
  type MoveAutomation,
  type Nudge,
  type Prefs,
  type Recovery,
  type RecordingRecovery,
  type UiPrefs,
  type VelocityCurve,
} from "../prefs";
import { setState, useStore, type Snap, type SettingsTab } from "../store";
import { Check, NumberField, Select, sheetKey, Slider } from "./Form";
import type { WheelScrolls } from "../wheel";

const toast = (text: string, ms = 3000) => setState({ toast: { text, at: Date.now(), ms } });

function SheetFrame({ title, children, onClose, width }: { title: string; children: React.ReactNode; onClose: () => void; width: number }) {
  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, [onClose]);
  return (
    <div className="sheet-backdrop" onMouseDown={onClose}>
      <div className="sheet" style={{ width }} onMouseDown={(e) => e.stopPropagation()} onKeyDown={(e) => sheetKey(e, onClose)}>
        <div className="sheet-title">{title}</div>
        {children}
      </div>
    </div>
  );
}

/** 20px tab glyphs, drawn like the ones in icons.tsx. */
function Glyph({ children }: { children: React.ReactNode }) {
  return (
    <svg width={20} height={20} viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth={1.4} strokeLinecap="round" strokeLinejoin="round">
      {children}
    </svg>
  );
}

const TabIcon = {
  general: () => (
    <Glyph>
      <circle cx="10" cy="10" r="2.6" />
      <path d="M10 2.8v2.1M10 15.1v2.1M2.8 10h2.1M15.1 10h2.1M4.9 4.9l1.5 1.5M13.6 13.6l1.5 1.5M4.9 15.1l1.5-1.5M13.6 6.4l1.5-1.5" />
    </Glyph>
  ),
  recording: () => (
    <Glyph>
      <circle cx="10" cy="10" r="7" />
      <circle cx="10" cy="10" r="3.6" fill="currentColor" stroke="none" />
    </Glyph>
  ),
  metronome: () => (
    <Glyph>
      <path d="M7.2 3h5.6l3 14H4.2z" />
      <path d="M10 14.2 14.6 5.4M6.4 12.5h7.2" />
    </Glyph>
  ),
  midi: () => (
    <Glyph>
      <circle cx="10" cy="10" r="7.2" />
      <circle cx="6.4" cy="10.4" r=".7" fill="currentColor" stroke="none" />
      <circle cx="13.6" cy="10.4" r=".7" fill="currentColor" stroke="none" />
      <circle cx="7.5" cy="7" r=".7" fill="currentColor" stroke="none" />
      <circle cx="12.5" cy="7" r=".7" fill="currentColor" stroke="none" />
      <circle cx="10" cy="5.8" r=".7" fill="currentColor" stroke="none" />
      <path d="M8.6 15.2h2.8" />
    </Glyph>
  ),
  editing: () => (
    <Glyph>
      <circle cx="6" cy="14.5" r="2.3" />
      <circle cx="14" cy="14.5" r="2.3" />
      <path d="M7.5 12.8 14 3.5M12.5 12.8 6 3.5" />
    </Glyph>
  ),
  plugins: () => (
    <Glyph>
      <path d="M7 2.8v4M13 2.8v4M5 6.8h10v3.4a5 5 0 0 1-10 0zM10 15.2v2.2" />
    </Glyph>
  ),
  project: () => (
    <Glyph>
      <rect x="3.5" y="3" width="13" height="14" rx="1.5" />
      <path d="M6.5 7h7M6.5 10h7M6.5 13h4" />
    </Glyph>
  ),
  notes: () => (
    <Glyph>
      <path d="M5 3h7.5L15.5 6v11H5z M12.5 3v3h3M7.5 9.5h5M7.5 12.5h5" />
    </Glyph>
  ),
};

/** The usual pane footer: settings save themselves, so there is only Done. */
function Foot({ close, note = "Changes apply immediately and are remembered.", children }: { close: () => void; note?: React.ReactNode; children?: React.ReactNode }) {
  return (
    <div className="sheet-foot">
      <span className="dim">{note}</span>
      <div className="sheet-buttons">
        {children}
        <button className="push default" onClick={close}>
          Done
        </button>
      </div>
    </div>
  );
}

function Row({ label, children, note }: { label: string; children: React.ReactNode; note?: React.ReactNode }) {
  return (
    <>
      <label>{label}</label>
      <div className="pref-cell">
        {children}
        {note && <div className="pref-note">{note}</div>}
      </div>
    </>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="pref-group">
      <div className="pref-group-title">{title}</div>
      <div className="pref-grid">{children}</div>
    </div>
  );
}

/** Updates one section of the session preferences. */
function setPrefs<K extends keyof Prefs>(key: K, patch: Partial<Prefs[K]>) {
  updateSettings((s) => ({ ...s, prefs: { ...s.prefs, [key]: { ...s.prefs[key], ...patch } } }));
}

function setUi(patch: Partial<UiPrefs>) {
  updateSettings((s) => ({ ...s, ui: { ...s.ui, ...patch } }));
}

function FolderField({ value, placeholder, onChange, title }: { value: string | null; placeholder: string; onChange: (v: string | null) => void; title: string }) {
  return (
    <div className="row-inline">
      <span className={"path-field pref-path" + (value ? "" : " placeholder")} title={value ?? placeholder}>
        <bdi>{value ?? placeholder}</bdi>
      </span>
      <button
        className="push"
        onClick={async () => {
          const dir = await open({ directory: true, title, defaultPath: value ?? undefined });
          if (typeof dir === "string") onChange(dir);
        }}
      >
        Choose…
      </button>
      {value && (
        <button className="push" onClick={() => onChange(null)}>
          Default
        </button>
      )}
    </div>
  );
}

// ---- General ------------------------------------------------------------------------------

function GeneralPane({ close }: { close: () => void }) {
  const s = useSettings((s) => s);
  const [root, setRoot] = useState("");
  useEffect(() => {
    call<string>("default_project_dir").then((d) => d && setRoot(d));
  }, [s.prefs.general.project_folder]);
  const g = s.prefs.general;
  return (
    <>
      <Section title="Startup">
        <Row label="At Launch:">
          <Select
            value={s.ui.startup}
            options={[
              ["chooser", "Show the Project Chooser"],
              ["last_project", "Open the most recent project"],
              ["empty_project", "Start with an empty project"],
            ]}
            onChange={(v) => setUi({ startup: v })}
          />
        </Row>
      </Section>
      <Section title="Projects">
        <Row label="Project Folder:" note="Where new projects are saved. The Project Chooser starts here.">
          <FolderField value={g.project_folder} placeholder={root || "~/Music/Fermata"} title="Choose the folder for new projects" onChange={(v) => setPrefs("general", { project_folder: v })} />
        </Row>
        <Row
          label="Autosave:"
          note="Unsaved changes are written to project.autosave.json inside the project folder, never over your saved project. If Fermata quits unexpectedly, it offers to recover them at the next launch, along with anything that was being recorded."
        >
          <Select
            value={g.autosave_minutes}
            options={[
              [0, "Off"],
              [1, "Every minute"],
              [2, "Every 2 minutes"],
              [5, "Every 5 minutes"],
              [10, "Every 10 minutes"],
              [15, "Every 15 minutes"],
            ]}
            onChange={(v) => setPrefs("general", { autosave_minutes: v })}
          />
        </Row>
        <Row
          label="After Recording:"
          note="Takes are protected the moment you stop: if Fermata quits before you save, they come back at the next launch. Works with the interval autosave off too. Your saved project only changes when you save."
        >
          <Check checked={g.autosave_after_recording} onChange={(v) => setPrefs("general", { autosave_after_recording: v })}>
            Autosave as soon as a recording, Bounce in Place, Freeze or import finishes
          </Check>
        </Row>
        <Row
          label="Backups:"
          note="Each save keeps the version it replaces in the project's Backups folder, per project alternative, so File › Revert to can go back to it. 0 keeps just the last one, as project.json.bak."
        >
          <div className="row-inline">
            <NumberField value={g.backups ?? 10} min={0} max={100} step={1} resolution={1} onChange={(v) => setPrefs("general", { backups: Math.round(v) })} width={64} />
            <span className="dim">versions to keep</span>
          </div>
        </Row>
        <Row label="Undo Steps:" note="How many edits Undo can step back through. More steps use more memory on large projects.">
          <NumberField value={g.undo_limit} min={10} max={1000} step={10} resolution={1} onChange={(v) => setPrefs("general", { undo_limit: Math.round(v) })} />
        </Row>
        <Row label="Unsaved Changes:">
          <Check checked={s.ui.confirm_unsaved} onChange={(v) => setUi({ confirm_unsaved: v })}>
            Ask before quitting or closing a project with unsaved changes
          </Check>
        </Row>
      </Section>
      <Section title="Plug-in Windows">
        <Row
          label="Open Effect Panels:"
          note="Where a built-in effect's panel or the Channel EQ opens when you click it: floating inside the main window, docked in its bottom section, or in a window of its own that you can move to another monitor. Each panel can still be floated, docked or detached from its header. VST3 editors always open in their own window."
        >
          <Select
            value={s.ui.fx_panels ?? "floating"}
            options={[
              ["floating", "Floating"],
              ["docked", "Docked in the bottom section"],
              ["window", "Separate windows"],
            ]}
            onChange={(v) => setUi({ fx_panels: v })}
          />
        </Row>
      </Section>
      <Foot close={close} />
    </>
  );
}

// ---- Recording ----------------------------------------------------------------------------

const BIT_DEPTHS: [BitDepth, string][] = [
  ["int24", "24-bit integer"],
  ["float32", "32-bit float"],
];

function RecordingPane({ close }: { close: () => void }) {
  const r = useSettings((s) => s.prefs.recording);
  const countIn = useStore((s) => s.snap?.project.count_in ?? false);
  const override = useStore((s) => s.snap?.project.bit_depth ?? null);
  return (
    <>
      <Section title="Files">
        <Row
          label="Bit Depth:"
          note={
            <>
              32-bit float can't clip on disk however hot the input runs; 24-bit files are a quarter smaller and open everywhere.
              {override && <b> This project overrides it ({BIT_DEPTHS.find((b) => b[0] === override)?.[1]}) in Project Settings.</b>}
            </>
          }
        >
          <Select value={r.bit_depth} options={BIT_DEPTHS} onChange={(v) => setPrefs("recording", { bit_depth: v })} />
        </Row>
        <Row label="Recording Folder:" note="Recordings go into a folder named after the project here. Projects copy them in when you Save As.">
          <FolderField value={r.folder} placeholder="Project › Audio Files" title="Choose where recordings go" onChange={(v) => setPrefs("recording", { folder: v })} />
        </Row>
        <Row label="Take Names:">
          <Select
            value={r.take_names}
            options={[
              ["track_number", "Track name and take number (Vocals#03)"],
              ["track", "Track name (Vocals)"],
              ["project_track", "Project, track and take number (Song - Vocals#03)"],
            ]}
            onChange={(v) => setPrefs("recording", { take_names: v })}
          />
        </Row>
      </Section>
      <Section title="Starting to Record">
        <Row label="Count-in:" note={`Counts in with the click before recording when Count-in is on (${keyHint("count-in") || fmt("Shift+K")}).`}>
          <div className="row-inline">
            <Check checked={countIn} onChange={(on) => act("set_count_in", { on })}>
              On
            </Check>
            <Select
              value={r.count_in_bars}
              options={[1, 2, 3, 4].map((n) => [n, `${n} bar${n > 1 ? "s" : ""}`] as [number, string])}
              onChange={(v) => setPrefs("recording", { count_in_bars: v })}
              width={110}
            />
          </div>
        </Row>
        <Row label="Pre-roll:" note="With Count-in off, playback starts this far before the playhead so you hear the lead-in; recording still starts at the playhead.">
          <div className="row-inline">
            <NumberField value={r.pre_roll_secs} min={0} max={30} step={0.5} onChange={(v) => setPrefs("recording", { pre_roll_secs: v })} />
            <span className="dim">seconds</span>
          </div>
        </Row>
      </Section>
      <Section title="Overlapping Recordings">
        <Row
          label="Audio:"
          note={
            {
              on_top: "The new take plays; the older audio stays underneath and comes back if you move the take.",
              new_track: "When the take overlaps a region, it goes on a new track below with the same settings and plug-ins.",
              replace: "The parts of older regions under the take are removed. Their audio files are kept.",
            }[r.audio_overlap]
          }
        >
          <Select
            value={r.audio_overlap}
            options={[
              ["on_top", "Put the new take on top"],
              ["new_track", "Create a new track"],
              ["replace", "Replace what's underneath"],
            ]}
            onChange={(v) => setPrefs("recording", { audio_overlap: v })}
          />
        </Row>
        <Row
          label="MIDI:"
          note={
            r.replace
              ? "Replace is on in the control bar: recordings replace what they overlap, audio and MIDI alike."
              : r.midi_overlap === "replace"
                ? "Notes and controllers from where recording starts (or the punch-in) to where it stops are replaced by what you play."
                : r.midi_overlap === "take_folders"
                  ? "Each cycle pass becomes a take, and recording over MIDI regions packs them into a take folder with the new take. Choose the take to hear from the folder's menu. The Replace button still replaces."
                  : undefined
          }
        >
          <Select
            value={r.midi_overlap}
            options={[
              ["new_region", "Create a new region"],
              ["merge", "Merge into the existing region"],
              ["replace", "Replace in the recorded range"],
              ["take_folders", "Create take folders"],
            ]}
            onChange={(v) => setPrefs("recording", { midi_overlap: v })}
          />
        </Row>
      </Section>
      <Foot close={close} />
    </>
  );
}

// ---- Metronome ----------------------------------------------------------------------------

const SOUNDS: { id: ClickSound; name: string; description: string }[] = [
  { id: "classic", name: "Classic", description: "A short sine tick" },
  { id: "click", name: "Click", description: "A sharp transient that cuts through a dense mix" },
  { id: "woodblock", name: "Woodblock", description: "A warm, woody knock" },
  { id: "beep", name: "Beep", description: "A longer electronic tone" },
];

/** The metronome form, shared by Settings and Project Settings. */
export function MetronomeFields() {
  const click = useStore((s) => s.snap?.project.click) ?? DEFAULT_CLICK;
  const on = useStore((s) => s.snap?.project.metronome ?? false);
  const set = (patch: Partial<ClickSettings>) => act("set_click", { click: { ...click, ...patch } });
  const st = (n: number) => (n > 0 ? `+${n}` : String(n));
  return (
    <>
      <Section title="Sound">
        <Row label="Click:">
          <Check checked={on} onChange={(v) => act("set_metronome", { on: v })}>
            Metronome on ({keyHint("metronome") || "K"})
          </Check>
        </Row>
        <Row label="Sound:" note={SOUNDS.find((s) => s.id === click.sound)?.description}>
          <div className="segmented">
            {SOUNDS.map((s) => (
              <button key={s.id} className={s.id === click.sound ? "on" : ""} onClick={() => set({ sound: s.id })}>
                {s.name}
              </button>
            ))}
          </div>
        </Row>
        <Row label="Volume:">
          <Slider value={click.volume_db} min={-40} max={6} step={0.5} onChange={(v) => set({ volume_db: v })} format={(v) => `${v > 0 ? "+" : ""}${v.toFixed(1)} dB`} />
        </Row>
        <Row label="Accent:">
          <Check checked={click.accent} onChange={(v) => set({ accent: v })}>
            Higher and louder on the first beat of each bar
          </Check>
        </Row>
        <Row label="Pitch:" note="In semitones, relative to the sound's own pitch.">
          <div className="row-inline">
            <span className="inline-label">Bar</span>
            <NumberField value={click.bar_pitch} min={-12} max={12} onChange={(v) => set({ bar_pitch: Math.round(v) })} />
            <span className="inline-label">Beat</span>
            <NumberField value={click.beat_pitch} min={-12} max={12} onChange={(v) => set({ beat_pitch: Math.round(v) })} />
            <span className="dim">{click.bar_pitch || click.beat_pitch ? `${st(click.bar_pitch)} / ${st(click.beat_pitch)}` : ""}</span>
          </div>
        </Row>
        <Row label="Subdivision:">
          <Select
            value={click.division}
            options={[
              [1, "Off (beats only)"],
              [2, "Eighth notes"],
              [3, "Triplets"],
              [4, "Sixteenth notes"],
            ]}
            onChange={(v) => set({ division: v })}
          />
        </Row>
      </Section>
      <Section title="When">
        <Row label="Playing:">
          <Check checked={click.while_playing} onChange={(v) => set({ while_playing: v })}>
            Click while playing
          </Check>
        </Row>
        <Row label="Recording:" note="The count-in always clicks, even with the metronome off.">
          <div className="pref-checks">
            <Check checked={click.while_recording} onChange={(v) => set({ while_recording: v })}>
              Click while recording
            </Check>
            <Check checked={click.only_count_in} onChange={(v) => set({ only_count_in: v })}>
              Only during the count-in
            </Check>
          </div>
        </Row>
      </Section>
    </>
  );
}

function MetronomePane({ close }: { close: () => void }) {
  const click = useStore((s) => s.snap?.project.click) ?? DEFAULT_CLICK;
  const deflt = useSettings((s) => s.prefs.click);
  const isDefault = JSON.stringify(click) === JSON.stringify(deflt);
  return (
    <>
      <MetronomeFields />
      <Foot close={close} note="Saved with the project, like Logic's metronome settings.">
        <button className="push" title="The settings new projects start with" onClick={() => act("set_click", { click: deflt })} disabled={isDefault}>
          Revert to Default
        </button>
        <button className="push" title="Fermata's original metronome settings" onClick={() => act("set_click", { click: DEFAULT_CLICK })} disabled={JSON.stringify(click) === JSON.stringify(DEFAULT_CLICK)}>
          Factory Settings
        </button>
        <button
          className="push"
          disabled={isDefault}
          onClick={() => {
            setPrefs("click", click);
            toast("New projects will use these metronome settings");
          }}
        >
          Use for New Projects
        </button>
      </Foot>
    </>
  );
}

// ---- MIDI ---------------------------------------------------------------------------------

interface MidiPort {
  name: string;
  enabled: boolean;
  connected: boolean;
}

interface MidiLogEntry {
  seq: number;
  ms: number;
  port: string;
  data: [number, number, number];
}

const NOTE_NAMES = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];

/** A MIDI message in words, Logic-style (C3 = middle C). */
function describeMidi(d: [number, number, number]): [string, string] {
  const st = d[0] & 0xf0;
  const ch = (d[0] & 0x0f) + 1;
  const note = (n: number) => `${NOTE_NAMES[n % 12]}${Math.floor(n / 12) - 2}`;
  const kind =
    st === 0x90 && d[2] > 0
      ? `Note On  ${note(d[1])}  vel ${d[2]}`
      : st === 0x80 || st === 0x90
        ? `Note Off ${note(d[1])}`
        : st === 0xb0
          ? `Control ${d[1]} = ${d[2]}${d[1] === 64 ? " (sustain)" : d[1] === 1 ? " (mod wheel)" : ""}`
          : st === 0xe0
            ? `Pitch Bend ${((d[2] << 7) | d[1]) - 8192}`
            : st === 0xc0
              ? `Program ${d[1] + 1}`
              : st === 0xd0
                ? `Aftertouch ${d[1]}`
                : st === 0xa0
                  ? `Poly Pressure ${note(d[1])} ${d[2]}`
                  : `0x${d[0].toString(16)}`;
  return [`Ch ${ch}`, kind];
}

function VelocityGraph({ curve, fixed }: { curve: VelocityCurve; fixed: number }) {
  const pts: string[] = [];
  for (let i = 0; i <= 32; i++) {
    const x = i / 32;
    const y = curve === "fixed" ? fixed / 127 : curve === "soft" ? Math.pow(x, 0.6) : curve === "hard" ? Math.pow(x, 1.7) : x;
    pts.push(`${(2 + x * 40).toFixed(1)},${(42 - y * 40).toFixed(1)}`);
  }
  return (
    <svg className="velocity-graph" width={44} height={44} viewBox="0 0 44 44">
      <rect x="2" y="2" width="40" height="40" rx="3" />
      <polyline points={pts.join(" ")} />
    </svg>
  );
}

function MidiPane({ close }: { close: () => void }) {
  const m = useSettings((s) => s.prefs.midi);
  const target = useStore((s) => s.snap?.midi_target);
  const [ports, setPorts] = useState<MidiPort[] | null>(null);
  const [log, setLog] = useState<MidiLogEntry[]>([]);
  const loadPorts = () => call<MidiPort[]>("midi_ports").then((p) => p && setPorts(p));
  useEffect(() => {
    loadPorts();
    const t = window.setInterval(loadPorts, 3000);
    return () => window.clearInterval(t);
  }, [m.disabled_inputs]);
  useEffect(() => {
    let last = -1;
    const t = window.setInterval(async () => {
      const l = await call<MidiLogEntry[]>("midi_monitor");
      const top = l?.length ? l[l.length - 1].seq : -1;
      if (l && top !== last) {
        last = top;
        setLog(l.slice(-10).reverse());
      }
    }, 120);
    return () => window.clearInterval(t);
  }, []);
  const toggle = (name: string, on: boolean) =>
    setPrefs("midi", { disabled_inputs: on ? m.disabled_inputs.filter((n) => n !== name) : [...m.disabled_inputs, name] });
  return (
    <>
      <Section title="Inputs">
        <Row label="MIDI Inputs:" note="Every enabled input plays the selected software instrument track. New devices are picked up within a few seconds.">
          <div className="midi-ports">
            {ports === null ? null : ports.length === 0 ? (
              <span className="dim">No MIDI inputs found</span>
            ) : (
              ports.map((p) => (
                <div key={p.name} className="midi-port">
                  <Check checked={p.enabled} onChange={(v) => toggle(p.name, v)}>
                    {p.name}
                  </Check>
                  <span className={"status-dot " + (p.connected ? "ok" : "off")} title={p.connected ? "Connected" : "Not connected"} />
                </div>
              ))
            )}
          </div>
        </Row>
        <Row label="MIDI Thru:" note={m.thru ? `Live MIDI now plays ${target ? `“${target}”` : "the selected instrument track"}.` : "Live MIDI is recorded on record-enabled tracks but not heard while you play — for keyboards that make their own sound."}>
          <Check checked={m.thru} onChange={(v) => setPrefs("midi", { thru: v })}>
            Play the selected instrument track live
          </Check>
        </Row>
        <Row label="Velocity:" note="Shapes how hard you need to play. Applies to MIDI inputs, not to Musical Typing.">
          <div className="row-inline">
            <Select
              value={m.velocity}
              options={[
                ["linear", "As played"],
                ["soft", "Soft (light touch plays louder)"],
                ["hard", "Hard (needs a firmer touch)"],
                ["fixed", "Fixed velocity"],
              ]}
              onChange={(v) => setPrefs("midi", { velocity: v })}
            />
            {m.velocity === "fixed" && <NumberField value={m.fixed_velocity} min={1} max={127} onChange={(v) => setPrefs("midi", { fixed_velocity: Math.round(v) })} />}
            <VelocityGraph curve={m.velocity} fixed={m.fixed_velocity} />
          </div>
        </Row>
        <Row label="Chase:" note="When playback starts, jumps or cycles back in the middle of a region, notes already held there start, and the controllers (sustain, modulation, pitch bend…) are set as they were.">
          <div className="row-inline">
            <Check checked={m.chase_notes ?? true} onChange={(v) => setPrefs("midi", { chase_notes: v })}>
              Notes
            </Check>
            <Check checked={m.chase_controllers ?? true} onChange={(v) => setPrefs("midi", { chase_controllers: v })}>
              Controllers
            </Check>
          </div>
        </Row>
      </Section>
      <Section title="Activity">
        <Row label="Monitor:">
          <div className="midi-monitor">
            {log.length === 0 ? (
              <span className="dim">Play something on a MIDI input to see its messages here.</span>
            ) : (
              log.map((e, i) => {
                const [ch, what] = describeMidi(e.data);
                return (
                  <div key={e.seq} className={"midi-msg" + (i === 0 ? " fresh" : "")}>
                    <span className="midi-port-name">{e.port}</span>
                    <span>{ch}</span>
                    <span className="midi-what">{what}</span>
                  </div>
                );
              })
            )}
          </div>
        </Row>
      </Section>
      <Foot close={close} />
    </>
  );
}

// ---- Editing ------------------------------------------------------------------------------

const SNAPS: [Snap, string][] = [
  ["smart", "Smart (follows the zoom)"],
  ["bar", "Bar"],
  ["beat", "Beat"],
  ["off", "Off"],
];

const CLICK_LOCATES: [ClickLocates, string][] = [
  ["anywhere", "Anywhere in the tracks"],
  ["empty", "Empty space only"],
  ["off", "Never"],
];

const WHEEL_SCROLLS: [WheelScrolls, string][] = [
  ["vertical", "Vertically (tracks)"],
  ["horizontal", "Horizontally (time)"],
];

const MOVE_AUTOMATION: [MoveAutomation, string][] = [
  ["never", "Never"],
  ["always", "Always"],
  ["ask", "Ask"],
];

const NUDGES: [Nudge, string][] = [
  ["bar", "Bar"],
  ["beat", "Beat"],
  ["8", "1/8 note"],
  ["16", "1/16 note"],
  ["32", "1/32 note"],
  ["ms10", "10 ms"],
  ["ms1", "1 ms"],
];

function EditingPane({ close }: { close: () => void }) {
  const ui = useSettings((s) => s.ui);
  const fade = useSettings((s) => s.prefs.editing.fade_ms);
  const moveAutomation = useSettings((s) => s.prefs.editing.move_automation) ?? "never";
  const xfadeMs = useSettings((s) => s.prefs.editing.crossfade_ms) ?? 20;
  const xfadeCurve = useSettings((s) => s.prefs.editing.crossfade_curve) ?? 0.5;
  return (
    <>
      <Section title="Regions">
        <Row label="Snap:" note="The snap setting new sessions start with; change it any time from the Snap pop-up above the tracks.">
          <Select
            value={ui.snap}
            options={SNAPS}
            onChange={(v) => {
              setUi({ snap: v });
              setState({ snapMode: v });
            }}
          />
        </Row>
        <Row label="Nudge Value:" note={nudgeNote()}>
          <Select value={ui.nudge} options={NUDGES} onChange={(v) => setUi({ nudge: v })} />
        </Row>
        <Row label="Click Moves Playhead:" note="Clicking without dragging puts the playhead there, snapped to the grid. Dragging still moves regions or draws a selection.">
          <Select value={ui.click_locates ?? "anywhere"} options={CLICK_LOCATES} onChange={(v) => setUi({ click_locates: v })} />
        </Row>
        <Row label="Anti-click Fades:" note="A short fade-in and fade-out on new recordings and imported audio, so region edges don't click. 0 = none.">
          <div className="row-inline">
            <NumberField value={fade} min={0} max={100} step={0.5} resolution={0.1} onChange={(v) => setPrefs("editing", { fade_ms: v })} />
            <span className="dim">ms</span>
          </div>
        </Row>
        <Row
          label="Move Automation with Regions:"
          note="Whether moving a region along its track takes the track's automation under it along, replacing the automation where it lands. Ask asks on each move that would."
        >
          <Select value={moveAutomation} options={MOVE_AUTOMATION} onChange={(v) => setPrefs("editing", { move_automation: v })} />
        </Row>
        <Row
          label="Crossfade Time:"
          note="The crossfades where one take hands over to the next in a take folder, those the X-Fade drag mode puts between regions that just touch, and the Crossfade menu's first choice."
        >
          <div className="row-inline">
            <NumberField value={xfadeMs} min={0} max={1000} step={1} resolution={1} onChange={(v) => setPrefs("editing", { crossfade_ms: v })} />
            <span className="dim">ms</span>
          </div>
        </Row>
        <Row label="Crossfade Curve:" note="Their shape, as in the Region inspector: 0 is a straight line, 50 (the default) a quarter sine, which keeps two different takes from dipping where they cross.">
          <NumberField value={Math.round(xfadeCurve * 100)} min={-100} max={100} step={10} resolution={1} onChange={(v) => setPrefs("editing", { crossfade_curve: Math.max(-1, Math.min(1, v / 100)) })} />
        </Row>
      </Section>
      <Section title="Scrolling">
        <Row
          label="Mouse Wheel Scrolls:"
          note="In the Tracks area and the Piano Roll. Horizontally suits a mouse with one wheel: it moves through time, and Shift+wheel scrolls the tracks instead. A trackpad's sideways swipe always scrolls in time."
        >
          <Select value={ui.wheel_scrolls ?? "vertical"} options={WHEEL_SCROLLS} onChange={(v) => setUi({ wheel_scrolls: v })} />
        </Row>
      </Section>
      <Foot close={close} />
    </>
  );
}

// ---- Plug-ins (Logic's Plug-in Manager) ---------------------------------------------------

interface ManagedPlugin extends PluginDesc {
  path: string;
  enabled: boolean;
}

interface ScanReport {
  at: number;
  bundles: number;
  plugins: number;
  failed: { path: string; reason: string }[];
  folders: string[];
}

interface PluginManagerData {
  plugins: ManagedPlugin[];
  report: ScanReport | null;
  folders: string[];
}

function pluginType(p: PluginDesc): string {
  const parts = p.subcategories.split("|").filter((s) => s && s !== "Fx" && s !== "Instrument" && s !== "Mono" && s !== "Stereo");
  return `${p.is_instrument ? "Instrument" : "Effect"}${parts.length ? ` · ${parts.join(", ")}` : ""}`;
}

function PluginsPane({ close }: { close: () => void }) {
  const pp = useSettings((s) => s.plugins);
  const [data, setData] = useState<PluginManagerData | null>(null);
  const [q, setQ] = useState("");
  const [scanning, setScanning] = useState(false);
  const [showFailed, setShowFailed] = useState(false);
  const load = () => call<PluginManagerData>("plugin_manager").then((d) => d && setData(d));
  useEffect(() => {
    load();
  }, []);
  const disabled = new Set(pp.disabled);
  const list = (data?.plugins ?? []).filter((p) => !q || `${p.name} ${p.vendor} ${p.subcategories}`.toLowerCase().includes(q.toLowerCase()));
  const setDisabled = (ids: string[], off: boolean) => {
    const next = new Set(pp.disabled);
    for (const id of ids) {
      if (off) next.add(id);
      else next.delete(id);
    }
    updateSettings((s) => ({ ...s, plugins: { ...s.plugins, disabled: [...next] } }));
  };
  const rescan = async () => {
    setScanning(true);
    const l = await call<PluginDesc[]>("rescan_plugins");
    if (l) setState({ plugins: l });
    await load();
    setScanning(false);
  };
  const setFolders = (folders: string[]) => updateSettings((s) => ({ ...s, plugins: { ...s.plugins, folders } }));
  const extra = new Set(pp.folders);
  const r = data?.report;
  return (
    <>
      <div className="pm-toolbar">
        <input className="search pm-search" placeholder="Search plug-ins" value={q} onChange={(e) => setQ(e.target.value)} onKeyDown={(e) => e.stopPropagation()} />
        <span className="dim">
          {data ? `${data.plugins.length} plug-ins${pp.disabled.length ? `, ${data.plugins.filter((p) => disabled.has(p.id)).length} off` : ""}` : ""}
        </span>
        <span className="spacer" />
        <button className="small-btn" disabled={!list.length} onClick={() => setDisabled(list.map((p) => p.id), false)}>
          Use All
        </button>
        <button className="small-btn" disabled={!list.length} onClick={() => setDisabled(list.map((p) => p.id), true)}>
          Use None
        </button>
      </div>
      <div className="pm-table">
        <div className="pm-row pm-head">
          <span>Use</span>
          <span>Name</span>
          <span>Manufacturer</span>
          <span>Type</span>
          <span>Version</span>
        </div>
        <div className="pm-body">
          {list.map((p) => (
            <div key={p.id} className={"pm-row" + (disabled.has(p.id) ? " off" : "")} title={p.path}>
              <span>
                <input type="checkbox" checked={!disabled.has(p.id)} onChange={(e) => setDisabled([p.id], !e.target.checked)} />
              </span>
              <span className="pm-name">{p.name}</span>
              <span>{p.vendor || "Unknown"}</span>
              <span className="pm-type">{pluginType(p)}</span>
              <span className="dim">{p.version}</span>
            </div>
          ))}
          {data && list.length === 0 && <div className="pm-empty">{data.plugins.length ? "No matching plug-ins" : "No VST3 plug-ins found. Add a folder below and rescan."}</div>}
        </div>
      </div>
      <div className="pref-note pm-note">Plug-ins switched off are hidden from the Library and the slot menus. Projects that already use them still load them.</div>

      <Section title="Folders">
        <Row label="VST3 Folders:" note="The standard locations are always searched. Rescan after adding a folder.">
          <div className="pm-folders">
            {(data?.folders ?? []).map((f) => (
              <div key={f} className={"pm-folder" + (extra.has(f) ? "" : " std")}>
                <span title={f}>{f}</span>
                {extra.has(f) && (
                  <button className="pm-remove" title="Remove folder" onClick={() => setFolders(pp.folders.filter((x) => x !== f)).then(load)}>
                    ×
                  </button>
                )}
              </div>
            ))}
            <div className="row-inline">
              <button
                className="push"
                onClick={async () => {
                  const dir = await open({ directory: true, title: "Add a VST3 folder" });
                  if (typeof dir === "string" && !pp.folders.includes(dir)) await setFolders([...pp.folders, dir]).then(load);
                }}
              >
                Add Folder…
              </button>
            </div>
          </div>
        </Row>
        <Row label="Last Scan:">
          {r ? (
            <div className="pm-report">
              <span>
                {new Date(r.at * 1000).toLocaleString()} — {r.bundles} bundle{r.bundles === 1 ? "" : "s"}, {r.plugins} plug-in{r.plugins === 1 ? "" : "s"}
                {r.failed.length > 0 && (
                  <>
                    ,{" "}
                    <button className="link-btn warn-text" onClick={() => setShowFailed(!showFailed)}>
                      {r.failed.length} failed {showFailed ? "▾" : "▸"}
                    </button>
                  </>
                )}
              </span>
              {showFailed && (
                <div className="pm-failed">
                  {r.failed.map((f) => (
                    <div key={f.path}>
                      <b title={f.path}>{f.path.split("/").pop()}</b>
                      <span className="dim">{f.reason}</span>
                    </div>
                  ))}
                </div>
              )}
            </div>
          ) : (
            <span className="dim">No scan report yet — rescan to make one.</span>
          )}
        </Row>
      </Section>
      <Foot close={close} note="Bundles are scanned one by one in a separate process, so a crashing plug-in can't take Fermata down.">
        <button className="push" disabled={scanning} onClick={rescan}>
          {scanning ? "Scanning…" : "Rescan Plug-ins"}
        </button>
      </Foot>
    </>
  );
}

/** "How far ⌥← and ⌥→ move…", naming the commands instead when they have no keys. */
function nudgeNote(): string {
  const keys = [keyHint("nudge-left"), keyHint("nudge-right")].filter(Boolean);
  const what = keys.length ? keys.join(" and ") : "Nudge Regions Left / Right (no keys assigned)";
  return `How far ${what} ${keys.length === 1 ? "moves" : "move"} the selected regions.`;
}

// ---- Key Commands -------------------------------------------------------------------------

function KeysPane({ close }: { close: () => void }) {
  const overrides = useSettings((s) => s.ui.keymap);
  const [q, setQ] = useState("");
  const [capture, setCapture] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const captureRef = useRef(capture);
  captureRef.current = capture;

  const setKeys = (map: Record<string, string[]>) => setUi({ keymap: map });
  const withKeys = (map: Record<string, string[]>, id: string, keys: string[]) => {
    const def = (KEY_COMMANDS.find((c) => c.id === id)?.keys ?? []).map(normalizeKey);
    const next = { ...map };
    if (JSON.stringify(keys) === JSON.stringify(def)) delete next[id];
    else next[id] = keys;
    return next;
  };

  useEffect(() => {
    if (!capture) return;
    const k = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") {
        setCapture(null);
        return;
      }
      const key = eventKey(e);
      if (!key) return;
      const id = captureRef.current!;
      setCapture(null);
      const map = getSettings().ui.keymap;
      const clash = keyConflict(key, id, map);
      if (clash && !clash.id) {
        setMessage(`${formatKey(key)} belongs to ${clash.label} in the menu bar, which takes it first. Choose another key.`);
        return;
      }
      let next = map;
      if (clash?.id) {
        next = withKeys(next, clash.id, keysFor(clash.id, next).filter((x) => x !== key));
        setMessage(`${formatKey(key)} was moved from “${clash.label}”.`);
      } else setMessage(null);
      const mine = keysFor(id, next);
      if (!mine.includes(key)) next = withKeys(next, id, [...mine, key]);
      setKeys(next);
    };
    // Capture phase, ahead of the sheet's Escape handler and the main window's shortcuts.
    window.addEventListener("keydown", k, true);
    return () => window.removeEventListener("keydown", k, true);
  }, [capture]);

  /** Puts a command's default keys back, taking them from any command that has them now (a
   *  key runs one command only). */
  const restoreDefault = (id: string, defaults: string[]) => {
    const keys = defaults.map(normalizeKey);
    let next = withKeys(overrides, id, keys);
    const moved: string[] = [];
    for (const k of keys) {
      const clash = keyConflict(k, id, next);
      if (!clash?.id) continue;
      next = withKeys(next, clash.id, keysFor(clash.id, next).filter((x) => x !== k));
      moved.push(`${formatKey(k)} from “${clash.label}”`);
    }
    setKeys(next);
    setMessage(moved.length ? `Moved ${moved.join(", ")}.` : null);
  };

  const match = (label: string, keys: string[]) => {
    if (!q) return true;
    const t = q.toLowerCase();
    return label.toLowerCase().includes(t) || keys.some((k) => formatKey(k).toLowerCase().includes(t) || k.toLowerCase().includes(t));
  };
  const customized = Object.keys(overrides).length > 0;

  return (
    <>
      <div className="pm-toolbar">
        <input className="search pm-search" placeholder="Search commands or keys" value={q} onChange={(e) => setQ(e.target.value)} onKeyDown={(e) => e.stopPropagation()} />
        <span className="dim">{message ?? "Click + and press a key to add a shortcut."}</span>
      </div>
      <div className="key-table">
        {KEY_GROUPS.map((g) => {
          const rows = KEY_COMMANDS.filter((c) => c.group === g && match(c.label, keysFor(c.id, overrides)));
          if (!rows.length) return null;
          return (
            <div key={g}>
              <div className="key-group">{g}</div>
              {rows.map((c) => {
                const keys = keysFor(c.id, overrides);
                return (
                  <div key={c.id} className={"key-row" + (overrides[c.id] ? " custom" : "")}>
                    <span className="key-label">{c.label}</span>
                    <span className="key-keys">
                      {keys.map((k) => (
                        <span key={k} className="key-chip">
                          {formatKey(k)}
                          <button title="Remove this shortcut" onClick={() => setKeys(withKeys(overrides, c.id, keys.filter((x) => x !== k)))}>
                            ×
                          </button>
                        </span>
                      ))}
                      {capture === c.id ? (
                        <span className="key-chip capturing">Press a key… (Esc cancels)</span>
                      ) : (
                        <button className="key-add" title="Add a shortcut" onClick={() => setCapture(c.id)}>
                          +
                        </button>
                      )}
                    </span>
                    {overrides[c.id] ? (
                      <button className="link-btn" onClick={() => restoreDefault(c.id, c.keys)}>
                        Default
                      </button>
                    ) : (
                      <span />
                    )}
                  </div>
                );
              })}
            </div>
          );
        })}
        {MENU_SHORTCUTS.some((m) => match(m.label, [m.key])) && <div className="key-group">Menu Bar (fixed)</div>}
        {MENU_SHORTCUTS.filter((m) => match(m.label, [m.key])).map((m) => (
          <div key={m.label} className="key-row fixed">
            <span className="key-label">
              {m.label} <span className="dim">— {m.menu}</span>
            </span>
            <span className="key-keys">
              <span className="key-chip">{formatKey(m.key)}</span>
            </span>
            <span />
          </div>
        ))}
      </div>
      <Foot close={close} note="Shortcuts work in the main window, not while typing in a text field.">
        <button className="push" disabled={!customized} onClick={() => setKeys({})}>
          Reset All
        </button>
      </Foot>
    </>
  );
}

// ---- the window ---------------------------------------------------------------------------

type PaneProps = { close: () => void };

/**
 * Fermata › Settings. The Audio and Appearance panes live in Dialogs.tsx and are passed in.
 */
export function SettingsWindow({ audio: AudioPane, appearance: AppearancePane }: { audio: React.FC<PaneProps>; appearance: React.FC<PaneProps> }) {
  const tab = useStore((s) => s.settingsTab);
  const close = () => setState({ dialog: null });
  const tabs: { id: SettingsTab; label: string; icon: React.ReactNode; pane: React.FC<PaneProps> }[] = [
    { id: "general", label: "General", icon: <TabIcon.general />, pane: GeneralPane },
    { id: "audio", label: "Audio", icon: <Icon.waveform />, pane: AudioPane },
    { id: "recording", label: "Recording", icon: <TabIcon.recording />, pane: RecordingPane },
    { id: "metronome", label: "Metronome", icon: <TabIcon.metronome />, pane: MetronomePane },
    { id: "midi", label: "MIDI", icon: <TabIcon.midi />, pane: MidiPane },
    { id: "editing", label: "Editing", icon: <TabIcon.editing />, pane: EditingPane },
    { id: "plugins", label: "Plug-ins", icon: <TabIcon.plugins />, pane: PluginsPane },
    { id: "keys", label: "Key Commands", icon: <Icon.keyboard />, pane: KeysPane },
    { id: "appearance", label: "Appearance", icon: <Icon.palette />, pane: AppearancePane },
  ];
  const current = tabs.find((t) => t.id === tab) ?? tabs[1];
  const Pane = current.pane;
  return (
    <SheetFrame title={`Settings — ${current.label}`} onClose={close} width={760}>
      <div className="settings-tabs wide">
        {tabs.map((t) => (
          <button key={t.id} className={"settings-tab" + (current.id === t.id ? " on" : "")} onClick={() => setState({ settingsTab: t.id })}>
            {t.icon}
            <span>{t.label}</span>
          </button>
        ))}
      </div>
      <div className="settings-body">
        <Pane close={close} />
      </div>
    </SheetFrame>
  );
}

// ---- Project Settings ---------------------------------------------------------------------

const KEYS = ["C", "C♯", "D", "E♭", "E", "F", "F♯", "G", "A♭", "A", "B♭", "B"];
const SIGNATURES: [number, number][] = [
  [2, 4],
  [3, 4],
  [4, 4],
  [5, 4],
  [6, 8],
  [7, 8],
  [9, 8],
  [12, 8],
];
const RATES = [44100, 48000, 88200, 96000, 176400, 192000];

function ProjectGeneral() {
  const p = useStore((s) => s.snap?.project);
  const audio = useStore((s) => s.snap?.audio);
  const [pending, setPending] = useState<number | null>(null);
  // Only the rates the audio devices can run at: the project's rate is the device's.
  const [rates, setRates] = useState<number[]>(RATES);
  useEffect(() => {
    (async () => {
      const cfg = await call<AudioConfig>("get_audio_config");
      const devices = cfg && (await call<DeviceList>("audio_devices", { host: cfg.host }));
      if (cfg && devices) setRates(RATES.filter((r) => supportedRates(cfg, devices).includes(r)));
    })();
  }, [audio?.output_name, audio?.input_name]);
  if (!p) return null;
  const hasAudio = p.tracks.some((t) => t.regions.some((r) => r.content.type === "audio"));
  const sigKey = `${p.sig_num}/${p.sig_den}`;
  const sigs: [string, string][] = SIGNATURES.map(([n, d]) => [`${n}/${d}`, `${n}/${d}`]);
  if (!sigs.some((s) => s[0] === sigKey)) sigs.push([sigKey, sigKey]);
  const applyRate = async (rate: number) => {
    const cfg = await call<AudioConfig>("get_audio_config");
    if (cfg) await act("set_audio_config", { config: { ...cfg, sample_rate: rate } });
    setPending(null);
  };
  return (
    <>
      <Section title="Tempo and Key">
        <Row label="Tempo:">
          <div className="row-inline">
            <NumberField value={p.tempo} min={5} max={990} step={1} resolution={0.01} onChange={(bpm) => act("set_tempo", { bpm })} width={96} />
            <span className="dim">{p.tempo_map.length > 1 ? "BPM at the start (see the Tempo track)" : "BPM"}</span>
          </div>
        </Row>
        <Row label="Time Signature:">
          <Select
            value={sigKey}
            options={sigs}
            onChange={(v) => {
              const [num, den] = v.split("/").map(Number);
              act("set_signature", { num, den });
            }}
            width={110}
          />
        </Row>
        <Row label="Key:">
          <Select
            value={p.key}
            options={KEYS.flatMap((k) => [[`${k} major`, `${k} major`] as [string, string], [`${k} minor`, `${k} minor`] as [string, string]])}
            onChange={(key) => act("set_key", { key })}
            width={140}
          />
        </Row>
      </Section>
      <Section title="Audio">
        <Row
          label="Sample Rate:"
          note={
            pending !== null && hasAudio ? (
              <span className="pref-warning">
                This project's audio was recorded at {(p.sample_rate / 1000).toFixed(1)} kHz. Fermata plays audio files without converting them, so at{" "}
                {(pending / 1000).toFixed(1)} kHz they would play {pending > p.sample_rate ? "faster and higher" : "slower and lower"}.{" "}
                <button className="link-btn" onClick={() => applyRate(pending)}>
                  Change Anyway
                </button>{" "}
                <button className="link-btn" onClick={() => setPending(null)}>
                  Cancel
                </button>
              </span>
            ) : (
              `The audio device runs at the project's rate${audio?.running ? ` (now ${(audio.sample_rate / 1000).toFixed(1)} kHz on ${audio.output_name})` : ""}. Imported files are converted to it.`
            )
          }
        >
          <Select
            value={pending ?? p.sample_rate}
            options={[...new Set([...rates, p.sample_rate])].sort((a, b) => a - b).map((r) => [r, `${(r / 1000).toFixed(1)} kHz`] as [number, string])}
            onChange={(r) => (r === p.sample_rate ? setPending(null) : hasAudio ? setPending(r) : applyRate(r))}
            width={140}
          />
        </Row>
      </Section>
    </>
  );
}

function ProjectRecording() {
  const depth = useStore((s) => s.snap?.project.bit_depth ?? null);
  const countIn = useStore((s) => s.snap?.project.count_in ?? false);
  const app = useSettings((s) => s.prefs.recording);
  return (
    <>
      <Section title="Recording">
        <Row label="Bit Depth:" note={`“Use Settings” follows Settings › Recording (now ${BIT_DEPTHS.find((b) => b[0] === app.bit_depth)?.[1]}).`}>
          <Select
            value={depth ?? "app"}
            options={[["app", "Use Settings"] as [string, string], ...BIT_DEPTHS]}
            onChange={(v) => act("set_record_bit_depth", { depth: v === "app" ? null : v })}
          />
        </Row>
        <Row label="Count-in:" note={`${app.count_in_bars} bar${app.count_in_bars > 1 ? "s" : ""}, set in Settings › Recording.`}>
          <Check checked={countIn} onChange={(on) => act("set_count_in", { on })}>
            Count in before recording
          </Check>
        </Row>
      </Section>
      <div className="pref-note">
        Overlapping recordings, take names and the recording folder are set for all projects in{" "}
        <button className="link-btn" onClick={() => setState({ dialog: "settings", settingsTab: "recording" })}>
          Settings › Recording
        </button>
        .
      </div>
    </>
  );
}

function ProjectNotes() {
  const notes = useStore((s) => s.snap?.project.notes ?? "");
  const [text, setText] = useState(notes);
  const latest = useRef({ text, notes });
  latest.current = { text, notes };
  useEffect(() => setText(notes), [notes]);
  // Save when leaving the tab or closing the sheet, too.
  useEffect(
    () => () => {
      const { text, notes } = latest.current;
      if (text !== notes) act("set_project_notes", { notes: text });
    },
    [],
  );
  return (
    <div className="pref-group">
      <textarea
        className="text-field project-notes"
        value={text}
        placeholder="Lyrics, session notes, who played what…"
        onChange={(e) => setText(e.target.value)}
        onBlur={() => text !== notes && act("set_project_notes", { notes: text })}
        onKeyDown={(e) => e.stopPropagation()}
      />
      <div className="pref-note">Saved with the project.</div>
    </div>
  );
}

/** File › Project Settings… : what is saved with this project. */
export function ProjectSettingsSheet() {
  const name = useStore((s) => s.snap?.project.name ?? "");
  const [tab, setTab] = useState<"general" | "recording" | "metronome" | "notes">("general");
  const close = () => setState({ dialog: null });
  const tabs = [
    { id: "general" as const, label: "General", icon: <TabIcon.project /> },
    { id: "recording" as const, label: "Recording", icon: <TabIcon.recording /> },
    { id: "metronome" as const, label: "Metronome", icon: <TabIcon.metronome /> },
    { id: "notes" as const, label: "Notes", icon: <TabIcon.notes /> },
  ];
  return (
    <SheetFrame title={`Project Settings — ${name}`} onClose={close} width={620}>
      <div className="settings-tabs">
        {tabs.map((t) => (
          <button key={t.id} className={"settings-tab" + (tab === t.id ? " on" : "")} onClick={() => setTab(t.id)}>
            {t.icon}
            <span>{t.label}</span>
          </button>
        ))}
      </div>
      <div className="settings-body">
        {tab === "general" && <ProjectGeneral />}
        {tab === "recording" && <ProjectRecording />}
        {tab === "metronome" && <MetronomeFields />}
        {tab === "notes" && <ProjectNotes />}
        <Foot close={close} note="Saved with the project. Changes can be undone." />
      </div>
    </SheetFrame>
  );
}

// ---- crash recovery -----------------------------------------------------------------------

function when(secs: number): string {
  const d = new Date(secs * 1000);
  const today = new Date().toDateString() === d.toDateString();
  return today ? `today at ${d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}` : d.toLocaleString([], { dateStyle: "medium", timeStyle: "short" });
}

/** "2 takes (up to 1:05) on Vocal and Guitar": what an interrupted recording left. */
function recordingSummary(r: RecordingRecovery): string {
  const m = Math.floor(r.seconds / 60);
  const sec = String(Math.floor(r.seconds % 60)).padStart(2, "0");
  const tracks = r.tracks.length > 1 ? `${r.tracks.slice(0, -1).join(", ")} and ${r.tracks[r.tracks.length - 1]}` : r.tracks[0];
  return `${r.takes} ${r.takes === 1 ? "take" : "takes"} (${r.takes === 1 ? "" : "up to "}${m}:${sec})${tracks ? ` on ${tracks}` : ""}`;
}

function recoveryIntro(list: Recovery[]): string {
  const single = list.length === 1;
  const recording = list.some((r) => r.recording);
  if (single && recording && !list[0].has_autosave)
    return "Fermata quit while this project was recording. The audio recorded up to that moment can be put back on its tracks, where it was recorded.";
  if (single)
    return recording
      ? "This project has autosaved changes that were never saved, and audio that was still being recorded when Fermata quit."
      : "This project has changes that were autosaved but never saved — most likely Fermata quit before you could save them.";
  return recording
    ? "These projects have unsaved work — autosaved changes, or audio that was still being recorded — most likely because Fermata quit."
    : "These projects have autosaved changes that were never saved — most likely Fermata quit before you could save them.";
}

/** Offers unsaved work found in autosaves and interrupted recordings (after a crash, or when
 * opening such a project). */
export function RecoverySheet() {
  const list = useRecoveries();
  const [busy, setBusy] = useState<string | null>(null);
  const [gone, setGone] = useState<string[]>([]);
  const items = useMemo(() => list.filter((r) => !gone.includes(r.path)), [list, gone]);
  const single = list.length === 1;
  const later = () => recoveryDone(false);
  useEffect(() => {
    if (list.length && !items.length) recoveryDone(false);
  }, [items.length, list.length]);

  const recover = async (r: Recovery) => {
    setBusy(r.path);
    const snap = await act("open_recovered", { path: r.path });
    setBusy(null);
    if (snap) {
      setState({ selectedRegions: [] });
      const takes = r.recording ? `, with ${r.recording.takes === 1 ? "the interrupted take" : `${r.recording.takes} interrupted takes`}` : "";
      toast(`Recovered “${r.name}”${takes}. Save it to keep the changes.`, 6000);
      recoveryDone(true);
    }
  };
  const discard = async (r: Recovery) => {
    const what = r.has_autosave ? `the autosaved changes to “${r.name}”` : `the interrupted recording in “${r.name}”`;
    const audio = r.recording ? " The recorded audio files stay in its Audio Files folder, where you can import them." : "";
    const ok = await ask(`Delete ${what}? ${r.saved ? "The last saved version stays as it is." : "The project was never saved, so nothing of it will remain."}${audio}`, {
      title: r.has_autosave ? "Discard Autosave" : "Discard Recording",
      kind: "warning",
      okLabel: "Discard",
      cancelLabel: "Cancel",
    });
    if (!ok) return;
    await call("discard_autosave", { path: r.path });
    setGone((g) => [...g, r.path]);
  };
  const openSaved = async (r: Recovery) => {
    await call("discard_autosave", { path: r.path });
    const snap = await act("open_project", { path: r.path });
    if (snap) setState({ selectedRegions: [] });
    recoveryDone(true);
  };

  return (
    <SheetFrame title={single ? "Recover Unsaved Changes?" : "Recover Unsaved Work"} onClose={later} width={560}>
      <div className="pref-note recovery-intro">
        {recoveryIntro(list)}
      </div>
      <div className="recovery-list">
        {items.map((r) => (
          <div key={r.path} className="recovery-item">
            <div className="recent-text">
              <span className="recent-name">{r.name}</span>
              <span className="recent-path">{r.path}</span>
              <span className="dim">
                {r.has_autosave && `Autosaved ${when(r.autosaved)} · `}
                {r.saved ? `last saved ${when(r.saved)}` : "never saved"}
              </span>
              {r.recording && (
                <span className="dim">
                  Recording interrupted {when(r.recording.started)}: {recordingSummary(r.recording)}
                </span>
              )}
            </div>
            <div className="sheet-buttons">
              <button className="push" disabled={busy !== null} onClick={() => discard(r)}>
                Discard
              </button>
              {single && r.saved && (
                <button className="push" disabled={busy !== null} onClick={() => openSaved(r)}>
                  Open Last Saved
                </button>
              )}
              <button className="push default" disabled={busy !== null} onClick={() => recover(r)}>
                {busy === r.path ? "Opening…" : "Recover"}
              </button>
            </div>
          </div>
        ))}
      </div>
      <div className="sheet-foot">
        <span className="dim">{single ? "" : "Projects you don't recover now are offered again next time."}</span>
        <div className="sheet-buttons">
          <button className="push" onClick={later}>
            {single ? "Cancel" : "Later"}
          </button>
        </div>
      </div>
    </SheetFrame>
  );
}

