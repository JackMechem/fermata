// Take folders in the arrange area, as in Logic: a folder region shows its comp, a disclosure
// triangle opens it into one row per take below the track's lane, and swiping across a take
// row puts that stretch of the take in the comp (Quick Swipe Comping). The comp logic lives in
// the engine (crates/engine/src/takes.rs); this file draws folders and sends the edits.
import { act, actMade, type Id, type Region, type Take, type TakeFolder, type Track } from "./api";
import { compNames, currentComp, isMidiFolder } from "./comps";
import type { MenuItem } from "./components/Menu";
import { getState, setState } from "./store";
import { mix, theme } from "./theme";
import { drawWave, ensureWave, waveCache } from "./waveform";

/** Height of one take row in an open folder. */
export const TAKE_ROW_H = 26;
/** Width of the triangle and take badge at the start of a folder's name bar. */
export const FOLDER_BADGE_W = 34;

/** An audio or MIDI take folder. */
export const isFolder = (r: Region): r is Region & { takes: TakeFolder } => !!r.takes;

/** Folder-relative offset: a timeline position `p` is take time `p + shift(r)`. */
export const takeShift = (r: Region) => (r.content.type === "audio" ? r.content.offset : (r.takes?.offset ?? 0)) - r.start;

/** The take heard throughout when the comp is one whole take, else null (a comp). */
export function wholeTake(f: TakeFolder): Id | null {
  if (f.comp.length !== 1) return null;
  const s = f.comp[0];
  const t = f.takes.find((x) => x.id === s.take);
  return t && t.start === s.start && t.end === s.end ? t.id : null;
}

/** What the folder's badge says: the whole take's number, or "C" for a comp. */
export function badgeText(f: TakeFolder): string {
  const w = wholeTake(f);
  if (w === null) return f.comp.length ? "C" : "–";
  return String(f.takes.findIndex((t) => t.id === w) + 1);
}

// ---- open folders: a view setting, like track heights ---------------------------------------

export function isOpen(id: Id): boolean {
  return getState().openFolders.includes(id);
}

export function toggleFolder(id: Id) {
  const open = getState().openFolders;
  setState({ openFolders: open.includes(id) ? open.filter((x) => x !== id) : [...open, id] });
}

/** Extra lane height a track gets for its open folders (the tallest one). */
export function takeRowsHeight(t: Track, open: Id[]): number {
  let rows = 0;
  for (const r of t.regions) if (r.takes && open.includes(r.id)) rows = Math.max(rows, r.takes.takes.length);
  return rows * TAKE_ROW_H;
}

/** Loads the waveforms of every take, not just each folder's own clip. */
export function ensureTakeWaves(tracks: Track[], done: () => void) {
  for (const t of tracks) for (const r of t.regions) if (r.content.type === "audio") for (const k of r.takes?.takes ?? []) ensureWave(k.clip, done);
}

// ---- drawing ----------------------------------------------------------------------------------

/** The disclosure triangle and the take badge, at the start of the folder's name bar. */
export function drawFolderBadge(ctx: CanvasRenderingContext2D, r: Region & { takes: TakeFolder }, x: number, y: number, ink: string) {
  const open = isOpen(r.id);
  const tx = Math.max(x, 0) + 5;
  ctx.fillStyle = ink;
  ctx.beginPath();
  if (open) {
    ctx.moveTo(tx, y + 5);
    ctx.lineTo(tx + 8, y + 5);
    ctx.lineTo(tx + 4, y + 10);
  } else {
    ctx.moveTo(tx + 1, y + 3);
    ctx.lineTo(tx + 6, y + 7);
    ctx.lineTo(tx + 1, y + 11);
  }
  ctx.closePath();
  ctx.fill();
  const bx = tx + 12;
  ctx.beginPath();
  ctx.roundRect(bx, y + 2, 16, 10, 2);
  ctx.globalAlpha *= 0.85;
  ctx.fill();
  ctx.globalAlpha /= 0.85;
  ctx.fillStyle = theme.region === "light" ? "#ffffff" : mix(ink, "#ffffff", 0.9);
  ctx.font = "700 9px Inter, 'SF Pro Text', system-ui, sans-serif";
  ctx.textAlign = "center";
  ctx.fillText(badgeText(r.takes), bx + 8, y + 7.5);
  ctx.textAlign = "left";
}

/** A folder's waveform: each comp segment drawn from its own take. `shift` is the trim being
 *  dragged (frames off the front), as for plain regions. */
export function drawCompWave(
  ctx: CanvasRenderingContext2D,
  r: Region & { takes: TakeFolder },
  x: number,
  w: number,
  top: number,
  h: number,
  pps: number,
  color: string,
  visX0: number,
  visX1: number,
  shift: number,
  scale: number,
) {
  const tt0 = takeShift(r) + r.start + shift;
  for (const s of r.takes.comp) {
    const t = r.takes.takes.find((k) => k.id === s.take);
    const wave = t && waveCache.get(t.clip);
    if (!t || !wave) continue;
    const sx0 = x + (s.start - tt0) * pps;
    const sx1 = x + (s.end - tt0) * pps;
    drawWave(ctx, wave, t.offset + tt0, x, w, top, h, pps, color, Math.max(visX0, sx0), Math.min(visX1, sx1), undefined, scale * 10 ** (t.gain_db / 20));
  }
}

