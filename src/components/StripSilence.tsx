// The Strip Silence sheet (Logic's Remove Silence from Audio Region): threshold, minimum time,
// pre-attack and post-release, with a live preview of where the cuts fall on the first selected
// region. OK splits the selected audio regions around the parts that sound
// (crates/engine/src/strip_silence.rs).
import { useCallback, useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore } from "react";
import { actMade, call, type Id, type Region } from "../api";
import { getState, setState, useStore } from "../store";
import { drawWave, ensureWave, waveCache } from "../waveform";
import { Sheet } from "./Dialogs";
import { NumberField, Slider } from "./Form";

/** The engine's StripSettings: a linear threshold and times in seconds. */
interface StripSettings {
  threshold: number;
  min_time: number;
  pre_attack: number;
  post_release: number;
}

/** Logic's defaults: 4 %, 0.1 s, 0.02 s and 0.1 s. */
const DEFAULTS: StripSettings = { threshold: 0.04, min_time: 0.1, pre_attack: 0.02, post_release: 0.1 };
const PREFS = "fermata.stripSilence";

function loadSettings(): StripSettings {
  try {
    return { ...DEFAULTS, ...JSON.parse(localStorage.getItem(PREFS) ?? "{}") };
  } catch {
    return DEFAULTS;
  }
}

function saveSettings(s: StripSettings) {
  try {
    localStorage.setItem(PREFS, JSON.stringify(s));
  } catch {
    // Storage blocked: the settings just aren't remembered.
  }
}

let current: Id[] | null = null;
const subs = new Set<() => void>();
const emit = () => subs.forEach((f) => f());

/** Opens the sheet for the given audio regions. */
export function openStripSilence(ids: Id[]) {
  current = ids;
  emit();
}

function close() {
  current = null;
  emit();
}

export function StripSilenceHost() {
  const ids = useSyncExternalStore(
    (f) => {
      subs.add(f);
      return () => subs.delete(f);
    },
    () => current,
  );
  return ids ? <StripSilenceSheet ids={ids} /> : null;
}

const toDb = (v: number) => 20 * Math.log10(Math.max(v, 1e-6));
const pct = (v: number) => (v * 100 >= 10 ? (v * 100).toFixed(0) : (v * 100).toFixed(1));

function StripSilenceSheet({ ids }: { ids: Id[] }) {
  const [settings, setSettings] = useState<StripSettings>(loadSettings);
  const [parts, setParts] = useState<[number, number][]>([]);
  const [busy, setBusy] = useState(false);
  const region = useStore((s) => s.snap?.project.tracks.flatMap((t) => t.regions).find((r) => r.id === ids[0])) as Region | undefined;
  const set = (p: Partial<StripSettings>) => {
    const next = { ...settings, ...p };
    setSettings(next);
    saveSettings(next);
  };

  // The preview, a moment after the settings settle.
  useEffect(() => {
    if (!region) return;
    const t = window.setTimeout(() => {
      call<[number, number][]>("strip_silence_preview", { id: region.id, settings }).then((p) => p && setParts(p));
    }, 80);
    return () => window.clearTimeout(t);
  }, [settings, region?.id, region?.start, region?.length]);

  const apply = async () => {
    setBusy(true);
    const made = await actMade("strip_silence", { ids, settings });
    setBusy(false);
    if (!made) return;
    setState({ selectedRegions: made, focus: "regions" });
    close();
  };

  const others = ids.length - 1;
  return (
    <Sheet title="Strip Silence" onClose={close} width={520}>
      {region && <Preview region={region} parts={parts} threshold={settings.threshold} />}
      <div className="strip-silence-summary dim">
        {region ? `“${region.name}” becomes ${parts.length} region${parts.length === 1 ? "" : "s"}` : "The region is gone"}
        {others > 0 ? `, and ${others} more selected region${others === 1 ? "" : "s"} are cut the same way` : ""}
      </div>
      <div className="settings-grid strip-silence-grid">
        <label>Threshold:</label>
        <Slider value={Math.round(toDb(settings.threshold) * 2) / 2} min={-60} max={0} step={0.5} width={220} onChange={(db) => set({ threshold: 10 ** (db / 20) })} format={(db) => `${pct(10 ** (db / 20))} % (${db.toFixed(1)} dB)`} />
        <label>Minimum Time:</label>
        <div className="row-inline">
          <NumberField value={settings.min_time} min={0} max={10} step={0.01} onChange={(v) => set({ min_time: v })} />
          <span className="dim">s of silence before it counts</span>
        </div>
        <label>Pre-Attack Time:</label>
        <div className="row-inline">
          <NumberField value={settings.pre_attack} min={0} max={2} step={0.01} onChange={(v) => set({ pre_attack: v })} />
          <span className="dim">s</span>
        </div>
        <label>Post-Release Time:</label>
        <div className="row-inline">
          <NumberField value={settings.post_release} min={0} max={5} step={0.01} onChange={(v) => set({ post_release: v })} />
          <span className="dim">s</span>
        </div>
      </div>
      <div className="sheet-foot">
        <button className="push" onClick={() => set(DEFAULTS)}>
          Defaults
        </button>
        <div className="sheet-buttons">
          <button className="push" onClick={close}>
            Cancel
          </button>
          <button className="push default" disabled={busy || !getState().snap} onClick={apply}>
            OK
          </button>
        </div>
      </div>
    </Sheet>
  );
}

