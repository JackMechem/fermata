// Dropping audio files into the project, from the desktop (native drag-and-drop) or from the
// All Files browser (an in-app drag). Both resolve the pointer to a target the same way:
//   - over the track headers: new tracks inserted there, starting at bar 1
//   - on an audio track's lane: imported there at the snapped position
//   - on an instrument lane or the empty area below: new audio tracks at that position
// MIDI files (.mid) work the same way with instrument tracks in place of audio tracks.
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { call, type Id, type Snapshot } from "./api";
import { indexAt, trackLayout, autoLayout } from "./layout";
import { importMidiFiles, isMidiPath } from "./midiFiles";
import { getState, setState } from "./store";
import { samplesPerBeat, snapPos } from "./time";
import { dropOnSampler, hoverSampler, samplerAt } from "./samplerDrop";

export type DropTarget =
  | { kind: "lane"; track: Id; index: number; sample: number }
  | { kind: "new"; index: number; sample: number }
  | { kind: "headers"; index: number };

/** What the engine can decode (crates/engine/src/decode.rs, AUDIO_EXTENSIONS). */
export const AUDIO_EXTENSIONS = ["wav", "wave", "aif", "aiff", "aifc", "flac", "mp3", "ogg", "oga", "m4a", "mp4", "aac", "caf"];
const AUDIO_EXT = new RegExp(`\\.(${AUDIO_EXTENSIONS.join("|")})$`, "i");

export function isAudioPath(p: string): boolean {
  return AUDIO_EXT.test(p);
}

function layoutNow() {
  const s = getState();
  return trackLayout(s.snap?.project.tracks ?? [], s.trackHeight, s.trackHeights, s.showHidden, s.openFolders, autoLayout());
}

/** Only MIDI files: they land on instrument lanes rather than audio ones. */
const midiOnly = (paths: string[]) => paths.some(isMidiPath) && !paths.some(isAudioPath);
const droppable = (paths: string[]) => paths.some((p) => isAudioPath(p) || isMidiPath(p));

/** Maps a pointer position (CSS pixels, viewport-relative) to a drop target, or null. */
export function resolveDrop(cx: number, cy: number, midi = false): DropTarget | null {
  const s = getState();
  const project = s.snap?.project;
  if (!project) return null;
  const layout = layoutNow();
  const n = project.tracks.length;

  const headers = document.querySelector(".headers") as HTMLElement | null;
  const hr = headers?.getBoundingClientRect();
  if (hr && cx >= hr.left && cx <= hr.right && cy >= hr.top && cy <= hr.bottom) {
    const scrollTop = (document.querySelector(".arrange-scroll") as HTMLElement | null)?.scrollTop ?? 0;
    const y = cy - hr.top + scrollTop;
    // Insert before the first track whose middle is below the pointer.
    const index = layout.tops.filter((t, i) => t + layout.heights[i] / 2 < y).length;
    return { kind: "headers", index };
  }

  const arrange = document.querySelector(".arrange-scroll") as HTMLElement | null;
  const ar = arrange?.getBoundingClientRect();
  if (!arrange || !ar || cx < ar.left || cx > ar.right || cy < ar.top || cy > ar.bottom) return null;
  const pps = s.pxPerBeat / samplesPerBeat(project);
  const x = cx - ar.left + arrange.scrollLeft;
  const y = cy - ar.top + arrange.scrollTop;
  const sample = Math.max(0, Math.round(snapPos(project, x / pps, s.snapMode, s.pxPerBeat)));
  if (n === 0 || y >= layout.total) return { kind: "new", index: n, sample };
  const index = indexAt(layout, y);
  const t = project.tracks[index];
  if (t.kind === (midi ? "instrument" : "audio")) return { kind: "lane", track: t.id, index, sample };
  return { kind: "new", index: index + 1, sample };
}

let lastKey = "";

/** Updates the highlighted drop target while dragging (null clears it). */
export function hoverDrop(target: DropTarget | null) {
  const key = JSON.stringify(target);
  if (key === lastKey) return;
  lastKey = key;
  setState({ dropTarget: target });
}

