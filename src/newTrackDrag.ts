// Dragging regions past the last track, into the empty area: each source lane that falls past
// the end becomes a new track below the last one (Logic's behaviour). The tracks moved decide
// which lanes fall past; the new tracks are then made one after another, in track order, so the
// preview draws them with no gaps however far down the pointer is. The engine's side is
// crates/engine/src/session/new_track_drag.rs.
//
// This module only imports types, so it runs under plain Node for the tests in tests/.
import type { Id } from "./api";

/** Whole lanes the pointer is past the last track at content y: 0 on the tracks, 1 from the
 *  first pixel of the empty area, and one more per `laneH` (the height a new track gets). */
export function lanesPast(y: number, total: number, laneH: number): number {
  return y < total ? 0 : 1 + Math.floor((y - total) / Math.max(1, laneH));
}

/** How many of `d` steps down from track `index` run out of tracks (those with no height, in a
 *  collapsed stack or hidden, don't count as steps). */
function stepsOver(heights: number[], index: number, d: number): number {
  let i = index;
  for (let left = d; left > 0; left--) {
    let k = i + 1;
    while (k < heights.length && heights[k] === 0) k++;
    if (k >= heights.length) return left;
    i = k;
  }
  return 0;
}

/** The new lane (0 is right below the last track) of each dragged-from track that falls past the
 *  end, by track index. `sources` are those tracks' indices, `dTrack` the visible tracks moved.
 *  The lower lanes of a selection can fall past while the pointer is still on a track. */
export function newLanes(heights: number[], sources: number[], dTrack: number): Map<number, number> {
  const lanes = new Map<number, number>();
  // A lower source runs out of tracks no later than one above it, so track order is lane order.
  for (const i of [...sources].sort((a, b) => a - b)) if (stepsOver(heights, i, dTrack) > 0) lanes.set(i, lanes.size);
  return lanes;
}

/** The indices of the tracks holding any of `ids`, in track order. */
export function sourceTracks(tracks: { regions: { id: Id }[] }[], ids: Id[]): number[] {
  return tracks.flatMap((t, i) => (t.regions.some((r) => ids.includes(r.id)) ? [i] : []));
}

/** A new lane's stand-in for a track id while drawing a drag: no real track has one below zero. */
export const newLaneId = (lane: number): Id => -1 - lane;

/** A drop's regions, each with the track index it came from: those that go onto existing tracks,
 *  and those that go onto new ones, one entry per new lane in lane order. */
export function splitDrop<M extends { source: number }>(moves: M[], lanes: Map<number, number>): { onTracks: M[]; onNew: { source: number; moves: M[] }[] } {
  const onNew = [...lanes.entries()].sort((a, b) => a[1] - b[1]).map(([source]) => ({ source, moves: moves.filter((m) => m.source === source) }));
  return { onTracks: moves.filter((m) => !lanes.has(m.source)), onNew };
}
