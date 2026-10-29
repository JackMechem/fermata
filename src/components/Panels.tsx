import { useEffect, useMemo, useState } from "react";
import { act, api, BUILTIN_SYNTH, call, INSTRUMENT_SLOT, isStack, type Region, type Track } from "../api";
import { Icon } from "../icons";
import { isAudioPath, startFileDrag } from "../dnd";
import { importMidiFiles, isMidiPath } from "../midiFiles";
import { currentTick, getState, setState, useStore } from "../store";
import { formatTime, inputLabel, toBbt } from "../time";
import { ChannelStrip } from "./ChannelStrip";
import { NumberField } from "./Form";
import { useSelectedTrack } from "./TrackHeaders";
import { isLooped } from "../loops";
import { toggleLoop } from "../ops";
import { loadPlugin } from "../fxHost";

function Disclosure({ title, children, initial = true }: { title: string; children: React.ReactNode; initial?: boolean }) {
  const [open, setOpen] = useState(initial);
  return (
    <div className="disclosure">
      <div className="disclosure-head" onClick={() => setOpen(!open)}>
        {Icon.disclosure(open)}
        <span>{title}</span>
      </div>
      {open && <div className="disclosure-body">{children}</div>}
    </div>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="insp-row">
      <span className="insp-label">{label}</span>
      <span className="insp-value">{children}</span>
    </div>
  );
}

export function Inspector() {
  const track = useSelectedTrack();
  const project = useStore((s) => s.snap?.project);
  const selectedRegions = useStore((s) => s.selectedRegions);
  const regions: Region[] = useMemo(
    () => project?.tracks.flatMap((t) => t.regions).filter((r) => selectedRegions.includes(r.id)) ?? [],
    [project, selectedRegions],
  );
  if (!project) return <div className="inspector" />;
  const region = regions.length === 1 ? regions[0] : null;
  const regionTitle = regions.length > 1 ? `${regions.length} selected` : region ? region.name : "Default Settings";
  const bbt = (s: number) => {
    const b = toBbt(project, s);
    return `${b.bar} ${b.beat} ${b.div} ${b.tick}`;
  };
  return (
    <div className="inspector">
      <div className="inspector-top">
        <Disclosure title={`Region: ${regionTitle}`} initial={false}>
          {region ? (
            <>
              <Row label="Mute">
                <input type="checkbox" checked={region.muted} onChange={(e) => act("set_region_muted", { id: region.id, muted: e.target.checked })} />
              </Row>
              <Row label="Position">{bbt(region.start)}</Row>
              <Row label="Length">{formatTime(project, region.length)}</Row>
              <Row label="Loop">
                <input type="checkbox" checked={isLooped(region)} onChange={() => toggleLoop([region.id])} />
              </Row>
              <Row label="Type">{region.content.type === "audio" ? "Audio" : `MIDI · ${region.content.notes.length} notes`}</Row>
              {region.content.type === "audio" && (
                <>
                  <Row label="Gain">
                    <NumberField value={Math.round(region.content.gain_db * 10) / 10} step={0.5} resolution={0.1} min={-96} max={30} width={70} onChange={(db) => act("set_region_gain", { ids: [region.id], db, delta: null })} />
                    <span className="dim"> dB</span>
                  </Row>
                  <Row label="Fade In">
                    <NumberField value={Math.round((region.content.fade_in / project.sample_rate) * 1000)} step={10} resolution={1} min={0} width={70} onChange={(v) => act("set_region_fades", { ids: [region.id], fadeIn: Math.round((v / 1000) * project.sample_rate), fadeOut: null })} />
                    <span className="dim"> ms</span>
                  </Row>
                  <Row label="Fade Out">
                    <NumberField value={Math.round((region.content.fade_out / project.sample_rate) * 1000)} step={10} resolution={1} min={0} width={70} onChange={(v) => act("set_region_fades", { ids: [region.id], fadeIn: null, fadeOut: Math.round((v / 1000) * project.sample_rate) })} />
                    <span className="dim"> ms</span>
                  </Row>
                </>
              )}
            </>
          ) : (
            <div className="insp-empty">No region selected</div>
          )}
        </Disclosure>
        <Disclosure title={`Track: ${track?.name ?? "—"}`} initial={false}>
          {track ? (
            <>
              <Row label="Type">{{ audio: "Audio", instrument: "Software Instrument", folder: "Folder Stack", summing: "Summing Stack", aux: "Aux" }[track.kind]}</Row>
              {track.kind === "aux" && <Row label="Input">Bus {track.bus}</Row>}
              {track.kind === "audio" && <Row label="Input">{inputLabel(track.input)}</Row>}
              {track.kind === "instrument" && <Row label="Instrument">{track.instrument?.name ?? "None"}</Row>}
              {track.kind === "aux" ? null : isStack(track.kind) ? (
                <Row label="Subtracks">{project.tracks.filter((t) => t.parent === track.id).length}</Row>
              ) : (
                <Row label="Regions">{track.regions.length}</Row>
              )}
              {track.group !== null && <Row label="Group">{project.groups.find((g) => g.id === track.group)?.name ?? "—"}</Row>}
            </>
          ) : (
            <div className="insp-empty">No track selected</div>
          )}
        </Disclosure>
      </div>
      <div className="inspector-strips">
        {track ? <ChannelStrip track={track} selected /> : <div className="strip placeholder" />}
        <ChannelStrip track={null} />
      </div>
    </div>
  );
}

