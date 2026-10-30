// Logic's Arrangement track, the top global track: named sections (Intro, Verse, Chorus…) that
// tile the timeline. The + button adds one after the last. Click a section to select it and
// the regions in it; click its name for the name pop-up; double-click to rename. Drag a
// section to move it, with its content, to another place in the order (Option/Alt-drag
// copies it); drag its right edge to resize it (the later sections move along). Right-click
// for colour and delete (with or without content).
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { act, actMade, type ArrangementMarker, type Id, type Project } from "../api";
import { ARRANGEMENT_NAMES, dropSlot, regionsInSection, resizePreview, sectionColor, sectionEnd, sections, type Drop } from "../arrangement";
import { coalesced } from "../frame";
import { Icon } from "../icons";
import { colorMenu } from "../ops";
import { getState, setState, useStore } from "../store";
import { tempoMap } from "../tempo";
import { barsFrom, samplesPerBeat, snapPos } from "../time";
import { mix, theme } from "../theme";
import { pixelRatio } from "../uiScale";
import { onViewport, viewport } from "../view";
import { openMenu, type MenuItem } from "./Menu";

/** Height of the Arrangement lane. */
export const ARR_H = 22;
/** How close to a section's right edge a drag resizes it, in pixels. */
const EDGE_PX = 5;

const project = () => getState().snap?.project;

export function addSection(name?: string) {
  actMade("add_arrangement_marker", { name: name ?? null }).then((ids) => ids?.length && setState({ selectedSection: ids[0] }));
}

function renameSection(s: ArrangementMarker) {
  const name = window.prompt("Arrangement marker name", s.name);
  if (name !== null && name.trim() && name !== s.name) act("rename_arrangement_marker", { id: s.id, name: name.trim() });
}

/** Selects a section and every region in it, as clicking it does in Logic. */
export function selectSection(s: ArrangementMarker) {
  const p = project();
  if (!p) return;
  setState({ selectedSection: s.id, selectedRegions: regionsInSection(p, s), focus: "regions" });
}

/** Logic's name pop-up: the usual names, then Rename… for any other. */
function nameMenu(s: ArrangementMarker): MenuItem[] {
  return [
    ...ARRANGEMENT_NAMES.map((n) => ({ label: n, checked: n === s.name, onClick: () => act("rename_arrangement_marker", { id: s.id, name: n }) })),
    { separator: true },
    { label: "Rename…", onClick: () => renameSection(s) },
  ];
}

/** Right-click on a section. */
function sectionMenu(s: ArrangementMarker): MenuItem[] {
  const end = sectionEnd(s);
  return [
    { label: "Name", children: nameMenu(s) },
    { label: "Color", children: colorMenu(s.color, (c) => act("set_arrangement_marker_color", { id: s.id, color: c }), "By Name") },
    { separator: true },
    { label: "Select Regions in Section", onClick: () => selectSection(s) },
    { label: "Set Locators by Section", onClick: () => act("set_cycle", { enabled: true, start: s.start, end }) },
    { label: "Go to Section", onClick: () => act("locate", { pos: s.start }) },
    { separator: true },
    { label: "Delete Arrangement Marker", onClick: () => act("delete_arrangement_marker", { id: s.id, content: false }) },
    { label: "Delete Arrangement Marker and Content", onClick: () => act("delete_arrangement_marker", { id: s.id, content: true }) },
  ];
}

/** The lane's header: the track name and the + button. */
export function ArrangementHeader() {
  return (
    <div className="global-name arrangement-name" style={{ height: ARR_H }}>
      Arrangement
      <button className="arrangement-add" title="Add Arrangement Marker" onClick={() => addSection()}>
        <Icon.plus />
      </button>
    </div>
  );
}

/** What a drag shows until it is let go. */
type Preview = { kind: "resize"; list: ArrangementMarker[] } | { kind: "move"; id: Id; start: number; drop: Drop | null; copy: boolean };

