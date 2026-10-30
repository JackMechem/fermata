import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { act, api, call, type InputLevel, type PluginDesc, type Snapshot, type Tick } from "./api";
import { ControlBar } from "./components/ControlBar";
import { GroupSheet, NewTrackDialog, SettingsDialog } from "./components/Dialogs";
import { BounceDialog, BounceInPlaceDialog } from "./components/Bounce";
import { isBouncing, openBounce } from "./bounce";
import * as ops from "./ops";
import { ProjectChooser } from "./components/ProjectChooser";
import { ProjectSettingsSheet, RecoverySheet } from "./components/Settings";
import { commandForEvent } from "./keymap";
import { installPluginKeys } from "./pluginKeys";
import { loadSettings, nudgeSamples, offerRecoveryFor, runStartup } from "./prefs";
import { toggleZeroCrossings } from "./zeroCrossings";
import { movedStart, nudgeTarget } from "./musicalMove";
import { tempoMap } from "./tempo";
import { restoreAppearance, useAppearancePersistence } from "./appearance";
import { Editor } from "./components/Editor";
import { EventList, eventListMenu } from "./components/EventList";
import { StepSequencer } from "./components/StepSequencer";
import { FxWindows } from "./components/fx/FxWindow";
import { FxDock } from "./components/fx/FxDock";
import { loadFxList } from "./fxHost";
import { MusicalTyping } from "./components/MusicalTyping";
import { StepInput } from "./components/StepInput";
import { MenuHost } from "./components/Menu";
import { loadPaneSizes, Splitter } from "./components/Splitter";
import { Browser, Inspector, Library, Mixer } from "./components/Panels";
import { TracksArea, editMenu } from "./components/TracksArea";
import { toggleOverview } from "./components/Overview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { confirmDiscard, importAudioDialog, openProjectDialog, saveProject, undoRedo } from "./files";
import { currentTick, getState, pushTick, setState, useStore } from "./store";
import { transport } from "./transport";
import { applyTheme } from "./themes";
import { applyUiScale, stepUiScale } from "./uiScale";
import { currentLayout, loadHeights, saveHeights, stepVisible, zoomTracks } from "./layout";
import { installNativeDrop } from "./dnd";
import { newAux } from "./routing";
import { editorMenuCommand } from "./audioEdit";
import { deleteSelectedPoints, showUsedAutomation } from "./automation";
import { deleteSelectedTempoEvent } from "./components/GlobalTracks";
import { pianoRollMenu } from "./components/PianoRoll";
import { exportSelectionMidi, importMidiDialog } from "./midiFiles";
import { freezeTracks, unfreezeTracks } from "./freeze";
import { PdcStatus } from "./components/Latency";
import { marqueeCopy, marqueeCut, marqueeDelete, marqueePaste, openToolMenu } from "./trackTools";
import { getSettings, updateSettings } from "./prefs";
import { createMarker, deleteMarker, deleteSelectedMarker, gotoMarker, markerAtPlayhead, openMarkerList } from "./markers";
import { MarkerList } from "./components/MarkerList";
import { packTakeFolder } from "./takes";
import { installDetach, receiveSnapshot, toggleDetached } from "./detach";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { moveAutomationAnswer } from "./moveAutomation";
import { resetClipIndicators } from "./stripSettings";
import { learnLastTouched, openControllerAssignments } from "./learn";
import { ControllerAssignmentsSheet, LearnBanner } from "./components/ControllerAssignments";
import { newAlternative, revertToBackup, revertToSaved, saveAsTemplate, switchAlternative, useProjectMenus } from "./projectFiles";
import { deleteTrackAlternative, newTrackAlternative, renameTrackAlternative, trackForAlternatives } from "./trackAlternatives";
import { NameSheetHost } from "./components/NameSheet";
import { AlternativesSheet } from "./components/AlternativesSheet";

function selectedTrack() {
  const s = getState().snap;
  return s?.project.tracks.find((t) => t.id === s.selected) ?? null;
}

function moveSelection(d: number) {
  const s = getState().snap;
  if (!s || !s.project.tracks.length) return;
  const i = s.project.tracks.findIndex((t) => t.id === s.selected);
  // Step over tracks that aren't in view (hidden, or inside a collapsed stack).
  const l = currentLayout();
  const from = i < 0 ? stepVisible(l, -1, 1) : i;
  const n = i < 0 ? from : stepVisible(l, from, d);
  if (n < 0 || l.heights[n] === 0) return;
  api.selectTrack(s.project.tracks[n].id);
}

