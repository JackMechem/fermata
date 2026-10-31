import { useEffect, useRef, useState } from "react";
import { threadOptions, type ThreadChoices } from "../threads";
import { useSheetKeys } from "../sheetKeys";
import type { SheetMode } from "../sheetKeysModel";
import { ask } from "@tauri-apps/plugin-dialog";
import { act, BUILTIN_SYNTH, call, supportedRates, type AudioConfig, type DeviceList, type Group, type InputLevel, type InputSel, type Tick, type TrackKind } from "../api";
import { updateGroup } from "../ops";
import { TRACK_COLORS } from "../view";
import { Icon } from "../icons";
import { NumberField, Select, sheetKey } from "./Form";
import { onTick, setState, useStore } from "../store";
import { inputLabel } from "../time";
import { mix } from "../theme";
import { applyTheme, THEMES, type Theme } from "../themes";
import { applyUiScale, UI_SCALES } from "../uiScale";
import { applyWaveStyle, WAVE_STYLES } from "../waveform";
import { SettingsWindow } from "./Settings";
import { PdcSettings } from "./Latency";
import { fmt } from "../keymap";

/** A sheet: modal for key commands unless `mode` says otherwise (sheetKeys.ts). */
export function Sheet({ title, children, onClose, width = 560, mode = "modal" }: { title: string; children: React.ReactNode; onClose: () => void; width?: number; mode?: SheetMode }) {
  const ref = useRef<HTMLDivElement>(null);
  useSheetKeys(ref, onClose, mode);
  return (
    <div className="sheet-backdrop" onMouseDown={onClose}>
      <div ref={ref} className="sheet" style={{ width }} onMouseDown={(e) => e.stopPropagation()} onKeyDown={(e) => sheetKey(e, onClose)}>
        <div className="sheet-title">{title}</div>
        {children}
      </div>
    </div>
  );
}

