// File › Bounce Project or Section, File › Export › All Tracks as Audio Files, Export Region as
// Audio File, and Track › Bounce in Place, after Logic's dialogs. The renders run offline in the
// engine; these sheets stay up with a progress bar until they finish.
import { regionEnd } from "../loops";
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import { call, type Id, type Made, type Project } from "../api";
import {
  bounceRequest,
  inPlaceRequest,
  projectEnd,
  setBouncing,
  type BipOptions,
  type BipSource,
  type BitDepth,
  type BounceResult,
  type RenderOptions,
} from "../bounce";
import { getState, setState, useStore } from "../store";
import { formatTime, samplesPerBar } from "../time";
import { Sheet } from "./Dialogs";
import { NumberField, Select } from "./Form";

const toast = (text: string, ms = 6000) => setState({ toast: { text, at: Date.now(), ms } });

/** Remembered between uses, like Logic's bounce settings. */
function loadPref<T extends object>(key: string, fallback: T): T {
  try {
    return { ...fallback, ...JSON.parse(localStorage.getItem(key) ?? "{}") };
  } catch {
    return fallback;
  }
}

const savePref = (key: string, v: unknown) => localStorage.setItem(key, JSON.stringify(v));

/** Tracks render progress while `run` is busy; null when idle. */
function useRender() {
  const [progress, setProgress] = useState<number | null>(null);
  const run = async <T,>(f: () => Promise<T>): Promise<T> => {
    setProgress(0);
    setBouncing(true);
    const off = await listen<number>("bounce-progress", (e) => setProgress(e.payload));
    try {
      return await f();
    } finally {
      off();
      setBouncing(false);
      setProgress(null);
    }
  };
  return { progress, busy: progress !== null, run };
}

function Progress({ value }: { value: number }) {
  return (
    <div className="bounce-progress">
      <div style={{ width: `${Math.round(value * 100)}%` }} />
    </div>
  );
}

/** The folder holding the project, and the platform's separator. */
function besideProject(path: string, name: string) {
  const sep = path.includes("\\") ? "\\" : "/";
  const dir = path.replace(/[\\/][^\\/]*[\\/]?$/, "");
  return { dir, file: `${dir}${sep}${name}` };
}

const fileName = (p: string) => p.split(/[\\/]/).pop() ?? p;

type RangeKind = "project" | "cycle" | "custom";

interface BouncePrefs {
  tail_secs: number;
  bit_depth: BitDepth;
  sample_rate: number | null;
  normalize: boolean;
}

const BOUNCE_PREFS = "fermata.bounce";
const RATES = [44100, 48000, 88200, 96000];
const DEPTHS: [BitDepth, string][] = [
  ["int16", "16-bit (dithered)"],
  ["int24", "24-bit"],
  ["float32", "32-bit float"],
];

const kHz = (r: number) => `${(r / 1000).toFixed(1).replace(/\.0$/, "")} kHz`;

