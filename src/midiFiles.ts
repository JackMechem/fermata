// Standard MIDI File import and export: the File menu, the region and track menus, and files
// dropped onto the tracks area (see dnd.ts).
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { call, type Id, type Made } from "./api";
import { currentTick, getState, setState } from "./store";
import { samplesPerBar } from "./time";

const MIDI_EXT = /\.(mid|midi|smf|kar)$/i;

export function isMidiPath(p: string): boolean {
  return MIDI_EXT.test(p);
}

const toast = (text: string, ms = 3000) => setState({ toast: { text, at: Date.now(), ms } });

/**
 * Imports MIDI files: the first onto `track` (an instrument track) or new tracks inserted at
 * `index`, the rest on new tracks after it. Each file's start lands at `start`.
 */
export async function importMidiFiles(paths: string[], track: Id | null, start: number, index: number | null) {
  const made: Id[] = [];
  const errors: string[] = [];
  let at = index;
  for (const [i, path] of paths.entries()) {
    const res = await call<Made>("import_midi", { path, track: i === 0 ? track : null, start: Math.max(0, Math.round(start)), index: i === 0 && track !== null ? null : at });
    if (!res) {
      errors.push(path.split(/[\\/]/).pop() ?? path);
      continue;
    }
    setState({ snap: res.snapshot });
    made.push(...res.ids);
    // Further files go below the tracks this one created.
    const tracks = res.snapshot.project.tracks;
    const last = Math.max(...res.ids.map((id) => tracks.findIndex((t) => t.regions.some((r) => r.id === id))));
    if (last >= 0) at = last + 1;
  }
  if (made.length) setState({ selectedRegions: made, focus: "regions" });
  if (errors.length) toast(`Couldn't import ${errors.join(", ")}`, 6000);
  else if (made.length) toast(paths.length > 1 ? `Imported ${paths.length} MIDI files` : "Imported MIDI file", 2000);
}

/** File › Import MIDI File…: onto the selected instrument track at the playhead's bar. */
export async function importMidiDialog() {
  const file = await open({ title: "Import MIDI File", multiple: true, filters: [{ name: "MIDI Files", extensions: ["mid", "midi", "smf", "kar"] }] });
  const paths = typeof file === "string" ? [file] : file ?? [];
  if (!paths.length) return;
  const snap = getState().snap;
  if (!snap) return;
  const p = snap.project;
  const bar = samplesPerBar(p);
  const start = Math.floor((currentTick()?.pos ?? 0) / bar) * bar;
  const sel = p.tracks.findIndex((t) => t.id === snap.selected);
  const t = sel >= 0 ? p.tracks[sel] : null;
  if (t?.kind === "instrument") await importMidiFiles(paths, t.id, start, null);
  else await importMidiFiles(paths, null, start, sel >= 0 ? sel + 1 : null);
}

/** Asks where to save, then writes the MIDI regions `ids` as a Standard MIDI File. */
export async function exportMidi(ids: Id[], name: string) {
  if (!ids.length) {
    toast("Select MIDI regions to export");
    return;
  }
  const dir = getState().snap?.path.replace(/[\\/][^\\/]*$/, "") ?? "";
  const clean = name.replace(/[\\/:*?"<>|]/g, "_") || "Untitled";
  let path = await save({ title: "Export MIDI File", defaultPath: dir ? `${dir}/${clean}.mid` : `${clean}.mid`, filters: [{ name: "MIDI File", extensions: ["mid"] }] });
  if (!path) return;
  if (!isMidiPath(path)) path += ".mid";
  try {
    await invoke("export_midi", { ids, path });
    toast(`Exported ${path.split(/[\\/]/).pop()}`, 2500);
  } catch (e) {
    toast(String(e), 6000);
  }
}

/** File › Export Selection as MIDI File…: the selected MIDI regions, or the selected track. */
export function exportSelectionMidi() {
  const s = getState();
  const p = s.snap?.project;
  if (!p) return;
  const midi = p.tracks.flatMap((t) => t.regions.filter((r) => r.content.type === "midi" && s.selectedRegions.includes(r.id)));
  if (midi.length) {
    exportMidi(midi.map((r) => r.id), midi.length === 1 ? midi[0].name : p.name);
    return;
  }
  const t = p.tracks.find((x) => x.id === s.snap?.selected);
  if (t?.kind === "instrument") exportTrackMidi(t.id);
  else toast("Select MIDI regions or an instrument track to export");
}

/** Exports every unmuted MIDI region on a track. */
export function exportTrackMidi(trackId: Id) {
  const t = getState().snap?.project.tracks.find((x) => x.id === trackId);
  if (!t) return;
  exportMidi(t.regions.filter((r) => r.content.type === "midi" && !r.muted).map((r) => r.id), t.name);
}