/** The take rows of an open folder, below its track's lane (`y` is the first row's top). Comp
 *  parts are in the region's colour, the rest dimmed, as in Logic. `preview` is a swipe in
 *  progress: take and take-time range. */
export function drawTakeRows(
  ctx: CanvasRenderingContext2D,
  r: Region & { takes: TakeFolder },
  color: string,
  x: number,
  w: number,
  y: number,
  pps: number,
  W: number,
  preview: { take: Id; start: number; end: number } | null,
) {
  const tt0 = takeShift(r) + r.start;
  const visX0 = Math.max(0, x);
  const visX1 = Math.min(W, x + w);
  const light = theme.region === "light";
  r.takes.takes.forEach((t, k) => {
    const ry = y + k * TAKE_ROW_H;
    const rh = TAKE_ROW_H - 2;
    // The take's own extent within the folder.
    const tx0 = Math.max(x, x + (t.start - tt0) * pps);
    const tx1 = Math.min(x + w, x + (t.end - tt0) * pps);
    ctx.save();
    ctx.beginPath();
    ctx.rect(visX0, ry, Math.max(0, visX1 - visX0), rh + 1);
    ctx.clip();
    ctx.fillStyle = mix(color, theme.regionBase, light ? 0.75 : 0.82, 0.9);
    ctx.fillRect(tx0, ry + 0.5, Math.max(0, tx1 - tx0), rh);
    // A MIDI take shows its notes, bright when it's the one heard.
    if (isMidiFolder(r)) {
      if (r.takes.comp.some((s) => s.take === t.id)) {
        ctx.fillStyle = light ? mix(color, "#ffffff", 0.45) : mix(color, theme.regionBase, 0.2, 0.9);
        ctx.fillRect(tx0, ry + 0.5, Math.max(0, tx1 - tx0), rh);
      }
      drawTakeNotes(ctx, t, x, tt0, ry + 2, rh - 4, pps, light ? mix(color, "#000000", 0.5) : theme.ink, visX0, visX1);
      ctx.fillStyle = light ? "rgba(0,0,0,0.6)" : "rgba(255,255,255,0.75)";
      ctx.font = "600 10px Inter, 'SF Pro Text', system-ui, sans-serif";
      ctx.textBaseline = "middle";
      ctx.fillText(t.name, visX0 + 4, ry + rh / 2 + 0.5);
      ctx.restore();
      return;
    }
    const wave = waveCache.get(t.clip);
    const scale = 10 ** (t.gain_db / 20);
    const segs = r.takes.comp.filter((s) => s.take === t.id).map((s) => [s.start, s.end]);
    if (preview && preview.take === t.id) segs.push([preview.start, preview.end]);
    // Selected stretches: a bright body under the waveform.
    for (const [a, b] of segs) {
      const sx0 = Math.max(tx0, x + (a - tt0) * pps);
      const sx1 = Math.min(tx1, x + (b - tt0) * pps);
      if (sx1 <= sx0) continue;
      ctx.fillStyle = light ? mix(color, "#ffffff", 0.45) : mix(color, theme.regionBase, 0.2, 0.9);
      ctx.fillRect(sx0, ry + 0.5, sx1 - sx0, rh);
    }
    if (wave) {
      ctx.globalAlpha = 0.45;
      drawWave(ctx, wave, t.offset + tt0, x, w, ry + 2, rh - 4, pps, light ? mix(color, "#000000", 0.4) : mix(color, "#ffffff", 0.55), tx0, tx1, undefined, scale);
      ctx.globalAlpha = 1;
      for (const [a, b] of segs) {
        const sx0 = Math.max(tx0, x + (a - tt0) * pps);
        const sx1 = Math.min(tx1, x + (b - tt0) * pps);
        if (sx1 > sx0) drawWave(ctx, wave, t.offset + tt0, x, w, ry + 2, rh - 4, pps, light ? mix(color, "#000000", 0.55) : theme.ink, sx0, sx1, undefined, scale);
      }
    }
    ctx.fillStyle = light ? "rgba(0,0,0,0.6)" : "rgba(255,255,255,0.75)";
    ctx.font = "600 10px Inter, 'SF Pro Text', system-ui, sans-serif";
    ctx.textBaseline = "middle";
    ctx.fillText(t.name, visX0 + 4, ry + rh / 2 + 0.5);
    ctx.restore();
  });
}