function handleMenu(id: string) {
  // With a range selected in the Audio Track editor, Split, Cut, Copy and Delete act on it.
  if (editorMenuCommand(id)) return;
  const s = getState();
  // A render holds the session; commands sent now would stall the window until it finishes.
  if (isBouncing()) return;
  // Edit commands act on the notes while the Piano Roll has focus.
  if (pianoRollMenu(id)) return;
  if (eventListMenu(id)) return;
  switch (id) {
    case "new":
      setState({ dialog: "chooser", chooserTab: "new" });
      break;
    case "undo":
      undoRedo("undo");
      break;
    case "redo":
      undoRedo("redo");
      break;
    case "musical-typing":
      setState({ showTyping: !getState().showTyping });
      break;
    case "step-input":
      setState({ showStepInput: !getState().showStepInput });
      break;
    case "chooser-recent":
      setState({ dialog: "chooser", chooserTab: "recent" });
      break;
    case "open":
      openProjectDialog();
      break;
    case "save":
      saveProject(false);
      break;
    case "save-as":
      saveProject(true);
      break;
    case "save-template":
      saveAsTemplate();
      break;
    case "revert-saved":
      revertToSaved();
      break;
    case "alt-new":
      newAlternative();
      break;
    case "alt-edit":
      setState({ dialog: "alternatives" });
      break;
    case "track-alt-new":
    case "track-alt-duplicate": {
      const t = trackForAlternatives();
      if (t) newTrackAlternative(t, id === "track-alt-duplicate");
      break;
    }
    case "track-alt-rename": {
      const t = trackForAlternatives();
      if (t) renameTrackAlternative(t);
      break;
    }
    case "track-alt-delete": {
      const t = trackForAlternatives();
      if (t) deleteTrackAlternative(t);
      break;
    }
    case "import-audio":
      importAudioDialog();
      break;
    case "bounce":
      openBounce("project");
      break;
    case "export-tracks":
      openBounce("tracks");
      break;
    case "export-selected-tracks": {
      const ids = ops.selectedTracks();
      if (ids.length) openBounce("selected", null, ids);
      else setState({ toast: { text: "Select the tracks to export first", at: Date.now(), ms: 2500 } });
      break;
    }
    case "bounce-in-place":
      ops.bounceInPlace();
      break;
    case "freeze-track":
      freezeTracks(ops.selectedTracks());
      break;
    case "unfreeze-track":
      unfreezeTracks(ops.selectedTracks());
      break;
    case "low-latency": {
      const on = !getSettings().prefs.latency.low_latency;
      updateSettings((st) => ({ ...st, prefs: { ...st.prefs, latency: { ...st.prefs.latency, low_latency: on } } }));
      setState({ toast: { text: `Low Latency Mode ${on ? "on" : "off"}`, at: Date.now(), ms: 1500 } });
      break;
    }
    case "import-midi":
      importMidiDialog();
      break;
    case "export-midi":
      exportSelectionMidi();
      break;
    case "settings":
    case "settings-audio":
      setState({ dialog: "settings", settingsTab: "audio" });
      break;
    case "settings-appearance":
      setState({ dialog: "settings", settingsTab: "appearance" });
      break;
    case "settings-keys":
      setState({ dialog: "settings", settingsTab: "keys" });
      break;
    case "project-settings":
      setState({ dialog: "project-settings" });
      break;
    case "new-tracks":
      setState({ dialog: "new-tracks" });
      break;
    case "new-aux":
      newAux();
      break;
    case "delete-track":
      ops.deleteTracks();
      break;
    case "duplicate":
      ops.duplicate();
      break;
    case "duplicate-track":
      ops.duplicateTracks(true);
      break;
    case "same-settings":
      ops.duplicateTracks(false);
      break;
    case "new-pattern-region":
      ops.newPatternRegion();
      break;
    case "delete-unused":
      ops.deleteUnusedTracks();
      break;
    case "folder-stack":
      ops.createStack(false);
      break;
    case "summing-stack":
      ops.createStack(true);
      break;
    case "group":
      ops.groupTracks();
      break;
    case "cut":
      if (!marqueeCut()) ops.cutRegions();
      break;
    case "copy":
      if (!marqueeCopy()) ops.copyRegions();
      break;
    case "paste":
      if (!marqueePaste()) ops.pasteRegions();
      break;
    case "repeat":
      ops.repeatRegions(1);
      break;
    case "join":
      ops.joinRegions();
      break;
    case "set-locators":
      ops.setLocatorsByRegions();
      break;
    case "zero-crossings":
      toggleZeroCrossings();
      break;
    case "loop":
      ops.toggleLoop();
      break;
    case "convert-loops":
      ops.convertLoops();
      break;
    case "hide-tracks":
      ops.hideTracks();
      break;
    case "unhide-all":
      ops.unhideAllTracks();
      break;
    case "show-hidden":
      ops.toggleShowHidden();
      break;
    case "split":
      act("split_regions", { pos: currentTick()?.pos ?? 0, ids: s.selectedRegions });
      break;
    case "select-all":
      ops.selectAllRegions();
      break;
    case "delete":
      editMenu().find((m) => m.label === "Delete")?.onClick?.();
      break;
    case "record":
      act("record");
      break;
    case "play":
      act("toggle_play");
      break;
    case "metronome":
      act("set_metronome", { on: !s.snap?.project.metronome });
      break;
    case "count-in":
      act("set_count_in", { on: !s.snap?.project.count_in });
      break;
    case "cycle":
      transport.toggleCycle();
      break;
    case "autopunch":
      transport.togglePunch();
      break;
    case "create-marker":
      createMarker();
      break;
    case "delete-marker": {
      const m = markerAtPlayhead();
      if (m) deleteMarker(m.id);
      break;
    }
    case "previous-marker":
      gotoMarker(-1);
      break;
    case "next-marker":
      gotoMarker(1);
      break;
    case "marker-list":
      openMarkerList();
      break;
    case "learn-assignment":
      learnLastTouched();
      break;
    case "controller-assignments":
      openControllerAssignments();
      break;
    case "pack-takes":
      packTakeFolder();
      break;
    case "mixer":
      setState({ bottom: s.bottom === "mixer" ? "none" : "mixer" });
      break;
    case "inspector":
      setState({ showInspector: !s.showInspector });
      break;
    case "library":
      setState({ showLibrary: !s.showLibrary });
      break;
    case "editor":
      setState({ bottom: s.bottom === "editor" ? "none" : "editor" });
      break;
    case "list-editors":
      setState({ bottom: s.bottom === "list" ? "none" : "list" });
      break;
    case "browser":
      setState({ showBrowser: !s.showBrowser });
      break;
    case "automation":
      setState({ showAutomation: !s.showAutomation });
      break;
    case "used-automation":
      showUsedAutomation();
      break;
    case "output-track":
      setState({ showOutput: !s.showOutput });
      break;
    case "global-tracks":
      setState({ showGlobalTracks: !s.showGlobalTracks });
      break;
    case "overview":
      toggleOverview();
      break;
    case "ui-larger":
      stepUiScale(1);
      break;
    case "ui-smaller":
      stepUiScale(-1);
      break;
    case "ui-actual":
      applyUiScale(1);
      break;
    default:
      if (id.startsWith("theme:")) applyTheme(id.slice(6));
      else if (id.startsWith("detach:")) toggleDetached(id.slice(7));
      else if (id.startsWith("alternative:")) switchAlternative(id.slice(12));
      else if (id.startsWith("revert:")) revertToBackup(id.slice(7));
      break;
    case "zoom-in":
      setState({ pxPerBeat: Math.min(400, s.pxPerBeat * 1.4) });
      break;
    case "zoom-out":
      setState({ pxPerBeat: Math.max(2, s.pxPerBeat / 1.4) });
      break;
  }
}

