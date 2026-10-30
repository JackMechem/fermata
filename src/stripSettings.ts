// Channel strip settings, as in Logic's channel strip Setting menu: copy and paste a strip's
// instrument, Audio FX and Channel EQ, reset a strip, and save settings to load in any project
// (stored by the engine, see crates/engine/src/strip_settings.rs). Also resetting the clip
// indicators, which sit on the same strips.
import { invoke } from "@tauri-apps/api/core";
import { ask } from "@tauri-apps/plugin-dialog";
import { act, call, MASTER_ID, type ChannelEq, type Id, type PluginSlot } from "./api";
import type { MenuItem } from "./components/Menu";
import { getState, setState } from "./store";

/** A channel strip setting as the engine hands it over (`StripSetting`). */
export interface StripSetting {
  name: string;
  version: number;
  instrument?: PluginSlot | null;
  inserts: (PluginSlot | null)[];
  eq?: ChannelEq | null;
}

// Kept here rather than in the store's project state, so it survives opening another project
// (like the plug-in clipboard in fxHost.ts).
let stripClip: StripSetting | null = null;

/** Resets the clip indicators of these strips (MASTER_ID for Stereo Out), or of all (null). */
export function resetClipIndicators(tracks: Id[] | null) {
  call("reset_clip_indicators", { tracks });
}

/** The strips a Setting menu command applies to: every selected track when this strip is one
 *  of several selected, as Logic applies it to the selected channel strips. */
function targets(track: Id): Id[] {
  const sel = getState().selectedTracks;
  return track !== MASTER_ID && sel.length > 1 && sel.includes(track) ? sel : [track];
}

export async function copyStrip(track: Id) {
  const s = await call<StripSetting>("copy_strip", { track });
  if (s) stripClip = s;
}

export function pasteStrip(tracks: Id[]) {
  if (stripClip) return act("paste_strip", { tracks, setting: stripClip });
}

function toast(text: string) {
  setState({ toast: { text, at: Date.now() } });
}

/** Save Channel Strip Setting As…: asks for a name, and before replacing a saved one. */
async function saveStrip(track: Id, suggested: string) {
  const name = window.prompt("Save the channel strip setting as:", suggested);
  if (name === null) return;
  let overwrite = false;
  for (;;) {
    try {
      const saved = await invoke<string>("save_strip_setting", { track, name, overwrite });
      toast(`Saved the channel strip setting “${saved}”`);
      return;
    } catch (e) {
      const msg = typeof e === "string" ? e : String(e);
      if (overwrite || !/already exists/.test(msg)) {
        toast(msg);
        return;
      }
      const ok = await ask(`A channel strip setting named “${name.trim()}” already exists. Replace it?`, { title: "Save Channel Strip Setting", kind: "warning", okLabel: "Replace" });
      if (!ok) return;
      overwrite = true;
    }
  }
}

async function deleteStrip(name: string) {
  const ok = await ask(`Delete the channel strip setting “${name}”? It is removed for every project.`, { title: "Delete Channel Strip Setting", kind: "warning", okLabel: "Delete" });
  if (ok) await call("delete_strip_setting", { name });
}

/**
 * The Setting menu of a channel strip (`track` is MASTER_ID for Stereo Out): copy, paste and
 * reset, saving, and the saved settings to load. Paste, Reset and loading apply to every
 * selected strip when this one is among them.
 */
export async function stripSettingMenu(track: Id, name: string): Promise<MenuItem[]> {
  const saved = (await call<string[]>("list_strip_settings")) ?? [];
  const to = targets(track);
  const many = to.length > 1 ? ` (${to.length} Strips)` : "";
  const items: MenuItem[] = [
    { label: "Copy Channel Strip Settings", onClick: () => copyStrip(track) },
    {
      label: `Paste Channel Strip Settings${many}`,
      hint: stripClip ? stripClip.name : undefined,
      disabled: !stripClip,
      onClick: () => pasteStrip(to),
    },
    { separator: true },
    { label: `Reset Channel Strip${many}`, onClick: () => act("reset_strip", { tracks: to }) },
    { separator: true },
    { label: "Save Channel Strip Setting As…", onClick: () => saveStrip(track, name) },
    { label: "Delete Channel Strip Setting", disabled: !saved.length, children: saved.map((n) => ({ label: n, onClick: () => deleteStrip(n) })) },
    { separator: true },
    { label: "Channel Strip Settings", header: true },
  ];
  if (!saved.length) items.push({ label: "No Saved Settings", disabled: true });
  for (const n of saved) items.push({ label: n, onClick: () => act("load_strip_setting", { tracks: to, name: n }) });
  return items;
}
