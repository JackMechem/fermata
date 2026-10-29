// Unit tests for the overview strip's mapping (src/overview.ts): time across the strip, track
// rows, the visible-area box, and the scrolling and zoom its drags produce.
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  ZOOM_MAX,
  ZOOM_MIN,
  anchoredLeft,
  centreOn,
  dragScroll,
  hitBox,
  overviewRows,
  overviewSpan,
  timeToX,
  visibleBox,
  wheelZoom,
  xToTime,
  zoomFromEdge,
  type StripMap,
} from "../src/overview.ts";

const close = (a: number, b: number, eps = 1e-6) => assert.ok(Math.abs(a - b) < eps, `${a} ≈ ${b}`);

// 1000 px strip showing 100 000 samples; 400 px of tracks in 35 px of rows from y = 4.
const map: StripMap = { span: 100_000, width: 1000, total: 400, rowsTop: 4, rowsH: 35 };

test("the span covers the content plus padding, and stretches to the view", () => {
  assert.equal(overviewSpan(80_000, 20_000, 5_000), 85_000);
  assert.equal(overviewSpan(80_000, 120_000, 5_000), 120_000, "the view past the end widens it");
  assert.equal(overviewSpan(0, 30_000, 5_000), 30_000, "an empty project shows the view");
  assert.equal(overviewSpan(0, 0, 0), 1, "never zero");
});

test("time and strip x map both ways", () => {
  assert.equal(timeToX(50_000, map.span, map.width), 500);
  assert.equal(xToTime(250, map.span, map.width), 25_000);
  close(xToTime(timeToX(12_345, map.span, map.width), map.span, map.width), 12_345);
});

test("N equal tracks share the rows' height; hidden tracks get none", () => {
  const rows = overviewRows([0, 64, 128, 192], [64, 64, 64, 64], 256, 4, 32);
  assert.deepEqual(rows, [
    { y: 4, h: 8 },
    { y: 12, h: 8 },
    { y: 20, h: 8 },
    { y: 28, h: 8 },
  ]);
  const hidden = overviewRows([0, 64, 64], [64, 0, 64], 128, 0, 30);
  assert.deepEqual(hidden, [
    { y: 0, h: 15 },
    { y: 15, h: 0 },
    { y: 15, h: 15 },
  ]);
});

test("many tracks give sub-pixel rows that still add up to the strip", () => {
  const n = 200;
  const tops = Array.from({ length: n }, (_, i) => i * 64);
  const rows = overviewRows(tops, Array(n).fill(64), n * 64, 4, 35);
  close(rows[0].h, 35 / 200);
  close(rows[n - 1].y + rows[n - 1].h, 39);
});

test("tall tracks get taller rows (rows follow the Tracks area's heights)", () => {
  const rows = overviewRows([0, 150], [150, 50], 200, 0, 40);
  assert.deepEqual(rows, [
    { y: 0, h: 30 },
    { y: 30, h: 10 },
  ]);
});

test("the box spans the time and tracks in view", () => {
  // At 0.02 px per sample, a view 400 px wide at left 200 px shows samples 10 000 to 30 000.
  const b = visibleBox({ left: 200, top: 100, width: 400, height: 200 }, 0.02, map);
  close(b.x, 100);
  close(b.w, 200);
  // Tracks 100–300 of 400 → a quarter to three quarters of the 35 px rows.
  close(b.y, 4 + 35 * 0.25);
  close(b.h, 35 * 0.5);
});

test("the box stays inside the strip and keeps a usable size", () => {
  // Scrolled below the last track: clamped to the bottom of the rows, minH tall.
  const below = visibleBox({ left: 0, top: 500, width: 400, height: 200 }, 0.02, map);
  close(below.y + below.h, 39);
  close(below.h, 4);
  // A tiny time span in a long project: at least minW wide, centred on the view.
  const tiny = visibleBox({ left: 1000, top: 0, width: 10, height: 400 }, 1, map);
  close(tiny.w, 6);
  close(tiny.x + tiny.w / 2, timeToX(1005, map.span, map.width));
  // Tracks shorter than the view: the box covers all the rows.
  const all = visibleBox({ left: 0, top: 0, width: 400, height: 900 }, 0.02, map);
  close(all.y, 4);
  close(all.h, 35);
  // No tracks at all.
  const none = visibleBox({ left: 0, top: 0, width: 400, height: 300 }, 0.02, { ...map, total: 0 });
  close(none.y, 4);
  close(none.h, 35);
});