/** Bounce Project or Section, Export All Tracks, and Export Region share one sheet. */
export function BounceDialog() {
  const { kind, region: regionId } = bounceRequest;
  const project = useStore((s) => s.snap?.project) as Project;
  const path = useStore((s) => s.snap?.path ?? "");
  const [prefs, setPrefs] = useState<BouncePrefs>(() =>
    loadPref<BouncePrefs>(BOUNCE_PREFS, { tail_secs: 2, bit_depth: "int24", sample_rate: null, normalize: false }),
  );
  const cycle = project.cycle;
  const hasCycle = cycle.end > cycle.start;
  const [range, setRange] = useState<RangeKind>(hasCycle && cycle.enabled ? "cycle" : "project");
  const bar = samplesPerBar(project);
  const end = projectEnd(project);
  const [custom, setCustom] = useState({ from: 1, to: Math.max(2, Math.ceil(end / bar) + 1) });
  const [targets, setTargets] = useState<Id[]>([]);
  const { progress, busy, run } = useRender();
  const region = regionId !== null ? project.tracks.flatMap((t) => t.regions).find((r) => r.id === regionId) : undefined;
  const close = () => !busy && setState({ dialog: null });

  useEffect(() => {
    if (kind === "tracks") call<Id[]>("export_targets").then((t) => t && setTargets(t));
  }, [kind]);

  const span: [number, number] = region
    ? [region.start, regionEnd(region)]
    : range === "cycle"
      ? [cycle.start, cycle.end]
      : range === "custom"
        ? [Math.round((custom.from - 1) * bar), Math.round((custom.to - 1) * bar)]
        : [0, end];
  const empty = span[1] <= span[0];
  const set = (p: Partial<BouncePrefs>) => {
    const next = { ...prefs, ...p };
    setPrefs(next);
    savePref(BOUNCE_PREFS, next);
  };
  const barOf = (pos: number) => Math.round((pos / bar) * 100) / 100 + 1;

  const go = async () => {
    const options: RenderOptions = { start: span[0], end: span[1], ...prefs };
    if (kind === "tracks") {
      const dir = await open({ directory: true, title: "Export All Tracks to Folder", defaultPath: besideProject(path, "").dir });
      if (typeof dir !== "string") return;
      const res = await run(() => call<BounceResult>("export_tracks", { dir, options }));
      if (!res) return;
      setState({ dialog: null });
      toast(`Exported ${res.paths.length} track${res.paths.length === 1 ? "" : "s"} to ${dir}`);
      return;
    }
    const name = (region?.name || project.name || "Bounce").replace(/[\\/:*?"<>|]/g, "_");
    const dest = await save({
      title: region ? "Export Region as Audio File" : "Bounce",
      defaultPath: besideProject(path, `${name}.wav`).file,
      filters: [{ name: "WAV audio", extensions: ["wav"] }],
    });
    if (!dest) return;
    const file = /\.wav$/i.test(dest) ? dest : `${dest}.wav`;
    const res = region
      ? await run(() => call<BounceResult>("export_region", { id: region.id, path: file, options }))
      : await run(() => call<BounceResult>("bounce_project", { path: file, options }));
    if (!res) return;
    setState({ dialog: null });
    const peak = Number.isFinite(res.peak_db) ? `, peak ${res.peak_db.toFixed(1)} dBFS` : ", silent";
    toast(`${region ? "Exported" : "Bounced"} ${fileName(file)} (${formatTime(project, res.seconds * project.sample_rate)}${peak}) to ${file}`);
  };

  const title = kind === "tracks" ? "Export All Tracks as Audio Files" : kind === "region" ? "Export Region as Audio File" : "Bounce Project or Section";
  const names = targets.map((id) => project.tracks.find((t) => t.id === id)?.name).filter(Boolean);
  const ranges: [RangeKind, string][] = [["project", "Whole Project"]];
  if (hasCycle) ranges.push(["cycle", "Cycle Range"]);
  ranges.push(["custom", "Bars…"]);

  return (
    <Sheet title={title} onClose={close} width={500}>
      <fieldset className="bounce-form" disabled={busy}>
        <div className="settings-grid">
          {region ? (
            <>
              <label>Region:</label>
              <span>{region.name}</span>
            </>
          ) : (
            <>
              <label>Range:</label>
              <Select value={range} options={ranges} onChange={setRange} />
              {range === "custom" && (
                <>
                  <label>Bars:</label>
                  <div className="row-inline">
                    <NumberField value={custom.from} min={1} max={9999} onChange={(v) => setCustom({ ...custom, from: Math.round(v) })} />
                    <span className="dim">to</span>
                    <NumberField value={custom.to} min={1} max={9999} onChange={(v) => setCustom({ ...custom, to: Math.round(v) })} />
                  </div>
                </>
              )}
            </>
          )}
          <span />
          <span className="dim">
            {empty
              ? "Nothing to bounce in this range"
              : `Bar ${barOf(span[0])} to bar ${barOf(span[1])} · ${formatTime(project, span[1] - span[0])}`}
          </span>
          <label>Audio Tail:</label>
          <div className="row-inline">
            <NumberField value={prefs.tail_secs} step={0.5} min={0} max={30} onChange={(v) => set({ tail_secs: v })} />
            <span className="dim">seconds for reverbs and releases</span>
          </div>
          <label>File Format:</label>
          <Select value="wav" options={[["wav", "WAVE"]]} onChange={() => {}} />
          <label>Resolution:</label>
          <Select value={prefs.bit_depth} options={DEPTHS} onChange={(v) => set({ bit_depth: v })} />
          <label>Sample Rate:</label>
          <Select
            value={prefs.sample_rate ?? 0}
            options={[[0, `Project (${kHz(project.sample_rate)})`] as [number, string], ...RATES.map((r) => [r, kHz(r)] as [number, string])]}
            onChange={(v) => set({ sample_rate: v || null })}
          />
          <label>Normalize:</label>
          <label className="check">
            <input type="checkbox" checked={prefs.normalize} onChange={(e) => set({ normalize: e.target.checked })} />
            Peak at −0.1 dBFS
          </label>
          {kind === "tracks" && (
            <>
              <span />
              <span className="form-note">
                {names.length
                  ? `${names.length} file${names.length === 1 ? "" : "s"}, each track through its own channel strip: ${names.join(", ")}.`
                  : "No tracks have regions to export."}{" "}
                Summing stacks are exported as a whole as well; folder stacks have no file of their own.
              </span>
            </>
          )}
        </div>
      </fieldset>
      {progress !== null && <Progress value={progress} />}
      <div className="sheet-foot">
        <span className="dim">{busy ? "Rendering offline; playback is silent until it finishes." : ""}</span>
        <div className="sheet-buttons">
          <button className="push" disabled={busy} onClick={close}>
            Cancel
          </button>
          <button className="push default" disabled={busy || empty || (kind === "tracks" && !names.length)} onClick={go}>
            {busy ? `${kind === "project" ? "Bouncing" : "Exporting"}… ${Math.round((progress ?? 0) * 100)}%` : kind === "project" ? "Bounce…" : "Export…"}
          </button>
        </div>
      </div>
    </Sheet>
  );
}

const BIP_PREFS = "fermata.bounce-in-place";
const SOURCES: [BipSource, string][] = [
  ["leave", "Leave"],
  ["mute", "Mute"],
  ["delete", "Delete"],
];

/** Track › Bounce in Place: options, then a new audio track below each source. */
export function BounceInPlaceDialog() {
  const { tracks: ids, regions } = inPlaceRequest;
  const project = useStore((s) => s.snap?.project) as Project;
  const [opts, setOpts] = useState<BipOptions>(() => loadPref<BipOptions>(BIP_PREFS, { effects: true, fader: false, source: "mute", tail: true }));
  const { progress, busy, run } = useRender();
  const close = () => !busy && setState({ dialog: null });
  const set = (p: Partial<BipOptions>) => {
    const next = { ...opts, ...p };
    setOpts(next);
    savePref(BIP_PREFS, next);
  };
  const tracks = project.tracks.filter((t) => ids.includes(t.id));
  const what = regions.length
    ? `${regions.length} region${regions.length === 1 ? "" : "s"}`
    : tracks.length === 1
      ? `“${tracks[0].name}”`
      : `${tracks.length} tracks`;

  const go = async () => {
    const res = await run(() => call<Made>("bounce_in_place", { ids, regions, options: opts }));
    if (!res) return;
    const kept = new Set(res.snapshot.project.tracks.flatMap((t) => t.regions.map((r) => r.id)));
    setState({ snap: res.snapshot, dialog: null, selectedTracks: res.ids, focus: "tracks", selectedRegions: getState().selectedRegions.filter((r) => kept.has(r)) });
    const made = res.snapshot.project.tracks.filter((t) => res.ids.includes(t.id)).map((t) => t.name);
    toast(`Bounced in place: ${made.join(", ")}`, 3500);
  };

  return (
    <Sheet title="Bounce in Place" onClose={close} width={440}>
      <fieldset className="bounce-form" disabled={busy}>
        <div className="settings-grid bip-grid">
          <label>Bounce:</label>
          <span>{what} to a new audio track</span>
          <label>Source:</label>
          <div className="segmented">
            {SOURCES.map(([v, label]) => (
              <button key={v} className={opts.source === v ? "on" : ""} onClick={() => set({ source: v })}>
                {label}
              </button>
            ))}
          </div>
          <label>Include:</label>
          <div className="bip-checks">
            <label className="check">
              <input type="checkbox" checked={opts.effects} onChange={(e) => set({ effects: e.target.checked })} />
              Audio FX <span className="dim">(Channel EQ and plug-ins)</span>
            </label>
            <label className="check">
              <input type="checkbox" checked={opts.fader} onChange={(e) => set({ fader: e.target.checked })} />
              Volume/Pan <span className="dim">{opts.fader ? "" : "(the new track takes them over)"}</span>
            </label>
            <label className="check">
              <input type="checkbox" checked={opts.tail} onChange={(e) => set({ tail: e.target.checked })} />
              Audio Tail
            </label>
          </div>
        </div>
      </fieldset>
      {progress !== null && <Progress value={progress} />}
      <div className="sheet-foot">
        <span className="dim">{busy ? "Rendering offline…" : ""}</span>
        <div className="sheet-buttons">
          <button className="push" disabled={busy} onClick={close}>
            Cancel
          </button>
          <button className="push default" disabled={busy} onClick={go}>
            {busy ? `Bouncing… ${Math.round((progress ?? 0) * 100)}%` : "OK"}
          </button>
        </div>
      </div>
    </Sheet>
  );
}