export function Mixer() {
  const project = useStore((s) => s.snap?.project);
  const selected = useStore((s) => s.snap?.selected);
  const [filter, setFilter] = useState({ audio: true, instrument: true, aux: true });
  const showHidden = useStore((s) => s.showHidden);
  if (!project) return null;
  const shown = project.tracks.filter((t) => (t.hidden ? showHidden : true) && (t.kind === "audio" || t.kind === "instrument" || t.kind === "aux" ? filter[t.kind] : true));
  // Auxes come after the tracks, as in Logic's mixer.
  const tracks = [...shown.filter((t) => t.kind !== "aux"), ...shown.filter((t) => t.kind === "aux")];
  return (
    <div className="mixer">
      <div className="local-bar">
        <div className="local-menus">
          <span className="local-title">Mixer</span>
        </div>
        <div className="mixer-filters">
          <button className={"filter" + (filter.audio ? " on" : "")} onClick={() => setFilter({ ...filter, audio: !filter.audio })}>
            Audio
          </button>
          <button className={"filter" + (filter.instrument ? " on" : "")} onClick={() => setFilter({ ...filter, instrument: !filter.instrument })}>
            Inst
          </button>
          <button className={"filter" + (filter.aux ? " on" : "")} onClick={() => setFilter({ ...filter, aux: !filter.aux })}>
            Aux
          </button>
        </div>
        <div className="local-right" />
      </div>
      <div className="mixer-strips">
        {tracks.map((t) => (
          <ChannelStrip key={t.id} track={t} selected={t.id === selected} />
        ))}
        <div className="mixer-gap" />
        <ChannelStrip track={null} />
      </div>
    </div>
  );
}

