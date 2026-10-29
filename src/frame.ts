// One requestAnimationFrame loop drives every continuously animated view (playhead, meters,
// LCD), so they move at the display's refresh rate instead of the engine's 30 Hz updates.
import { currentTick, getState, tickArrival } from "./store";

type FrameFn = (now: number) => void;
const subs = new Set<FrameFn>();
let running = false;

function loop(now: number) {
  subs.forEach((f) => f(now));
  if (subs.size) requestAnimationFrame(loop);
  else running = false;
}

export function onFrame(f: FrameFn): () => void {
  subs.add(f);
  if (!running) {
    running = true;
    requestAnimationFrame(loop);
  }
  return () => subs.delete(f);
}

/**
 * Playhead position for this frame: the engine's last reported position advanced by the time
 * since that report. Capped at two report intervals so a stalled engine doesn't run away.
 */
export function framePos(now: number = performance.now()): number {
  const t = currentTick();
  if (!t) return 0;
  if (!t.playing) return t.pos;
  const sr = getState().snap?.project.sample_rate ?? 48000;
  const dt = Math.min(0.1, Math.max(0, (now - tickArrival()) / 1000));
  let pos = t.pos + dt * sr;
  // Wrap with Cycle so the playhead doesn't overshoot the right locator between reports.
  const c = getState().snap?.project.cycle;
  if (c?.enabled && !t.recording && t.pos < c.end && pos >= c.end) pos = c.start + (pos - c.end);
  return pos;
}

/** Schedules `f` once for the next frame, coalescing repeated requests. */
export function coalesced(f: () => void): () => void {
  let pending = false;
  return () => {
    if (pending) return;
    pending = true;
    requestAnimationFrame(() => {
      pending = false;
      f();
    });
  };
}
