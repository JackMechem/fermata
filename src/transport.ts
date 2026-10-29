import { act } from "./api";
import { currentTick, getState } from "./store";
import { barFloor, barsFrom } from "./time";
import { tempoMap } from "./tempo";
import { revealPlayhead } from "./view";

/** Moves the playhead and scrolls the time views to show it (see `revealPlayhead`). */
export async function locateAndReveal(pos: number) {
  await act("locate", { pos });
  revealPlayhead(pos);
}

export const transport = {
  nudgeBars(n: number) {
    const p = getState().snap?.project;
    if (!p) return;
    const m = tempoMap(p);
    const at = m.barPosition(currentTick()?.pos ?? 0);
    const cur = n < 0 ? Math.ceil(at - 0.001) : Math.floor(at + 0.001);
    locateAndReveal(Math.max(0, Math.round(m.barStart(cur + n + 1))));
  },
  toggleCycle() {
    const p = getState().snap?.project;
    if (!p) return;
    const c = p.cycle;
    if (c.end > c.start) {
      act("set_cycle", { enabled: !c.enabled, start: c.start, end: c.end });
    } else {
      // No locators yet: cycle the four bars from the playhead's bar, as Logic does.
      const pos = currentTick()?.pos ?? 0;
      act("set_cycle", { enabled: true, start: Math.round(barFloor(p, pos)), end: Math.round(barsFrom(p, pos, 4)) });
    }
  },
  toStart() {
    locateAndReveal(0);
  },
  /** The Autopunch button: switches the punch range on or off. With none yet, it takes the
   *  cycle's range, or the two bars from the playhead's bar. */
  togglePunch() {
    const p = getState().snap?.project;
    if (!p) return;
    const r = p.punch;
    if (r.end > r.start) {
      act("set_punch", { enabled: !r.enabled, start: r.start, end: r.end });
      return;
    }
    const c = p.cycle;
    const pos = currentTick()?.pos ?? 0;
    const start = c.end > c.start ? c.start : barFloor(p, pos);
    const end = c.end > c.start ? c.end : barsFrom(p, pos, 2);
    act("set_punch", { enabled: true, start: Math.round(start), end: Math.round(end) });
  },
  /**
   * The Stop button. Pressed while stopped, the engine sends the playhead back to the left
   * locator with Cycle on, otherwise to the start (`Session::stop`); the time views follow it.
   */
  async stop() {
    const t = currentTick();
    const p = getState().snap?.project;
    const again = !t?.playing && !t?.recording;
    await act("stop");
    if (again && p) revealPlayhead(p.cycle.enabled ? p.cycle.start : 0);
  },
};