export function NewTrackDialog() {
  const plugins = useStore((s) => s.plugins);
  const audio = useStore((s) => s.snap?.audio);
  const [kind, setKind] = useState<TrackKind>("audio");
  const [count, setCount] = useState(1);
  const [inputKey, setInputKey] = useState("mono:0");
  const [ascending, setAscending] = useState(true);
  const [instrument, setInstrument] = useState(BUILTIN_SYNTH);
  const [monitor, setMonitor] = useState(false);
  const [armed, setArmed] = useState(true);
  const close = () => setState({ dialog: null });
  const ch = Math.max(2, audio?.input_channels ?? 2);
  const inputs: [string, string][] = [["none:0", "No Input"]];
  for (let c = 0; c < ch; c++) inputs.push([`mono:${c}`, inputLabel({ type: "mono", ch: c })]);
  for (let c = 0; c + 1 < ch; c += 2) inputs.push([`stereo:${c}`, inputLabel({ type: "stereo", ch: c })]);
  const instruments: [string, string][] = [[BUILTIN_SYNTH, "Fermata Synth"], ...plugins.filter((p) => p.is_instrument).map((p) => [p.id, `${p.name} — ${p.vendor}`] as [string, string])];

  const create = async () => {
    const [t, c] = inputKey.split(":");
    const input: InputSel = t === "none" ? { type: "none" } : { type: t as "mono" | "stereo", ch: parseInt(c) };
    const snap = await act("add_tracks", {
      kind,
      count,
      input,
      ascending,
      instrument: kind === "instrument" ? instrument : null,
      armed,
      monitor: kind === "audio" && monitor,
    });
    if (snap) close();
  };

  return (
    <Sheet title="Choose a track type" onClose={close} width={600}>
      <div className="track-types">
        <button className={"track-type" + (kind === "instrument" ? " on" : "")} onClick={() => setKind("instrument")} onDoubleClick={create}>
          <Icon.piano />
          <span className="tt-title">Software Instrument</span>
          <span className="tt-sub">Play and record using a VST3 instrument or the built-in synth.</span>
        </button>
        <button className={"track-type" + (kind === "audio" ? " on" : "")} onClick={() => setKind("audio")} onDoubleClick={create}>
          <Icon.mic />
          <span className="tt-title">Audio</span>
          <span className="tt-sub">Record using a microphone or a line input — vocals, guitar, bass.</span>
        </button>
      </div>
      <div className="sheet-details">
        <div className="details-title">Details:</div>
        {kind === "instrument" ? (
          <div className="form-row">
            <label>Instrument:</label>
            <Select value={instrument} options={instruments} onChange={setInstrument} />
          </div>
        ) : (
          <>
            <div className="form-row">
              <label>Audio Input:</label>
              <Select value={inputKey} options={inputs} onChange={setInputKey} />
              <label className="check">
                <input type="checkbox" checked={ascending} onChange={(e) => setAscending(e.target.checked)} /> Ascending
              </label>
            </div>
            <div className="form-hint">{audio?.input_name ? `Input device: ${audio.input_name}` : "No input device — choose one in Settings › Audio"}</div>
          </>
        )}
        <div className="form-row">
          <label>Audio Output:</label>
          {/* New tracks always start on Stereo Out; the strip's Output slot routes them elsewhere. */}
          <span className="dim">Stereo Out — change it later in the channel strip's Output slot</span>
        </div>
        <div className="form-row checks">
          <label className="check">
            <input type="checkbox" checked={armed} onChange={(e) => setArmed(e.target.checked)} /> Record Enable
          </label>
          {kind === "audio" && (
            <label className="check">
              <input type="checkbox" checked={monitor} onChange={(e) => setMonitor(e.target.checked)} /> Input Monitoring
            </label>
          )}
        </div>
      </div>
      <div className="sheet-foot">
        <div className="count">
          <label>Number of tracks:</label>
          <NumberField value={count} min={1} max={64} onChange={(v) => setCount(Math.round(v))} width={84} />
        </div>
        <div className="sheet-buttons">
          <button className="push" onClick={close}>
            Cancel
          </button>
          <button className="push default" onClick={create}>
            Create
          </button>
        </div>
      </div>
    </Sheet>
  );
}

function LatencyReadout() {
  const [lat, setLat] = useState<Tick["latency"] | null>(null);
  useEffect(() => onTick((t) => setLat((p) => (p && Math.abs(p.compensation_ms - t.latency.compensation_ms) < 0.05 ? p : t.latency))), []);
  if (!lat || (lat.input_ms === 0 && lat.output_ms === 0)) return null;
  return (
    <div className="latency-readout">
      <span>
        Input <b>{lat.input_ms.toFixed(1)} ms</b>
      </span>
      <span>
        Output <b>{lat.output_ms.toFixed(1)} ms</b>
      </span>
      <span>
        Recordings shifted <b>{lat.compensation_ms.toFixed(1)} ms</b> earlier to line up with what you heard
      </span>
    </div>
  );
}

/** Warns when the sound server's own input volume is attenuating recordings. */
function InputLevelCheck() {
  const audio = useStore((s) => s.snap?.audio);
  const [level, setLevel] = useState<InputLevel | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    // Read again now and then and when the window comes back: the level can be changed in the
    // system mixer while this pane is open.
    const read = () => call<InputLevel | null>("input_level").then(setLevel);
    read();
    const timer = window.setInterval(read, 5000);
    window.addEventListener("focus", read);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener("focus", read);
    };
  }, [audio?.input_name, audio?.host]);
  if (!level?.attenuating) return null;
  const pct = Math.round(level.volume * 100);
  return (
    <div className="level-warning">
      <div>
        <b>
          {level.device} is {level.muted ? "muted" : `set to ${pct}%`} in the system mixer
        </b>
        {level.muted
          ? ", so Fermata receives silence."
          : `, which lowers everything you record by ${Math.abs(level.gain_db).toFixed(0)} dB before it reaches Fermata.`}{" "}
        Recording software expects 100% — set your level with the gain knob on the interface instead.
      </div>
      <button
        className="push default"
        disabled={busy}
        onClick={async () => {
          setBusy(true);
          const l = await call<InputLevel | null>("reset_input_level");
          setBusy(false);
          // No answer (the mixer couldn't be set or read back): keep the warning up rather than
          // hiding it as if it were fixed. Errors were reported by call().
          if (!l) return;
          setLevel(l);
          if (!l.attenuating) setState({ toast: { text: `${l.device} input set to 100%`, at: Date.now() } });
        }}
      >
        {busy ? "Setting…" : "Set to 100%"}
      </button>
    </div>
  );
}

