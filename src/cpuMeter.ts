// The control bar's CPU meter, kept pure so it can be tested. The engine measures it (`BlockLoad`
// in crates/engine/src/dsp.rs) and sends it with each tick.

/** Where the peak tick goes. The average is smoothed over blocks, so with a long buffer it can
 *  still be coming down after the peak has fallen; a peak under the average would make no sense. */
export function shownPeak(cpu: number, peak: number): number {
  return Math.max(cpu, peak);
}

/** The meter's tooltip. `late` is the blocks over 100% since the meter was last clicked. */
export function cpuTitle(cpu: number, peak: number, late: number): string {
  const pct = (v: number) => `${(v * 100).toFixed(0)}%`;
  const lines = [
    "Audio load: how much of each buffer's time the audio thread uses. Over 100% means dropouts. This isn't the same as your system monitor's CPU figure, which is shared across all cores.",
    `Average ${pct(cpu)}, recent peak ${pct(peak)}.`,
  ];
  if (late > 0) lines.push(`${late} ${late === 1 ? "block" : "blocks"} over 100%. Click to clear.`);
  return lines.join("\n");
}