/** Logic's Library lists patches for the selected track; here it lists plug-ins to load. */
export function Library() {
  const track = useSelectedTrack();
  const plugins = useStore((s) => s.plugins);
  const [q, setQ] = useState("");
  const [rescanning, setRescanning] = useState(false);
  const isInst = track?.kind === "instrument";
  const fxList = useStore((s) => s.fxList);
  // Built-in effects first, by category, as in Logic's Library.
  const builtins = isInst ? [] : fxList.filter((f) => !q || `${f.name} ${f.category}`.toLowerCase().includes(q.toLowerCase()));
  const fxGroups = new Map<string, typeof builtins>();
  for (const f of builtins) {
    if (!fxGroups.has(f.category)) fxGroups.set(f.category, []);
    fxGroups.get(f.category)!.push(f);
  }
  const list = plugins
    .filter((p) => p.is_instrument === isInst)
    .filter((p) => !q || `${p.name} ${p.vendor} ${p.subcategories}`.toLowerCase().includes(q.toLowerCase()));
  const groups = new Map<string, typeof list>();
  for (const p of list) {
    const g = p.vendor || "Unknown";
    if (!groups.has(g)) groups.set(g, []);
    groups.get(g)!.push(p);
  }
  const load = (id: string) => {
    if (!track) return;
    if (isInst) api.setPlugin(track.id, INSTRUMENT_SLOT, id, true);
    else {
      const slot = track.inserts.findIndex((s) => !s);
      if (slot < 0) setState({ toast: { text: "All Audio FX slots are in use", at: Date.now() } });
      else loadPlugin(track.id, slot, id, true);
    }
  };
  const current = (t: Track | null) => (t?.kind === "instrument" ? t.instrument?.plugin_id : undefined);
  return (
    <div className="library">
      <div className="pane-title">Library</div>
      <input className="search" placeholder="Search" value={q} onChange={(e) => setQ(e.target.value)} onKeyDown={(e) => e.stopPropagation()} />
      {!track ? (
        <div className="insp-empty">Select a track</div>
      ) : track.kind === "folder" ? (
        // A folder stack has no channel strip of its own to put plug-ins on.
        <div className="insp-empty">Folder stacks have no plug-ins</div>
      ) : (
        <div className="lib-list">
          <div className="lib-group">{isInst ? "Instruments" : "Audio FX"}</div>
          {isInst && (
            <div className={"lib-item" + (current(track) === BUILTIN_SYNTH ? " on" : "")} onClick={() => load(BUILTIN_SYNTH)}>
              Fermata Synth
            </div>
          )}
          {[...fxGroups.entries()]
            .sort((a, b) => a[0].localeCompare(b[0]))
            .map(([cat, fs]) => (
              <div key={`fx:${cat}`}>
                <div className="lib-vendor">{cat}</div>
                {fs.map((f) => (
                  <div key={f.id} className="lib-item" onClick={() => load(f.id)} title={`Fermata ${f.category}`}>
                    {f.name}
                  </div>
                ))}
              </div>
            ))}
          {[...groups.entries()].map(([vendor, ps]) => (
            <div key={vendor}>
              <div className="lib-vendor">{vendor}</div>
              {ps.map((p) => (
                <div key={p.id} className={"lib-item" + (current(track) === p.id ? " on" : "")} onClick={() => load(p.id)} title={p.subcategories}>
                  {p.name}
                </div>
              ))}
            </div>
          ))}
          {list.length === 0 && builtins.length === 0 && <div className="insp-empty">No matching plug-ins</div>}
        </div>
      )}
      <div className="pane-foot">
        <span className="dim">{plugins.length} plug-ins</span>
        <button
          className="small-btn"
          disabled={rescanning}
          onClick={async () => {
            setRescanning(true);
            const l = await call<typeof plugins>("rescan_plugins");
            if (l) setState({ plugins: l });
            setRescanning(false);
          }}
        >
          {rescanning ? "Scanning…" : "Rescan"}
        </button>
      </div>
    </div>
  );
}

interface DirListing {
  path: string;
  parent: string | null;
  entries: { name: string; path: string; is_dir: boolean }[];
}

export function Browser() {
  const [dir, setDir] = useState<DirListing | null>(null);
  const load = (path: string | null) => call<DirListing>("list_dir", { path }).then((d) => d && setDir(d));
  useEffect(() => {
    load(null);
  }, []);
  const importAt = async (path: string) => {
    const snap = getState().snap;
    const pos = currentTick()?.pos ?? 0;
    // MIDI files go onto the selected instrument track, or a new one.
    if (isMidiPath(path)) {
      const inst = snap?.project.tracks.find((t) => t.id === snap.selected && t.kind === "instrument");
      await importMidiFiles([path], inst?.id ?? null, pos, null);
      return;
    }
    if (!isAudioPath(path)) {
      setState({ toast: { text: "Only audio and MIDI files can be imported", at: Date.now() } });
      return;
    }
    const track = snap?.project.tracks.find((t) => t.id === snap.selected && t.kind === "audio");
    if (!track) {
      setState({ toast: { text: "Select an audio track to import into", at: Date.now() } });
      return;
    }
    act("import_audio", { path, track: track.id, start: pos });
  };
  return (
    <div className="browser">
      <div className="pane-title">All Files</div>
      <div className="browser-path">
        <button className="small-btn icon" disabled={!dir?.parent} onClick={() => dir?.parent && load(dir.parent)} title="Enclosing folder">
          <Icon.up />
        </button>
        <span title={dir?.path}>{dir?.path.split("/").pop() || dir?.path}</span>
      </div>
      <div className="browser-list">
        {dir?.entries.map((e) => (
          <div
            key={e.path}
            className={"browser-item" + (e.is_dir ? " dir" : "")}
            onMouseDown={(ev) => !e.is_dir && startFileDrag(ev, e.path, e.name)}
            onDoubleClick={() => (e.is_dir ? load(e.path) : importAt(e.path))}
            title={e.is_dir ? "Double-click to open" : "Drag onto a track (or the track list for a new track), or double-click to import at the playhead"}
          >
            {e.is_dir ? <Icon.folder /> : <Icon.file />}
            <span>{e.name}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
