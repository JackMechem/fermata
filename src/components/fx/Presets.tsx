// The preset pop-up in built-in effect panels and the Channel EQ window: factory and user
// presets (shared by every project, see crates/engine/src/presets.rs), saving, renaming and
// deleting user presets, and stepping through them.
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { act, call, type ChannelEq, type Id } from "../../api";
import { openMenuAt, type MenuItem } from "../Menu";

/** A preset as the engine stores it. */
export interface Preset {
  name: string;
  effect: string;
  version: number;
  values?: [number, number][];
  eq?: ChannelEq;
}

interface PresetList {
  factory: string[];
  user: string[];
}

/** The preset each instance last loaded or saved (by instance key), and its settings then. */
const loaded = new Map<string, { name: string; factory: boolean; norm: string }>();
const listeners = new Set<() => void>();

function remember(key: string, v: { name: string; factory: boolean; norm: string } | null) {
  if (v) loaded.set(key, v);
  else loaded.delete(key);
  listeners.forEach((l) => l());
}

type Sheet = { mode: "save" | "rename"; text: string; error?: string; replace?: boolean } | { mode: "delete" } | null;

/**
 * The preset pop-up.
 * - `effect`: the preset id ("builtin:deesser", "builtin:channel-eq").
 * - `track`/`slot`: where presets load to and save from (`EQ_SLOT` for the Channel EQ).
 * - `instance`: a key for the instance (its uid, or the EQ's track), remembering what it loaded.
 * - `current` and `normalize`: the settings now and a preset's, in one comparable form, which
 *   tells "(edited)" apart.
 * - `defaultName`: the factory preset that resets it ("Default", "Flat").
 */
