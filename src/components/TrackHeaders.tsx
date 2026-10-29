import { useEffect, useRef, useState, type ReactNode } from "react";
import { act, api, canRecord, isStack, type Group, type Id, type Track } from "../api";
import { Icon } from "../icons";
import { getState, setState, useStore } from "../store";
import { clampHeight, setTrackHeight, useLayout } from "../layout";
import { onViewport, viewport } from "../view";
import { clickTrack, selectedTracks, setHidden, toggleCollapsed, trackMenu } from "../ops";
import { openMenu, openMenuAt, type MenuItem } from "./Menu";
import { automatable, paramMenu, useAutomation, VOLUME } from "../automation";
import { AutoModeButton, AutomationControls } from "./AutomationHeader";
import { canFreeze, toggleFreeze } from "../freeze";
import { takeRowsHeight } from "../takes";
import { fmt, keyHint } from "../keymap";
import { headerTier } from "../trackHeader";
import { LevelSlider } from "./LevelSlider";

const INDENT = 12;

function StackIcon({ summing }: { summing: boolean }) {
  return (
    <svg width="22" height="22" viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinejoin="round">
      <path d="M2.5 6h5l1.5 1.8h8.5v8.7h-15z" />
      {summing && <path d="M7 12.2h6M10 9.2v6" />}
    </svg>
  );
}

/** The "⋯" of the button menu on short headers. */
function MoreIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" fill="currentColor">
      <circle cx="2" cy="6" r="1.25" />
      <circle cx="6" cy="6" r="1.25" />
      <circle cx="10" cy="6" r="1.25" />
    </svg>
  );
}

/** One of a header's switches (M S R I, freeze): its button and its item in the "⋯" menu. */
interface Switch {
  cls: string;
  label: ReactNode;
  name: string;
  on: boolean;
  title: string;
  hint?: string;
  toggle: () => void;
}

/** Two signals merging into one bus. */
function AuxIcon() {
  return (
    <svg width="22" height="22" viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round">
      <path d="M2.5 5.5h4l4 4.5h7M2.5 14.5h4l4-4.5" />
      <path d="M15 7.5l2.5 2.5-2.5 2.5" />
    </svg>
  );
}

function GroupBadge({ group, onClick }: { group: Group; onClick: () => void }) {
  const short = /(\d+)$/.exec(group.name)?.[1] ?? group.name.slice(0, 2);
  return (
    <button
      className={"th-group" + (group.enabled ? "" : " off")}
      style={{ background: group.enabled ? group.color : undefined, borderColor: group.color }}
      title={`${group.name}${group.enabled ? "" : " (off)"} — click for settings`}
      onMouseDown={(e) => {
        e.stopPropagation();
        onClick();
      }}
    >
      {/^\d+$/.test(short) ? `G${short}` : short}
    </button>
  );
}

