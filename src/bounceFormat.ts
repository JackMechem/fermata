// File formats and resolutions for bounces and exports (audio_file.rs in the engine): what
// the Bounce dialog offers for each format, and the file names it writes.

export type AudioFormat = "wav" | "aiff" | "flac";
export type BitDepth = "int16" | "int24" | "float32";

export const FORMATS: [AudioFormat, string][] = [
  ["wav", "WAVE"],
  ["aiff", "AIFF"],
  ["flac", "FLAC"],
];

const DEPTHS: [BitDepth, string][] = [
  ["int16", "16-bit (dithered)"],
  ["int24", "24-bit"],
  ["float32", "32-bit float"],
];

/** The resolutions a format can store: FLAC has no floating point. */
export function depthsFor(format: AudioFormat): [BitDepth, string][] {
  return format === "flac" ? DEPTHS.filter(([d]) => d !== "float32") : DEPTHS;
}

/** The resolution actually written: a remembered 32-bit float becomes 24-bit for FLAC. */
export function depthFor(format: AudioFormat, depth: BitDepth): BitDepth {
  return depthsFor(format).some(([d]) => d === depth) ? depth : "int24";
}

/** File name extension the engine uses, and the others each format is known by. */
const EXTENSIONS: Record<AudioFormat, string[]> = {
  wav: ["wav", "wave"],
  aiff: ["aif", "aiff", "aifc"],
  flac: ["flac"],
};

export const extensionOf = (format: AudioFormat) => EXTENSIONS[format][0];

/** Save-dialog filter for a format. */
export function fileFilter(format: AudioFormat): { name: string; extensions: string[] } {
  const label = FORMATS.find(([f]) => f === format)![1];
  return { name: `${label} audio`, extensions: EXTENSIONS[format] };
}

/** `path` with the format's extension: kept if it already has one of the format's, swapped if
 *  it has another audio format's (the user changed format after naming the file), else added. */
export function withExtension(path: string, format: AudioFormat): string {
  const m = /\.([A-Za-z0-9]+)$/.exec(path);
  const ext = m?.[1].toLowerCase();
  if (ext && EXTENSIONS[format].includes(ext)) return path;
  const known = Object.values(EXTENSIONS).flat();
  const stem = ext && known.includes(ext) ? path.slice(0, -m![0].length) : path;
  return `${stem}.${extensionOf(format)}`;
}