/** What each key command does (see keymap.ts for the commands and their keys). */
const KEY_ACTIONS: Record<string, () => void> = {
  play: () => act("toggle_play"),
  record: () => act("record"),
  cycle: () => transport.toggleCycle(),
  metronome: () => handleMenu("metronome"),
  "count-in": () => handleMenu("count-in"),
  "to-start": () => transport.toStart(),
  rewind: () => transport.nudgeBars(-1),
  forward: () => transport.nudgeBars(1),
  "catch-playhead": () => setState({ catchPlayhead: !getState().catchPlayhead }),
  "show-playhead": () => transport.showPlayhead(),
  mixer: () => handleMenu("mixer"),
  inspector: () => handleMenu("inspector"),
  library: () => handleMenu("library"),
  editor: () => handleMenu("editor"),
  "list-editors": () => handleMenu("list-editors"),
  browser: () => handleMenu("browser"),
  "zoom-in": () => handleMenu("zoom-in"),
  "zoom-out": () => handleMenu("zoom-out"),
  // Logic: ⌘↓ zooms in vertically (taller tracks), ⌘↑ zooms out.
  "taller-tracks": () => zoomTracks(1.2),
  "shorter-tracks": () => zoomTracks(1 / 1.2),
  "track-mute": () => {
    const t = selectedTrack();
    if (t) api.updateTrack(t.id, { mute: !t.mute });
  },
  "track-solo": () => {
    const t = selectedTrack();
    if (t) api.updateTrack(t.id, { solo: !t.solo });
  },
  "track-arm": () => {
    const t = selectedTrack();
    if (t) api.updateTrack(t.id, { armed: !t.armed });
  },
  "select-previous": () => moveSelection(-1),
  "select-next": () => moveSelection(1),
  // Clipboard keys stay with text fields (onKey skips them), so the menu bar doesn't take them.
  // With a marquee they act on the part of the regions inside it.
  copy: () => marqueeCopy() || ops.copyRegions(),
  cut: () => marqueeCut() || ops.cutRegions(),
  paste: () => marqueePaste() || ops.pasteRegions(),
  "mute-regions": () => ops.toggleRegionMute(),
  "delete-regions": () => {
    // Selected automation points (or a tempo change) go first, as in Logic.
    if (deleteSelectedPoints() || deleteSelectedTempoEvent() || deleteSelectedMarker() || marqueeDelete()) return;
    if (getState().selectedRegions.length) ops.deleteRegions();
  },
  "set-locators": () => ops.setLocatorsByRegions(),
  "zero-crossings": () => handleMenu("zero-crossings"),
  loop: () => ops.toggleLoop(),
  "show-hidden": () => ops.toggleShowHidden(),
  "hide-tracks": () => ops.hideTracks(),
  "reset-clips": () => resetClipIndicators(null),
  automation: () => handleMenu("automation"),
  "used-automation": () => handleMenu("used-automation"),
  "global-tracks": () => handleMenu("global-tracks"),
  "nudge-left": () => nudgeRegions(-1),
  "nudge-right": () => nudgeRegions(1),
  "project-settings": () => handleMenu("project-settings"),
  "settings-keys": () => handleMenu("settings-keys"),
  autopunch: () => handleMenu("autopunch"),
  "create-marker": () => handleMenu("create-marker"),
  "delete-marker": () => handleMenu("delete-marker"),
  "previous-marker": () => handleMenu("previous-marker"),
  "next-marker": () => handleMenu("next-marker"),
  "marker-list": () => handleMenu("marker-list"),
  "pack-takes": () => handleMenu("pack-takes"),
  "tool-menu": openToolMenu,
  "learn-assignment": () => handleMenu("learn-assignment"),
  "controller-assignments": () => handleMenu("controller-assignments"),
  "step-input": () => handleMenu("step-input"),
};