/** Audio settings changed but not applied yet. Kept while Settings is open, so switching tabs
 *  doesn't lose them; dropped when it closes (see SettingsDialog). */
let audioDraft: AudioConfig | null = null;

function AudioPane({ close }: { close: () => void }) {
  const audio = useStore((s) => s.snap?.audio);
  const project = useStore((s) => s.snap?.project);
  const [devices, setDevices] = useState<DeviceList | null>(null);
  const [cfg, setCfgState] = useState<AudioConfig | null>(audioDraft);
  const [applied, setApplied] = useState<AudioConfig | null>(null);
  const [busy, setBusy] = useState(false);
  const [threads, setThreads] = useState<ThreadChoices | null>(null);
  const setCfg = (c: AudioConfig) => {
    audioDraft = c;
    setCfgState(c);
  };
  useEffect(() => {
    call<AudioConfig>("get_audio_config").then((c) => {
      setApplied(c);
      if (c && !audioDraft) setCfgState(c);
    });
    call<ThreadChoices>("thread_choices").then(setThreads);
  }, []);
  useEffect(() => {
    if (cfg) call<DeviceList>("audio_devices", { host: cfg.host }).then(setDevices);
  }, [cfg?.host]);
  if (!cfg) return null;
  const rates = supportedRates(cfg, devices);
  const changed = applied !== null && JSON.stringify(cfg) !== JSON.stringify(applied);
  const bufferMs = ((cfg.buffer_size / cfg.sample_rate) * 1000).toFixed(1);
  const hostLabel = (h: { name: string; devices: number }) =>
    h.name === "JACK" && h.devices === 0 ? "JACK (no server running)" : h.name === "ALSA" ? "ALSA (direct hardware)" : h.name;
  const apply = async (): Promise<boolean> => {
    // The project runs at the device's rate, and its audio files play unconverted: warn as
    // Project Settings does before a rate change makes recorded audio play at the wrong speed.
    const sr = project?.sample_rate;
    if (sr && cfg.sample_rate !== sr && project.tracks.some((t) => t.regions.some((r) => r.content.type === "audio"))) {
      const ok = await ask(
        `This project's audio was recorded at ${(sr / 1000).toFixed(1)} kHz. Fermata plays audio files without converting them, so at ${(cfg.sample_rate / 1000).toFixed(1)} kHz they would play ${cfg.sample_rate > sr ? "faster and higher" : "slower and lower"}.`,
        { title: "Change Sample Rate", kind: "warning", okLabel: "Change Anyway", cancelLabel: "Cancel" },
      );
      if (!ok) return false;
    }
    setBusy(true);
    const snap = await act("set_audio_config", { config: cfg });
    setBusy(false);
    if (snap) {
      audioDraft = null;
      setApplied(cfg);
    }
    return !!snap;
  };
  // Done applies what's still pending, like OK in a settings dialog.
  const done = async () => {
    if (!changed || (await apply())) close();
  };
  return (
    <>
      <div className="settings-grid">
        <label>Driver:</label>
        <Select
          value={cfg.host ?? devices?.host ?? ""}
          options={(devices?.hosts ?? []).map((h) => [h.name, hostLabel(h)] as [string, string])}
          onChange={(h) => setCfg({ ...cfg, host: h, input: null, output: null })}
        />
        <label>Output Device:</label>
        <Select
          value={cfg.output ?? ""}
          options={[["", "System Default"] as [string, string], ...(devices?.outputs ?? []).map((d) => [d.id, `${d.name} (${d.channels} ch)`] as [string, string])]}
          onChange={(v) => setCfg({ ...cfg, output: v || null })}
        />
        <label>Input Device:</label>
        <Select
            value={cfg.input_enabled ? cfg.input ?? "" : "__off"}
            options={[
              ["__off", "None (playback only)"] as [string, string],
              ["", "System Default"] as [string, string],
              ...(devices?.inputs ?? []).map((d) => [d.id, `${d.name} (${d.channels} ch)`] as [string, string]),
            ]}
            onChange={(v) => setCfg({ ...cfg, input_enabled: v !== "__off", input: v === "__off" || v === "" ? null : v })}
          />
        <label>Sample Rate:</label>
        <Select value={cfg.sample_rate} options={rates.map((r) => [r, `${(r / 1000).toFixed(1)} kHz`] as [number, string])} onChange={(r) => setCfg({ ...cfg, sample_rate: r })} />
        <label>I/O Buffer Size:</label>
        <div className="row-inline">
          <Select value={cfg.buffer_size} options={[32, 64, 128, 256, 512, 1024, 2048].map((b) => [b, `${b} samples`] as [number, string])} onChange={(b) => setCfg({ ...cfg, buffer_size: b })} />
          <span className="dim">{bufferMs} ms per buffer</span>
        </div>
        <label>Processing Threads:</label>
        {threads ? <Select value={cfg.processing_threads} options={threadOptions(threads)} onChange={(n) => setCfg({ ...cfg, processing_threads: n })} /> : <span />}
        <span />
        <span className="form-note">
          How many threads play tracks at once. Automatic uses more only when a project needs them; 1 keeps everything on the audio thread. Changes take effect
          without restarting audio.
        </span>
        <label>Monitoring:</label>
        <label className="check">
          <input type="checkbox" checked={cfg.auto_monitor} onChange={(e) => setCfg({ ...cfg, auto_monitor: e.target.checked })} />
          Hear record-enabled tracks without pressing I
        </label>
        <span />
        <span className="form-note">
          Leave this off if your interface or pedal monitors directly — otherwise you hear a second, delayed copy, which sounds hollow
          or phasey. Use a track's I button to hear its input through Fermata (and its plug-ins).
        </span>
        <label>Recording Delay:</label>
        <div className="row-inline">
          <NumberField value={cfg.recording_delay_ms} step={0.5} resolution={0.1} min={-500} max={500} onChange={(v) => setCfg({ ...cfg, recording_delay_ms: v })} />
          <span className="dim">ms, added to the measured latency</span>
        </div>
        <PdcSettings />
      </div>
      <LatencyReadout />
      <InputLevelCheck />
      <div className="settings-status">
        {audio?.running ? (
          <>
            <span className="status-dot ok" />
            Running on {audio.host}: {audio.output_name} → {audio.output_channels} out
            {audio.input_name ? `, ${audio.input_name} → ${audio.input_channels} in` : ", no input"} @ {audio.sample_rate} Hz, {audio.buffer_size} samples
          </>
        ) : (
          <>
            <span className="status-dot bad" />
            Audio stopped{audio?.error ? `: ${audio.error}` : ""}
          </>
        )}
        {audio?.error && audio.running && <div className="warn">{audio.error}</div>}
      </div>
      <div className="sheet-foot">
        <span className="dim">PipeWire is the recommended driver on Linux.</span>
        <div className="sheet-buttons">
          <button
            className="push"
            disabled={busy}
            title="Close and reopen the audio devices, e.g. after an interface was replugged"
            onClick={async () => {
              setBusy(true);
              await act("restart_audio");
              setBusy(false);
            }}
          >
            Reset Audio Engine
          </button>
          <button className="push" disabled={busy} onClick={done} title={changed ? "Apply the changes and close" : undefined}>
            Done
          </button>
          <button className="push default" disabled={busy || !changed} onClick={apply}>
            {busy ? "Applying…" : "Apply Changes"}
          </button>
        </div>
      </div>
    </>
  );
}

