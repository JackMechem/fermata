// Built-in effects on the UI side: the description of each effect (mirroring
// crates/engine/src/fx/mod.rs) and the custom panels that replace the generic one.
import type { ComponentType } from "react";
import type { Id } from "./api";
import { CompressorPanel } from "./components/fx/Compressor";
import { DeEsserPanel } from "./components/fx/DeEsser";
import { DelayPanel } from "./components/fx/Delay";
import { DistortionPanel, OverdrivePanel } from "./components/fx/Distortion";
import { GainPanel } from "./components/fx/Gain";
import { GatePanel } from "./components/fx/Gate";
import { LimiterPanel } from "./components/fx/Limiter";
import { LinearPhaseEqPanel } from "./components/fx/LinearPhaseEq";
import { LoudnessPanel } from "./components/fx/Loudness";
import { ChorusPanel, FlangerPanel, PhaserPanel, TremoloPanel } from "./components/fx/Modulation";
import { ReverbPanel } from "./components/fx/Reverb";
import { TunerPanel } from "./components/fx/Tuner";

export type FxUnit = "none" | "db" | "ms" | "hz" | "ratio" | "percent" | "semitones" | "note" | "deg";

export type FxParamKind = { type: "continuous" } | { type: "toggle" } | { type: "choice"; labels: string[] };

/** One parameter, in plain units (see `ParamSpec` in the engine). */
export interface FxParamSpec {
  id: number;
  name: string;
  min: number;
  max: number;
  default: number;
  unit: FxUnit;
  kind: FxParamKind;
  log: boolean;
}

/** A built-in effect's registry entry (see `EffectInfo` in the engine). */
export interface FxInfo {
  /** The plug-in id, for example "builtin:compressor". */
  id: string;
  name: string;
  category: string;
  params: FxParamSpec[];
  /** Names of the meter values the effect reports, in order. */
  meters: string[];
  sidechain: boolean;
}

/** What a panel gets from the host. */
export interface FxPanelProps {
  info: FxInfo;
  /** The track and the insert's instance id. */
  track: Id;
  uid: Id;
  /** Current parameter values by id, in plain units. */
  values: Record<number, number>;
  /** Sets a parameter (undoable; drags of one parameter merge into one undo step). */
  set: (id: number, value: number) => void;
  /** Tells Touch/Latch automation that a control is held (call with true on grab, false on release). */
  touch: (id: number, on: boolean) => void;
  /** The latest meter values, in the order of `info.meters` (updated about 30 times a second). */
  meters: number[];
  /** Sets several parameters as one undo step (a drag that moves more than one). */
  setMany?: (values: [number, number][]) => void;
}

/**
 * Custom panels by plug-in id. Effects without an entry get the generic panel, which lays out
 * one control per parameter plus a bar per meter.
 */
export const FX_PANELS: Record<string, ComponentType<FxPanelProps>> = {
  "builtin:compressor": CompressorPanel,
  "builtin:limiter": LimiterPanel,
  "builtin:noise-gate": GatePanel,
  "builtin:deesser": DeEsserPanel,
  "builtin:reverb": ReverbPanel,
  "builtin:delay": DelayPanel,
  "builtin:tuner": TunerPanel,
  "builtin:loudness": LoudnessPanel,
  "builtin:chorus": ChorusPanel,
  "builtin:flanger": FlangerPanel,
  "builtin:phaser": PhaserPanel,
  "builtin:tremolo": TremoloPanel,
  "builtin:overdrive": OverdrivePanel,
  "builtin:distortion": DistortionPanel,
  "builtin:gain": GainPanel,
  "builtin:linear-phase-eq": LinearPhaseEqPanel,
};