function nudgeRegions(dir: number) {
  const s = getState();
  if (!s.snap || !s.selectedRegions.length) return;
  // A bar or beat as long as it is where the selection starts.
  const at = Math.min(...s.snap.project.tracks.flatMap((t) => t.regions.filter((r) => s.selectedRegions.includes(r.id)).map((r) => r.start)));
  const ids = s.selectedRegions;
  const delta = Math.round(dir * nudgeSamples(s.snap.project, Number.isFinite(at) ? at : 0));
  // By a bar, beat or note value the first region moves through the tempo map and the others
  // keep their bars and beats; by milliseconds they all move the same number of samples.
  const map = tempoMap(s.snap.project);
  const to = Number.isFinite(at) ? nudgeTarget(map, at, getSettings().ui.nudge, dir) : null;
  const moves = s.snap.project.tracks.flatMap((t) =>
    t.regions.filter((r) => ids.includes(r.id)).map((r) => ({ id: r.id, start: to === null ? Math.max(0, r.start + delta) : movedStart(map, r.start, at, to, true) })),
  );
  const musical = to === null ? {} : { from: at, to };
  moveAutomationAnswer(s.snap.project, moves).then((automation) => act("nudge_regions", { ids, delta, automation, ...musical }));
}

function onKey(e: KeyboardEvent) {
  const tgt = e.target as HTMLElement;
  if (tgt && (tgt.tagName === "INPUT" || tgt.tagName === "SELECT" || tgt.tagName === "TEXTAREA")) return;
  if (getState().dialog) return;
  // Shortcuts the native menu owns never reach here; everything else comes from the keymap.
  const id = commandForEvent(e);
  const run = id ? KEY_ACTIONS[id] : undefined;
  if (!run) return;
  run();
  e.preventDefault();
}