function ThemePreview({ t }: { t: Theme }) {
  const c = t.css;
  const cv = t.canvas;
  const region = (color: string) =>
    cv.region === "light" ? mix(color, "#ffffff", 0.6) : cv.region === "flat" ? mix(color, cv.regionBase, 0.55) : mix(color, cv.regionBase, 0.25, 0.85);
  return (
    <div className="theme-preview" style={{ background: c["--bg"], boxShadow: `0 0 0 1px ${c["--edge"]}` }}>
      <div className="tp-bar" style={{ background: c["--cb-bg"], borderBottom: `1px solid ${c["--cb-edge"]}` }}>
        <i style={{ background: c["--btn"] }} />
        <i style={{ background: c["--btn"] }} />
        <b style={{ background: c["--lcd-bg"], boxShadow: c["--lcd-shadow"] }} />
        <i style={{ background: c["--cycle"] }} />
        <i style={{ background: c["--accent"] }} />
      </div>
      <div className="tp-body">
        <div className="tp-heads" style={{ background: c["--headers-bg"], borderRight: `1px solid ${c["--edge"]}` }}>
          <div style={{ background: c["--header-sel"], borderBottom: `1px solid ${c["--header-line"]}` }} />
          <div style={{ background: c["--header"], borderBottom: `1px solid ${c["--header-line"]}` }} />
          <div style={{ background: c["--header"] }} />
        </div>
        <div className="tp-lanes" style={{ background: cv.arrangeBg }}>
          <div style={{ background: cv.laneSelected, borderBottom: `1px solid ${cv.arrangeBg}` }} />
          <div style={{ background: cv.lane, borderBottom: `1px solid ${cv.arrangeBg}` }} />
          <div style={{ background: cv.lane }} />
          <span className="tp-region" style={{ left: "6%", width: "52%", top: 2, background: region("#3b8fd9") }} />
          <span className="tp-region" style={{ left: "38%", width: "50%", top: "calc(33.3% + 2px)", background: region("#f2a63b") }} />
          <span className="tp-region" style={{ left: "6%", width: "40%", top: "calc(66.6% + 2px)", background: region("#4fae4c") }} />
          <span style={{ position: "absolute", left: "64%", top: 0, bottom: 0, width: 1, background: cv.playhead }} />
        </div>
      </div>
    </div>
  );
}