function Header({
  track,
  index,
  selected,
  primary,
  height,
  depth,
  group,
  hideMode,
  onReorderStart,
}: {
  track: Track;
  index: number;
  selected: boolean;
  primary: boolean;
  height: number;
  depth: number;
  group: Group | undefined;
  /** Hide mode (H) is on: show the H button. */
  hideMode: boolean;
  onReorderStart: (e: React.MouseEvent, index: number) => void;
}) {
  const [editing, setEditing] = useState(false);
  const stack = isStack(track.kind);
  const [volume, volumeTouch] = useAutomation(track, VOLUME, track.volume_db);
  // Short headers put everything on one line and move the buttons into a "⋯" menu.
  const tier = headerTier(height);
  const full = tier === "full";
  // The automation row takes a third line on tall tracks, or the buttons' place.
  const automation = useStore((s) => s.showAutomation) && automatable(track);
  const autoRow = full && automation && height >= 76;
  const switches: Switch[] = [
    { cls: "mute", label: "M", name: "Mute", on: track.mute, title: "Mute", hint: keyHint("track-mute"), toggle: () => api.updateTrack(track.id, { mute: !track.mute }) },
    { cls: "solo", label: "S", name: "Solo", on: track.solo, title: "Solo", hint: keyHint("track-solo"), toggle: () => api.updateTrack(track.id, { solo: !track.solo }) },
  ];
  if (canRecord(track.kind)) switches.push({ cls: "rec", label: "R", name: "Record Enable", on: track.armed, title: "Record Enable", hint: keyHint("track-arm"), toggle: () => api.updateTrack(track.id, { armed: !track.armed }) });
  if (track.kind === "audio") switches.push({ cls: "mon", label: "I", name: "Input Monitoring", on: track.monitor, title: "Input Monitoring", toggle: () => api.updateTrack(track.id, { monitor: !track.monitor }) });
  if (canFreeze(track)) {
    switches.push({
      cls: "freeze",
      label: <Icon.snowflake />,
      name: "Freeze",
      on: !!track.frozen,
      title: track.frozen ? "Frozen: plays a render, plug-ins unloaded. Click to unfreeze" : "Freeze: render the instrument and plug-ins, then unload them",
      // Acts on the selection when this track is part of it, as Logic's buttons do.
      toggle: () => toggleFreeze(selected ? selectedTracks() : [track.id]),
    });
  }
  const btn = (sw: Switch) => (
    <button
      key={sw.cls}
      className={`tbtn ${sw.cls}` + (sw.on ? " on" : "")}
      title={sw.title}
      onMouseDown={(e) => {
        e.stopPropagation();
        sw.toggle();
      }}
    >
      {sw.label}
    </button>
  );
  // The "⋯" menu: every switch, checked when on, then the automation lane's parameter when
  // automation is shown (its mode button stays in the header).
  const openMore = async (el: HTMLElement) => {
    const items: MenuItem[] = switches.map((sw) => ({ label: sw.name, checked: sw.on, hint: sw.hint, onClick: sw.toggle }));
    if (automation) items.push({ separator: true }, { label: "Automation Parameter", children: await paramMenu(track) });
    openMenuAt(el, items);
  };
  // Drag the bottom edge to resize this track; Alt-drag sizes every track.
  const startResize = (e: React.MouseEvent) => {
    e.stopPropagation();
    e.preventDefault();
    const startY = e.clientY;
    // Open take folders add rows below the lane; the track's own height is what's resized.
    const startH = height - takeRowsHeight(track, getState().openFolders);
    const all = e.altKey;
    const move = (ev: MouseEvent) => {
      const h = clampHeight(startH + ev.clientY - startY);
      if (all) setState({ trackHeight: h, trackHeights: {} });
      else setTrackHeight(track.id, h);
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      document.body.style.cursor = "";
    };
    document.body.style.cursor = "ns-resize";
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };
  const menu = (e: React.MouseEvent) => {
    e.preventDefault();
    // Right-clicking outside the selection selects just that track first, as in Logic.
    if (!selected) clickTrack(track.id, { shiftKey: false, ctrlKey: false, metaKey: false });
    openMenu(e.clientX, e.clientY, trackMenu(track, () => setEditing(true)));
  };
  return (
    <div
      className={`track-header tier-${tier}` + (selected ? " selected" : "") + (primary ? " primary" : "") + (stack ? " stack" : "") + (track.hidden ? " is-hidden" : "") + (track.frozen ? " frozen" : "")}
      style={{ height }}
      onMouseDown={(e) => {
        if (e.button === 0) clickTrack(track.id, e);
      }}
      onContextMenu={menu}
    >
      <div className="th-color" style={{ background: track.color }} />
      <div className="th-num" onMouseDown={(e) => onReorderStart(e, index)} title="Drag to reorder">
        {index + 1}
      </div>
      {depth > 0 && <div className="th-indent" style={{ width: depth * INDENT }} />}
      {stack && (
        <button
          className="th-disclosure"
          title={track.collapsed ? "Show subtracks" : "Hide subtracks"}
          onMouseDown={(e) => {
            e.stopPropagation();
            toggleCollapsed(track);
          }}
        >
          {Icon.disclosure(!track.collapsed)}
        </button>
      )}
      {tier !== "tiny" && (
        <div className="th-icon" style={{ color: track.color }} onMouseDown={(e) => onReorderStart(e, index)} title={stack ? (track.kind === "summing" ? "Summing Stack" : "Folder Stack") : track.kind === "aux" ? `Aux · Bus ${track.bus}` : undefined}>
          {track.kind === "audio" ? <Icon.waveform /> : track.kind === "instrument" ? <Icon.keyboard /> : track.kind === "aux" ? <AuxIcon /> : <StackIcon summing={track.kind === "summing"} />}
        </div>
      )}
      <div className="th-main">
        {editing ? (
          <input
            className="th-rename"
            autoFocus
            defaultValue={track.name}
            onMouseDown={(e) => e.stopPropagation()}
            onBlur={(e) => {
              setEditing(false);
              const v = e.target.value.trim();
              if (v && v !== track.name) api.updateTrack(track.id, { name: v });
            }}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "Enter") (e.target as HTMLInputElement).blur();
              if (e.key === "Escape") setEditing(false);
            }}
          />
        ) : (
          <div className="th-name" onDoubleClick={() => setEditing(true)} title={track.name}>
            {track.name}
          </div>
        )}
        {full && automation && !autoRow && <AutomationControls track={track} />}
        <div className="th-buttons" hidden={full && automation && !autoRow}>
          {/* Short headers keep only the switches that are on, so a muted, soloed, armed or
              frozen track still shows it; the rest are in the "⋯" menu. */}
          {(full ? switches : switches.filter((sw) => sw.on)).map(btn)}
          {group && <GroupBadge group={group} onClick={() => setState({ groupSheet: group.id })} />}
          {hideMode && (
            <button
              className={"tbtn hide" + (track.hidden ? " on" : "")}
              title={track.hidden ? "Hidden: click to show this track" : "Hide this track"}
              onMouseDown={(e) => {
                e.stopPropagation();
                setHidden([track.id], !track.hidden);
              }}
            >
              H
            </button>
          )}
          {!full && automation && <AutoModeButton track={track} compact />}
          {!full && (
            <button
              className="tbtn th-more"
              title="Mute, Solo, Record and more"
              onMouseDown={(e) => {
                e.stopPropagation();
                openMore(e.currentTarget);
              }}
            >
              <MoreIcon />
            </button>
          )}
        </div>
        {autoRow && <AutomationControls track={track} />}
      </div>
      <div className="th-volume">
        <LevelSlider
          db={volume}
          onChange={(db) => api.updateTrack(track.id, { volume_db: db })}
          width={Math.max(56, 88 - depth * INDENT)}
          tier={tier}
          track={track.kind === "folder" ? null : track.id}
          label={track.kind === "folder" ? "Folder volume (moves every subtrack)" : "Volume"}
          {...volumeTouch}
        />
      </div>
      <div
        className="th-resize"
        title={`Drag to resize · ${fmt("Alt")}-drag resizes all tracks · double-click to reset`}
        onMouseDown={startResize}
        onDoubleClick={(e) => {
          e.stopPropagation();
          setTrackHeight(track.id, null);
        }}
      />
    </div>
  );
}