export function Toast() {
  const toast = useStore((s) => s.toast);
  const [visible, setVisible] = useState(false);
  useEffect(() => {
    if (!toast) return;
    setVisible(true);
    const t = window.setTimeout(() => setVisible(false), toast.ms ?? 3500);
    return () => window.clearTimeout(t);
  }, [toast]);
  if (!toast || !visible) return null;
  return <div className="toast">{toast.text}</div>;
}

function StatusLine() {
  const audio = useStore((s) => s.snap?.audio);
  const midi = useStore((s) => s.snap?.midi_inputs) ?? [];
  const name = useStore((s) => s.snap?.project.name);
  const dirty = useStore((s) => s.snap?.dirty);
  return (
    <div className="status-line">
      <span>
        {name}
        {dirty ? " — Edited" : ""}
      </span>
      <span className="spacer" />
      <PdcStatus />
      <span className={audio?.running ? "" : "bad"} onClick={() => setState({ dialog: "settings", settingsTab: "audio" })} title="Audio settings">
        {audio?.running
          ? `${audio.input_name ?? "No input"} → ${audio.output_name} · ${(audio.sample_rate / 1000).toFixed(1)} kHz · ${audio.buffer_size}`
          : `Audio stopped${audio?.error ? ` — ${audio.error}` : ""}`}
      </span>
      <span title={midi.join("\n")}>MIDI: {midi.length ? midi.length + " input" + (midi.length > 1 ? "s" : "") : "none"}</span>
    </div>
  );
}

/** Remembers track heights per project folder. */
function useHeightPersistence() {
  const path = useStore((s) => s.snap?.path);
  const base = useStore((s) => s.trackHeight);
  const heights = useStore((s) => s.trackHeights);
  const output = useStore((s) => s.showOutput);
  const [loaded, setLoaded] = useState<string | null>(null);
  useEffect(() => {
    if (path && path !== loaded) {
      loadHeights(path);
      setLoaded(path);
    }
  }, [path, loaded]);
  useEffect(() => {
    if (path && path === loaded) saveHeights(path);
  }, [path, loaded, base, heights, output]);
}

