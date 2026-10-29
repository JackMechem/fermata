// The root of a detached panel's window (main.tsx renders it instead of App in `panel-*`
// windows): the panel filling the window, plus the sheets and menus its controls can open.
// detach.ts keeps the window's state in step with the main window.
import { useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Toast } from "../App";
import { editorMenuCommand } from "../audioEdit";
import { installPanelWindow, ownPanel, panelKind } from "../detach";
import { parsePanelKey } from "../detachModel";
import { useStore } from "../store";
import { BounceDialog, BounceInPlaceDialog } from "./Bounce";
import { GroupSheet, NewTrackDialog, SettingsDialog } from "./Dialogs";
import { MenuHost } from "./Menu";
import { pianoRollMenu } from "./PianoRoll";
import { ProjectSettingsSheet } from "./Settings";

export function DetachedWindow() {
  const { kind, arg } = parsePanelKey(ownPanel ?? "");
  const k = panelKind(kind);
  const ready = useStore((s) => s.snap !== null);
  const title = useStore((s) => (k ? k.title(arg, s) : "Fermata"));
  const dialog = useStore((s) => s.dialog);
  const groupSheet = useStore((s) => s.groupSheet);

  // Edit commands from the menu bar act on this window's Piano Roll or Audio Track editor.
  useEffect(() => installPanelWindow({ menu: (id) => editorMenuCommand(id) || pianoRollMenu(id) }), []);

  useEffect(() => {
    document.title = title;
    getCurrentWindow()
      .setTitle(title)
      .catch(() => {});
  }, [title]);

  useEffect(() => {
    const ctx = (e: MouseEvent) => e.preventDefault();
    window.addEventListener("contextmenu", ctx);
    return () => window.removeEventListener("contextmenu", ctx);
  }, []);

  if (!k) return <div className="loading">Nothing to show here.</div>;
  if (!ready) return <div className="loading">Loading…</div>;
  return (
    <div className="panel-window">
      {k.render(arg)}
      {dialog === "new-tracks" && <NewTrackDialog />}
      {dialog === "settings" && <SettingsDialog />}
      {dialog === "bounce" && <BounceDialog />}
      {dialog === "bounce-in-place" && <BounceInPlaceDialog />}
      {dialog === "project-settings" && <ProjectSettingsSheet />}
      {groupSheet !== null && <GroupSheet />}
      <MenuHost />
      <Toast />
    </div>
  );
}