const PREVIEW_H = 96;

/** The region's waveform with the parts Strip Silence keeps lit and the rest dimmed, and the
 *  threshold drawn as lines. */
function Preview({ region, parts, threshold }: { region: Region; parts: [number, number][]; threshold: number }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const themeId = useStore((s) => s.themeId);
  const [, setLoaded] = useState(0);
  const content = region.content.type === "audio" ? region.content : null;

  const draw = useCallback(() => {
    const c = ref.current;
    if (!c || !content) return;
    const w = c.clientWidth;
    const dpr = window.devicePixelRatio || 1;
    c.width = Math.round(w * dpr);
    c.height = Math.round(PREVIEW_H * dpr);
    const ctx = c.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, PREVIEW_H);
    const css = getComputedStyle(document.documentElement);
    const v = (name: string, fallback: string) => css.getPropertyValue(name).trim() || fallback;
    const accent = v("--accent", "#4c9eff");
    const pps = w / Math.max(1, region.length);
    const gain = 10 ** (content.gain_db / 20);
    // The parts kept, lit behind the waveform.
    ctx.fillStyle = accent;
    ctx.globalAlpha = 0.22;
    for (const [a, b] of parts) ctx.fillRect((a - region.start) * pps, 0, Math.max(1, (b - a) * pps), PREVIEW_H);
    ctx.globalAlpha = 1;
    const wave = waveCache.get(content.clip);
    if (wave) drawWave(ctx, wave, content.offset, 0, w, 0, PREVIEW_H, pps, v("--text", "#ddd"), 0, w, undefined, gain);
    // The threshold, above and below the centre line.
    const half = PREVIEW_H / 2 - 1;
    const y = Math.min(half, threshold * half);
    ctx.strokeStyle = accent;
    ctx.setLineDash([4, 3]);
    ctx.beginPath();
    ctx.moveTo(0, PREVIEW_H / 2 - y + 0.5);
    ctx.lineTo(w, PREVIEW_H / 2 - y + 0.5);
    ctx.moveTo(0, PREVIEW_H / 2 + y - 0.5);
    ctx.lineTo(w, PREVIEW_H / 2 + y - 0.5);
    ctx.stroke();
    ctx.setLineDash([]);
    // The cuts.
    ctx.fillStyle = accent;
    for (const [a, b] of parts) {
      ctx.fillRect(Math.round((a - region.start) * pps), 0, 1, PREVIEW_H);
      ctx.fillRect(Math.round((b - region.start) * pps) - 1, 0, 1, PREVIEW_H);
    }
  }, [region, parts, threshold, content, themeId]);

  useEffect(() => {
    if (content) ensureWave(content.clip, () => setLoaded((n) => n + 1));
  }, [content?.clip]);
  useLayoutEffect(draw);

  return <canvas ref={ref} className="strip-silence-preview" style={{ height: PREVIEW_H }} />;
}
