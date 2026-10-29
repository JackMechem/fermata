// Dragging a plug-in between Audio FX slots, as in Logic: onto another slot of the same strip
// moves it there (the plug-ins between shift over), onto another strip's slot moves it to that
// strip, and with Alt held copies it instead. Insert slots carry data-track and data-slot.
import { act, type Id } from "./api";

function slotAt(x: number, y: number): HTMLElement | null {
  return (document.elementFromPoint(x, y)?.closest(".slot[data-slot]") as HTMLElement | null) ?? null;
}

/** Starts dragging the plug-in in `slot` from a mousedown on its name. Letting go without
 *  dragging runs `click` (opening the plug-in), so the name still opens it. */
export function startSlotDrag(e: React.MouseEvent, track: Id, slot: number, label: string, click: () => void) {
  const sx = e.clientX;
  const sy = e.clientY;
  const own = (e.currentTarget as HTMLElement).closest(".slot") as HTMLElement | null;
  let ghost: HTMLDivElement | null = null;
  let target: HTMLElement | null = null;
  const mark = (el: HTMLElement | null) => {
    if (el === target) return;
    target?.classList.remove("slot-drop");
    target = el;
    target?.classList.add("slot-drop");
  };
  const show = (ev: MouseEvent) => {
    if (!ghost) return;
    ghost.textContent = ev.altKey ? `${label} (copy)` : label;
    document.body.style.cursor = ev.altKey ? "copy" : "grabbing";
    ghost.style.transform = `translate(${ev.clientX + 12}px, ${ev.clientY + 10}px)`;
    const el = slotAt(ev.clientX, ev.clientY);
    // Dropping a plug-in back on its own slot does nothing, unless it is copied.
    mark(el && (el !== own || ev.altKey) ? el : null);
  };
  const move = (ev: MouseEvent) => {
    if (!ghost) {
      if (Math.hypot(ev.clientX - sx, ev.clientY - sy) < 4) return;
      ghost = document.createElement("div");
      ghost.className = "drag-ghost";
      document.body.appendChild(ghost);
      own?.classList.add("slot-dragging");
    }
    show(ev);
  };
  const end = () => {
    window.removeEventListener("mousemove", move);
    window.removeEventListener("mouseup", up);
    window.removeEventListener("keydown", key, true);
    document.body.style.cursor = "";
    own?.classList.remove("slot-dragging");
    ghost?.remove();
    mark(null);
  };
  const up = (ev: MouseEvent) => {
    const dragged = !!ghost;
    const drop = target;
    end();
    if (!dragged) return click();
    if (drop) {
      act("move_plugin", { from: track, fromSlot: slot, to: Number(drop.dataset.track), toSlot: Number(drop.dataset.slot), copy: ev.altKey });
    }
  };
  const key = (ev: KeyboardEvent) => {
    if (ev.key === "Escape" && ghost) {
      ev.stopPropagation();
      end();
    }
  };
  window.addEventListener("mousemove", move);
  window.addEventListener("mouseup", up);
  window.addEventListener("keydown", key, true);
}
