// The layout of a small piano keyboard (pure, tested in tests/pianoKeys.test.ts).

const NAMES = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];
const isBlack = (note: number) => [1, 3, 6, 8, 10].includes(note % 12);

export interface PianoKey {
  note: number;
  name: string;
  black: boolean;
  /** Where its left edge sits, in white keys from the keyboard's left. */
  at: number;
}

/** `count` keys up from `first`, whites first so blacks draw over them. */
export function keyboardKeys(first: number, count: number): { list: PianoKey[]; whites: number } {
  const list: PianoKey[] = [];
  let whites = 0;
  for (let note = first; note < first + count; note++) {
    const name = `${NAMES[note % 12]}${Math.floor(note / 12) - 2}`;
    if (isBlack(note)) list.push({ note, name, black: true, at: whites - 0.31 });
    else list.push({ note, name, black: false, at: whites++ });
  }
  list.sort((a, b) => Number(a.black) - Number(b.black));
  return { list, whites };
}
