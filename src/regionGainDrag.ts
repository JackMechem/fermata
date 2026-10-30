// The region gain handle in the Tracks area (Arrange.tsx): drawing it, finding it under the
// pointer, and the drag, which sends every selected audio region's new gain as it goes. The
// first update of a drag records the undo step and the rest join it (`set_region_gains`), so
// a drag is one step. The geometry and arithmetic are in regionGain.ts.
import { act, type Id, type Project, type Region } from "./api";
import { coalesced } from "./frame";
import { regionBar } from "./regionBar";
import { dragDelta, gainLabel, handleRect, inRect, shiftedGains, type Rect } from "./regionGain";
import { theme } from "./theme";

/** The gains a drag in progress shows, ahead of the engine's. */
let live: Map<Id, number> | null = null;

/** `r` as it draws: with the gain a drag in progress gives it. */
export function withLiveGain(r: Region): Region {
  const db = live?.get(r.id);
  if (db === undefined || r.content.type !== "audio") return r;
  return { ...r, content: { ...r.content, gain_db: db } };
}

export function gainDragging(id: Id): boolean {
  return !!live?.has(id);
}

/** The handle of a region drawn at `x`, `y` (canvas pixels), `w` wide and `h` tall, in a view
 *  `W` wide. */
function rectFor(x: number, y: number, w: number, h: number, W: number): Rect | null {
  return handleRect(x, y, w, regionBar(h).bar, 0, W);
}

/** Whether a press at content `x` and lane-relative `localY` is on audio region `r`'s handle
 *  (regions are drawn 1 px into their lane and 3 px shorter than it). */
export function onGainHandle(r: Region, x: number, localY: number, laneH: number, pps: number, vpLeft: number, W: number): boolean {
  if (r.content.type !== "audio") return false;
  return inRect(rectFor(r.start * pps - vpLeft, 1, r.length * pps, laneH - 3, W), x - vpLeft, localY);
}

/** Draws the handle with the region's gain on an audio region drawn at `x`, `y`, `w` × `h`. */
export function drawGainHandle(ctx: CanvasRenderingContext2D, r: Region, x: number, y: number, w: number, h: number, W: number) {
  if (r.content.type !== "audio") return;
  const rc = rectFor(x, y, w, h, W);
  if (!rc) return;
  ctx.save();
  ctx.beginPath();
  ctx.roundRect(rc.x + 0.5, rc.y + 0.5, rc.w - 1, rc.h - 1, rc.h / 2);
  ctx.globalAlpha = 0.88;
  ctx.fillStyle = theme.bg0;
  ctx.fill();
  ctx.globalAlpha = 1;
  ctx.fillStyle = theme.text;
  ctx.font = `600 ${Math.min(10, rc.h - 2)}px Inter, 'SF Pro Text', system-ui, sans-serif`;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillText(gainLabel(r.content.gain_db), rc.x + rc.w / 2, rc.y + rc.h / 2 + 0.5);
  ctx.restore();
}

/** A press on `grabbed`'s handle: a vertical drag changes the gain of it and the other
 *  selected audio regions (`sel`) by the same amount, Shift for fine steps; a double-click
 *  sets them to 0 dB. */
export function startGainDrag(e: { clientY: number; detail: number }, project: Project, grabbed: Region, sel: Id[], redraw: () => void) {
  const ids = sel.includes(grabbed.id) ? sel : [grabbed.id];
  const from: [Id, number][] = project.tracks.flatMap((t) =>
    t.frozen ? [] : t.regions.flatMap((r) => (ids.includes(r.id) && r.content.type === "audio" ? [[r.id, r.content.gain_db] as [Id, number]] : [])),
  );
  if (!from.length) return;
  if (e.detail >= 2) {
    act("set_region_gains", { gains: from.map(([id]) => [id, 0]), first: true });
    return;
  }
  let delta = 0;
  let last = e.clientY;
  let first = true;
  // Updates go one after another, so the first (which records the undo step) lands first.
  let chain: Promise<unknown> = Promise.resolve();
  const send = coalesced(() => {
    const gains = shiftedGains(from, delta);
    const f = first;
    first = false;
    chain = chain.then(() => act("set_region_gains", { gains, first: f }));
  });
  const move = (ev: MouseEvent) => {
    delta = dragDelta(delta, ev.clientY - last, ev.shiftKey);
    last = ev.clientY;
    live = new Map(shiftedGains(from, delta));
    redraw();
    send();
  };
  const up = () => {
    window.removeEventListener("mousemove", move);
    window.removeEventListener("mouseup", up);
    // Keep showing the dragged gains until the engine's snapshot has them.
    requestAnimationFrame(() =>
      chain.then(() => {
        live = null;
        redraw();
      }),
    );
  };
  window.addEventListener("mousemove", move);
  window.addEventListener("mouseup", up);
}