export function ArrangementLane() {
  const proj = useStore((s) => s.snap?.project);
  const pxPerBeat = useStore((s) => s.pxPerBeat);
  const selected = useStore((s) => s.selectedSection);
  useStore((s) => s.themeId);
  useStore((s) => s.uiScale);
  const live = useRef({ project: proj, pxPerBeat, selected });
  live.current = { project: proj, pxPerBeat, selected };
  const ref = useRef<HTMLCanvasElement>(null);
  const preview = useRef<Preview | null>(null);
  /** Each section's name label as last drawn (content x), for the name pop-up. */
  const labels = useRef<Map<Id, [number, number]>>(new Map());

  const pps = () => {
    const { project, pxPerBeat } = live.current;
    return project ? pxPerBeat / samplesPerBeat(project) : 1;
  };

  const draw = () => {
    const cv = ref.current;
    const { project, selected } = live.current;
    if (!cv || !project) return;
    const W = viewport().width;
    const left = viewport().left;
    const k = pps();
    const dpr = pixelRatio();
    if (cv.width !== Math.round(W * dpr) || cv.height !== Math.round(ARR_H * dpr)) {
      cv.width = Math.round(W * dpr);
      cv.height = Math.round(ARR_H * dpr);
      cv.style.width = `${W}px`;
      cv.style.height = `${ARR_H}px`;
    }
    const ctx = cv.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.fillStyle = theme.rulerCycleZone;
    ctx.fillRect(0, 0, W, ARR_H);
    ctx.fillStyle = theme.barLine;
    for (const b of tempoMap(project).bars(Math.max(0, left / k), (left + W) / k)) ctx.fillRect(Math.round(b.start * k - left) + 0.5, 0, 1, ARR_H);
    ctx.fillStyle = theme.rulerEdge;
    ctx.fillRect(0, ARR_H - 1, W, 1);

    const pv = preview.current;
    const list = pv?.kind === "resize" ? pv.list : sections(project);
    ctx.font = "600 10px Inter, 'SF Pro Text', system-ui, sans-serif";
    ctx.textBaseline = "middle";
    labels.current = new Map();
    const box = (s: ArrangementMarker, x0: number, x1: number, alpha: number, on: boolean) => {
      if (x1 < 0 || x0 > W) return;
      const color = sectionColor(s);
      ctx.globalAlpha = alpha;
      ctx.fillStyle = on ? mix(color, "#ffffff", 0.2) : color;
      ctx.beginPath();
      ctx.roundRect(x0 + 0.5, 2.5, Math.max(2, x1 - x0 - 1), ARR_H - 5, 3);
      ctx.fill();
      if (on) {
        ctx.strokeStyle = theme.selectOutline;
        ctx.lineWidth = 1;
        ctx.stroke();
      }
      // The name, with the pop-up's chevron, kept in view while the section scrolls past.
      const tx = Math.max(x0 + 6, Math.min(6, x1 - 40));
      ctx.save();
      ctx.beginPath();
      ctx.rect(x0 + 2, 0, Math.max(0, x1 - x0 - 6), ARR_H);
      ctx.clip();
      ctx.fillStyle = "rgba(10,10,12,0.9)";
      ctx.fillText(s.name, tx, ARR_H / 2);
      const tw = ctx.measureText(s.name).width;
      ctx.fillText("▾", tx + tw + 3, ARR_H / 2 + 0.5);
      ctx.restore();
      ctx.globalAlpha = 1;
      labels.current.set(s.id, [tx + left, tx + tw + 12 + left]);
    };
    for (const s of list) {
      const moving = pv?.kind === "move" && pv.id === s.id && !pv.copy;
      box(s, s.start * k - left, sectionEnd(s) * k - left, moving ? 0.35 : 1, s.id === selected);
    }
    if (pv?.kind === "move") {
      // The section under the pointer, and where it would land.
      const s = list.find((x) => x.id === pv.id);
      if (s) box({ ...s, start: pv.start }, pv.start * k - left, (pv.start + s.length) * k - left, 0.75, true);
      if (pv.drop) {
        ctx.fillStyle = theme.text;
        ctx.fillRect(Math.round(pv.drop.at * k - left) - 1, 0, 2, ARR_H);
      }
    }
  };

  const drawRef = useRef(draw);
  drawRef.current = draw;
  const [requestDraw] = useState(() => coalesced(() => drawRef.current()));
  useLayoutEffect(() => draw());
  useEffect(() => {
    const off = onViewport(() => requestDraw());
    // A click anywhere else drops the section selection (the regions stay selected).
    const down = (e: MouseEvent) => {
      if (e.target !== ref.current && getState().selectedSection !== null) setState({ selectedSection: null });
    };
    window.addEventListener("mousedown", down, true);
    return () => {
      off();
      window.removeEventListener("mousedown", down, true);
    };
  }, [requestDraw]);

  const sampleAt = (clientX: number) => (clientX - ref.current!.getBoundingClientRect().left + viewport().left) / pps();

  const onMouseDown = (e: React.MouseEvent) => {
    const p = live.current.project;
    if (!p || e.button !== 0) return;
    const at = sampleAt(e.clientX);
    const list = sections(p);
    const k = pps();
    const edge = list.find((s) => Math.abs((sectionEnd(s) - at) * k) <= EDGE_PX);
    if (edge) return resizeDown(e, p, list, edge);
    const s = list.find((x) => at >= x.start && at < sectionEnd(x));
    if (!s) {
      setState({ selectedSection: null });
      if (e.detail === 2) addSection();
      return;
    }
    if (e.detail === 2) {
      renameSection(s);
      return;
    }
    moveDown(e, list, s, at);
  };

  const resizeDown = (e: React.MouseEvent, p: Project, list: ArrangementMarker[], s: ArrangementMarker) => {
    e.preventDefault();
    const minEnd = barsFrom(p, s.start, 1);
    const move = (ev: MouseEvent) => {
      // Sections snap to bars.
      const end = Math.max(minEnd, snapPos(p, sampleAt(ev.clientX), "bar", live.current.pxPerBeat));
      preview.current = { kind: "resize", list: resizePreview(list, s.id, Math.round(end - s.start)) };
      draw();
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      const pv = preview.current;
      preview.current = null;
      const len = pv?.kind === "resize" ? pv.list.find((x) => x.id === s.id)?.length : undefined;
      if (len !== undefined && len !== s.length) act("resize_arrangement_marker", { id: s.id, length: len });
      else draw();
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  const moveDown = (e: React.MouseEvent, list: ArrangementMarker[], s: ArrangementMarker, at: number) => {
    e.preventDefault();
    const startX = e.clientX;
    let moved = false;
    const move = (ev: MouseEvent) => {
      if (!moved && Math.abs(ev.clientX - startX) < 4) return;
      moved = true;
      const start = Math.max(0, s.start + sampleAt(ev.clientX) - at);
      const copy = ev.altKey;
      preview.current = { kind: "move", id: s.id, start, drop: dropSlot(list, s.id, start, copy), copy };
      draw();
    };
    const up = async (ev: MouseEvent) => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      const pv = preview.current;
      preview.current = null;
      if (!moved) {
        selectSection(s);
        // A click on the name opens the name pop-up.
        const lab = labels.current.get(s.id);
        const x = at * pps();
        if (lab && x >= lab[0] - 2 && x <= lab[1]) openMenu(ev.clientX, ev.clientY, nameMenu(s));
        return;
      }
      if (pv?.kind !== "move" || !pv.drop) return draw();
      const copy = pv.copy || ev.altKey;
      const ids = await actMade("move_arrangement_marker", { id: s.id, before: pv.drop.before, copy });
      if (ids) setState({ selectedRegions: ids, focus: "regions", selectedSection: copy ? null : s.id });
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  const onContextMenu = (e: React.MouseEvent) => {
    const p = live.current.project;
    if (!p) return;
    e.preventDefault();
    const at = sampleAt(e.clientX);
    const s = sections(p).find((x) => at >= x.start && at < sectionEnd(x));
    if (s) {
      setState({ selectedSection: s.id });
      openMenu(e.clientX, e.clientY, sectionMenu(s));
    } else {
      openMenu(e.clientX, e.clientY, [
        { label: "Add Arrangement Marker", onClick: () => addSection() },
        { label: "Add", children: ARRANGEMENT_NAMES.map((n) => ({ label: n, onClick: () => addSection(n) })) },
      ]);
    }
  };

  return <canvas ref={ref} className="arrangement-lane" onMouseDown={onMouseDown} onContextMenu={onContextMenu} />;
}
