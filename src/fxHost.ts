// Hosting built-in effects in the UI: the registry, their panel windows, the plug-in and Channel
// EQ clipboard, and the slot right-click menu.
import { act, BUILTIN_SYNTH, call, EQ_SLOT, INSTRUMENT_SLOT, MASTER_ID, type ChannelEq, type Id, type PluginSlot, type Project, type Track } from "./api";
import type { FxInfo, InstInfo } from "./fx";
import type { MenuItem } from "./components/Menu";
import { getState, setState } from "./store";
import { closePanel, fxKey, openPanel, togglePanel } from "./fxDockState";
import { sideChainMenu } from "./sideChain";

/** The preset id of the Channel EQ. */
export const CHANNEL_EQ = "builtin:channel-eq";

export async function loadFxList() {
  const [list, instruments] = await Promise.all([call<FxInfo[]>("list_builtin_effects"), call<InstInfo[]>("list_builtin_instruments")]);
  if (list) setState({ fxList: list });
  if (instruments) setState({ instList: instruments });
}

/** A built-in effect's or instrument's registry entry. */
export function fxInfo(pluginId: string | undefined): FxInfo | undefined {
  if (!pluginId) return undefined;
  const st = getState();
  return st.fxList.find((f) => f.id === pluginId) ?? st.instList.find((f) => f.id === pluginId);
}

/** Built-in effects and instruments with settings have a panel; Fermata Synth has none. */
export const isBuiltinFx = (pluginId: string | undefined) => !!fxInfo(pluginId)?.params.length;

/** Where a built-in instance sits: its strip (MASTER_ID for Stereo Out) and slot. */
export function findFxSlot(project: Project | undefined, uid: Id): { track: Id; slot: number; plugin: PluginSlot } | null {
  if (!project) return null;
  for (const t of project.tracks) {
    if (t.instrument?.uid === uid) return { track: t.id, slot: INSTRUMENT_SLOT, plugin: t.instrument };
    const i = t.inserts.findIndex((s) => s?.uid === uid);
    if (i >= 0) return { track: t.id, slot: i, plugin: t.inserts[i]! };
  }
  const i = project.master.inserts.findIndex((s) => s?.uid === uid);
  return i >= 0 ? { track: MASTER_ID, slot: i, plugin: project.master.inserts[i]! } : null;
}

/** An instrument's panel opens in the bottom section whatever Settings say for effects: its
 *  sections run in one row, across the width. Its panel can still be floated from there. */
const panelModeFor = (uid: Id) => (getState().snap?.project.tracks.some((t) => t.instrument?.uid === uid) ? "docked" : undefined);

/** Opens an effect's panel (or brings it forward), docked or floating as Settings say (fxDock.ts). */
export function openFxPanel(uid: Id) {
  openPanel(fxKey(uid), panelModeFor(uid));
}

export function closeFxPanel(uid: Id) {
  closePanel(fxKey(uid));
}

/** A slot click: closes the panel if it is on screen, otherwise opens or shows it. */
export function toggleFxPanel(uid: Id) {
  togglePanel(fxKey(uid), panelModeFor(uid));
}

/** Loads a plug-in into a slot; a built-in effect then opens its panel (VST3s their editor). */
export async function loadPlugin(track: Id, slot: number, pluginId: string, open = true) {
  const snap = await act("set_plugin", { track, slot, pluginId, open });
  if (!snap || !open || !isBuiltinFx(pluginId)) return;
  const strip = track === MASTER_ID ? snap.project.master : snap.project.tracks.find((t) => t.id === track);
  const uid = slot === INSTRUMENT_SLOT ? (strip as Track | undefined)?.instrument?.uid : strip?.inserts[slot]?.uid;
  if (uid !== undefined) openFxPanel(uid);
}

// ---- clipboard --------------------------------------------------------------------------------
// In the store, shared by every window, and outside the project, so it survives opening
// another.

