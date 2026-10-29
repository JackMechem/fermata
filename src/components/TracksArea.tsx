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

const SNAP_LABELS: Record<Snap, string> = { smart: "Smart", bar: "Bar", beat: "Beat", off: "Off" };

export function editMenu(): MenuItem[] {
  const sel = getState().selectedRegions;
  const snap = getState().snap;
  const regs = snap?.project.tracks.flatMap((t) => t.regions).filter((r) => sel.includes(r.id)) ?? [];
  return [
    { label: snap?.undo ? `Undo ${snap.undo}` : "Undo", hint: "⌘Z", disabled: !snap?.undo, onClick: () => undoRedo("undo") },
    { label: snap?.redo ? `Redo ${snap.redo}` : "Redo", hint: "⇧⌘Z", disabled: !snap?.redo, onClick: () => undoRedo("redo") },
    { separator: true },
    { label: "Cut", hint: "⌘X", disabled: !sel.length, onClick: () => ops.cutRegions() },
    { label: "Copy", hint: "⌘C", disabled: !sel.length, onClick: () => ops.copyRegions() },
    { label: "Paste", hint: "⌘V", disabled: !getState().clipboard.length, onClick: () => ops.pasteRegions() },
    { label: "Duplicate", hint: "⌘D", onClick: ops.duplicate },
    { label: "Repeat", disabled: !sel.length, children: [1, 2, 3, 4, 8, 16].map((n) => ({ label: n === 1 ? "Once" : `${n} Times`, onClick: () => ops.repeatRegions(n) })) },
    { separator: true },
    { label: "Split Regions at Playhead", hint: "⌘T", onClick: () => ops.splitAtPlayhead() },
    { label: "Join Regions", hint: "⌘J", disabled: sel.length < 2, onClick: () => ops.joinRegions() },
    { label: "Loop Regions", hint: "L", disabled: !sel.length, checked: regs.length > 0 && regs.every(isLooped), onClick: () => ops.toggleLoop() },
    { label: "Convert Loops to Real Copies", disabled: !regs.some(isLooped), onClick: () => ops.convertLoops() },
    { label: "Set Locators by Regions", hint: "U", disabled: !sel.length, onClick: () => ops.setLocatorsByRegions() },
    { separator: true },
    { label: "Select All", hint: "⌘A", onClick: ops.selectAllRegions },
    { label: "Delete", hint: "⌫", disabled: !sel.length, onClick: () => ops.deleteRegions() },
  ];
}

export function TracksArea() {
  const pxPerBeat = useStore((s) => s.pxPerBeat);
  const catchPlayhead = useStore((s) => s.catchPlayhead);
  const snapMode = useStore((s) => s.snapMode);
  const trackHeight = useStore((s) => s.trackHeight);
  const headerW = useStore((s) => s.paneSizes.headers);
  const showHidden = useStore((s) => s.showHidden);
  // Menus read the rest of the state when they open.
  const s = { ...getState(), pxPerBeat, catchPlayhead, snapMode, trackHeight };
  const zoom = Math.log(s.pxPerBeat / 2) / Math.log(200);

  const functionsMenu = (): MenuItem[] => [
    { label: "New Tracks…", hint: "⌥⌘N", onClick: () => setState({ dialog: "new-tracks" }) },
    { label: "Duplicate Track", onClick: () => ops.duplicateTracks(true) },
    { label: "New Track with Same Settings", onClick: () => ops.duplicateTracks(false) },
    { separator: true },
    { label: "Create Folder Stack", hint: "⇧⌘D", onClick: () => ops.createStack(false) },
    { label: "Create Summing Stack", onClick: () => ops.createStack(true) },
    { label: "Group Selected Tracks", hint: "⌘G", onClick: () => ops.groupTracks() },
    { separator: true },
    { label: "Import Audio File…", hint: "⇧⌘I", onClick: importAudioDialog },
    { label: "Import MIDI File…", onClick: importMidiDialog },
    { label: "Export Selection as MIDI File…", onClick: exportSelectionMidi },
    { separator: true },
    { label: "Hide Track", hint: "⌃H", onClick: () => ops.hideTracks() },
    { label: "Unhide All Tracks", disabled: !getState().snap?.project.tracks.some((t) => t.hidden), onClick: ops.unhideAllTracks },
    { separator: true },
    { label: "Delete Track", hint: "⌘⌫", onClick: () => ops.deleteTracks() },
    { label: "Delete Unused Tracks", onClick: ops.deleteUnusedTracks },
  ];
  const viewMenu = (): MenuItem[] => {
    const s = getState();
    return [
    { label: "Inspector", hint: "I", checked: s.showInspector, onClick: () => setState({ showInspector: !s.showInspector }) },
    { label: "Library", hint: "Y", checked: s.showLibrary, onClick: () => setState({ showLibrary: !s.showLibrary }) },
    { label: "Mixer", hint: "X", checked: s.bottom === "mixer", onClick: () => setState({ bottom: s.bottom === "mixer" ? "none" : "mixer" }) },
    { label: "Browsers", hint: "F", checked: s.showBrowser, onClick: () => setState({ showBrowser: !s.showBrowser }) },
    { label: "Show Automation", hint: "A", checked: s.showAutomation, onClick: () => setState({ showAutomation: !s.showAutomation }) },
    { separator: true },
    { label: "Show Hidden Tracks", hint: "H", checked: s.showHidden, onClick: ops.toggleShowHidden },
    { separator: true },
    { label: "Zoom In", hint: "⌘→", onClick: () => setState({ pxPerBeat: Math.min(400, s.pxPerBeat * 1.4) }) },
    { label: "Zoom Out", hint: "⌘←", onClick: () => setState({ pxPerBeat: Math.max(2, s.pxPerBeat / 1.4) }) },
    { separator: true },
    { label: "Taller Tracks", hint: "⌘↓", onClick: () => zoomTracks(1.2) },
    { label: "Shorter Tracks", hint: "⌘↑", onClick: () => zoomTracks(1 / 1.2) },
    {
      label: "Track Height",
      children: [
        ["Small", 48],
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
            title="Track height (Alt-scroll, ⌘↑ / ⌘↓)"
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
            title="Horizontal zoom (⌘← / ⌘→)"
            onChange={(e) => setState({ pxPerBeat: 2 * Math.pow(200, parseFloat(e.target.value)) })}
          />
        </div>
      </div>
      <div className="tracks-body" style={{ gridTemplateColumns: `${headerW}px 1fr`, gridTemplateRows: `${RULER_H}px 1fr` }}>
        <div className="headers-top">
          <button className="add-track" title="New Tracks (⌥⌘N)" onClick={() => setState({ dialog: "new-tracks" })}>
            <Icon.plus />
          </button>
          <button className={"hide-mode" + (showHidden ? " on" : "")} title="Show Hidden Tracks (H)" onClick={ops.toggleShowHidden}>
            H
          </button>
          <span className="global-label">Marker</span>
        </div>
        <Ruler />
        <div className="headers-cell">
          <TrackHeaders />
          <Splitter pane="headers" dir="x" sign={1} />
        </div>
        <Arrange />
      </div>
    </div>
  );
}
