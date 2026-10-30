// Which key commands run again while their key is held. Moving and zooming do; a toggle held down
// would flip back and forth (Space starting and stopping the transport), and a one-shot command
// would run many times.
//
// This module has no imports, so it runs under plain Node for the tests in tests/.
const REPEATS = new Set(["rewind", "forward", "previous-marker", "next-marker", "zoom-in", "zoom-out", "taller-tracks", "shorter-tracks", "select-previous", "select-next", "nudge-left", "nudge-right"]);

/** Whether command `id` runs for this key press, `repeat` being the event's auto-repeat flag. */
export function runsOnKey(id: string, repeat: boolean): boolean {
  return !repeat || REPEATS.has(id);
}
