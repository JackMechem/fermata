// Track On/Off, kept pure so it can be tested: a track is off through its own switch or through a
// stack above it (`Project::is_off` in model.rs is the engine's copy of this rule).
import type { Id, Track } from "./api";

type Node = Pick<Track, "id" | "parent" | "off">;

/** Whether `t` is off, itself or through a stack it is in. */
export function isTrackOff(tracks: readonly Node[], t: Node): boolean {
  const seen = new Set<Id>();
  for (let cur: Node | undefined = t; cur && !seen.has(cur.id); cur = tracks.find((x) => x.id === cur!.parent)) {
    if (cur.off) return true;
    seen.add(cur.id);
    if (cur.parent === null || cur.parent === undefined) break;
  }
  return false;
}

/** Every track that is off, for drawing a whole project at once. */
export function offTracks(tracks: readonly Node[]): Set<Id> {
  return new Set(tracks.filter((t) => isTrackOff(tracks, t)).map((t) => t.id));
}

/** The power button's tooltip. It shows the track's own switch; a stack above can also hold it off. */
export function powerTitle(tracks: readonly Node[], t: Node): string {
  if (t.off) return "Track Off: plays nothing and runs nothing, plug-ins stay loaded. Click to turn it on";
  if (isTrackOff(tracks, t)) return "Off with its stack. Turn the stack on to hear it";
  return "Track On. Click to turn it off and free the CPU its plug-ins use (Mute doesn't)";
}
