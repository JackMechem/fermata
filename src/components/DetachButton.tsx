// The button on a detachable panel's bar: opens the panel in a window of its own, or, in that
// window, puts it back in the main window (detach.ts).
import { attachPanel, detachPanel, ownPanel } from "../detach";

export function DetachButton({ panel, className = "lb-btn" }: { panel: string; className?: string }) {
  const own = ownPanel === panel;
  return (
    <button className={`${className} detach-btn`} title={own ? "Attach to Main Window" : "Open in Separate Window"} onClick={() => (own ? attachPanel(panel) : detachPanel(panel))}>
      {own ? <AttachGlyph /> : <DetachGlyph />}
    </button>
  );
}

/** A window with an arrow leaving it by the top right corner. */
function DetachGlyph() {
  return (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round">
      <path d="M6 2.5H3A1.5 1.5 0 0 0 1.5 4v7A1.5 1.5 0 0 0 3 12.5h7a1.5 1.5 0 0 0 1.5-1.5V8" />
      <path d="M8.5 1.5h4v4M12.5 1.5 6.5 7.5" />
    </svg>
  );
}

/** A window with an arrow coming into it towards the bottom left corner. */
function AttachGlyph() {
  return (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round">
      <path d="M6 2.5H3A1.5 1.5 0 0 0 1.5 4v7A1.5 1.5 0 0 0 3 12.5h7a1.5 1.5 0 0 0 1.5-1.5V8" />
      <path d="M4.5 5.5v4h4M4.5 9.5l8-8" />
    </svg>
  );
}
