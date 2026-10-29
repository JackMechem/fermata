import * as ops from "../ops";
import { Icon } from "../icons";
import { getState, setState, useStore, type Snap } from "../store";
import { MAX_TRACK_H, MIN_TRACK_H, zoomTracks } from "../layout";
import { RULER_H } from "../view";
import { Splitter } from "./Splitter";
import { Arrange } from "./Arrange";
import { openMenuAt, type MenuItem } from "./Menu";
import { Ruler } from "./Ruler";
import { TrackHeaders } from "./TrackHeaders";
import { importAudioDialog, undoRedo } from "../files";
import { isLooped } from "../loops";
import { exportSelectionMidi, importMidiDialog } from "../midiFiles";
import { fmt, keyHint } from "../keymap";
import { transport } from "../transport";
import { GlobalTracks, globalTracksHeight } from "./GlobalTracks";
import { Overview, toggleOverview } from "./Overview";
import { OVERVIEW_H } from "../overview";
import { act } from "../api";
import type { DragMode } from "../fades";

const SNAP_LABELS: Record<Snap, string> = { smart: "Smart", bar: "Bar", beat: "Beat", off: "Off" };
/** Logic's Drag pop-up; only Overlap and X-Fade so far. */
const DRAG_LABELS: Record<DragMode, string> = { overlap: "Overlap", xfade: "X-Fade" };

export function editMenu(): MenuItem[] {
  const sel = getState().selectedRegions;
  const snap = getState().snap;
  const regs = snap?.project.tracks.flatMap((t) => t.regions).filter((r) => sel.includes(r.id)) ?? [];
  return [
    { label: snap?.undo ? `Undo ${snap.undo}` : "Undo", hint: fmt("Mod+Z"), disabled: !snap?.undo, onClick: () => undoRedo("undo") },
    { label: snap?.redo ? `Redo ${snap.redo}` : "Redo", hint: fmt("Mod+Shift+Z"), disabled: !snap?.redo, onClick: () => undoRedo("redo") },
    { separator: true },
    { label: "Cut", hint: keyHint("cut"), disabled: !sel.length, onClick: () => ops.cutRegions() },
    { label: "Copy", hint: keyHint("copy"), disabled: !sel.length, onClick: () => ops.copyRegions() },
    { label: "Paste", hint: keyHint("paste"), disabled: !getState().clipboard.length, onClick: () => ops.pasteRegions() },
    { label: "Duplicate", hint: fmt("Mod+D"), onClick: ops.duplicate },
    { label: "Repeat", disabled: !sel.length, children: [1, 2, 3, 4, 8, 16].map((n) => ({ label: n === 1 ? "Once" : `${n} Times`, onClick: () => ops.repeatRegions(n) })) },
    { separator: true },
    { label: "Split Regions at Playhead", hint: fmt("Mod+T"), onClick: () => ops.splitAtPlayhead() },
    { label: "Join Regions", hint: fmt("Mod+J"), disabled: sel.length < 2, onClick: () => ops.joinRegions() },
    { label: "Loop Regions", hint: keyHint("loop"), disabled: !sel.length, checked: regs.length > 0 && regs.every(isLooped), onClick: () => ops.toggleLoop() },
    { label: "Convert Loops to Real Copies", disabled: !regs.some(isLooped), onClick: () => ops.convertLoops() },
    { label: "Set Locators by Regions", hint: keyHint("set-locators"), disabled: !sel.length, onClick: () => ops.setLocatorsByRegions() },
    { separator: true },
    { label: "Select All", hint: fmt("Mod+A"), onClick: ops.selectAllRegions },
    { label: "Delete", hint: keyHint("delete-regions"), disabled: !sel.length, onClick: () => ops.deleteRegions() },
  ];
}