export default function App() {
  // Only the layout-level flags: panes re-render themselves on the state they use.
  const s = {
    snap: useStore((s) => s.snap !== null),
    showLibrary: useStore((s) => s.showLibrary),
    showInspector: useStore((s) => s.showInspector),
    showBrowser: useStore((s) => s.showBrowser),
    bottom: useStore((s) => s.bottom),
    dialog: useStore((s) => s.dialog),
  };
  useHeightPersistence();
  useAppearancePersistence();
  useProjectMenus();
  const showTyping = useStore((s) => s.showTyping);
  const showStepInput = useStore((s) => s.showStepInput);
  const groupSheet = useStore((s) => s.groupSheet);
  const showMarkerList = useStore((s) => s.showMarkerList);
  const showControllerAssignments = useStore((s) => s.showControllerAssignments);
  const panes = useStore((s) => s.paneSizes);
  useEffect(loadPaneSizes, []);
  useEffect(() => {
    act("get_state").then(async (snap) => {
      await loadSettings();
      restoreAppearance();
      // Launched without a project: recover lost work, or open the last project or the Project
      // Chooser, as Settings › General says.
      if (snap && !snap.saved && snap.project.tracks.length === 0) await runStartup();
      // A project from the command line: offer its autosave (a dirty one is this session's own).
      else if (snap?.saved && !snap.dirty) await offerRecoveryFor(snap.path);
      return call<InputLevel | null>("input_level").then((l) => {
        if (l?.attenuating) {
          setState({
            toast: {
              text: `${l.device} input is ${l.muted ? "muted" : `at ${Math.round(l.volume * 100)}%`} in the system mixer (${l.muted ? "silent" : `${Math.round(l.gain_db)} dB`}). Fix it in Settings › Audio.`,
              at: Date.now(),
              ms: 10000,
            },
          });
        }
      });
    });
    call<PluginDesc[]>("list_plugins").then((p) => p && setState({ plugins: p }));
    loadFxList();
    const offs = [
      listen<Tick>("tick", (e) => pushTick(e.payload)),
      listen<Snapshot>("project", (e) => receiveSnapshot(e.payload)),
      // On this window only: menu items for a focused panel window go there (windows.rs), and a
      // plain listen() would hear those too.
      getCurrentWebviewWindow().listen<string>("menu", (e) => handleMenu(e.payload)),
      // Keys a plug-in's editor window didn't use (pluginKeys.ts).
      installPluginKeys(),
      getCurrentWindow().onCloseRequested(async (e) => {
        if (!(await confirmDiscard("Quit"))) e.preventDefault();
      }),
    ];
    window.addEventListener("keydown", onKey);
    const offDrop = installNativeDrop();
    // Detached panel windows: their key commands run here, as if pressed in this window.
    const offDetach = installDetach({
      key: (id) => {
        if (!getState().dialog) KEY_ACTIONS[id]?.();
      },
    });
    const ctx = (e: MouseEvent) => e.preventDefault();
    window.addEventListener("contextmenu", ctx);
    return () => {
      offs.forEach((p) => p.then((f) => f()));
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("contextmenu", ctx);
      offDrop();
      offDetach();
    };
  }, []);

  if (!s.snap) return <div className="loading">Starting audio engine…</div>;

  return (
    <div className="app">
      <ControlBar />
      <div
        className="main"
        style={
          {
            "--library-w": `${panes.library}px`,
            "--inspector-w": `${panes.inspector}px`,
            "--browser-w": `${panes.browser}px`,
            "--mixer-h": `${panes.mixer}px`,
            "--editor-h": `${panes.editor}px`,
            "--fx-h": `${panes.fx}px`,
          } as React.CSSProperties
        }
      >
        {s.showLibrary && <Library />}
        {s.showLibrary && <Splitter pane="library" dir="x" sign={1} />}
        {s.showInspector && <Inspector />}
        {s.showInspector && <Splitter pane="inspector" dir="x" sign={1} />}
        <div className="center">
          <TracksArea />
          {/* The Event List and the Step Sequencer take the Editors' height, so switching between them doesn't jump. */}
          {s.bottom !== "none" && <Splitter pane={s.bottom === "list" || s.bottom === "steps" ? "editor" : s.bottom} dir="y" sign={-1} />}
          {s.bottom === "mixer" && <Mixer />}
          {s.bottom === "editor" && <Editor />}
          {s.bottom === "list" && <EventList />}
          {s.bottom === "steps" && <StepSequencer />}
          {s.bottom === "fx" && <FxDock />}
        </div>
        {s.showBrowser && <Splitter pane="browser" dir="x" sign={-1} />}
        {s.showBrowser && <Browser />}
      </div>
      <StatusLine />
      {s.dialog === "new-tracks" && <NewTrackDialog />}
      {s.dialog === "settings" && <SettingsDialog />}
      {s.dialog === "chooser" && <ProjectChooser />}
      {s.dialog === "bounce" && <BounceDialog />}
      {s.dialog === "bounce-in-place" && <BounceInPlaceDialog />}
      {s.dialog === "project-settings" && <ProjectSettingsSheet />}
      {s.dialog === "recovery" && <RecoverySheet />}
      {s.dialog === "alternatives" && <AlternativesSheet />}
      {groupSheet !== null && <GroupSheet />}
      {showMarkerList && <MarkerList />}
      {showControllerAssignments && <ControllerAssignmentsSheet />}
      <LearnBanner />
      <FxWindows />
      {showTyping && <MusicalTyping />}
      {showStepInput && <StepInput />}
      <MenuHost />
      <NameSheetHost />
      <Toast />
    </div>
  );
}