/** A MIDI take's notes in its row, spread over the take's pitch range. */
function drawTakeNotes(ctx: CanvasRenderingContext2D, t: Take, x: number, tt0: number, top: number, h: number, pps: number, ink: string, visX0: number, visX1: number) {
  const notes = t.notes ?? [];
  if (!notes.length) return;
  let lo = 127;
  let hi = 0;
  for (const n of notes) {
    lo = Math.min(lo, n.pitch);
    hi = Math.max(hi, n.pitch);
  }
  const span = Math.max(12, hi - lo + 4);
  const nh = Math.max(1.5, Math.min(3, h / span));
  ctx.fillStyle = ink;
  ctx.globalAlpha = 0.8;
  for (const n of notes) {
    const nx = x + (n.start - tt0) * pps;
    const nw = Math.max(2, n.length * pps);
    if (nx > visX1 || nx + nw < visX0) continue;
    ctx.fillRect(nx, top + h - ((n.pitch - lo + 2) / span) * h - nh / 2, nw - 0.5, nh);
  }
  ctx.globalAlpha = 1;
}

// ---- edits --------------------------------------------------------------------------------------

/** Puts `take` in the comp over timeline `start..end` of the folder (a swipe). */
export function swipeComp(r: Region, take: Id, start: number, end: number) {
  act("set_comp_range", { region: r.id, take, start: Math.round(Math.min(start, end)), end: Math.round(Math.max(start, end)) });
}

/** The take row under content y, for an open folder whose rows start at `rowsTop`. */
export function takeAtRow(r: Region & { takes: TakeFolder }, y: number, rowsTop: number): Take | null {
  const k = Math.floor((y - rowsTop) / TAKE_ROW_H);
  return k >= 0 && k < r.takes.takes.length ? r.takes.takes[k] : null;
}

function renameTake(r: Region, t: Take) {
  const name = window.prompt("Take name", t.name);
  if (name) act("rename_take", { region: r.id, take: t.id, name });
}

function renameComp(r: Region & { takes: TakeFolder }, index: number) {
  const name = window.prompt("Comp name", compNames(r.takes)[index]);
  if (name?.trim()) act("rename_comp", { region: r.id, index, name });
}

/** The comps part of an audio folder's menu, as in Logic: the comps to switch between, then
 *  Create New Comp, Duplicate Comp, Rename Comp and Delete Comp. */
function compItems(r: Region & { takes: TakeFolder }): MenuItem[] {
  const names = compNames(r.takes);
  const current = currentComp(r.takes);
  return [
    { label: "Comps", header: true },
    ...names.map((name, index) => ({ label: name, checked: index === current, onClick: () => act("select_comp", { region: r.id, index }) })),
    { separator: true },
    { label: "Create New Comp", onClick: () => act("create_comp", { region: r.id }) },
    { label: "Duplicate Comp", onClick: () => act("duplicate_comp", { region: r.id }) },
    { label: "Rename Comp…", onClick: () => renameComp(r, current) },
    { label: "Delete Comp", disabled: names.length < 2, onClick: () => act("delete_comp", { region: r.id, index: current }) },
    { separator: true },
  ];
}

/** The take folder menu (the badge, or Take Folder in the region menu). A MIDI folder plays
 *  whole takes, so it has no comps, and Flatten and Merge is just Flatten. */
export function takeFolderMenu(r: Region & { takes: TakeFolder }): MenuItem[] {
  const f = r.takes;
  const midi = isMidiFolder(r);
  const whole = midi ? (f.comp[0]?.take ?? f.takes[f.takes.length - 1]?.id) : wholeTake(f);
  const select = (ids: Id[] | null) => ids && setState({ selectedRegions: ids, focus: "regions" });
  return [
    ...(midi ? [] : compItems(r)),
    { label: "Takes", header: true },
    ...f.takes.map((t, k) => ({ label: `${k + 1}  ${t.name}`, checked: whole === t.id, onClick: () => act("select_take", { region: r.id, take: t.id }) })),
    { separator: true },
    { label: isOpen(r.id) ? "Close Take Folder" : "Open Take Folder", onClick: () => toggleFolder(r.id) },
    { label: "Rename Take", children: f.takes.map((t) => ({ label: t.name, onClick: () => renameTake(r, t) })) },
    { label: "Delete Take", disabled: f.takes.length < 2, children: f.takes.map((t) => ({ label: t.name, onClick: () => act("delete_take", { region: r.id, take: t.id }) })) },
    { label: "Delete Unused Takes", disabled: f.comp.length === 0 || new Set(f.comp.map((s) => s.take)).size === f.takes.length, onClick: () => act("delete_unused_takes", { region: r.id }) },
    { separator: true },
    { label: "Flatten", onClick: () => actMade("flatten_take_folder", { region: r.id }).then(select) },
    ...(midi ? [] : [{ label: "Flatten and Merge", onClick: () => actMade("merge_take_folder", { region: r.id }).then(select) }]),
    { label: "Unpack to New Tracks", onClick: () => actMade("unpack_take_folder", { region: r.id }) },
  ];
}

/** Pack Take Folder: the selected audio regions on one track become a folder. */
export function packTakeFolder(ids: Id[] = getState().selectedRegions) {
  if (ids.length < 2) return;
  actMade("pack_take_folder", { ids }).then((made) => made && setState({ selectedRegions: made, focus: "regions" }));
}
