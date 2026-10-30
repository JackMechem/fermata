// Settings › Editing › Move Automation with Regions: the Ask setting's question before a move.
import { ask } from "@tauri-apps/plugin-dialog";
import type { Id, Project } from "./api";
import { regionEnd } from "./loops";
import { getSettings } from "./prefs";

/** A region move as the move commands take it. */
export interface Move {
  id: Id;
  start: number;
  track?: Id | null;
}

/** Whether moving regions would take automation along (crates/engine/src/auto_move.rs): a
 *  region that stays on its track and changes place has automation points under it. */
export function carriesAutomation(project: Project, moves: Move[]): boolean {
  return moves.some((m) => {
    const t = project.tracks.find((t) => t.regions.some((r) => r.id === m.id));
    const r = t?.regions.find((r) => r.id === m.id);
    if (!t || !r || (m.track != null && m.track !== t.id) || m.start === r.start) return false;
    const e = regionEnd(r);
    return t.automation.some((l) => l.points.some((p) => p.pos >= r.start && p.pos < e));
  });
}

/** The `automation` argument of a move or nudge command. Undefined leaves it to the setting
 *  (Never or Always); with Ask, the answer to Logic's question when the move would take
 *  automation along, and false when it wouldn't. */
export async function moveAutomationAnswer(project: Project, moves: Move[]): Promise<boolean | undefined> {
  if (getSettings().prefs.editing.move_automation !== "ask") return undefined;
  if (!carriesAutomation(project, moves)) return false;
  return ask("Do you want to move the automation with the region?", { title: "Move Automation", kind: "info", okLabel: "Move", cancelLabel: "Don't Move" });
}
