import { useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore } from "react";
import { Icon } from "../icons";

export interface MenuItem {
  label?: string;
  onClick?: () => void;
  children?: MenuItem[];
  checked?: boolean;
  disabled?: boolean;
  separator?: boolean;
  header?: boolean;
  hint?: string;
  /** Renders a colour swatch instead of the label text. */
  color?: string;
}

interface Open {
  x: number;
  y: number;
  items: MenuItem[];
  minWidth?: number;
}

let current: Open | null = null;
const subs = new Set<() => void>();
const emit = () => subs.forEach((s) => s());

export function openMenu(x: number, y: number, items: MenuItem[], minWidth?: number) {
  current = { x, y, items, minWidth };
  emit();
}

/** Opens a menu below an element, like a pop-up button. */
export function openMenuAt(el: HTMLElement, items: MenuItem[]) {
  const r = el.getBoundingClientRect();
  openMenu(r.left, r.bottom + 2, items, r.width);
}

/** Whether a menu is open (key handlers leave Esc and arrows to it). */
export const menuOpen = () => current !== null;

export function closeMenu() {
  current = null;
  emit();
}

function List({ items, x, y, minWidth, depth }: { items: MenuItem[]; x: number; y: number; minWidth?: number; depth: number }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x, y });
  const [sub, setSub] = useState<{ i: number; x: number; y: number } | null>(null);
  const timer = useRef<number>(0);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    let nx = x;
    let ny = y;
    if (nx + r.width > window.innerWidth - 4) nx = depth > 0 ? x - r.width - (minWidth ?? 0) : window.innerWidth - r.width - 4;
    if (ny + r.height > window.innerHeight - 4) ny = Math.max(4, window.innerHeight - r.height - 4);
    setPos({ x: nx, y: ny });
  }, [x, y, depth, minWidth]);

  return (
    <>
      <div
        ref={ref}
        className="menu"
        style={{ left: pos.x, top: pos.y, minWidth: depth === 0 ? minWidth : undefined }}
        onMouseDown={(e) => e.stopPropagation()}
      >
        {items.map((it, i) =>
          it.separator ? (
            <div key={i} className="menu-sep" />
          ) : it.header ? (
            <div key={i} className="menu-header">
              {it.label}
            </div>
          ) : (
            <div
              key={i}
              className={"menu-item" + (it.disabled ? " disabled" : "") + (sub?.i === i ? " open" : "")}
              onMouseEnter={(e) => {
                window.clearTimeout(timer.current);
                const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
                if (it.children) {
                  timer.current = window.setTimeout(() => setSub({ i, x: r.right - 2, y: r.top - 5 }), 90);
                } else {
                  timer.current = window.setTimeout(() => setSub(null), 120);
                }
              }}
              onClick={() => {
                if (it.disabled || it.children) return;
                closeMenu();
                it.onClick?.();
              }}
            >
              <span className="menu-check">{it.checked ? "✓" : ""}</span>
              {it.color ? (
                <span className="menu-swatch" style={{ background: it.color }} />
              ) : (
                <span className="menu-label">{it.label}</span>
              )}
              {it.hint && <span className="menu-hint">{it.hint}</span>}
              {it.children && (
                <span className="menu-arrow">
                  <Icon.chevronRight />
                </span>
              )}
            </div>
          ),
        )}
      </div>
      {sub && items[sub.i]?.children && (
        <List items={items[sub.i].children!} x={sub.x} y={sub.y} depth={depth + 1} minWidth={ref.current?.offsetWidth} />
      )}
    </>
  );
}

export function MenuHost() {
  const open = useSyncExternalStore(
    (l) => {
      subs.add(l);
      return () => subs.delete(l);
    },
    () => current,
  );
  useEffect(() => {
    if (!open) return;
    // Capture phase, so dialogs that stop propagation can't keep a menu open; clicks inside a
    // menu are left alone.
    const down = (e: Event) => {
      if (!(e.target instanceof Element && e.target.closest(".menu"))) closeMenu();
    };
    // Esc closes just the menu: taken in the capture phase, before the sheet or editor under
    // the menu sees it.
    const key = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      closeMenu();
    };
    window.addEventListener("mousedown", down, true);
    window.addEventListener("keydown", key, true);
    window.addEventListener("blur", down);
    return () => {
      window.removeEventListener("mousedown", down, true);
      window.removeEventListener("keydown", key, true);
      window.removeEventListener("blur", down);
    };
  }, [open]);
  if (!open) return null;
  return (
    <div className="menu-layer">
      <List key={`${open.x},${open.y}`} items={open.items} x={open.x} y={open.y} minWidth={open.minWidth} depth={0} />
    </div>
  );
}