test("dragging the box scrolls by the strip's scale", () => {
  const d = dragScroll(10, 7, 0.02, map);
  // 10 strip px = 1000 samples = 20 view px; 7 of 35 row px = 80 of 400 track px.
  close(d.dLeft, 20);
  close(d.dTop, 80);
});

test("a click outside centres the view on it", () => {
  const v = { left: 0, top: 0, width: 400, height: 200 };
  const c = centreOn(500, 4 + 35 / 2, v, 0.02, map);
  // Sample 50 000 at 0.02 px/sample is 1000 px; centred in 400 px → left 800.
  close(c.left, 800);
  // The middle of the tracks (200 px) centred in 200 px → top 100.
  close(c.top, 100);
  const start = centreOn(0, 4, v, 0.02, map);
  assert.deepEqual(start, { left: 0, top: 0 }, "clamped at the start");
});

test("dragging an edge zooms so the view spans the two edges", () => {
  const spb = 24_000; // 120 bpm at 48 kHz
  // Right edge: the left stays at 48 000, the right goes to 96 000 (two bars) in an 800 px view.
  const r = zoomFromEdge("right", 48_000, 96_000, 800, spb);
  close(r.pxPerBeat, 400); // 48 000 samples = 2 beats in 800 px
  close(r.start, 48_000);
  // Left edge: the right stays at 480 000, the left goes to 0 → 20 beats in 800 px.
  const l = zoomFromEdge("left", 480_000, 0, 800, spb);
  close(l.pxPerBeat, 40);
  close(l.start, 0);
});

test("edge zoom keeps the zoom limits and moves the dragged edge to match", () => {
  const spb = 24_000;
  const inMax = zoomFromEdge("right", 48_000, 48_100, 800, spb);
  assert.equal(inMax.pxPerBeat, ZOOM_MAX);
  close(inMax.start, 48_000);
  const crossed = zoomFromEdge("left", 48_000, 60_000, 800, spb);
  assert.equal(crossed.pxPerBeat, ZOOM_MAX, "dragging past the other edge zooms all the way in");
  close(crossed.start, 48_000 - (800 / ZOOM_MAX) * spb);
  const outMax = zoomFromEdge("left", 100_000_000, 0, 800, spb);
  assert.equal(outMax.pxPerBeat, ZOOM_MIN);
  assert.equal(outMax.start, 100_000_000 - (800 / ZOOM_MIN) * spb);
});

test("wheel zoom steps like the tracks, within the limits, around an anchor", () => {
  close(wheelZoom(100, -1), 115);
  close(wheelZoom(115, 1), 100);
  assert.equal(wheelZoom(ZOOM_MAX, -1), ZOOM_MAX);
  assert.equal(wheelZoom(ZOOM_MIN, 1), ZOOM_MIN);
  // Sample 10 000 at 150 px from the left, at 0.05 px/sample → left 350.
  close(anchoredLeft(10_000, 150, 0.05), 350);
  assert.equal(anchoredLeft(100, 150, 0.05), 0);
});

test("presses grab an edge, the box, or the strip outside it", () => {
  const b = { x: 100, y: 10, w: 200, h: 20 };
  assert.equal(hitBox(b, 98, 2), "left", "an edge is grabbed at any height");
  assert.equal(hitBox(b, 103, 20), "left");
  assert.equal(hitBox(b, 302, 20), "right");
  assert.equal(hitBox(b, 200, 20), "inside");
  assert.equal(hitBox(b, 200, 35), "outside", "below the box");
  assert.equal(hitBox(b, 50, 20), "outside");
  // A narrow box keeps its middle for moving.
  const n = { x: 100, y: 10, w: 6, h: 20 };
  assert.equal(hitBox(n, 103, 20), "inside");
  assert.equal(hitBox(n, 101, 20), "left");
  assert.equal(hitBox(n, 105.5, 20), "right");
});