export function TrackHeaders() {
  const project = useStore((s) => s.snap?.project);
  const selected = useStore((s) => s.snap?.selected ?? null);
  const multi = useStore((s) => s.selectedTracks);
  const selectedSet = new Set(selected !== null && multi.includes(selected) ? multi : selected !== null ? [selected] : []);
  const layout = useLayout();
  const dropTarget = useStore((s) => s.dropTarget);
  const hideMode = useStore((s) => s.showHidden);
  const fileDrop = dropTarget && dropTarget.kind !== "lane" ? dropTarget.index : null;
  const inner = useRef<HTMLDivElement>(null);
  const [drop, setDrop] = useState<number | null>(null);

  useEffect(() => {
    const apply = () => {
      if (inner.current) inner.current.style.transform = `translateY(${-viewport().top}px)`;
    };
    apply();
    return onViewport(apply);
  }, []);

  // Dragging a track moves it with its subtracks. Where it lands decides its stack: it joins
  // the stack of the track it's dropped above (or the stack whose header it's dropped below).
  const startReorder = (e: React.MouseEvent, index: number) => {
    if (e.button !== 0 || !project) return;
    e.stopPropagation();
    clickTrack(project.tracks[index].id, e);
    const startY = e.clientY;
    const headerTop = (e.currentTarget as HTMLElement).closest(".track-header")!.getBoundingClientRect().top;
    const startContentY = layout.tops[index] + (startY - headerTop);
    let target = index;
    let moved = false;
    const move = (ev: MouseEvent) => {
      if (!moved && Math.abs(ev.clientY - startY) < 4) return;
      moved = true;
      // Insert before the first track whose middle is below the pointer.
      const y = startContentY + (ev.clientY - startY);
      target = layout.tops.filter((t, i) => t + layout.heights[i] / 2 < y).length;
      setDrop(target);
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      setDrop(null);
      if (!moved) return;
      const tracks = project.tracks;
      const id = tracks[index].id;
      const block = new Set<Id>([id]);
      for (const t of tracks) if (t.parent !== null && block.has(t.parent)) block.add(t.id);
      // Dropping inside its own block changes nothing.
      const blockEnd = index + block.size;
      if (target >= index && target <= blockEnd) return;
      const below = tracks.slice(target).find((t) => !block.has(t.id));
      const above = tracks.slice(0, target).reverse().find((t) => !block.has(t.id));
      let parent: Id | null = below ? below.parent : null;
      // Dropped right under an open stack's header: into the stack.
      if (above && isStack(above.kind) && !above.collapsed) parent = above.id;
      // A collapsed stack's hidden subtracks sit under its header: drop after them instead.
      const rest = tracks.filter((t) => !block.has(t.id));
      let at = rest.findIndex((t) => t.id === below?.id);
      if (at < 0) at = rest.length;
      const ids = rest.map((t) => t.id);
      ids.splice(at, 0, ...tracks.filter((t) => block.has(t.id)).map((t) => t.id));
      act("move_tracks", { ids, parents: [[id, parent]] });
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  if (!project) return null;
  return (
    <div
      className="headers"
      onWheel={(e) => {
        // Forward wheel scrolling to the arrange view.
        const el = document.querySelector(".arrange-scroll") as HTMLElement | null;
        if (el) el.scrollTop += e.deltaY;
      }}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) setState({ selectedRegions: [] });
      }}
      onContextMenu={(e) => {
        if (e.target !== e.currentTarget && !(e.target as HTMLElement).classList.contains("headers-inner")) return;
        e.preventDefault();
        openMenu(e.clientX, e.clientY, [{ label: "New Tracks…", hint: fmt("Mod+Alt+N"), onClick: () => setState({ dialog: "new-tracks" }) }]);
      }}
    >
      <div ref={inner} className="headers-inner">
        {project.tracks.map((t, i) =>
          layout.heights[i] > 0 ? (
            <Header
              key={t.id}
              track={t}
              index={i}
              selected={selectedSet.has(t.id)}
              primary={t.id === selected && selectedSet.size > 1}
              height={layout.heights[i]}
              depth={layout.depth[i]}
              group={t.group !== null ? project.groups.find((g) => g.id === t.group) : undefined}
              hideMode={hideMode}
              onReorderStart={startReorder}
            />
          ) : null,
        )}
        {drop !== null && <div className="reorder-line" style={{ top: (drop < layout.tops.length ? layout.tops[drop] : layout.total) - 1 }} />}
        {fileDrop !== null && (
          <div className="drop-new-track" style={{ top: (fileDrop < layout.tops.length ? layout.tops[fileDrop] : layout.total) - 1 }}>
            <span>New track</span>
          </div>
        )}
        {project.tracks.length === 0 && (
          <div className="headers-empty">
            <button className="big-add" onClick={() => setState({ dialog: "new-tracks" })}>
              New Track…
            </button>
            <span className="dim">{fmt("Mod+Alt+N")}</span>
          </div>
        )}
      </div>
    </div>
  );
}

export function useSelectedTrack(): Track | null {
  return useStore((s) => s.snap?.project.tracks.find((t) => t.id === s.snap?.selected) ?? null);
}