/** Imports `paths` at the target. Non-audio files are skipped with a notice. */
export async function performDrop(paths: string[], target: DropTarget | null) {
  hoverDrop(null);
  if (!target) return;
  const audio = paths.filter(isAudioPath);
  const midi = paths.filter(isMidiPath);
  const skipped = paths.length - audio.length - midi.length;
  if (!audio.length && !midi.length) {
    setState({ toast: { text: "Only audio files (WAV, AIFF, CAF, FLAC, MP3, OGG, M4A, MP4, AAC) and MIDI files can be dropped here", at: Date.now(), ms: 5000 } });
    return;
  }
  if (midi.length) {
    const lane = target.kind === "lane" && getState().snap?.project.tracks.find((t) => t.id === target.track)?.kind === "instrument";
    await importMidiFiles(midi, lane ? target.track : null, target.kind === "headers" ? 0 : target.sample, lane ? null : target.index);
    if (!audio.length) return;
  }
  const args =
    target.kind === "lane"
      ? { paths: audio, track: target.track, start: target.sample, index: null }
      : { paths: audio, track: null, start: target.kind === "new" ? target.sample : 0, index: target.index };
  setState({ toast: { text: audio.length > 1 ? `Importing ${audio.length} files…` : "Importing…", at: Date.now(), ms: 20000 } });
  const res = await call<{ snapshot: Snapshot; errors: string[] }>("import_audio_files", args);
  if (!res) return;
  setState({ snap: res.snapshot });
  const notes = [...res.errors];
  if (skipped) notes.push(`${skipped} non-audio file${skipped > 1 ? "s" : ""} skipped`);
  const done = audio.length - res.errors.length;
  setState({
    toast: {
      text: notes.length ? `Imported ${done} of ${audio.length}. ${notes.join(" · ")}` : `Imported ${done} file${done > 1 ? "s" : ""}`,
      at: Date.now(),
      ms: notes.length ? 8000 : 2500,
    },
  });
}

// ---- native drops from the desktop -------------------------------------------------------

/** Native drop positions arrive in webview pixels; convert to CSS pixels. */
function toCss(p: { x: number; y: number }): [number, number] {
  // WebKitGTK reports GTK logical pixels (page zoom aside); other webviews report physical.
  const f = navigator.userAgent.includes("Linux") ? getState().uiScale : window.devicePixelRatio;
  return [p.x / f, p.y / f];
}

export function installNativeDrop(): () => void {
  let paths: string[] = [];
  const un = getCurrentWebview().onDragDropEvent((e) => {
    const ev = e.payload;
    if (ev.type === "enter") {
      paths = ev.paths;
      hoverSampler(...toCss(ev.position));
      if (!droppable(paths)) return;
      hoverDrop(resolveDrop(...toCss(ev.position), midiOnly(paths)));
    } else if (ev.type === "over") {
      hoverSampler(...toCss(ev.position));
      if (droppable(paths)) hoverDrop(resolveDrop(...toCss(ev.position), midiOnly(paths)));
    } else if (ev.type === "drop") {
      hoverSampler(null, null);
      const at = samplerAt(...toCss(ev.position));
      if (at) {
        hoverDrop(null);
        dropOnSampler(at.track, ev.paths, at.key);
      } else {
        performDrop(ev.paths, resolveDrop(...toCss(ev.position), midiOnly(ev.paths)));
      }
      paths = [];
    } else {
      paths = [];
      hoverSampler(null, null);
      hoverDrop(null);
    }
  });
  return () => {
    un.then((f) => f());
  };
}

// ---- in-app drags (All Files browser) -------------------------------------------------------

/** Starts dragging `path` from a mousedown; shows a label under the pointer. */
export function startFileDrag(e: React.MouseEvent, path: string, label: string) {
  if (e.button !== 0) return;
  const sx = e.clientX;
  const sy = e.clientY;
  let ghost: HTMLDivElement | null = null;
  const move = (ev: MouseEvent) => {
    if (!ghost) {
      if (Math.hypot(ev.clientX - sx, ev.clientY - sy) < 4) return;
      ghost = document.createElement("div");
      ghost.className = "drag-ghost";
      ghost.textContent = label;
      document.body.appendChild(ghost);
      document.body.style.cursor = "copy";
    }
    ghost.style.transform = `translate(${ev.clientX + 12}px, ${ev.clientY + 10}px)`;
    hoverSampler(ev.clientX, ev.clientY);
    hoverDrop(resolveDrop(ev.clientX, ev.clientY, midiOnly([path])));
  };
  const up = (ev: MouseEvent) => {
    window.removeEventListener("mousemove", move);
    window.removeEventListener("mouseup", up);
    document.body.style.cursor = "";
    if (ghost) {
      ghost.remove();
      hoverSampler(null, null);
      const at = samplerAt(ev.clientX, ev.clientY);
      if (at) {
        hoverDrop(null);
        dropOnSampler(at.track, [path], at.key);
      } else {
        performDrop([path], resolveDrop(ev.clientX, ev.clientY, midiOnly([path])));
      }
    }
  };
  window.addEventListener("mousemove", move);
  window.addEventListener("mouseup", up);
}
