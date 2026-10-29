import { useCallback, useLayoutEffect, useRef, useState } from "react";
import { act, api, canRecord, INSTRUMENT_SLOT, isStack, MASTER_ID, type InputSel, type PluginSlot, type Track } from "../api";
import { Icon } from "../icons";
import { pluginMenu } from "../pluginMenu";
import { useStore } from "../store";
import { formatDb, inputLabel } from "../time";
import { Fader, Knob, Meter } from "./Controls";
import { EqThumb } from "./Eq";
import { automatable, useAutomation, VOLUME } from "../automation";
import { AutoModeButton } from "./AutomationHeader";
import { openMenu, openMenuAt, type MenuItem } from "./Menu";
import { OutputSlot, SendsSection } from "./Sends";
import { StripLatencyBadge, useSlotLatency } from "./Latency";
import { refuseFrozen } from "../freeze";
import { isBuiltinFx, loadPlugin, slotMenu, toggleFxPanel } from "../fxHost";
import { startSlotDrag } from "../slotDrag";
import { fxKey, usePanelShown } from "../fxDockState";

const PAN = { type: "pan" } as const;

export function PluginSlotView({
  trackId,
  slot,
  plugin,
  instrument,
  title,
}: {
  trackId: number;
  slot: number;
  plugin: PluginSlot | null;
  instrument?: boolean;
  title: string;
}) {
  const plugins = useStore((s) => s.plugins);
  const openEditors = useStore((s) => s.snap?.open_editors) ?? [];
  const fxOpen = usePanelShown(plugin ? fxKey(plugin.uid) : null);
  useStore((s) => s.fxList);
  // A frozen track's plug-ins are unloaded and can't change until it is unfrozen.
  const frozen = useStore((s) => !!s.snap?.project.tracks.find((t) => t.id === trackId)?.frozen);
  const latency = useSlotLatency(plugin?.uid);
  const rate = useStore((s) => s.snap?.project.sample_rate ?? 48000);
  const menu = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (e.button !== 0 || refuseFrozen([trackId])) return;
    const items = pluginMenu(plugins, {
      instrument: !!instrument,
      current: plugin?.plugin_id,
      pick: (id) => loadPlugin(trackId, slot, id),
      remove: () => api.removePlugin(trackId, slot),
    });
    const r = (e.currentTarget as HTMLElement).closest(".slot")!.getBoundingClientRect();
    openMenu(r.right - 4, r.top, items);
  };
  // Right-click: Copy and Paste Plug-in.
  const context = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    if (refuseFrozen([trackId])) return;
    openMenu(e.clientX, e.clientY, slotMenu(trackId, slot, plugin));
  };
  // Audio FX slots are drop targets for dragged plug-ins (see slotDrag.ts).
  const target = instrument ? {} : { "data-track": trackId, "data-slot": slot };
  if (!plugin) {
    return (
      <div className={"slot empty" + (instrument ? " instrument" : "")} {...target} onMouseDown={menu} onContextMenu={context}>
        {instrument && <span className="slot-name dim">Instrument</span>}
      </div>
    );
  }
  const open = openEditors.includes(plugin.uid) || fxOpen;
  return (
    <div className={"slot filled" + (plugin.bypass ? " bypassed" : "") + (open ? " editing" : "") + (instrument ? " instrument" : "") + (frozen ? " frozen" : "")} {...target} onContextMenu={context}>
      <button
        className="slot-power"
        title={plugin.bypass ? "Turn on" : "Bypass"}
        onMouseDown={(e) => {
          e.stopPropagation();
          if (refuseFrozen([trackId])) return;
          api.setBypass(trackId, slot, !plugin.bypass);
        }}
      >
        <Icon.power />
      </button>
      <span
        className="slot-name"
        title={`${plugin.name}${latency ? ` — latency ${latency} samples (${((latency * 1000) / rate).toFixed(1)} ms)` : ""}${frozen ? " — frozen" : instrument ? " — click to open" : " — click to open, drag to move (Alt to copy)"}`}
        onMouseDown={(e) => {
          e.stopPropagation();
          if (e.button !== 0) return;
          const open = () => {
            // A built-in effect opens its panel (a frozen track's plug-ins are unloaded).
            if (isBuiltinFx(plugin.plugin_id)) {
              if (!refuseFrozen([trackId])) toggleFxPanel(plugin.uid);
              return;
            }
            if (plugin.plugin_id.startsWith("builtin:") || refuseFrozen([trackId])) return;
            api.openEditor(plugin.uid, `${title} — ${plugin.name}`);
          };
          if (instrument || frozen) open();
          else startSlotDrag(e, trackId, slot, plugin.name, open);
        }}
      >
        {plugin.name}
      </span>
      <button className="slot-arrow" onMouseDown={menu}>
        <Icon.chevron />
      </button>
    </div>
  );
}

