// Track and region operations, shared by the right-click menus, the local Edit menu, the native
// menu bar and the keyboard, plus the context menus themselves.
import { open } from "@tauri-apps/plugin-dialog";
import { act, actMade, api, call, canRecord, isStack, type Group, type Id, type Region, type Snapshot, type Track } from "./api";
import { newAux } from "./routing";
import type { MenuItem } from "./components/Menu";
import { currentLayout, setTrackHeight, stepVisible } from "./layout";
import { isLooped, regionEnd } from "./loops";
import { BOUNCE_IN_PLACE_HINT, openBounce, openBounceInPlace } from "./bounce";
import { fmt, keyHint } from "./keymap";
import { exportMidi, exportTrackMidi, importMidiFiles } from "./midiFiles";
import { canFreeze, freezeTracks, unfreezeTracks } from "./freeze";
import { currentTick, getState, setState } from "./store";
import { tempoMap } from "./tempo";
import { TRACK_COLORS } from "./view";
import { isFolder, packTakeFolder, takeFolderMenu } from "./takes";
import { followingRegion } from "./fades";
import { canHaveAlternatives, trackAltMenu } from "./trackAlternatives";
import { isDefault, paramsOf } from "./regionParams";

const project = () => getState().snap?.project;
const tracks = (): Track[] => project()?.tracks ?? [];
const playhead = () => currentTick()?.pos ?? 0;
const toast = (text: string, ms = 2500) => setState({ toast: { text, at: Date.now(), ms } });

export function trackOf(regionId: Id): Track | undefined {
  return tracks().find((t) => t.regions.some((r) => r.id === regionId));
}

function regionById(id: Id): Region | undefined {
  for (const t of tracks()) for (const r of t.regions) if (r.id === id) return r;
  return undefined;
}

const inOrder = (ids: Id[]) => tracks().map((t) => t.id).filter((id) => ids.includes(id));

// ---- track selection ------------------------------------------------------------------------

/** The selected tracks, in track order. Falls back to the inspector's track. */
export function selectedTracks(): Id[] {
  const s = getState();
  const cur = s.snap?.selected ?? null;
  if (cur !== null && s.selectedTracks.includes(cur)) return inOrder(s.selectedTracks);
  return cur !== null ? [cur] : [];
}

/** Adds the members of groups that link selection. */
function withGroups(ids: Id[]): Id[] {
  const p = project();
  if (!p) return ids;
  const out = new Set(ids);
  for (const id of ids) {
    const t = p.tracks.find((x) => x.id === id);
    const g = t?.group != null ? p.groups.find((x) => x.id === t.group) : undefined;
    if (g?.enabled && g.select) p.tracks.filter((x) => x.group === g.id).forEach((x) => out.add(x.id));
  }
  return inOrder([...out]);
}

/** A click on a track header: plain selects, ⌘/Ctrl toggles, Shift extends. */
export function clickTrack(id: Id, e: { shiftKey: boolean; ctrlKey: boolean; metaKey: boolean }) {
  const s = getState();
  const all = tracks().map((t) => t.id);
  let sel: Id[];
  const cur = s.snap?.selected ?? null;
  if (e.shiftKey && cur !== null) {
    const [a, b] = [all.indexOf(cur), all.indexOf(id)].sort((x, y) => x - y);
    // Tracks out of sight (hidden, or in a collapsed stack) stay out of the range.
    const heights = currentLayout().heights;
    sel = all.slice(a, b + 1).filter((x, k) => heights[a + k] > 0 || x === id || x === cur);
  } else if (e.ctrlKey || e.metaKey) {
    const base = selectedTracks();
    sel = base.includes(id) ? base.filter((x) => x !== id) : [...base, id];
    if (!sel.length) sel = [id];
  } else {
    sel = selectedTracks().includes(id) && selectedTracks().length > 1 ? selectedTracks() : [id];
  }
  setState({ selectedTracks: withGroups(sel), focus: "tracks" });
  if (cur !== id) api.selectTrack(id);
}

// ---- track operations ---------------------------------------------------------------------

async function selectMadeTracks(ids: Id[] | null) {
  if (ids?.length) setState({ selectedTracks: ids, focus: "tracks" });
}

export const duplicateTracks = (content = true) => actMade("duplicate_tracks", { ids: selectedTracks(), content }).then(selectMadeTracks);

