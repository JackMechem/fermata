// Plug-in latency in the UI: the Settings › Audio options, the channel strip readout and the
// status line. The figures come from the session (crates/engine/src/session/pdc.rs).
import { type Id } from "../api";
import { updateSettings, useSettings, type PdcMode } from "../prefs";
import { setState, useStore } from "../store";
import { NumberField, Select } from "./Form";

const ms = (frames: number, rate: number) => `${((frames * 1000) / Math.max(1, rate)).toFixed(1)} ms`;

function setLatency(patch: Partial<{ compensation: PdcMode; low_latency: boolean; low_latency_limit_ms: number }>) {
  updateSettings((s) => ({ ...s, prefs: { ...s.prefs, latency: { ...s.prefs.latency, ...patch } } }));
}

/** A plug-in instance's latency in frames (0 when it reports none). */
export function useSlotLatency(uid: Id | undefined): number {
  return useStore((s) => {
    if (uid === undefined) return 0;
    for (const st of s.snap?.pdc?.strips ?? []) for (const [u, f] of st.slots) if (u === uid) return f;
    return 0;
  });
}

/** Rows for the Audio pane's settings grid. They apply at once, like Logic's. */
export function PdcSettings() {
  const lat = useSettings((s) => s.prefs.latency);
  const total = useStore((s) => s.snap?.pdc?.total_ms ?? 0);
  return (
    <>
      <label>Plug-in Delay Compensation:</label>
      <Select
        value={lat.compensation}
        options={[
          ["all", "All"],
          ["off", "Off"],
        ]}
        onChange={(v) => setLatency({ compensation: v })}
      />
      <span />
      <span className="form-note">
        {lat.compensation === "all"
          ? `Every track, aux and send is delayed to line up with the slowest path at Stereo Out${total > 0 ? ` (${total.toFixed(1)} ms now)` : ""}. Recordings are moved to match.`
          : "Plug-ins with latency put their tracks (and anything they feed) late."}
      </span>
      <label>Low Latency Mode:</label>
      <div className="row-inline">
        <label className="check">
          <input type="checkbox" checked={lat.low_latency} onChange={(e) => setLatency({ low_latency: e.target.checked })} />
          On, limit
        </label>
        <NumberField value={lat.low_latency_limit_ms} step={0.5} resolution={0.1} min={0} max={100} width={60} onChange={(v) => setLatency({ low_latency_limit_ms: v })} />
        <span className="dim">ms</span>
      </div>
      <span />
      <span className="form-note">
        For playing live: plug-ins with more latency than the limit are switched off on record-enabled tracks and on audio tracks monitoring their input,
        and those tracks skip their compensation delay. An armed audio track always skips it while recording.
      </span>
    </>
  );
}

/** The Audio FX label's latency readout: the strip's plug-in latency, when it has any. Strips
 *  without latency show nothing, even when they are delayed to match others. */
export function StripLatencyBadge({ id }: { id: Id }) {
  const strip = useStore((s) => s.snap?.pdc?.strips.find((x) => x.track === id));
  const rate = useStore((s) => s.snap?.project.sample_rate ?? 48000);
  const enabled = useStore((s) => s.snap?.pdc?.enabled ?? true);
  if (!strip || strip.latency === 0) return null;
  const lines = [`Plug-in latency: ${strip.latency} samples (${ms(strip.latency, rate)})`];
  if (enabled && strip.delay > 0) lines.push(`Compensation delay: ${strip.delay} samples (${ms(strip.delay, rate)})`);
  if (!enabled) lines.push("Delay compensation is off (Settings › Audio)");
  return (
    <span className="strip-latency" title={lines.join("\n")}>
      {ms(strip.latency, rate)}
    </span>
  );
}

/** The status line's compensation readout (hidden when nothing is latent). */
export function PdcStatus() {
  const pdc = useStore((s) => s.snap?.pdc);
  if (!pdc || (!pdc.low_latency && pdc.total === 0)) return null;
  const title = pdc.enabled
    ? `Plug-in delay compensation: Stereo Out plays ${pdc.total} samples (${pdc.total_ms.toFixed(1)} ms) behind the playhead so every path lines up`
    : "Plug-in delay compensation is off";
  return (
    <span className="pdc-status" title={title + (pdc.low_latency ? "\nLow Latency Mode is on" : "")} onClick={() => setState({ dialog: "settings", settingsTab: "audio" })}>
      {pdc.low_latency ? "Low Latency · " : ""}
      PDC {pdc.total_ms.toFixed(1)} ms
    </span>
  );
}