function AppearancePane({ close }: { close: () => void }) {
  const current = useStore((s) => s.themeId);
  const scale = useStore((s) => s.uiScale);
  const waveStyle = useStore((s) => s.waveStyle);
  return (
    <>
      <div className="pref-section">
        <div className="pref-title">Interface Size</div>
        <div className="segmented">
          {UI_SCALES.map((s) => (
            <button key={s} className={Math.abs(s - scale) < 0.001 ? "on" : ""} onClick={() => applyUiScale(s)}>
              {Math.round(s * 100)}%
            </button>
          ))}
        </div>
        <div className="pref-hint">Also in View › Larger / Smaller Interface ({fmt("Mod+=")} / {fmt("Mod+-")}, {fmt("Mod+0")} for actual size).</div>
      </div>
      <div className="pref-section">
        <div className="pref-title">Waveforms</div>
        <div className="segmented">
          {WAVE_STYLES.map((s) => (
            <button key={s.id} className={s.id === waveStyle ? "on" : ""} title={s.description} onClick={() => applyWaveStyle(s.id)}>
              {s.name}
            </button>
          ))}
        </div>
        <div className="pref-hint">{WAVE_STYLES.find((s) => s.id === waveStyle)?.description}.</div>
      </div>
      <div className="pref-title">Theme</div>
      <div className="theme-grid">
        {THEMES.map((t) => (
          <button key={t.id} className={"theme-card" + (t.id === current ? " on" : "")} onClick={() => applyTheme(t.id)}>
            <ThemePreview t={t} />
            <span className="theme-name">{t.name}</span>
            <span className="theme-desc">{t.description}</span>
          </button>
        ))}
      </div>
      <div className="sheet-foot">
        <span className="dim">Changes apply immediately and are remembered.</span>
        <div className="sheet-buttons">
          <button className="push default" onClick={close}>
            Done
          </button>
        </div>
      </div>
    </>
  );
}

