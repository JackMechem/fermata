// MIDI Learn: Learn Assignment for the last touched parameter (a key command and the Control
// Surfaces menu), and the "Learn MIDI Assignment" item of the right-click menus on faders,
// knobs and plug-in parameters. The engine waits for the next controller move and assigns it
// (session/controllers.rs); the banner in ControllerAssignments.tsx shows the learn from the
// ticks. The types and pure helpers are in controllers.ts.
import { createContext, useContext } from "react";
import { call, type Id } from "./api";
import { stripTarget, type CtlTarget } from "./controllers";
import { openMenu, type MenuItem } from "./components/Menu";
import { currentTick, setState } from "./store";

/** Starts learning for `target`, or for the last touched parameter with null. */
export function learn(target: CtlTarget | null) {
  call<string>("start_learn", { target });
}

export function cancelLearn() {
  call("cancel_learn");
}

/** The key command: learns for the last touched parameter, or cancels a learn in progress. */
export function learnLastTouched() {
  if (currentTick()?.learn) cancelLearn();
  else learn(null);
}

export function openControllerAssignments() {
  setState({ showControllerAssignments: true });
}

/** The right-click menu items for a learnable control. */
export function learnItems(target: CtlTarget): MenuItem[] {
  return [
    { label: "Learn MIDI Assignment", onClick: () => learn(target) },
    { label: "Controller Assignments…", onClick: openControllerAssignments },
  ];
}

/** An `onContextMenu` handler that offers to learn `target`. */
export function learnContext(target: CtlTarget) {
  return (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    openMenu(e.clientX, e.clientY, learnItems(target));
  };
}

/**
 * The channel strip's right-click handler: the fader and pan knob offer Learn (`track` null is
 * Stereo Out). Other right-clicks are left alone.
 */
export function stripLearnMenu(e: React.MouseEvent, track: Id | null) {
  const el = e.target as Element;
  const target = stripTarget(track, (sel) => !!el.closest?.(sel));
  if (target) learnContext(target)(e);
}

/** The strip and instance of the built-in effect panel being drawn (set by FxPanelContent). */
export const FxLearnContext = createContext<{ track: Id; uid: Id } | null>(null);

/** A built-in effect control's right-click handler for parameter `param`, inside a panel. */
export function useFxLearn(param: number): ((e: React.MouseEvent) => void) | undefined {
  const fx = useContext(FxLearnContext);
  return fx ? learnContext({ type: "plugin", track: fx.track, uid: fx.uid, plugin: "", param }) : undefined;
}