function inputMenu(track: Track, channels: number): MenuItem[] {
  const n = Math.max(channels, 2);
  const set = (input: InputSel) => api.updateTrack(track.id, { input });
  const items: MenuItem[] = [{ label: "No Input", checked: track.input.type === "none", onClick: () => set({ type: "none" }) }, { separator: true }];
  for (let c = 0; c < n; c++) {
    items.push({ label: `Input ${c + 1}`, checked: track.input.type === "mono" && track.input.ch === c, onClick: () => set({ type: "mono", ch: c }) });
  }
  items.push({ separator: true });
  for (let c = 0; c + 1 < n; c += 2) {
    items.push({
      label: `Input ${c + 1}–${c + 2}`,
      checked: track.input.type === "stereo" && track.input.ch === c,
      onClick: () => set({ type: "stereo", ch: c }),
    });
  }
  return items;
}

/** Logic-style channel strip. `track` is null for the Stereo Out strip. */
export function ChannelStrip({ track, selected }: { track: Track | null; selected?: boolean }) {
  // The fader takes whatever height is left, like Logic's strips.
  const faderBox = useRef<HTMLDivElement>(null);
  const [faderHeight, setFaderHeight] = useState(160);
  useLayoutEffect(() => {
    const el = faderBox.current;
    if (!el) return;
    const ro = new ResizeObserver(([e]) => setFaderHeight(Math.max(60, Math.floor(e.contentRect.height))));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  const master = useStore((s) => s.snap?.project.master);
  const inputs = useStore((s) => s.snap?.audio.input_channels ?? 2);
  const [peak, setPeak] = useState(-Infinity);
  const [peakReset, setPeakReset] = useState(0);
  const onPeak = useCallback((db: number) => setPeak(db), []);
  const isMaster = track === null;
  const id = track?.id ?? MASTER_ID;
  const inserts = isMaster ? master?.inserts ?? [] : track.inserts;
  const volume = isMaster ? master?.volume_db ?? 0 : track.volume_db;
  const lastFilled = inserts.reduce((acc, s, i) => (s ? i : acc), -1);
  const shown = Math.min(inserts.length, Math.max(lastFilled + 2, 1));
  const name = isMaster ? "Stereo Out" : track.name;
  const folder = track?.kind === "folder";
  // Subtracks of a summing stack play through it (a folder passes them on).
  const tracks = useStore((s) => s.snap?.project.tracks);
  const output = (() => {
    let p = track?.parent ?? null;
    while (p !== null) {
      const up = tracks?.find((t) => t.id === p);
      if (!up) break;
      if (up.kind === "summing") return up.name;
      p = up.parent;
    }
    return "Stereo Out";
  })();
  const setVolume = (db: number) => (isMaster ? act("set_master_volume", { db }) : api.updateTrack(id, { volume_db: db }));
  // While playing, the fader and pan knob follow the automation.
  const [autoVolume, volumeTouch] = useAutomation(track, VOLUME, volume);
  const [autoPan, panTouch] = useAutomation(track, PAN, track?.pan ?? 0);

  return (
    <div className={"strip" + (selected ? " selected" : "")} onMouseDown={() => track && !selected && api.selectTrack(track.id)}>
      <div className="strip-section">
        <div className="strip-label">Input</div>
        {isMaster ? (
          <div className="slot static" />
        ) : isStack(track.kind) ? (
          <div className="slot static" title={track.kind === "summing" ? "Its subtracks play through this strip" : "Folder: its fader, mute and solo apply to its subtracks"}>
            {track.kind === "summing" ? "Subtracks" : "Folder"}
          </div>
        ) : track.kind === "aux" ? (
          <div className="slot static output" title="Sends and outputs set to this bus play through this strip">
            Bus {track.bus}
          </div>
        ) : track.kind === "instrument" ? (
          <PluginSlotView trackId={id} slot={INSTRUMENT_SLOT} plugin={track.instrument} instrument title={name} />
        ) : (
          <div className="slot input-slot">
            <button
              className="slot-format"
              title="Mono / Stereo"
              onMouseDown={(e) => {
                e.stopPropagation();
                const ch = track.input.type === "none" ? 0 : track.input.ch;
                api.updateTrack(id, { input: track.input.type === "stereo" ? { type: "mono", ch } : { type: "stereo", ch: ch - (ch % 2) } });
              }}
            >
              {track.input.type === "stereo" ? <Icon.stereo /> : <Icon.mono />}
            </button>
            <span
              className="slot-name"
              onMouseDown={(e) => {
                e.stopPropagation();
                openMenuAt(e.currentTarget.parentElement as HTMLElement, inputMenu(track, inputs));
              }}
            >
              {inputLabel(track.input)}
            </span>
          </div>
        )}
      </div>

      {folder ? (
        <div className="strip-section strip-folder-note">A folder has no audio of its own.</div>
      ) : (
        <>
          <div className="strip-section">
            <EqThumb trackId={id} name={name} />
          </div>

          <div className="strip-section">
            <div className="strip-label">
              Audio FX
              <StripLatencyBadge id={id} />
            </div>
            {inserts.slice(0, shown).map((p, i) => (
              <PluginSlotView key={i} trackId={id} slot={i} plugin={p} title={name} />
            ))}
          </div>

          {!isMaster && <SendsSection track={track} />}
        </>
      )}

      <div className="strip-section">
        {isMaster || folder ? <div className="slot static output">{isMaster ? "Output 1–2" : output}</div> : <OutputSlot track={track} />}
      </div>

      <div className="strip-pan">
        {/* Logic puts the automation mode right above the pan knob. */}
        {track && automatable(track) ? <AutoModeButton track={track} /> : <div className="auto-mode-spacer" />}
        {isMaster || folder ? <div style={{ height: 26 }} /> : <Knob value={autoPan} onChange={(pan) => api.updateTrack(id, { pan })} {...panTouch} />}
      </div>

      <div className="strip-readouts">
        <span className="readout">{formatDb(autoVolume)}</span>
        <span className={"readout peak" + (peak > 0 ? " clip" : "")} onMouseDown={(e) => { e.stopPropagation(); setPeak(-Infinity); setPeakReset((n) => n + 1); }}>
          {peak === -Infinity || peak < -96 ? "-∞" : formatDb(peak)}
        </span>
      </div>

      <div className="strip-fader" ref={faderBox}>
        <Fader db={autoVolume} onChange={setVolume} height={faderHeight} {...(isMaster ? {} : volumeTouch)} />
        {folder ? <div style={{ width: 11 }} /> : <Meter source={isMaster ? { kind: "master" } : { kind: "track", id }} height={faderHeight} width={11} onPeak={onPeak} resetKey={peakReset} />}
      </div>

      {!isMaster ? (
        <div className="strip-buttons">
          <div className="btn-row">
            <button className={"tbtn mute" + (track.mute ? " on" : "")} onMouseDown={(e) => { e.stopPropagation(); api.updateTrack(id, { mute: !track.mute }); }}>
              M
            </button>
            <button className={"tbtn solo" + (track.solo ? " on" : "")} onMouseDown={(e) => { e.stopPropagation(); api.updateTrack(id, { solo: !track.solo }); }}>
              S
            </button>
          </div>
          <div className="btn-row">
            {!canRecord(track.kind) ? (
              <span className="tbtn ghost" />
            ) : (
              <button className={"tbtn rec" + (track.armed ? " on" : "")} onMouseDown={(e) => { e.stopPropagation(); api.updateTrack(id, { armed: !track.armed }); }}>
                R
              </button>
            )}
            {track.kind === "audio" ? (
              <button className={"tbtn mon" + (track.monitor ? " on" : "")} onMouseDown={(e) => { e.stopPropagation(); api.updateTrack(id, { monitor: !track.monitor }); }}>
                I
              </button>
            ) : (
              <span className="tbtn ghost" />
            )}
          </div>
        </div>
      ) : (
        <div className="strip-buttons">
          <div className="btn-row" style={{ height: 41 }} />
        </div>
      )}

      <div className="strip-name" style={{ borderTopColor: isMaster ? "#8d9099" : track.color }} title={name}>
        {name}
      </div>
    </div>
  );
}
