import { useEffect, useState } from "react";
import { act, BUILTIN_SYNTH, call, type AudioConfig, type DeviceList, type Group, type InputLevel, type InputSel, type Tick, type TrackKind } from "../api";
import { updateGroup } from "../ops";
import { TRACK_COLORS } from "../view";
import { Icon } from "../icons";
import { NumberField, Select } from "./Form";
import { onTick, setState, useStore } from "../store";
import { inputLabel } from "../time";
import { mix } from "../theme";
import { applyTheme, THEMES, type Theme } from "../themes";
import { applyUiScale, UI_SCALES } from "../uiScale";
import { applyWaveStyle, WAVE_STYLES } from "../waveform";
import { SettingsWindow } from "./Settings";
import { PdcSettings } from "./Latency";

export function Sheet({ title, children, onClose, width = 560 }: { title: string; children: React.ReactNode; onClose: () => void; width?: number }) {
  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, [onClose]);
  return (
    <div className="sheet-backdrop" onMouseDown={onClose}>
      <div className="sheet" style={{ width }} onMouseDown={(e) => e.stopPropagation()} onKeyDown={(e) => e.stopPropagation()}>
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
          <Select value="out" options={[["out", "Output 1 + 2"]]} onChange={() => {}} />
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
    call<InputLevel | null>("input_level").then(setLevel);
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
          setLevel(l);
          setBusy(false);
          if (l && !l.attenuating) setState({ toast: { text: `${l.device} input set to 100%`, at: Date.now() } });
        }}
      >
        {busy ? "Setting…" : "Set to 100%"}
      </button>
    </div>
  );
}

function AudioPane({ close }: { close: () => void }) {
  const audio = useStore((s) => s.snap?.audio);
  const [devices, setDevices] = useState<DeviceList | null>(null);
  const [cfg, setCfg] = useState<AudioConfig | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    call<AudioConfig>("get_audio_config").then(setCfg);
  }, []);
  useEffect(() => {
    if (cfg) call<DeviceList>("audio_devices", { host: cfg.host }).then(setDevices);
  }, [cfg?.host]);
  if (!cfg) return null;
  const outDev = devices?.outputs.find((d) => d.id === cfg.output) ?? devices?.outputs.find((d) => d.is_default);
  const inDev = devices?.inputs.find((d) => d.id === cfg.input) ?? devices?.inputs.find((d) => d.is_default);
  const rates = (outDev?.sample_rates ?? [44100, 48000]).filter((r) => !inDev || !cfg.input_enabled || inDev.sample_rates.includes(r));
  const bufferMs = ((cfg.buffer_size / cfg.sample_rate) * 1000).toFixed(1);
  const hostLabel = (h: { name: string; devices: number }) =>
    h.name === "JACK" && h.devices === 0 ? "JACK (no server running)" : h.name === "ALSA" ? "ALSA (direct hardware)" : h.name;
  const apply = async () => {
    setBusy(true);
    await act("set_audio_config", { config: cfg });
    setBusy(false);
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
          <NumberField value={cfg.recording_delay_ms} step={0.5} min={-500} max={500} onChange={(v) => setCfg({ ...cfg, recording_delay_ms: v })} />
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
          <button className="push" onClick={close}>
            Done
          </button>
          <button className="push default" disabled={busy} onClick={apply}>
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
        <div className="pref-hint">Also in View › Larger / Smaller Interface (⌘= / ⌘−, ⌘0 for actual size).</div>
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
  return <SettingsWindow audio={AudioPane} appearance={AppearancePane} />;
}

/** Logic's Group Settings: name, colour and what the group's tracks share. */
export function GroupSheet() {
  const id = useStore((s) => s.groupSheet);
  const group = useStore((s) => s.snap?.project.groups.find((g) => g.id === s.groupSheet));
  const members = useStore((s) => s.snap?.project.tracks.filter((t) => t.group === s.groupSheet) ?? []);
  const close = () => setState({ groupSheet: null });
  const [name, setName] = useState(group?.name ?? "");
  useEffect(() => setName(group?.name ?? ""), [id]);
  if (!group) return null;
  const set = (patch: Partial<Group>) => updateGroup({ ...group, ...patch });
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
    <Sheet title="Group Settings" onClose={close} width={460}>
      <div className="group-sheet">
        <div className="group-row">
          <input
            className="group-name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            onBlur={() => name.trim() && name !== group.name && set({ name: name.trim() })}
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
          <button className="push default" onClick={close}>
            Done
          </button>
        </div>
      </div>
    </Sheet>
  );
}