export function deleteTracks(ids = selectedTracks()) {
  if (ids.length) act("delete_tracks", { ids }).then(() => setState({ selectedTracks: [] }));
}

export async function deleteUnusedTracks() {
  const n = await actMade("delete_unused_tracks");
  toast(n?.[0] ? `Deleted ${n[0]} unused track${n[0] > 1 ? "s" : ""}` : "No unused tracks");
}

export async function createStack(summing: boolean, ids = selectedTracks()) {
  const made = await actMade("create_stack", { ids, summing });
  if (made) setState({ selectedTracks: made });
}

export const flattenStack = (id: Id) => act("flatten_stack", { id });

/** Moves tracks out of their stack, one level up. */
export function removeFromStack(ids = selectedTracks()) {
  const p = project();
  if (!p) return;
  const parents = ids.map((id) => {
    const t = p.tracks.find((x) => x.id === id);
    const up = t?.parent != null ? p.tracks.find((x) => x.id === t.parent)?.parent ?? null : null;
    return [id, up] as [Id, Id | null];
  });
  act("move_tracks", { ids: p.tracks.map((t) => t.id), parents });
}

/** Track › Bounce in Place: the selected regions when they were last worked on (like ⌘D),
 *  otherwise the selected tracks. */
export function bounceInPlace(ids?: Id[]) {
  const s = getState();
  if (!ids && s.focus === "regions" && s.selectedRegions.length) openBounceInPlace([], s.selectedRegions);
  else openBounceInPlace(ids ?? selectedTracks());
}

export const toggleCollapsed = (t: Track) => api.updateTrack(t.id, { collapsed: !t.collapsed });

export const setHidden = (ids: Id[], hidden: boolean) => act("set_tracks_hidden", { ids, hidden });

/** ⌃H, as Logic's Hide Current Track and Select Next Track: hides the selected tracks (shows
 *  them again when they're all hidden already) and moves the selection to the next track left
 *  in view. */
export async function hideTracks(ids = selectedTracks()) {
  if (!ids.length) return;
  const hide = !tracks().filter((t) => ids.includes(t.id)).every((t) => t.hidden);
  if (!(await setHidden(ids, hide)) || !hide || getState().showHidden) return;
  const all = tracks();
  const l = currentLayout();
  const last = Math.max(...ids.map((id) => all.findIndex((t) => t.id === id)));
  let next = stepVisible(l, last, 1);
  if (l.heights[next] === 0) next = stepVisible(l, last, -1);
  const t = l.heights[next] > 0 ? all[next] : undefined;
  setState({ selectedTracks: t ? [t.id] : [] });
  if (t) api.selectTrack(t.id);
}

export function unhideAllTracks() {
  const ids = tracks().filter((t) => t.hidden).map((t) => t.id);
  if (ids.length) setHidden(ids, false);
}

/** H: Logic's hide mode, which shows hidden tracks (dimmed) and an H button on every header. */
export const toggleShowHidden = () => setState({ showHidden: !getState().showHidden });

export async function groupTracks(ids = selectedTracks()) {
  if (!ids.length) return;
  const made = await actMade("create_group", { ids });
  if (made?.[0] !== undefined) setState({ groupSheet: made[0] });
}

export const setGroup = (ids: Id[], group: Id | null) => act("set_track_group", { ids, group });
export const updateGroup = (group: Group) => act("update_group", { group });

/** Applies a switch to every selected track, the way the clicked track goes. */
function switchAll(field: "mute" | "solo" | "armed", ids = selectedTracks()) {
  const sel = tracks().filter((t) => ids.includes(t.id));
  const on = !sel.every((t) => t[field]);
  sel.forEach((t) => api.updateTrack(t.id, { [field]: on }));
}

// ---- region operations --------------------------------------------------------------------

export const selectedRegions = () => getState().selectedRegions;

/** A region plus, when its track's group links editing, the regions at the same place on the
 *  group's other tracks. */
export function linkedRegions(id: Id): Id[] {
  const p = project();
  const t = trackOf(id);
  const r = t?.regions.find((x) => x.id === id);
  const g = t?.group != null ? p?.groups.find((x) => x.id === t.group) : undefined;
  if (!p || !r || !g?.enabled || !g.editing) return [id];
  const out = [id];
  for (const o of p.tracks) {
    if (o.id === t!.id || o.group !== g.id) continue;
    for (const x of o.regions) if (x.start < regionEnd(r) && regionEnd(x) > r.start) out.push(x.id);
  }
  return out;
}

