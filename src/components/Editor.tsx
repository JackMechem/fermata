import type { Region, Track } from "../api";
import { useStore } from "../store";
import { AudioEditor } from "./AudioEditor";
import { DetachButton } from "./DetachButton";
import { PianoRoll } from "./PianoRoll";
import { useEditorFollow } from "./StepSequencer";

/**
 * The bottom editor pane (E). Follows the selection like Logic's main window editor: the Piano
 * Roll for a software instrument track, the Audio Track editor for an audio track. The region
 * shown first is the selected one on that track, if any.
 */
export function Editor() {
  const project = useStore((s) => s.snap?.project);
  const selectedTrack = useStore((s) => s.snap?.selected ?? null);
  const selected = useStore((s) => s.selectedRegions);
  let region: Region | null = null;
  let track: Track | null = null;
  for (const t of project?.tracks ?? []) {
    const r = t.regions.find((r) => r.id === selected[0]);
    if (r) {
      region = r;
      track = t;
    }
  }
  // A pattern region opens in the Step Sequencer.
  useEditorFollow(region, "editor");
  // No region selected: edit the selected track.
  if (!track) track = project?.tracks.find((t) => t.id === selectedTrack) ?? null;
  if (track?.kind === "instrument") return <PianoRoll track={track} region={region} />;
  if (track?.kind === "audio") return <AudioEditor track={track} region={region} />;
  return (
    <div className="editor">
      <div className="local-bar">
        <div className="local-menus">
          <span className="local-title">Editor</span>
        </div>
        <div className="local-right">
          <DetachButton panel="editor" />
        </div>
      </div>
      <div className="editor-body">
        <div className="editor-empty">Select an audio or software instrument track to edit it here.</div>
      </div>
    </div>
  );
}
