// VCA faders: pure helpers, tested in tests/vca.test.ts. The rules mirror
// crates/engine/src/vca.rs, which is what the engine plays; here they only draw the strips.
import type { Id, TrackKind, Vca } from "./api";

/** At or below this a VCA silences its strips. */
export const VCA_MIN_DB = -96;
export const VCA_MAX_DB = 6;

/** Folder stacks have no audio of their own, so no VCA slot. */
export function canAssignVca(kind: TrackKind): boolean {
  return kind !== "folder";
}

/** The VCAs a strip on `first` answers to, nearest first; a missing VCA or a loop ends it. */
export function vcaChain(vcas: readonly Vca[], first: Id | null | undefined): Vca[] {
  const out: Vca[] = [];
  let next = first ?? null;
  while (next !== null) {
    const v = vcas.find((x) => x.id === next);
    if (!v || out.includes(v)) break;
    out.push(v);
    next = v.parent;
  }
  return out;
}

/** What a strip assigned to `vca` gets: the levels along the chain add up (-Infinity once one
 *  is at the bottom), and any muted VCA mutes it. */
export function vcaOffset(vcas: readonly Vca[], vca: Id | null | undefined): { db: number; mute: boolean } {
  let db = 0;
  let mute = false;
  for (const v of vcaChain(vcas, vca)) {
    db = v.level_db <= VCA_MIN_DB ? -Infinity : db + Math.min(v.level_db, VCA_MAX_DB);
    mute ||= v.mute;
  }
  return { db, mute };
}

/** The level a strip plays at: its fader plus the VCA offset, floored at silence. */
export function effectiveDb(faderDb: number, offsetDb: number): number {
  return Math.max(VCA_MIN_DB, faderDb + offsetDb);
}

/** Whether `vca` may be assigned to `parent` without a loop. */
export function canNest(vcas: readonly Vca[], vca: Id, parent: Id): boolean {
  return vca !== parent && !vcaChain(vcas, parent).some((v) => v.id === vca);
}

/** The VCA slot's text: the VCA's name, or empty for none. */
export function vcaSlotLabel(vcas: readonly Vca[], vca: Id | null | undefined): string {
  return vca == null ? "" : (vcas.find((v) => v.id === vca)?.name ?? "");
}