function select(ids: Id[] | null) {
  if (ids) setState({ selectedRegions: ids, focus: "regions" });
}

export function copyRegions(ids = selectedRegions()) {
  const all = tracks();
  const clip = ids.flatMap((id) => {
    const index = all.findIndex((t) => t.regions.some((r) => r.id === id));
    const region = all[index]?.regions.find((r) => r.id === id);
    return region ? [{ index, region }] : [];
  });
  if (clip.length) setState({ clipboard: clip, clipboardStart: null });
  return clip.length;
}

export function cutRegions(ids = selectedRegions()) {
  if (copyRegions(ids)) act("delete_regions", { ids }).then(() => setState({ selectedRegions: [] }));
}

/** Pastes at `at` (a track and position), or at the playhead on the selected track. The copied
 *  regions keep their spacing in time and across tracks. */
export async function pasteRegions(at?: { track: Id; pos: number }) {
  const s = getState();
  const clip = s.clipboard;
  const all = tracks();
  if (!clip.length || !all.length) return;
  const targetId = at?.track ?? s.snap?.selected ?? all[0].id;
  const target = Math.max(0, all.findIndex((t) => t.id === targetId));
  const top = Math.min(...clip.map((c) => c.index));
  const earliest = s.clipboardStart ?? Math.min(...clip.map((c) => c.region.start));
  const items = clip.flatMap(({ index, region }) => {
    const t = all[target + index - top];
    return t ? [[t.id, region] as [Id, Region]] : [];
  });
  const made = await actMade("paste_regions", { items, offset: (at?.pos ?? playhead()) - earliest, label: "Paste" });
  if (made && made.length < clip.length) toast("Some regions didn't fit the track they'd land on");
  select(made);
}

export const repeatRegions = (times = 1, ids = selectedRegions()) => ids.length && actMade("repeat_regions", { ids, times }).then(select);

export const joinRegions = (ids = selectedRegions()) => actMade("join_regions", { ids }).then(select);

export const splitAtPlayhead = (ids = selectedRegions()) => act("split_regions", { pos: playhead(), ids });

export function deleteRegions(ids = selectedRegions()) {
  if (ids.length) act("delete_regions", { ids }).then(() => setState({ selectedRegions: [] }));
}

export function toggleRegionMute(ids = selectedRegions()) {
  const regs = ids.map(regionById).filter((r): r is Region => !!r);
  const muted = !regs.every((r) => r.muted);
  regs.forEach((r) => act("set_region_muted", { id: r.id, muted }));
}

export function setLocatorsByRegions(ids = selectedRegions()) {
  const regs = ids.map(regionById).filter((r): r is Region => !!r);
  if (!regs.length) return;
  const start = Math.min(...regs.map((r) => r.start));
  const end = Math.max(...regs.map(regionEnd));
  act("set_cycle", { enabled: true, start, end });
}

/** L, Logic's Loop switch: loops each region up to the next region on its track (or four times
 *  its length), or turns looping off when they all loop already. */
export function toggleLoop(ids = selectedRegions()) {
  const regs = ids.map(regionById).filter((r): r is Region => !!r);
  if (regs.length) act("set_regions_looped", { ids: regs.map((r) => r.id), on: !regs.every(isLooped) });
}

/** Turns loops into plain regions, one per repetition, and selects them all. */
export function convertLoops(ids = selectedRegions()) {
  const looped = ids.filter((id) => {
    const r = regionById(id);
    return r && isLooped(r);
  });
  if (looped.length) actMade("convert_loops", { ids: looped }).then((made) => made && select([...new Set([...ids, ...made])]));
}

export function selectOnTrack(trackId: Id, from?: number) {
  const t = tracks().find((x) => x.id === trackId);
  if (t) select(t.regions.filter((r) => from === undefined || r.start >= from).map((r) => r.id));
}

export function selectAllRegions() {
  const showHidden = getState().showHidden;
  select(tracks().flatMap((t) => (t.hidden && !showHidden ? [] : t.regions.map((r) => r.id))));
}

async function createMidiRegion(track: Id, pos: number) {
  const p = project();
  if (!p) return;
  const m = tempoMap(p);
  const bar = m.barAt(pos);
  const start = Math.round(m.barStart(bar));
  select(await actMade("create_midi_region", { track, start, length: Math.round(m.barStart(bar + 1)) - start, key: null }));
}