export function TracksArea() {
  const pxPerBeat = useStore((s) => s.pxPerBeat);
  const catchPlayhead = useStore((s) => s.catchPlayhead);
  const snapMode = useStore((s) => s.snapMode);
  const trackHeight = useStore((s) => s.trackHeight);
  const headerW = useStore((s) => s.paneSizes.headers);
  const showHidden = useStore((s) => s.showHidden);
  const globalH = useStore(globalTracksHeight);
  const showOverview = useStore((s) => s.showOverview);
  const dragMode = useStore((s) => s.snap?.project.drag_mode) ?? "overlap";
  // Menus read the rest of the state when they open.
  const s = { ...getState(), pxPerBeat, catchPlayhead, snapMode, trackHeight };
  const zoom = Math.log(s.pxPerBeat / 2) / Math.log(200);

  const functionsMenu = (): MenuItem[] => [
    { label: "New Tracks…", hint: fmt("Mod+Alt+N"), onClick: () => setState({ dialog: "new-tracks" }) },
    { label: "Duplicate Track", onClick: () => ops.duplicateTracks(true) },
    { label: "New Track with Same Settings", onClick: () => ops.duplicateTracks(false) },
    { separator: true },
    { label: "Create Folder Stack", hint: fmt("Mod+Shift+D"), onClick: () => ops.createStack(false) },
    { label: "Create Summing Stack", onClick: () => ops.createStack(true) },
    { label: "Group Selected Tracks", hint: fmt("Mod+G"), onClick: () => ops.groupTracks() },
    { separator: true },
    { label: "Import Audio File…", hint: fmt("Mod+Shift+I"), onClick: importAudioDialog },
    { label: "Import MIDI File…", onClick: importMidiDialog },
    { label: "Export Selection as MIDI File…", onClick: exportSelectionMidi },
    { separator: true },
    { label: "Hide Track", hint: keyHint("hide-tracks"), onClick: () => ops.hideTracks() },
    { label: "Unhide All Tracks", disabled: !getState().snap?.project.tracks.some((t) => t.hidden), onClick: ops.unhideAllTracks },
    { separator: true },
    { label: "Delete Track", hint: fmt("Mod+Backspace"), onClick: () => ops.deleteTracks() },
    { label: "Delete Unused Tracks", onClick: ops.deleteUnusedTracks },
  ];
  const viewMenu = (): MenuItem[] => {
    const s = getState();
    return [
    { label: "Inspector", hint: keyHint("inspector"), checked: s.showInspector, onClick: () => setState({ showInspector: !s.showInspector }) },
    { label: "Library", hint: keyHint("library"), checked: s.showLibrary, onClick: () => setState({ showLibrary: !s.showLibrary }) },
    { label: "Mixer", hint: keyHint("mixer"), checked: s.bottom === "mixer", onClick: () => setState({ bottom: s.bottom === "mixer" ? "none" : "mixer" }) },
    { label: "Browsers", hint: keyHint("browser"), checked: s.showBrowser, onClick: () => setState({ showBrowser: !s.showBrowser }) },
    { label: "Show Automation", hint: keyHint("automation"), checked: s.showAutomation, onClick: () => setState({ showAutomation: !s.showAutomation }) },
    { label: "Show Global Tracks", hint: keyHint("global-tracks"), checked: s.showGlobalTracks, onClick: () => setState({ showGlobalTracks: !s.showGlobalTracks }) },
    { label: "Show Overview", checked: s.showOverview, onClick: toggleOverview },
    { separator: true },
    { label: "Show Hidden Tracks", hint: keyHint("show-hidden"), checked: s.showHidden, onClick: ops.toggleShowHidden },
    { separator: true },
    { label: "Zoom In", hint: keyHint("zoom-in"), onClick: () => setState({ pxPerBeat: Math.min(400, s.pxPerBeat * 1.4) }) },
    { label: "Zoom Out", hint: keyHint("zoom-out"), onClick: () => setState({ pxPerBeat: Math.max(2, s.pxPerBeat / 1.4) }) },
    { separator: true },
    { label: "Taller Tracks", hint: keyHint("taller-tracks"), onClick: () => zoomTracks(1.2) },
    { label: "Shorter Tracks", hint: keyHint("shorter-tracks"), onClick: () => zoomTracks(1 / 1.2) },
    {
      label: "Track Height",
      children: [
        ["Mini", 24],
        ["Compact", 36],
        ["Small", 52],
        ["Medium", 64],
        ["Large", 96],
        ["Extra Large", 150],
      ].map(([label, h]) => ({
        label: label as string,
        checked: s.trackHeight === h && Object.keys(s.trackHeights).length === 0,
        onClick: () => setState({ trackHeight: h as number, trackHeights: {} }),
      })),
    },
    ];
  };

  return (
    <div className="tracks-area">
      <div className="local-bar">
        <div className="local-menus">
          <button className="local-menu" onMouseDown={(e) => openMenuAt(e.currentTarget, editMenu())}>
            Edit
          </button>
          <button className="local-menu" onMouseDown={(e) => openMenuAt(e.currentTarget, functionsMenu())}>
            Functions
          </button>
          <button className="local-menu" onMouseDown={(e) => openMenuAt(e.currentTarget, viewMenu())}>
            View
          </button>
        </div>
        <div className="local-right">
          <button
            className={"lb-btn catch" + (s.catchPlayhead ? " on" : "")}
            title="Catch Playhead (`)"
            onClick={() => setState({ catchPlayhead: !s.catchPlayhead })}
          >
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3">
              <path d="M3 2v10M3 7h8M8 4l3 3-3 3" />
            </svg>
          </button>
          <button className="lb-btn" title={"Scroll to Playhead" + (keyHint("show-playhead") ? ` (${keyHint("show-playhead")})` : "")} onClick={() => transport.showPlayhead()}>
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3">
              <path d="M7 1v12M2 4h3M2 10h3M9 4h3M9 10h3" />
            </svg>
          </button>
          <span className="lb-label">Snap:</span>
          <button
            className="popup"
            onMouseDown={(e) =>
              openMenuAt(
                e.currentTarget,
                (Object.keys(SNAP_LABELS) as Snap[]).map((k) => ({ label: SNAP_LABELS[k], checked: s.snapMode === k, onClick: () => setState({ snapMode: k }) })),
              )
            }
          >
            {SNAP_LABELS[s.snapMode]} <Icon.chevron />
          </button>
          <span className="lb-label">Drag:</span>
          <button
            className="popup"
            title="X-Fade crossfades audio regions that a move, trim or paste leaves overlapping"
            onMouseDown={(e) =>
              openMenuAt(
                e.currentTarget,
                (Object.keys(DRAG_LABELS) as DragMode[]).map((k) => ({ label: DRAG_LABELS[k], checked: dragMode === k, onClick: () => act("set_drag_mode", { mode: k }) })),
              )
            }
          >
            {DRAG_LABELS[dragMode]} <Icon.chevron />
          </button>
          <span className="lb-label" title="Track height">
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.3">
              <path d="M6 1v10M3.5 3.5 6 1l2.5 2.5M3.5 8.5 6 11l2.5-2.5" />
            </svg>
          </span>
          <input
            className="zoom vzoom"
            type="range"
            min={MIN_TRACK_H}
            max={MAX_TRACK_H}
            step={1}
            value={s.trackHeight}
            title={`Track height (${fmt("Alt")}-scroll, ${keyHint("shorter-tracks")} / ${keyHint("taller-tracks")})`}
            onChange={(e) => setState({ trackHeight: parseInt(e.target.value), trackHeights: {} })}
          />
          <span className="lb-label" title="Horizontal zoom">
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.3">
              <path d="M1 6h10M3.5 3.5 1 6l2.5 2.5M8.5 3.5 11 6 8.5 8.5" />
            </svg>
          </span>
          <input
            className="zoom"
            type="range"
            min={0}
            max={1}
            step={0.001}
            value={zoom}
            title={`Horizontal zoom (${keyHint("zoom-out")} / ${keyHint("zoom-in")})`}
            onChange={(e) => setState({ pxPerBeat: 2 * Math.pow(200, parseFloat(e.target.value)) })}
          />
        </div>
      </div>
      <div
        className={"tracks-body" + (showOverview ? " with-overview" : "")}
        style={{
          gridTemplateColumns: `${headerW}px 1fr`,
          gridTemplateRows: (globalH ? `${RULER_H}px ${globalH}px 1fr` : `${RULER_H}px 1fr`) + (showOverview ? ` ${OVERVIEW_H + 1}px` : ""),
        }}
      >
        <div className="headers-top">
          <button className="add-track" title={`New Tracks (${fmt("Mod+Alt+N")})`} onClick={() => setState({ dialog: "new-tracks" })}>
            <Icon.plus />
          </button>
          <button className={"hide-mode" + (showHidden ? " on" : "")} title={`Show Hidden Tracks (${keyHint("show-hidden")})`} onClick={ops.toggleShowHidden}>
            H
          </button>
          <span className="global-label">Marker</span>
        </div>
        <Ruler />
        {globalH > 0 && <GlobalTracks />}
        <div className="headers-cell">
          <TrackHeaders />
          <Splitter pane="headers" dir="x" sign={1} />
        </div>
        <Arrange />
        {showOverview && <div className="overview-corner" />}
        {showOverview && <Overview />}
      </div>
    </div>
  );
}