export const pluginClipboard = () => getState().pluginClip;
export const eqClipboard = () => getState().eqClip;

export async function copyPlugin(track: Id, slot: number) {
  const s = await call<PluginSlot>("copy_plugin", { track, slot });
  if (s) setState({ pluginClip: s });
}

export function pastePlugin(tracks: Id[], slot: number) {
  const plugin = pluginClipboard();
  if (plugin) return act("paste_plugin", { tracks, slot, plugin });
}

export function copyEq(eq: ChannelEq) {
  setState({ eqClip: structuredClone(eq) });
}

export function pasteEq(tracks: Id[]) {
  const eq = eqClipboard();
  if (eq) return act("paste_eq", { tracks, eq });
}

/** The tracks a paste goes to: every selected track when `track` is among several selected. */
function pasteTargets(track: Id): Id[] {
  const sel = getState().selectedTracks;
  return sel.length > 1 && sel.includes(track) ? sel : [track];
}

/**
 * The Side Chain item of a VST3 insert that has a side-chain input, then a separator: the editor is
 * a native window with no room for it, so it lives in the slot's menus. Nothing for other slots.
 */
export function slotSideChainItems(track: Id, slot: number, plugin: PluginSlot | null): MenuItem[] {
  const snap = getState().snap;
  if (!snap || !plugin || !snap.key_inputs?.includes(plugin.uid)) return [];
  const set = (source: Id | null) => act("set_sidechain", { track, slot, source });
  const setPre = (pre: boolean) => act("set_sidechain_pre", { track, slot, pre });
  return [sideChainMenu(snap.project.tracks, track, plugin, set, setPre), { separator: true }];
}

/** The right-click menu of an insert or instrument slot. */
export function slotMenu(track: Id, slot: number, plugin: PluginSlot | null): MenuItem[] {
  const instrument = slot === INSTRUMENT_SLOT;
  const clip = pluginClipboard();
  // An instrument only pastes into an instrument slot, an effect only into an Audio FX slot.
  // Like Session::find_plugin: a plug-in found at another path matches by its class id.
  const cid = (id: string) => id.slice(id.lastIndexOf("|") + 1).toLowerCase();
  const isInstrument = (id: string) => {
    const { plugins, instList } = getState();
    if (id === BUILTIN_SYNTH || instList.some((i) => i.id === id)) return true;
    const desc = plugins.find((p) => p.id === id) ?? (id.includes("|") ? plugins.find((p) => p.id.includes("|") && cid(p.id) === cid(id)) : undefined);
    return !!desc?.is_instrument;
  };
  const fits = !!clip && isInstrument(clip.plugin_id) === instrument;
  const targets = pasteTargets(track);
  const items: MenuItem[] = [
    ...slotSideChainItems(track, slot, plugin),
    { label: "Copy Plug-in", disabled: !plugin, onClick: () => copyPlugin(track, slot) },
    { label: clip ? `Paste ${clip.name}` : "Paste Plug-in", disabled: !fits, onClick: () => pastePlugin([track], slot) },
  ];
  if (targets.length > 1) items.push({ label: "Paste to Selected Tracks", disabled: !fits, onClick: () => pastePlugin(targets, slot) });
  return items;
}

/** The Channel EQ thumbnail's right-click menu. */
export function eqMenu(track: Id, eq: ChannelEq | undefined): MenuItem[] {
  const targets = pasteTargets(track);
  const eqClip = eqClipboard();
  const items: MenuItem[] = [
    { label: "Copy Channel EQ", disabled: !eq, onClick: () => eq && copyEq(eq) },
    { label: "Paste Channel EQ", disabled: !eqClip, onClick: () => pasteEq([track]) },
  ];
  if (targets.length > 1) items.push({ label: "Paste Channel EQ to Selected Tracks", disabled: !eqClip, onClick: () => pasteEq(targets) });
  return items;
}

export { EQ_SLOT };