export function SettingsDialog() {
  // Unapplied audio settings last as long as the window.
  useEffect(
    () => () => {
      audioDraft = null;
    },
    [],
  );
  return <SettingsWindow audio={AudioPane} appearance={AppearancePane} />;
}

/** Logic's Group Settings: name, colour and what the group's tracks share. */
export function GroupSheet() {
  const id = useStore((s) => s.groupSheet);
  const group = useStore((s) => s.snap?.project.groups.find((g) => g.id === s.groupSheet));
  // Select the stable track list and filter it here: a selector that builds a new array on
  // every call makes useSyncExternalStore re-render forever, which blanks the window.
  const tracks = useStore((s) => s.snap?.project.tracks);
  const members = (tracks ?? []).filter((t) => t.group === id);
  const close = () => setState({ groupSheet: null });
  const [name, setName] = useState(group?.name ?? "");
  useEffect(() => setName(group?.name ?? ""), [id]);
  if (!group) return null;
  const set = (patch: Partial<Group>) => updateGroup({ ...group, ...patch });
  const commitName = () => name.trim() && name !== group.name && set({ name: name.trim() });
  // Closing (Done, Esc or a click outside) keeps a name still being typed: the field may not
  // get its blur before the sheet goes.
  const done = () => {
    commitName();
    close();
  };
  const links: [keyof Group, string, string][] = [
    ["volume", "Volume", "Moving one fader moves the others by the same amount"],
    ["pan", "Pan", "Pan moves together, by the same amount"],
    ["mute", "Mute", ""],
    ["solo", "Solo", ""],
    ["record", "Record Enable", ""],
    ["select", "Track Selection", "Clicking one track selects them all"],
    ["editing", "Editing (Selection)", "Selecting a region selects the regions at the same place on the other tracks"],
  ];
  return (
    <Sheet title="Group Settings" onClose={done} width={460}>
      <div className="group-sheet">
        <div className="group-row">
          <input
            className="group-name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            onBlur={commitName}
            onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
          />
          <label className="check">
            <input type="checkbox" checked={group.enabled} onChange={(e) => set({ enabled: e.target.checked })} />
            Group on
          </label>
        </div>
        <div className="group-colors">
          {TRACK_COLORS.map((c) => (
            <button key={c} className={"group-swatch" + (c === group.color ? " on" : "")} style={{ background: c }} onClick={() => set({ color: c })} />
          ))}
        </div>
        <div className="pref-title">Shared by the group's tracks</div>
        <div className="group-links">
          {links.map(([k, label, hint]) => (
            <label key={k} className="check" title={hint}>
              <input type="checkbox" checked={group[k] as boolean} onChange={(e) => set({ [k]: e.target.checked })} />
              {label}
              {hint && <span className="dim"> — {hint}</span>}
            </label>
          ))}
        </div>
        <div className="pref-hint">
          {members.length} track{members.length === 1 ? "" : "s"}: {members.map((t) => t.name).join(", ")}
        </div>
      </div>
      <div className="sheet-foot">
        <button className="push" onClick={() => act("delete_group", { id: group.id }).then(close)}>
          Delete Group
        </button>
        <div className="sheet-buttons">
          <button className="push default" onClick={done}>
            Done
          </button>
        </div>
      </div>
    </Sheet>
  );
}
