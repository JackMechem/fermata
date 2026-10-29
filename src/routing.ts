// Sends, aux buses and track outputs: the menus behind the channel strip's Sends and Output
// slots. The engine refuses feedback loops; the menus grey out the choices that would make one.
import { act, actMade, MASTER_ID, type Id, type Project, type Send, type Track } from "./api";
import type { MenuItem } from "./components/Menu";
import { getState } from "./store";

const project = () => getState().snap?.project;

export const auxes = (p: Project) => p.tracks.filter((t) => t.kind === "aux");

/** The bus feeding an aux, as its Input slot shows it. */
export const busLabel = (aux: Track) => `Bus ${aux.bus}`;

/** What a send slot shows: the bus, or the aux's own name once it has been renamed. */
export const sendLabel = (aux: Track) => (/^Aux \d+$/.test(aux.name) ? busLabel(aux) : aux.name);

const auxItemLabel = (aux: Track) => `${busLabel(aux)} → ${aux.name}`;

/** The summing stack a track plays through when its output isn't set. */
export function defaultOutput(p: Project, t: Track): Track | null {
  let up = t.parent !== null ? p.tracks.find((x) => x.id === t.parent) : undefined;
  for (let d = 0; up && d < 32; d++) {
    if (up.kind === "summing") return up;
    up = up.parent !== null ? p.tracks.find((x) => x.id === up!.parent) : undefined;
  }
  return null;
}

/** Where a track's signal goes (null = Stereo Out); mirrors `Project::output_of`. */
function outputOf(p: Project, t: Track): Id | null {
  if (t.output === MASTER_ID) return null;
  if (t.output !== null && t.output !== t.id && p.tracks.some((x) => x.id === t.output && x.kind === "aux")) return t.output;
  return defaultOutput(p, t)?.id ?? null;
}

function feeds(p: Project, id: Id): Id[] {
  const t = p.tracks.find((x) => x.id === id);
  if (!t) return [];
  const out = outputOf(p, t);
  return [...(out !== null ? [out] : []), ...(t.sends ?? []).flatMap((s) => (s ? [s.bus] : []))];
}

/** Whether `from`'s audio reaches `to` through outputs, stacks and sends. */
export function reaches(p: Project, from: Id, to: Id): boolean {
  const seen = new Set<Id>();
  const todo = [from];
  while (todo.length) {
    const id = todo.pop()!;
    if (seen.has(id)) continue;
    seen.add(id);
    for (const d of feeds(p, id)) {
      if (d === to) return true;
      todo.push(d);
    }
  }
  return false;
}

/** Whether `track` can play into `aux` without making a loop. */
const canFeed = (p: Project, track: Track, aux: Track) => aux.id !== track.id && !reaches(p, aux.id, track.id);

/** The Output slot's text. */
export function outputLabel(p: Project, t: Track): string {
  if (t.output === MASTER_ID) return "Stereo Out";
  const aux = t.output !== null ? p.tracks.find((x) => x.id === t.output) : undefined;
  if (aux) return aux.name;
  return defaultOutput(p, t)?.name ?? "Stereo Out";
}

/** A new aux at the end of the track list, optionally fed by a send slot or a track's output. */
export const newAux = (from: { send?: [Id, number]; output?: Id } = {}) =>
  actMade("create_aux", { name: null, sendFrom: from.send ?? null, outputFrom: from.output ?? null });

export const setSend = (track: Id, slot: number, send: Send | null) => act("set_send", { track, slot, send });
export const setSendLevel = (track: Id, slot: number, levelDb: number) => act("set_send_level", { track, slot, levelDb });
export const setOutput = (track: Id, output: Id | null) => act("set_output", { track, output });

/** The auxes a send or output could go to, plus New Aux. */
function auxChoices(t: Track, current: Id | null, pick: (aux: Track) => void, create: () => void): MenuItem[] {
  const p = project();
  if (!p) return [];
  const list = auxes(p).filter((a) => a.id !== t.id);
  return [
    ...list.map((a) => ({ label: auxItemLabel(a), checked: a.id === current, disabled: a.id !== current && !canFeed(p, t, a), onClick: () => pick(a) })),
    ...(list.length ? [{ separator: true } as MenuItem] : []),
    { label: "New Aux", onClick: create },
  ];
}

/** Clicking an empty send slot. */
export function emptySendMenu(t: Track, slot: number): MenuItem[] {
  return auxChoices(
    t,
    null,
    (a) => setSend(t.id, slot, { bus: a.id, level_db: 0, pre_fader: false, muted: false }),
    () => newAux({ send: [t.id, slot] }),
  );
}

/** Clicking a filled send slot. */
export function sendMenu(t: Track, slot: number, s: Send): MenuItem[] {
  const set = (patch: Partial<Send>) => setSend(t.id, slot, { ...s, ...patch });
  return [
    { label: "Pre Fader", checked: s.pre_fader, onClick: () => set({ pre_fader: true }) },
    { label: "Post Fader", checked: !s.pre_fader, onClick: () => set({ pre_fader: false }) },
    { separator: true },
    { label: "Mute Send", checked: s.muted, onClick: () => set({ muted: !s.muted }) },
    {
      label: "Send to",
      children: auxChoices(t, s.bus, (a) => set({ bus: a.id }), () => newAux({ send: [t.id, slot] })),
    },
    { separator: true },
    { label: "Remove Send", onClick: () => setSend(t.id, slot, null) },
  ];
}

/** Clicking the Output slot. A summing stack's subtrack can also skip its stack. */
export function outputMenu(t: Track): MenuItem[] {
  const p = project();
  if (!p) return [];
  const stack = defaultOutput(p, t);
  const head: MenuItem[] = stack
    ? [
        { label: stack.name, checked: t.output === null, onClick: () => setOutput(t.id, null) },
        { label: "Stereo Out", checked: t.output === MASTER_ID, onClick: () => setOutput(t.id, MASTER_ID) },
      ]
    : [{ label: "Stereo Out", checked: t.output === null || t.output === MASTER_ID, onClick: () => setOutput(t.id, null) }];
  return [
    ...head,
    { separator: true },
    ...auxChoices(t, t.output, (a) => setOutput(t.id, a.id), () => newAux({ output: t.id })),
  ];
}
