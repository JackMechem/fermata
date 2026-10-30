// The Track Alternatives pop-up in a track header (Logic 10.5+): the active alternative's
// name; clicking lists the others with New, Duplicate, Rename and Delete. Shown once the
// track has more than one (Track › Track Alternatives › New makes the second).
import type { Track } from "../api";
import { activeTrackAlt } from "../alternativesModel";
import { trackAltMenu } from "../trackAlternatives";
import { openMenuAt } from "./Menu";

export function TrackAltButton({ track }: { track: Track }) {
  return (
    <button
      className="th-alt"
      title={`Track Alternative: ${activeTrackAlt(track)}`}
      onMouseDown={(e) => {
        e.stopPropagation();
        openMenuAt(e.currentTarget, trackAltMenu(track));
      }}
    >
      <span className="th-alt-name">{activeTrackAlt(track)}</span>
      <svg width="7" height="5" viewBox="0 0 7 5" fill="currentColor">
        <path d="M0 0h7L3.5 5z" />
      </svg>
    </button>
  );
}
