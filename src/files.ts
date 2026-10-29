import { ask, open, save } from "@tauri-apps/plugin-dialog";
import { act, call, type Snapshot } from "./api";
import { getSettings, openProjectPath } from "./prefs";
import { currentTick, getState, setState } from "./store";

/** Asks before discarding unsaved changes. Resolves true when it is fine to continue. */
export async function confirmDiscard(action: string): Promise<boolean> {
  const snap = getState().snap;
  if (!snap?.dirty) return true;
  // With the question turned off in Settings › General, the autosave stays as a safety net.
  if (!getSettings().ui.confirm_unsaved) return true;
  const ok = await ask(`The project "${snap.project.name}" has unsaved changes. ${action} anyway and discard them?`, {
    title: "Unsaved Changes",
    kind: "warning",
    okLabel: "Don't Save",
    cancelLabel: "Cancel",
  });
  if (ok) await call("discard_autosave", { path: null });
  return ok;
}

/** Undoes (or redoes) one edit and says which. */
export async function undoRedo(which: "undo" | "redo") {
  const res = await call<{ snapshot: Snapshot; label: string | null }>(which);
  if (!res) return;
  const ids = new Set(res.snapshot.project.tracks.flatMap((t) => t.regions.map((r) => r.id)));
  setState({ snap: res.snapshot, selectedRegions: getState().selectedRegions.filter((id) => ids.has(id)) });
  setState({
    toast: {
      text: res.label ? `${which === "undo" ? "Undo" : "Redo"} ${res.label}` : which === "undo" ? "Nothing to undo" : "Nothing to redo",
      at: Date.now(),
      ms: 1800,
    },
  });
}

export async function openProjectDialog() {
  if (!(await confirmDiscard("Open another project"))) return;
  const dir = await open({ directory: true, title: "Open Project (select a .fermata folder)" });
  if (typeof dir === "string") await openProjectPath(dir);
}

export async function saveProject(as = false) {
  const snap = getState().snap;
  if (!snap) return;
  if (!as && snap.saved) {
    await act("save_project", { path: null });
    return;
  }
  const path = await save({ title: "Save Project", defaultPath: snap.path.replace(/\.fermata$/, "") + ".fermata" });
  if (path) await act("save_project", { path });
}

export async function importAudioDialog() {
  const snap = getState().snap;
  const track = snap?.project.tracks.find((t) => t.id === snap.selected && t.kind === "audio") ?? snap?.project.tracks.find((t) => t.kind === "audio");
  if (!track) {
    setState({ toast: { text: "Create or select an audio track first", at: Date.now() } });
    return;
  }
  const file = await open({ title: "Import Audio File", filters: [{ name: "WAV audio", extensions: ["wav", "wave"] }] });
  if (typeof file === "string") {
    await act("import_audio", { path: file, track: track.id, start: currentTick()?.pos ?? 0 });
  }
}
