// Settings › Audio › Processing Threads, kept pure so it can be tested. The engine says what this
// machine offers (`crates/engine/src/threads.rs`).

/** What the machine offers: what Automatic comes to here, and the highest number. */
export interface ThreadChoices {
  automatic: number;
  max: number;
}

/** The setting's value for Automatic. */
export const AUTOMATIC_THREADS = 0;

/** The menu: Automatic with what it means here, then the numbers. */
export function threadOptions(c: ThreadChoices): [number, string][] {
  const plural = (n: number) => `${n} ${n === 1 ? "thread" : "threads"}`;
  const options: [number, string][] = [[AUTOMATIC_THREADS, `Automatic (up to ${plural(c.automatic)})`]];
  options.push([1, "1 thread (the audio thread alone)"]);
  for (let n = 2; n <= c.max; n++) options.push([n, plural(n)]);
  return options;
}