/** Create Pattern Region: a bar-long pattern region at the bar containing `pos`, selected and
 *  open in the Step Sequencer. */
export async function createPatternRegion(track: Id, pos: number) {
  const p = project();
  if (!p) return;
  const m = tempoMap(p);
  const bar = m.barAt(pos);
  const start = Math.round(m.barStart(bar));
  const made = await actMade("create_pattern_region", { track, start, length: Math.round(m.barStart(bar + 1)) - start });
  if (made) setState({ selectedRegions: made, focus: "regions", bottom: "steps" });
}

/** Track › New Pattern Region: on the selected software instrument track, at the playhead. */
export function newPatternRegion() {
  const t = tracks().find((x) => x.id === getState().snap?.selected);
  if (t?.kind === "instrument") createPatternRegion(t.id, playhead());
  else toast("Select a software instrument track for the pattern region");
}

async function importAt(track: Track, pos: number) {
  const file = await open({ title: "Import Audio File", multiple: true });
  const paths = typeof file === "string" ? [file] : file ?? [];
  if (!paths.length) return;
  const res = await call<{ snapshot: Snapshot; errors: string[] }>("import_audio_files", { paths, track: track.id, start: Math.round(pos), index: null });
  if (res) {
    setState({ snap: res.snapshot });
    if (res.errors.length) toast(res.errors.join(" · "), 6000);
  }
}

async function importMidiAt(track: Track, pos: number) {
  const file = await open({ title: "Import MIDI File", multiple: true, filters: [{ name: "MIDI Files", extensions: ["mid", "midi", "smf", "kar"] }] });
  const paths = typeof file === "string" ? [file] : file ?? [];
  if (paths.length) importMidiFiles(paths, track.id, pos, null);
}

// ---- menus ----------------------------------------------------------------------------------

const ms = (p: { sample_rate: number }, v: number) => Math.round((v / 1000) * p.sample_rate);

export function colorMenu(current: string | null | undefined, set: (c: string | null) => void, defaultLabel?: string): MenuItem[] {
  const items: MenuItem[] = TRACK_COLORS.map((c) => ({ label: c, color: c, checked: c === current, onClick: () => set(c) }));
  if (defaultLabel) return [{ label: defaultLabel, checked: !current, onClick: () => set(null) }, { separator: true }, ...items];
  return items;
}

