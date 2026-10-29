import { act } from "./api";
import { currentTick, getState } from "./store";
import { samplesPerBar } from "./time";

export const transport = {
  nudgeBars(n: number) {
    const p = getState().snap?.project;
    if (!p) return;
    const bar = samplesPerBar(p);
    const pos = currentTick()?.pos ?? 0;
    const cur = n < 0 ? Math.ceil(pos / bar - 0.001) : Math.floor(pos / bar + 0.001);
    act("locate", { pos: Math.max(0, Math.round((cur + n) * bar)) });
  },
  toggleCycle() {
    const p = getState().snap?.project;
    if (!p) return;
    const c = p.cycle;
    if (c.end > c.start) {
      act("set_cycle", { enabled: !c.enabled, start: c.start, end: c.end });
    } else {
      // No locators yet: cycle the four bars from the playhead's bar, as Logic does.
      const bar = samplesPerBar(p);
      const start = Math.floor((currentTick()?.pos ?? 0) / bar) * bar;
      act("set_cycle", { enabled: true, start: Math.round(start), end: Math.round(start + 4 * bar) });
    }
  },
  toStart() {
    act("locate", { pos: 0 });
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
    const bar = samplesPerBar(p);
    const c = p.cycle;
    const start = c.end > c.start ? c.start : Math.floor((currentTick()?.pos ?? 0) / bar) * bar;
    const end = c.end > c.start ? c.end : start + 2 * bar;
    act("set_punch", { enabled: true, start: Math.round(start), end: Math.round(end) });
  },
};
