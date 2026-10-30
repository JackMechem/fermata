// Pure helpers for project alternatives, backups, user templates and track alternatives
// (the Rust side is crates/engine/src/session/{alternatives,backups,templates,track_alternatives}.rs).

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

const pad = (n: number) => String(n).padStart(2, "0");

function dayStart(ms: number): number {
  const d = new Date(ms);
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

/** A backup's label in File › Revert to, in local time: "Today 14:32", "Yesterday 09:10",
 *  else "27 Oct 2026 14:32" (as Logic lists them, with the time, since a day can have many). */
export function backupLabel(ms: number, now: number = Date.now()): string {
  const d = new Date(ms);
  const time = `${pad(d.getHours())}:${pad(d.getMinutes())}`;
  // Calendar days, not 24-hour spans (and robust to a daylight-saving change in between).
  const days = Math.round((dayStart(now) - dayStart(ms)) / 86_400_000);
  if (days === 0) return `Today ${time}`;
  if (days === 1) return `Yesterday ${time}`;
  return `${d.getDate()} ${MONTHS[d.getMonth()]} ${d.getFullYear()} ${time}`;
}

/** Why `name` can't name an alternative or template (also a folder or file name), or null. */
export function nameProblem(name: string): string | null {
  const n = name.trim();
  if (!n) return "The name can't be empty.";
  if (n.startsWith(".") || /[/\\:*?"<>|\u0000-\u001f]/.test(n)) return "A name can't start with a dot or contain / \\ : * ? \" < > |";
  if ([...n].length > 100) return "The name is too long.";
  return null;
}

/** The first free "base", "base 2", "base 3"… among `taken`, ignoring case. */
export function freeName(base: string, taken: string[]): string {
  const lower = new Set(taken.map((t) => t.toLowerCase()));
  if (!lower.has(base.toLowerCase())) return base;
  for (let i = 2; ; i++) if (!lower.has(`${base} ${i}`.toLowerCase())) return `${base} ${i}`;
}

interface AltTrack {
  alternatives?: { name: string }[];
  alternative?: string;
}

/** The active track alternative's name ("A" until it has another). */
export function activeTrackAlt(t: AltTrack): string {
  return t.alternative || "A";
}

/** Every alternative of a track, the active one included, by name. */
export function trackAltNames(t: AltTrack): string[] {
  return [...(t.alternatives ?? []).map((a) => a.name), activeTrackAlt(t)].sort((a, b) => a.toLowerCase().localeCompare(b.toLowerCase()));
}

/** Whether a track shows the Track Alternatives pop-up in its header: once it has two. */
export function showsTrackAlts(t: AltTrack): boolean {
  return (t.alternatives?.length ?? 0) > 0;
}