/** The menu for right-clicking a region (acting on the whole selection). */
export function regionMenu(r: Region, rename: () => void): MenuItem[] {
  const p = project()!;
  const ids = selectedRegions().includes(r.id) ? selectedRegions() : [r.id];
  const regs = ids.map(regionById).filter((x): x is Region => !!x);
  const audio = regs.filter((x) => x.content.type === "audio").map((x) => x.id);
  const midi = regs.filter((x) => x.content.type === "midi").map((x) => x.id);
  // A beat and a bar at the tempo where the (first) region starts.
  const beat = tempoMap(p).samplesPerBeatAt(r.start);
  const bar = tempoMap(p).samplesPerBarAt(r.start);
  const fades = (which: "fade_in" | "fade_out"): MenuItem[] =>
    (
      [
        ["None", 0],
        ["10 ms", ms(p, 10)],
        ["50 ms", ms(p, 50)],
        ["250 ms", ms(p, 250)],
        ["1 Beat", Math.round(beat)],
        ["1 Bar", Math.round(bar)],
      ] as [string, number][]
    ).map(([label, v]) => ({ label, onClick: () => act("set_region_fades", { ids: audio, fadeIn: which === "fade_in" ? v : null, fadeOut: which === "fade_out" ? v : null }) }));
  // Crossfade with the following region (Logic's EqP by default, or the region's own type).
  const meets = regs.filter((x) => x.content.type === "audio" && followingRegion(trackOf(x.id)?.regions ?? [], x)).map((x) => x.id);
  const crossfades = (
    [
      ["None", 0],
      ["10 ms", ms(p, 10)],
      ["20 ms", ms(p, 20)],
      ["50 ms", ms(p, 50)],
      ["250 ms", ms(p, 250)],
      ["1 Beat", Math.round(beat)],
    ] as [string, number][]
  ).map(([label, v]): MenuItem => ({ label, onClick: () => act("crossfade_regions", { ids: meets, length: v }) }));
  const muted = regs.every((x) => x.muted);
  const looped = regs.filter(isLooped);
  const items: MenuItem[] = [
    { label: "Cut", hint: keyHint("cut"), onClick: () => cutRegions(ids) },
    { label: "Copy", hint: keyHint("copy"), onClick: () => copyRegions(ids) },
    { label: "Paste", hint: keyHint("paste"), disabled: !getState().clipboard.length, onClick: () => pasteRegions() },
    { label: "Duplicate", hint: fmt("Mod+D"), onClick: () => repeatRegions(1, ids) },
    {
      label: "Repeat",
      children: [2, 3, 4, 8, 16].map((n) => ({ label: `${n} Times`, onClick: () => repeatRegions(n, ids) })),
    },
    { separator: true },
    { label: "Split at Playhead", hint: fmt("Mod+T"), onClick: () => splitAtPlayhead(ids) },
    { label: "Join", hint: fmt("Mod+J"), disabled: ids.length < 2, onClick: () => joinRegions(ids) },
    { label: "Loop", hint: keyHint("loop"), checked: looped.length === regs.length, onClick: () => toggleLoop(ids) },
    { label: "Convert Loops to Real Copies", disabled: !looped.length, onClick: () => convertLoops(ids) },
    // Writes the Region inspector's playback parameters into the regions (Logic's Normalize).
    { label: "Apply Region Parameters", disabled: regs.every((x) => isDefault(paramsOf(x))), onClick: () => act("apply_region_params", { ids }) },
    { label: muted ? "Unmute" : "Mute", hint: keyHint("mute-regions"), onClick: () => toggleRegionMute(ids) },
    { label: "Rename…", disabled: ids.length > 1, onClick: rename },
    { label: "Color", children: colorMenu(regs.length === 1 ? r.color : undefined, (c) => act("set_region_color", { ids, color: c }), "Track Color") },
  ];
  if (audio.length) {
    items.push(
      { separator: true },
      {
        label: "Gain",
        children: [
          ...([6, 3, 1, -1, -3, -6] as number[]).map((d) => ({ label: `${d > 0 ? "+" : ""}${d} dB`, onClick: () => act("set_region_gain", { ids: audio, delta: d, db: null }) })),
          { separator: true },
          { label: "Reset to 0 dB", onClick: () => act("set_region_gain", { ids: audio, db: 0, delta: null }) },
          { label: "Normalize", onClick: () => act("normalize_regions", { ids: audio }) },
        ],
      },
      { label: "Fade In", children: fades("fade_in") },
      { label: "Fade Out", children: fades("fade_out") },
      { label: "Crossfade", disabled: !meets.length, children: crossfades },
      { label: "Reverse", onClick: () => act("reverse_regions", { ids: audio }) },
    );
    // Take folders: the folder's own menu, and Pack Take Folder for plain regions on one track.
    const packable = audio.length > 1 && new Set(audio.map((id) => trackOf(id)?.id)).size === 1 && regs.every((x) => !x.takes);
    items.push(
      { separator: true },
      ...(isFolder(r) ? [{ label: "Take Folder", children: takeFolderMenu(r) }] : []),
      { label: "Pack Take Folder", disabled: !packable, onClick: () => packTakeFolder(audio) },
    );
  }
  if (midi.length) {
    items.push(
      { separator: true },
      {
        label: "Quantize",
        children: [4, 8, 16, 32].map((d) => ({ label: `1/${d} Note`, onClick: () => act("quantize_regions", { ids: midi, grid: Math.round((p.sample_rate * 60 * 4) / p.tempo / d) }) })),
      },
      {
        label: "Transpose",
        children: (
          [
            ["Octave Up", 12],
            ["Semitone Up", 1],
            ["Semitone Down", -1],
            ["Octave Down", -12],
          ] as [string, number][]
        ).map(([label, n]) => ({ label, onClick: () => act("transpose_regions", { ids: midi, semitones: n }) })),
      },
      { label: "Export as MIDI File…", onClick: () => exportMidi(midi, midi.length === 1 ? regs.find((x) => x.id === midi[0])!.name : p.name) },
    );
    // Pattern regions (Step Sequencer) and back.
    const patterns = regs.filter((x) => x.pattern).map((x) => x.id);
    const plain = regs.filter((x) => x.content.type === "midi" && !x.pattern).map((x) => x.id);
    items.push(
      patterns.length
        ? { label: "Convert to MIDI Region", onClick: () => act("convert_patterns_to_midi", { ids: patterns }) }
        : { label: "Convert to Pattern Region", onClick: () => act("convert_midi_to_patterns", { ids: plain }) },
    );
  }
  items.push(
    { separator: true },
    { label: "Bounce in Place…", onClick: () => openBounceInPlace([], ids) },
    { label: "Export as Audio File…", disabled: ids.length > 1, onClick: () => openBounce("region", r.id) },
  );
  const track = trackOf(r.id);
  items.push(
    { separator: true },
    { label: "Move to New Track", onClick: () => actMade("regions_to_new_track", { ids }).then(select) },
    { label: "Set Locators by Regions", hint: keyHint("set-locators"), onClick: () => setLocatorsByRegions(ids) },
    {
      label: "Select",
      children: [
        { label: "All on Track", disabled: !track, onClick: () => track && selectOnTrack(track.id) },
        { label: "All Following on Track", disabled: !track, onClick: () => track && selectOnTrack(track.id, r.start) },
        { label: "All", hint: fmt("Mod+A"), onClick: selectAllRegions },
      ],
    },
    { separator: true },
    { label: "Delete", hint: keyHint("delete-regions"), onClick: () => deleteRegions(ids) },
  );
  return items;
}

