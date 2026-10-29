import type { Track } from "../api";
import { MODE_LABELS, modeMenu, paramLabel, paramMenu, VOLUME } from "../automation";
import { Icon } from "../icons";
import { useStore } from "../store";
import { openMenuAt } from "./Menu";

/** The automation mode pop-up: green in Read, orange-red in the writing modes, as in Logic. */
export function AutoModeButton({ track, compact }: { track: Track; compact?: boolean }) {
  return (
    <button
      className={`auto-mode ${track.auto_mode}` + (compact ? " compact" : "")}
      title="Automation mode"
      onMouseDown={(e) => {
        e.stopPropagation();
        openMenuAt(e.currentTarget, modeMenu(track));
      }}
    >
      {MODE_LABELS[track.auto_mode]}
    </button>
  );
}

/** A track header's automation row: which parameter the lane shows, and the mode. */
export function AutomationControls({ track }: { track: Track }) {
  const param = useStore((s) => s.autoLane[track.id]) ?? VOLUME;
  return (
    <div className="th-auto" onMouseDown={(e) => e.stopPropagation()}>
      <AutoModeButton track={track} compact />
      <button
        className="popup auto-param"
        title="Parameter shown on the lane"
        onMouseDown={async (e) => {
          const el = e.currentTarget;
          openMenuAt(el, await paramMenu(track));
        }}
      >
        <span>{paramLabel(param)}</span>
        <Icon.chevron />
      </button>
    </div>
  );
}
