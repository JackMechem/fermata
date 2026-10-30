// The panels that can be detached into windows of their own (detach.ts): the Mixer and the
// Editors from the bottom section, and the Channel EQ and built-in effect panels (fxDock.ts),
// docked or floating. main.tsx imports this before anything renders.
import { useEffect } from "react";
import { MASTER_ID, type Id, type Project, type Region, type Track } from "./api";
import { bottomPanel, registerPanelKind, type PanelKind } from "./detach";
import { panelKey } from "./detachModel";
import * as dock from "./fxDock";
import { closePanel, dockPatch, panelMode, savedPanelSize } from "./fxDockState";
import { CHANNEL_EQ, findFxSlot, fxInfo } from "./fxHost";
import type { BottomPane, State } from "./store";
import { getState } from "./store";
import { DetachButton } from "./components/DetachButton";
import { Editor } from "./components/Editor";
import { EventList, eventListTitle } from "./components/EventList";
import { StepSequencer, stepSequencerTitle } from "./components/StepSequencer";
import { FxWindows } from "./components/fx/FxWindow";
import { PanelContent, PanelLead, PanelTools, panelExists, usePanelTitle } from "./components/fx/FxPanel";
import { Mixer } from "./components/Panels";

const projectName = (s: State) => s.snap?.project.name || "Untitled";

const stripName = (project: Project | undefined, track: Id) => (track === MASTER_ID ? "Stereo Out" : project?.tracks.find((t) => t.id === track)?.name ?? "");

/**
 * A bottom pane's own window has no bottom section: it keeps showing its pane, and the effect
 * panels opened in it (a slot in a detached Mixer) float in it, since it has no dock.
 */
const ownBottom =
  (pane: BottomPane) =>
  (_: string, s: State): Partial<State> | null => {
    const docked = dock.dockedPanels(s.fxDock);
    if (s.bottom === pane && !docked.length) return null;
    return { bottom: pane, ...(docked.length ? { fxDock: { ...s.fxDock, floating: [...s.fxDock.floating, ...docked], active: null } } : {}) };
  };

registerPanelKind(
  "mixer",
  bottomPanel("mixer", {
    title: (_, s) => `Mixer — ${projectName(s)}`,
    render: () => (
      <>
        <Mixer />
        <FxWindows />
      </>
    ),
    size: { w: 1100, h: 560 },
    own: ownBottom("mixer"),
  }),
);

/** What the editor edits, found the way Editor.tsx finds it. */
function editorTitle(s: State): string {
  const project = s.snap?.project;
  let region: Region | null = null;
  let track: Track | null = null;
  for (const t of project?.tracks ?? []) {
    const r = t.regions.find((r) => r.id === s.selectedRegions[0]);
    if (r) {
      region = r;
      track = t;
    }
  }
  if (!track) track = project?.tracks.find((t) => t.id === s.snap?.selected) ?? null;
  if (track?.kind === "instrument") return `Piano Roll — ${region?.name ?? track.name}`;
  if (track?.kind === "audio") return `Audio Track Editor — ${track.name}`;
  return `Editor — ${projectName(s)}`;
}

registerPanelKind(
  "editor",
  bottomPanel("editor", {
    title: (_, s) => editorTitle(s),
    render: () => <Editor />,
    size: { w: 1100, h: 520 },
    own: ownBottom("editor"),
  }),
);

registerPanelKind(
  "list",
  bottomPanel("list", {
    title: (_, s) => eventListTitle(s),
    render: () => <EventList />,
    size: { w: 640, h: 520 },
    own: ownBottom("list"),
  }),
);

registerPanelKind(
  "steps",
  bottomPanel("steps", {
    title: (_, s) => stepSequencerTitle(s),
    render: () => <StepSequencer />,
    size: { w: 980, h: 420 },
    own: ownBottom("steps"),
  }),
);

/**
 * An effect panel or Channel EQ (fxDock.ts keys "fx:<uid>" and "eq:<track>"). It counts as shown
 * in a window while it is open there, docked or floating, so a window never has a copy of a
 * detached one. Attached again, it opens in the main window as Settings › General › Open Effect
 * Panels says (docked, or floating). Each type of effect remembers its own window's place and
 * size, and a first window takes the size the panel last had floating.
 */
function plugPanel(kind: "fx" | "eq", title: (arg: string, s: State) => string, size: PanelKind["size"]): PanelKind {
  const key = (arg: string) => panelKey(kind, arg);
  const type = (arg: string, s: State) => (kind === "eq" ? CHANNEL_EQ : (findFxSlot(s.snap?.project, Number(arg))?.plugin.plugin_id ?? null));
  return {
    memoryKey: (arg, s) => `${kind}:${type(arg, s) ?? ""}`,
    sizeOf: (arg, s) => {
      const t = type(arg, s);
      return (t && savedPanelSize(t)) || { w: (t && dock.DEFAULT_WIDTH[t]) || size.w, h: size.h };
    },
    title,
    render: (arg) => <OwnPlugPanel panelKey={key(arg)} />,
    shown: (arg, s) => s.fxDock.open.includes(key(arg)),
    hide: (arg, s) => dockPatch(s, (d) => dock.closePanel(d, key(arg))) ?? {},
    show: (arg, s) => dockPatch(s, (d) => dock.openPanel(d, key(arg), panelMode())) ?? {},
    alive: (arg, s) => !!s.snap && panelExists(s.snap.project, key(arg)),
    size,
  };
}

registerPanelKind(
  "fx",
  plugPanel(
    "fx",
    (arg, s) => {
      const at = findFxSlot(s.snap?.project, Number(arg));
      const name = fxInfo(at?.plugin.plugin_id)?.name ?? at?.plugin.name ?? "Effect";
      return at ? `${name} — ${stripName(s.snap?.project, at.track)}` : name;
    },
    { w: 640, h: 440 },
  ),
);

registerPanelKind(
  "eq",
  plugPanel("eq", (arg, s) => `Channel EQ — ${stripName(s.snap?.project, Number(arg))}`, { w: 820, h: 520 }),
);

/** An effect panel or Channel EQ filling its own window, with the dock's bar above it. */
function OwnPlugPanel({ panelKey }: { panelKey: string }) {
  const title = usePanelTitle(panelKey);

  // Esc closes it, as it would docked or floating (FxWindows isn't in this window).
  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented || getState().dialog) return;
      const tgt = e.target as HTMLElement | null;
      if (tgt && ["INPUT", "SELECT", "TEXTAREA"].includes(tgt.tagName)) return;
      e.preventDefault();
      closePanel(panelKey);
    };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, [panelKey]);

  if (!title) return null;
  return (
    <div className="fx-dock panel-plug">
      <div className="local-bar fx-dock-bar">
        <div className="local-menus">
          <PanelLead panelKey={panelKey} />
          <span className="local-title">{title.name}</span>
          <span className="panel-plug-track">{title.track}</span>
        </div>
        <div className="fx-dock-tools">
          <PanelTools panelKey={panelKey} />
        </div>
        <div className="local-right">
          <DetachButton panel={panelKey} />
          <button className="eq-close fx-dock-close" title="Close (Esc)" onClick={() => closePanel(panelKey)}>
            ×
          </button>
        </div>
      </div>
      <PanelContent panelKey={panelKey} />
    </div>
  );
}
