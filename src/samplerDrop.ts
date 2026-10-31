// Files going into Fermata Sampler: from its panel's Add… button, or dropped on it from the desktop
// or the All Files browser (src/dnd.ts asks `samplerAt` first).
import { call, type Id, type Snapshot } from "./api";
import { isSfzPath, keyAt } from "./samplerMap";
import { setState } from "./store";

/** The sampler panel under a point, and the key if it's over the map. */
export function samplerAt(cx: number, cy: number): { track: Id; key: number | null } | null {
  const el = document.elementFromPoint(cx, cy);
  const panel = el?.closest<HTMLElement>("[data-sampler-track]");
  if (!panel) return null;
  const map = el?.closest<HTMLElement>("[data-sampler-map]");
  let key: number | null = null;
  if (map) {
    const r = map.getBoundingClientRect();
    key = keyAt({ width: r.width, height: r.height, strip: 0 }, cx - r.left);
  }
  return { track: Number(panel.dataset.samplerTrack), key };
}

/** Marks the panel under a drag. */
export function hoverSampler(cx: number | null, cy: number | null) {
  document.querySelectorAll(".smp-panel.smp-drop").forEach((e) => e.classList.remove("smp-drop"));
  if (cx === null || cy === null) return;
  document.elementFromPoint(cx, cy)?.closest(".smp-panel")?.classList.add("smp-drop");
}

interface Added {
  zones: number[];
  errors: string[];
}

interface SfzReport {
  zones: number;
  ignored: [string, number][];
  left_out: string[];
  missing: string[];
}

const toast = (text: string, ms: number) => setState({ toast: { text, at: Date.now(), ms } });

/**
 * Takes files: an SFZ replaces the zones, audio is added, mapped by the note in its name or
 * chromatically (see `mapping.rs`). Dropped on the map, one file sits on that key and several go
 * up from it; elsewhere one file takes the whole keyboard. Returns what to say about an SFZ.
 */
export async function dropOnSampler(track: Id, paths: string[], key: number | null): Promise<string | null> {
  const sfz = paths.find(isSfzPath);
  if (sfz) {
    toast("Loading the SFZ file…", 60000);
    const r = await call<{ snapshot: Snapshot; result: SfzReport }>("sampler_load_sfz", { track, path: sfz });
    if (!r) return null;
    setState({ snap: r.snapshot });
    const s = r.result;
    const parts = [`${s.zones} zones from the SFZ`];
    if (s.missing.length) parts.push(`${s.missing.length} samples not found: ${s.missing.slice(0, 3).join(", ")}`);
    if (s.left_out.length) parts.push(`left out: ${s.left_out.slice(0, 3).join(", ")}`);
    if (s.ignored.length) parts.push(`ignored: ${s.ignored.map(([k, n]) => `${k} ×${n}`).join(", ")}`);
    const text = parts.join(". ");
    toast(parts[0], 3000);
    return text;
  }
  toast(paths.length > 1 ? `Loading ${paths.length} files…` : "Loading…", 60000);
  const r = await call<{ snapshot: Snapshot; result: Added }>("sampler_add_files", { track, paths, atKey: key });
  if (!r) return null;
  setState({ snap: r.snapshot });
  const { zones, errors } = r.result;
  toast(errors.length ? `Added ${zones.length}. ${errors.join(" · ")}` : `Added ${zones.length} zone${zones.length === 1 ? "" : "s"}`, errors.length ? 8000 : 2000);
  return null;
}
