// The Left-click Tool and Command-click Tool pop-ups in the Tracks area's bar (Logic's), with
// the tools' icons. Choosing and using the tools is in trackTools.ts.
import { Icon } from "../icons";
import { fmt, keyHint } from "../keymap";
import { useStore } from "../store";
import type { TrackTool } from "../tools";
import { toolMenuItems } from "../trackTools";
import { openMenuAt } from "./Menu";
import { ToolIcon } from "./PianoRoll";

/** A Tracks-area tool's icon: the Piano Roll's where the two share a tool. */
export function TrackToolIcon({ tool }: { tool: TrackTool }) {
  const p = { width: 14, height: 14, viewBox: "0 0 14 14", fill: "none", stroke: "currentColor", strokeWidth: 1.3, strokeLinecap: "round" as const, strokeLinejoin: "round" as const };
  switch (tool) {
    case "fade":
      return (
        <svg {...p}>
          <path d="M2 11.5C6.5 11.5 9.5 7.5 12 2.5M2 11.5h10" />
        </svg>
      );
    case "marquee":
      return (
        <svg {...p}>
          <rect x="2" y="3" width="10" height="8" strokeDasharray="2 1.6" />
          <path d="M7 5.5v3M5.5 7h3" />
        </svg>
      );
    default:
      return <ToolIcon tool={tool} />;
  }
}

export function ToolPopups() {
  const tools = useStore((s) => s.tools);
  const hint = keyHint("tool-menu");
  return (
    <>
      <button className="popup tool-popup left" title={`Left-click Tool${hint ? ` (${hint})` : ""}`} onMouseDown={(e) => openMenuAt(e.currentTarget, toolMenuItems("left"))}>
        <TrackToolIcon tool={tools.left} /> <Icon.chevron />
      </button>
      <button className="popup tool-popup cmd" title={`${fmt("Mod")}-click Tool`} onMouseDown={(e) => openMenuAt(e.currentTarget, toolMenuItems("cmd"))}>
        <TrackToolIcon tool={tools.cmd} /> <Icon.chevron />
      </button>
    </>
  );
}
