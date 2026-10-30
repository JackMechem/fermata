// File › Project Alternatives, Revert to Saved, Revert to and Save as Template (Rust:
// crates/engine/src/session/{alternatives,backups,templates}.rs). The native menu's two lists
// are rebuilt from here (menu.rs `set_project_menus`) whenever the project changes in a way
// that can change them.
import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ask } from "@tauri-apps/plugin-dialog";
import { act, call, type ProjectFiles, type UserTemplate } from "./api";
import { backupLabel, freeName, nameProblem } from "./alternativesModel";
import { askName } from "./components/NameSheet";
import { saveProject } from "./files";
import { getSettings } from "./prefs";
import { getState, setState, useStore } from "./store";

const toast = (text: string) => setState({ toast: { text, at: Date.now(), ms: 3000 } });

/** Reads the alternatives and backups and puts them in the File menu. Returns them. */
export async function refreshProjectMenus(): Promise<ProjectFiles | null> {
  const files = await call<ProjectFiles>("project_files");
  if (!files) return null;
  await call("set_project_menus", {
    alternatives: getState().snap?.saved ? files.alternatives : [],
    active: files.active,
    backups: files.backups.map((b) => ({ file: b.file, label: backupLabel(b.time) })),
  });
  return files;
}

/** Keeps the File menu's lists current: on opening, saving, switching and renaming. */
export function useProjectMenus() {
  const path = useStore((s) => s.snap?.path);
  const saved = useStore((s) => s.snap?.saved);
  const dirty = useStore((s) => s.snap?.dirty);
  const alternative = useStore((s) => s.snap?.project.alternative);
  useEffect(() => {
    refreshProjectMenus();
  }, [path, saved, dirty, alternative]);
}

/** After an alternative or a backup replaced the open project. */
function replaced() {
  setState({ selectedRegions: [] });
  refreshProjectMenus();
}

/** Alternatives live in the project folder: a project never saved is saved first. */
async function ensureSaved(): Promise<boolean> {
  if (getState().snap?.saved) return true;
  await saveProject(true);
  return !!getState().snap?.saved;
}

/** File › Project Alternatives › New Alternative…: a copy of the current one, under a name. */
export async function newAlternative() {
  if (!(await ensureSaved())) return;
  const files = await call<ProjectFiles>("project_files");
  if (!files) return;
  const check = (n: string) => nameProblem(n) ?? (files.alternatives.some((a) => a.toLowerCase() === n.trim().toLowerCase()) ? `There's already an alternative named “${n.trim()}”.` : null);
  const name = await askName({ title: "New Alternative", label: "Name:", value: freeName(files.active, files.alternatives), ok: "Create", check });
  if (!name) return;
  if (await act("new_alternative", { name })) {
    toast(`Now working on the alternative “${name}”`);
    replaced();
  }
}

/** Switches to another alternative. Unsaved changes are saved first, after asking. */
export async function switchAlternative(name: string) {
  const snap = getState().snap;
  if (!snap || name === snap.project.alternative) return;
  if (snap.dirty && getSettings().ui.confirm_unsaved) {
    const cur = snap.project.alternative || snap.project.name;
    const ok = await ask(`“${cur}” has unsaved changes. Save them before switching to “${name}”?`, { title: "Project Alternatives", kind: "warning", okLabel: "Save", cancelLabel: "Cancel" });
    if (!ok) return;
  }
  if (await act("switch_alternative", { name })) replaced();
}

export async function renameAlternative(name: string, taken: string[]) {
  const check = (n: string) => nameProblem(n) ?? (taken.some((a) => a !== name && a.toLowerCase() === n.trim().toLowerCase()) ? `There's already an alternative named “${n.trim()}”.` : null);
  const newName = await askName({ title: "Rename Alternative", value: name, ok: "Rename", check });
  if (newName && newName !== name && (await act("rename_alternative", { name, newName }))) refreshProjectMenus();
}

export async function deleteAlternative(name: string) {
  const active = getState().snap?.project.alternative;
  const more = name === active ? " The project switches to another alternative, and this one's unsaved changes are lost too." : "";
  const ok = await ask(`Delete the alternative “${name}” and its backups? This can't be undone.${more}`, { title: "Project Alternatives", kind: "warning", okLabel: "Delete", cancelLabel: "Cancel" });
  if (ok && (await act("delete_alternative", { name }))) replaced();
}

/** File › Revert to Saved. */
export async function revertToSaved() {
  const snap = getState().snap;
  if (!snap?.saved) return toast("The project hasn't been saved yet");
  if (!snap.dirty) return toast("No changes since the last save");
  const ok = await ask(`Revert “${snap.project.name}” to its last saved version? The changes since then are lost.`, {
    title: "Revert to Saved",
    kind: "warning",
    okLabel: "Revert",
    cancelLabel: "Cancel",
  });
  if (ok && (await act("revert_to_saved"))) replaced();
}

/** File › Revert to › a backup (`file` as `project_files` lists it). */
export async function revertToBackup(file: string) {
  const files = await call<ProjectFiles>("project_files");
  const b = files?.backups.find((x) => x.file === file);
  if (!b) return toast("That backup no longer exists");
  const ok = await ask(
    `Revert to the version saved ${backupLabel(b.time).replace(/^(Today|Yesterday)/, (w) => w.toLowerCase())}? The project as it is now is saved as a backup, so you can revert to it again.`,
    { title: "Revert to", kind: "warning", okLabel: "Revert", cancelLabel: "Cancel" },
  );
  if (ok && (await act("revert_to_backup", { file }))) replaced();
}

/** File › Save as Template…: the tracks, routing, plug-ins and settings, without regions. */
export async function saveAsTemplate() {
  const snap = getState().snap;
  if (!snap) return;
  const name = await askName({ title: "Save as Template", label: "Template name:", value: snap.project.name, ok: "Save" });
  if (!name) return;
  const existing = (await call<UserTemplate[]>("user_templates")) ?? [];
  const replace = existing.some((t) => t.name.toLowerCase() === name.toLowerCase());
  if (replace) {
    const ok = await ask(`A template named “${name}” already exists. Replace it?`, { title: "Save as Template", kind: "warning", okLabel: "Replace", cancelLabel: "Cancel" });
    if (!ok) return;
  }
  // `call` returns null both for success (the command returns nothing) and failure.
  try {
    await invoke("save_template", { name, replace });
    toast(`Saved the template “${name}”. It's in the Project Chooser under My Templates.`);
  } catch (e) {
    toast(String(e));
  }
}