/** Right-clicking empty space in a track's lane (or below the tracks). */
export function laneMenu(track: Track | null, pos: number): MenuItem[] {
  const items: MenuItem[] = [];
  if (track) items.push({ label: "Paste Here", hint: keyHint("paste"), disabled: !getState().clipboard.length, onClick: () => pasteRegions({ track: track.id, pos }) });
  if (track?.kind === "instrument") items.push({ label: "Create MIDI Region", onClick: () => createMidiRegion(track.id, pos) });
  if (track?.kind === "instrument") items.push({ label: "Create Pattern Region", onClick: () => createPatternRegion(track.id, pos) });
  if (track?.kind === "audio") items.push({ label: "Import Audio File Here…", onClick: () => importAt(track, pos) });
  if (track?.kind === "instrument") items.push({ label: "Import MIDI File Here…", onClick: () => importMidiAt(track, pos) });
  if (track) items.push({ label: "Select All on Track", onClick: () => selectOnTrack(track.id) });
  if (items.length) items.push({ separator: true });
  items.push({ label: "Select All", hint: fmt("Mod+A"), onClick: selectAllRegions }, { label: "New Tracks…", hint: fmt("Mod+Alt+N"), onClick: () => setState({ dialog: "new-tracks" }) });
  return items;
}

/** Right-clicking a track header (acting on the selected tracks). */
export function trackMenu(t: Track, rename: () => void): MenuItem[] {
  const p = project()!;
  const ids = selectedTracks().includes(t.id) ? selectedTracks() : [t.id];
  const many = ids.length > 1;
  const sel = p.tracks.filter((x) => ids.includes(x.id));
  const group = t.group != null ? p.groups.find((g) => g.id === t.group) : undefined;
  const inStack = sel.some((x) => x.parent !== null);
  const s = (one: string, more: string) => (many ? more : one);
  const items: MenuItem[] = [
    { label: s("Duplicate Track", "Duplicate Tracks"), hint: fmt("Mod+D"), onClick: () => duplicateTracks(true) },
    { label: "New Track with Same Settings", disabled: many, onClick: () => duplicateTracks(false) },
    { label: "New Tracks…", hint: fmt("Mod+Alt+N"), onClick: () => setState({ dialog: "new-tracks" }) },
    { label: "New Aux", onClick: () => newAux() },
    { label: s("Bounce in Place…", "Bounce Tracks in Place…"), hint: BOUNCE_IN_PLACE_HINT, onClick: () => bounceInPlace(ids) },
    ...(sel.some(canFreeze)
      ? [
          sel.filter(canFreeze).every((x) => x.frozen)
            ? { label: s("Unfreeze Track", "Unfreeze Tracks"), onClick: () => unfreezeTracks(ids) }
            : { label: s("Freeze Track", "Freeze Tracks"), disabled: !sel.some((x) => canFreeze(x) && x.regions.length), onClick: () => freezeTracks(ids) },
        ]
      : []),
    { separator: true },
    { label: "Rename Track", disabled: many, onClick: rename },
    { label: "Track Color", children: colorMenu(many ? undefined : t.color, (c) => c && sel.forEach((x) => api.updateTrack(x.id, { color: c }))) },
    ...(canHaveAlternatives(t) && !many ? [{ label: "Track Alternatives", children: trackAltMenu(t) }] : []),
    { separator: true },
    {
      label: "Create Track Stack",
      children: [
        { label: "Folder Stack", hint: fmt("Mod+Shift+D"), onClick: () => createStack(false, ids) },
        { label: "Summing Stack", onClick: () => createStack(true, ids) },
      ],
    },
  ];
  if (isStack(t.kind) && !many) {
    items.push(
      { label: t.collapsed ? "Expand Stack" : "Collapse Stack", onClick: () => toggleCollapsed(t) },
      { label: "Flatten Stack", onClick: () => flattenStack(t.id) },
    );
  }
  if (inStack) items.push({ label: "Remove from Stack", onClick: () => removeFromStack(ids) });
  items.push(
    { separator: true },
    {
      label: "Group",
      children: [
        { label: s("New Group from Track", "New Group from Selected Tracks"), hint: fmt("Mod+G"), onClick: () => groupTracks(ids) },
        ...(p.groups.length ? [{ separator: true } as MenuItem, { label: "Assign to", header: true } as MenuItem] : []),
        ...p.groups.map((g) => ({ label: g.name, checked: sel.every((x) => x.group === g.id), onClick: () => setGroup(ids, g.id) })),
        { separator: true },
        { label: "Remove from Group", disabled: !sel.some((x) => x.group !== null), onClick: () => setGroup(ids, null) },
        { label: "Group Settings…", disabled: !group, onClick: () => group && setState({ groupSheet: group.id }) },
        { label: group?.enabled === false ? "Turn Group On" : "Turn Group Off", disabled: !group, onClick: () => group && updateGroup({ ...group, enabled: !group.enabled }) },
      ],
    },
    { separator: true },
    { label: many ? "Mute Tracks" : "Mute", hint: keyHint("track-mute"), checked: sel.every((x) => x.mute), onClick: () => switchAll("mute", ids) },
    { label: many ? "Solo Tracks" : "Solo", hint: keyHint("track-solo"), checked: sel.every((x) => x.solo), onClick: () => switchAll("solo", ids) },
  );
  if (sel.some((x) => canRecord(x.kind))) items.push({ label: "Record Enable", hint: keyHint("track-arm"), checked: sel.every((x) => x.armed || !canRecord(x.kind)), onClick: () => switchAll("armed", ids.filter((id) => canRecord(p.tracks.find((x) => x.id === id)!.kind))) });
  items.push(
    { separator: true },
    { label: "Select All Regions on Track", disabled: many, onClick: () => selectOnTrack(t.id) },
    ...(t.kind === "instrument" && !many ? [{ label: "Export Track as MIDI File…", disabled: !t.regions.length, onClick: () => exportTrackMidi(t.id) }] : []),
    { label: "Reset Track Height", onClick: () => ids.forEach((id) => setTrackHeight(id, null)) },
    { separator: true },
    // One key for both: hideTracks (⌃H) shows the tracks again when they're all hidden.
    sel.every((x) => x.hidden)
      ? { label: s("Unhide Track", "Unhide Tracks"), hint: keyHint("hide-tracks"), onClick: () => setHidden(ids, false) }
      : { label: s("Hide Track", "Hide Tracks"), hint: keyHint("hide-tracks"), onClick: () => hideTracks(ids) },
    { label: "Unhide All Tracks", disabled: !p.tracks.some((x) => x.hidden), onClick: unhideAllTracks },
    { label: "Show Hidden Tracks", hint: keyHint("show-hidden"), checked: getState().showHidden, onClick: toggleShowHidden },
    { separator: true },
    { label: s("Delete Track", "Delete Tracks"), hint: fmt("Mod+Backspace"), onClick: () => deleteTracks(ids) },
    { label: "Delete Unused Tracks", onClick: deleteUnusedTracks },
  );
  return items;
}

/** ⌘D: duplicates whatever was last worked on, the tracks or the regions. */
export function duplicate() {
  const s = getState();
  if (s.focus === "regions" && s.selectedRegions.length) repeatRegions(1);
  else duplicateTracks(true);
}

/** ⌫ / Delete. */
export function deleteSelection() {
  const s = getState();
  if (s.selectedRegions.length && s.focus === "regions") deleteRegions();
}
