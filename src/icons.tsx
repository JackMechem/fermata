// Hand-drawn glyphs in the style of Logic's control bar: 1.4px strokes on a 16px grid.
import type { ReactNode } from "react";

const S = (props: { children: ReactNode; size?: number; vb?: string }) => (
  <svg
    width={props.size ?? 16}
    height={props.size ?? 16}
    viewBox={props.vb ?? "0 0 16 16"}
    fill="none"
    stroke="currentColor"
    strokeWidth={1.4}
    strokeLinecap="round"
    strokeLinejoin="round"
  >
    {props.children}
  </svg>
);

export const Icon = {
  library: () => (
    <S>
      <path d="M3 3v10M6 3v10M9.2 3.4l3.4 9.4" />
    </S>
  ),
  inspector: () => (
    <S>
      <circle cx="8" cy="8" r="6" />
      <path d="M8 7.2v4M8 4.9v.1" strokeWidth={1.6} />
    </S>
  ),
  mixer: () => (
    <S>
      <path d="M4 2.5v11M8 2.5v11M12 2.5v11" />
      <rect x="2.6" y="8.5" width="2.8" height="2" rx=".5" fill="currentColor" stroke="none" />
      <rect x="6.6" y="4.5" width="2.8" height="2" rx=".5" fill="currentColor" stroke="none" />
      <rect x="10.6" y="9.8" width="2.8" height="2" rx=".5" fill="currentColor" stroke="none" />
    </S>
  ),
  editors: () => (
    <S>
      <circle cx="4.5" cy="11.5" r="2" />
      <circle cx="11.5" cy="11.5" r="2" />
      <path d="M5.8 10 11.5 2.5M10.2 10 4.5 2.5" />
    </S>
  ),
  browser: () => (
    <S>
      <path d="M2.5 4.5h4l1.2 1.4h5.8v6.6h-11z" />
    </S>
  ),
  rewind: () => (
    <S>
      <path d="M8 4 3.5 8 8 12zM13 4 8.5 8 13 12z" fill="currentColor" strokeWidth={1} />
    </S>
  ),
  forward: () => (
    <S>
      <path d="M3 4 7.5 8 3 12zM8 4l4.5 4L8 12z" fill="currentColor" strokeWidth={1} />
    </S>
  ),
  stop: () => (
    <S>
      <rect x="4" y="4" width="8" height="8" rx="1" fill="currentColor" stroke="none" />
    </S>
  ),
  play: () => (
    <S>
      <path d="M5 3.5v9l7.5-4.5z" fill="currentColor" strokeWidth={1} />
    </S>
  ),
  record: () => (
    <S>
      <circle cx="8" cy="8" r="4.6" fill="currentColor" stroke="none" />
    </S>
  ),
  cycle: () => (
    <S>
      <path d="M3 9.5V8a3 3 0 0 1 3-3h7M11 3l2 2-2 2M13 6.5V8a3 3 0 0 1-3 3H3M5 13l-2-2 2-2" />
    </S>
  ),
  metronome: () => (
    <S>
      <path d="M5.6 2.5h4.8l2.4 11H3.2z" />
      <path d="M8 11 11.8 4.2" />
    </S>
  ),
  plus: () => (
    <S size={12} vb="0 0 12 12">
      <path d="M6 2v8M2 6h8" />
    </S>
  ),
  power: () => (
    <S size={10} vb="0 0 10 10">
      <path d="M5 1.2v3.3M2.6 2.7a3.2 3.2 0 1 0 4.8 0" strokeWidth={1.3} />
    </S>
  ),
  chevron: () => (
    <S size={8} vb="0 0 8 8">
      <path d="M2 3l2 2 2-2" strokeWidth={1.3} />
    </S>
  ),
  chevronRight: () => (
    <S size={8} vb="0 0 8 8">
      <path d="M3 2l2 2-2 2" strokeWidth={1.3} />
    </S>
  ),
  disclosure: (open: boolean) => (
    <S size={9} vb="0 0 9 9">
      <path d={open ? "M1.5 3 4.5 6.5 7.5 3z" : "M3 1.5 6.5 4.5 3 7.5z"} fill="currentColor" stroke="none" />
    </S>
  ),
  mono: () => (
    <S size={12} vb="0 0 12 12">
      <circle cx="6" cy="6" r="3.6" strokeWidth={1.2} />
    </S>
  ),
  stereo: () => (
    <S size={14} vb="0 0 14 12">
      <circle cx="5" cy="6" r="3.4" strokeWidth={1.2} />
      <circle cx="9" cy="6" r="3.4" strokeWidth={1.2} />
    </S>
  ),
  waveform: () => (
    <S size={20} vb="0 0 20 20">
      <path d="M2 10h1.5M4.5 7v6M7 4v12M9.5 6.5v7M12 3v14M14.5 7.5v5M17 9v2" />
    </S>
  ),
  keyboard: () => (
    <S size={20} vb="0 0 20 20">
      <rect x="2" y="5" width="16" height="10" rx="1.5" />
      <path d="M6 5v10M10 5v10M14 5v10" />
      <path d="M5 5v5.5h2V5M9 5v5.5h2V5M13 5v5.5h2V5" fill="currentColor" strokeWidth={0} />
    </S>
  ),
  mic: () => (
    <S size={30} vb="0 0 30 30">
      <rect x="11" y="4" width="8" height="14" rx="4" />
      <path d="M7.5 14.5a7.5 7.5 0 0 0 15 0M15 22v4M11 26h8" />
    </S>
  ),
  piano: () => (
    <S size={30} vb="0 0 30 30">
      <rect x="4" y="7" width="22" height="16" rx="2" />
      <path d="M9.5 7v16M15 7v16M20.5 7v16" />
      <path d="M8 7v9h3V7M13.5 7v9h3V7M19 7v9h3V7" fill="currentColor" strokeWidth={0} />
    </S>
  ),
  palette: () => (
    <S size={20} vb="0 0 20 20">
      <path d="M10 2.8a7.2 7.2 0 1 0 0 14.4c1.1 0 1.6-.7 1.6-1.4 0-1-1-1.3-1-2.3 0-.8.6-1.3 1.4-1.3h2.1a3.1 3.1 0 0 0 3.1-3.1C17.2 5.6 14 2.8 10 2.8z" />
      <circle cx="6.4" cy="9" r="1" fill="currentColor" stroke="none" />
      <circle cx="8.6" cy="6" r="1" fill="currentColor" stroke="none" />
      <circle cx="12.2" cy="6" r="1" fill="currentColor" stroke="none" />
    </S>
  ),
  folder: () => (
    <S size={14} vb="0 0 14 14">
      <path d="M1.5 3.5h3.8l1.1 1.3h6.1v6.7h-11z" />
    </S>
  ),
  file: () => (
    <S size={14} vb="0 0 14 14">
      <path d="M3 1.5h5l3 3v8H3z M8 1.5v3h3" />
    </S>
  ),
  up: () => (
    <S size={12} vb="0 0 12 12">
      <path d="M6 10V2.5M2.8 5.5 6 2.3l3.2 3.2" />
    </S>
  ),
  /** Track Freeze. */
  snowflake: () => (
    <S size={12} vb="0 0 12 12">
      <path d="M6 1v10M1.7 3.5l8.6 5M1.7 8.5l8.6-5" strokeWidth={1.2} />
      <path d="M4.6 1.8 6 3l1.4-1.2M4.6 10.2 6 9l1.4 1.2" strokeWidth={1} />
    </S>
  ),
};
