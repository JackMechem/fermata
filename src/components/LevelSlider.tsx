import { useMemo, useRef } from "react";
import type { Id } from "../api";
import { useStore } from "../store";
import { theme } from "../theme";
import { dbToFader, faderToDb, formatDb } from "../time";
import { fmt } from "../keymap";
import { levelSliderSize, meterScale, thumbCenter, type HeaderTier } from "../trackHeader";
import { Meter, useDrag, type TouchProps } from "./Controls";

/**
 * A track header's volume slider with the track's level meter as its groove, as in Logic's
 * track headers. The meter is drawn on the slider's scale (see `meterScale`), so the thumb marks
 * where a full-scale signal lands. Drag to move (Shift for fine), double-click or Alt-click for
 * 0 dB. `track` is null for a folder stack, which has no meter of its own.
 */
export function LevelSlider({
  db,
  onChange,
  width,
  tier,
  track,
  label = "Volume",
  onTouchStart,
  onTouchEnd,
}: {
  db: number;
  onChange: (db: number) => void;
  width: number;
  tier: HeaderTier;
  track: Id | null;
  /** Starts the tooltip, which goes on with the level. */
  label?: string;
} & TouchProps) {
  // The groove takes the meter's colours from the canvas theme.
  useStore((s) => s.themeId);
  const { groove, thumbW, thumbH } = levelSliderSize(tier);
  const pos = useRef(dbToFader(db));
  pos.current = dbToFader(db);
  const travel = Math.max(1, width - thumbW);
  const start = useDrag((d, fine) => {
    pos.current = Math.min(1, Math.max(0, pos.current + d / travel / (fine ? 6 : 1)));
    onChange(Math.round(faderToDb(pos.current) * 10) / 10);
  }, onTouchEnd);
  // Stable per size, so the meter isn't restarted on every render.
  const scale = useMemo(() => meterScale(width, thumbW, dbToFader), [width, thumbW]);
  const left = thumbCenter(dbToFader(db), width, thumbW) - thumbW / 2;
  return (
    <div
      className="level-slider"
      style={{ width, height: thumbH }}
      title={`${label}: ${formatDb(db)} dB · double-click or ${fmt("Alt")}-click for 0 dB`}
      onMouseDown={(e) => {
        e.stopPropagation();
        if (e.button !== 0) return;
        if (e.altKey) {
          e.preventDefault();
          onChange(0);
          return;
        }
        onTouchStart?.();
        start(e, "x");
      }}
      onDoubleClick={(e) => {
        e.stopPropagation();
        onChange(0);
      }}
    >
      <div className="level-groove" style={{ height: groove, background: theme.meterBg }}>
        {track !== null && <Meter source={{ kind: "track", id: track }} height={width} width={groove} horizontal scale={scale} className="level-meter" />}
      </div>
      <div className="level-thumb" style={{ left, width: thumbW, height: thumbH }} />
    </div>
  );
}