export function PresetButton({
  effect,
  track,
  slot,
  instance,
  current,
  normalize,
  defaultName = "Default",
}: {
  effect: string;
  track: Id;
  slot: number;
  instance: string;
  current: string;
  normalize: (p: Preset) => string;
  defaultName?: string;
}) {
  const [, setTick] = useState(0);
  const [sheet, setSheet] = useState<Sheet>(null);
  const [defaultNorm, setDefaultNorm] = useState<string | null>(null);
  useEffect(() => {
    const l = () => setTick((n) => n + 1);
    listeners.add(l);
    return () => {
      listeners.delete(l);
    };
  }, []);
  useEffect(() => {
    call<Preset>("get_preset", { effect, name: defaultName, factory: true }).then((p) => p && setDefaultNorm(normalize(p)));
    // `normalize` only depends on the effect.
  }, [effect, defaultName]);

  const state = loaded.get(instance);
  const edited = !!state && state.norm !== current;
  const label = state ? state.name : current === defaultNorm ? defaultName : "Manual";

  const load = async (name: string, factory: boolean) => {
    const snap = await act("load_preset", { track, slot, effect, name, factory });
    if (!snap) return;
    const p = await call<Preset>("get_preset", { effect, name, factory });
    if (p) remember(instance, { name, factory, norm: normalize(p) });
  };

  const save = async (name: string, overwrite: boolean): Promise<string | null> => {
    try {
      const saved = await invoke<string>("save_preset", { track, slot, name, overwrite });
      remember(instance, { name: saved, factory: false, norm: current });
      return null;
    } catch (e) {
      return typeof e === "string" ? e : String(e);
    }
  };

  const step = async (dir: 1 | -1) => {
    const list = await call<PresetList>("list_presets", { effect });
    if (!list) return;
    const all = [...list.factory.map((n) => ({ n, f: true })), ...list.user.map((n) => ({ n, f: false }))];
    if (!all.length) return;
    const at = state ? all.findIndex((x) => x.n === state.name && x.f === state.factory) : -1;
    const next = all[(at + dir + all.length) % all.length];
    load(next.n, next.f);
  };

  const menu = async (el: HTMLElement) => {
    const list = (await call<PresetList>("list_presets", { effect })) ?? { factory: [], user: [] };
    const isUser = !!state && !state.factory;
    const items: MenuItem[] = [{ label: "Factory", header: true }];
    for (const n of list.factory.filter((n) => n !== defaultName)) items.push({ label: n, checked: state?.factory && state.name === n, onClick: () => load(n, true) });
    items.push({ separator: true }, { label: "User", header: true });
    if (!list.user.length) items.push({ label: "No User Presets", disabled: true });
    for (const n of list.user) items.push({ label: n, checked: isUser && state?.name === n, onClick: () => load(n, false) });
    items.push(
      { separator: true },
      {
        label: "Save Preset…",
        // Saving a user preset updates it; otherwise it needs a name.
        onClick: () => (isUser ? save(state!.name, true) : setSheet({ mode: "save", text: "" })),
      },
      { label: "Save Preset As…", onClick: () => setSheet({ mode: "save", text: state?.name ?? "" }) },
      { label: "Rename Preset…", disabled: !isUser, onClick: () => setSheet({ mode: "rename", text: state!.name }) },
      { label: "Delete Preset", disabled: !isUser, onClick: () => setSheet({ mode: "delete" }) },
      { separator: true },
      { label: state ? `Revert to “${state.name}”` : "Revert", disabled: !edited, onClick: () => state && load(state.name, state.factory) },
      { label: defaultName, onClick: () => load(defaultName, true) },
    );
    openMenuAt(el, items);
  };

  const submit = async () => {
    if (!sheet || sheet.mode === "delete") return;
    if (sheet.mode === "save") {
      const err = await save(sheet.text, !!sheet.replace);
      if (!err) setSheet(null);
      // A name already taken asks before replacing it.
      else setSheet({ ...sheet, error: err, replace: /already exists/.test(err) });
      return;
    }
    const renamed = await call<string>("rename_preset", { effect, old: state!.name, new: sheet.text });
    if (renamed) {
      remember(instance, { ...state!, name: renamed });
      setSheet(null);
    }
  };

  const remove = async () => {
    if (!state) return;
    const ok = await call("delete_preset", { effect, name: state.name });
    if (ok !== null) remember(instance, null);
    setSheet(null);
  };

  return (
    <span className="fx-presets">
      <button className="fx-step" title="Previous preset" onClick={() => step(-1)}>
        ‹
      </button>
      <button className="fx-popup fx-preset" onMouseDown={(e) => menu(e.currentTarget)} title="Presets">
        {label}
        {edited && <span className="dim"> (edited)</span>}
      </button>
      <button className="fx-step" title="Next preset" onClick={() => step(1)}>
        ›
      </button>
      {sheet && (
        <div className="fx-sheet" onMouseDown={(e) => e.stopPropagation()}>
          {sheet.mode === "delete" ? (
            <>
              <div>Delete the preset “{state?.name}”? It is removed for every project.</div>
              <div className="fx-sheet-buttons">
                <button className="push" onClick={() => setSheet(null)}>
                  Cancel
                </button>
                <button className="push default" onClick={remove}>
                  Delete
                </button>
              </div>
            </>
          ) : (
            <>
              <div>{sheet.mode === "save" ? "Save the current settings as a preset named:" : "Rename the preset to:"}</div>
              <input
                className="fx-sheet-input"
                autoFocus
                value={sheet.text}
                onChange={(e) => setSheet({ ...sheet, text: e.target.value, error: undefined, replace: false })}
                onKeyDown={(e) => {
                  e.stopPropagation();
                  if (e.key === "Enter") submit();
                  if (e.key === "Escape") setSheet(null);
                }}
              />
              {sheet.error && <div className="fx-sheet-error">{sheet.replace ? `“${sheet.text.trim()}” already exists. Replace it?` : sheet.error}</div>}
              <div className="fx-sheet-buttons">
                <button className="push" onClick={() => setSheet(null)}>
                  Cancel
                </button>
                <button className="push default" disabled={!sheet.text.trim()} onClick={submit}>
                  {sheet.replace ? "Replace" : sheet.mode === "save" ? "Save" : "Rename"}
                </button>
              </div>
            </>
          )}
        </div>
      )}
    </span>
  );
}
