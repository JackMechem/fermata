// The Project Chooser shown at launch and from File › New: start from a template, or reopen a
// recent project. Modelled on Logic Pro's chooser.
import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { act, call, type InputSel, type RecentProject, type TrackKind } from "../api";
import { confirmDiscard } from "../files";
import { getState, setState, useStore } from "../store";
import { NumberField, Select, sheetKey } from "./Form";
import { getSettings, openProjectPath, updateSettings, type Startup } from "../prefs";

interface TemplateTrack {
  kind: TrackKind;
  name: string;
  input?: InputSel;
}

interface Template {
  id: string;
  name: string;
  description: string;
  tracks: TemplateTrack[];
}

const TEMPLATES: Template[] = [
  { id: "empty", name: "Empty Project", description: "A blank project. Add tracks as you go.", tracks: [] },
  {
    id: "audio",
    name: "Record Audio",
    description: "One audio track on input 1, record-enabled and ready for a mic or instrument.",
    tracks: [{ kind: "audio", name: "Audio 1", input: { type: "mono", ch: 0 } }],
  },
  {
    id: "instrument",
    name: "Software Instrument",
    description: "One instrument track with the built-in synth, ready for your MIDI keyboard.",
    tracks: [{ kind: "instrument", name: "Inst 1" }],
  },
  {
    id: "songwriter",
    name: "Songwriter",
    description: "Vocals and guitar on inputs 1 and 2, plus a keys track.",
    tracks: [
      { kind: "audio", name: "Vocals", input: { type: "mono", ch: 0 } },
      { kind: "audio", name: "Guitar", input: { type: "mono", ch: 1 } },
      { kind: "instrument", name: "Keys" },
    ],
  },
];

function TemplateArt({ id }: { id: string }) {
  // Miniature track lanes: blue audio, green instrument, as in the tracks area.
  const lanes: string[] = { empty: [], audio: ["a"], instrument: ["i"], songwriter: ["a", "a", "i"] }[id] ?? [];
  return (
    <div className="tpl-art">
      {lanes.length === 0 ? (
        <span className="tpl-plus">+</span>
      ) : (
        lanes.map((k, i) => (
          <div key={i} className="tpl-lane">
            <span className={"tpl-head " + k} />
            <span className={"tpl-region " + k} style={{ left: `${18 + i * 9}%`, width: `${46 - i * 6}%` }} />
          </div>
        ))
      )}
    </div>
  );
}

function ago(secs: number | null): string {
  if (!secs) return "";
  const d = Date.now() / 1000 - secs;
  if (d < 60) return "just now";
  if (d < 3600) return `${Math.floor(d / 60)} min ago`;
  if (d < 86400) return `${Math.floor(d / 3600)} h ago`;
  if (d < 86400 * 7) return `${Math.floor(d / 86400)} d ago`;
  return new Date(secs * 1000).toLocaleDateString();
}

const KEYS = ["C", "C♯", "D", "E♭", "E", "F", "F♯", "G", "A♭", "A", "B♭", "B"];
const SIGNATURES: [string, string][] = [
  ["4/4", "4/4"],
  ["3/4", "3/4"],
  ["2/4", "2/4"],
  ["5/4", "5/4"],
  ["6/8", "6/8"],
  ["7/8", "7/8"],
  ["9/8", "9/8"],
  ["12/8", "12/8"],
];

/** Settings › General › At Launch, which this sheet's "Show at launch" box also sets. */
export function chooserAtLaunch(): boolean {
  return getSettings().ui.startup === "chooser";
}

/** Turning the box off goes back to `otherwise` (the At Launch choice the sheet opened with,
 *  unless that was the Chooser), so "Open last project" survives a check and uncheck. */
function setChooserAtLaunch(on: boolean, otherwise: Startup) {
  updateSettings((s) => ({ ...s, ui: { ...s.ui, startup: on ? "chooser" : otherwise === "chooser" ? "empty_project" : otherwise } }));
}

export function ProjectChooser() {
  const tab = useStore((s) => s.chooserTab);
  const [template, setTemplate] = useState("empty");
  const [name, setName] = useState("Untitled");
  const [location, setLocation] = useState("");
  const [tempo, setTempo] = useState(120);
  const [sig, setSig] = useState("4/4");
  const [key, setKey] = useState("C major");
  const [recent, setRecent] = useState<RecentProject[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [atLaunch, setAtLaunch] = useState(chooserAtLaunch());
  const [launchWas] = useState(() => getSettings().ui.startup);
  const close = () => setState({ dialog: null });

  useEffect(() => {
    call<string>("default_project_dir").then((d) => d && setLocation((l) => l || d));
    call<RecentProject[]>("recent_projects").then((r) => {
      setRecent(r ?? []);
      setSelected((r ?? []).find((p) => p.exists)?.path ?? null);
    });
  }, []);

  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, []);

  const toast = (text: string) => setState({ toast: { text, at: Date.now(), ms: 5000 } });
  const target = `${location.replace(/\/+$/, "")}/${name.trim() || "Untitled"}.fermata`;

  const create = async (templateId: string = template) => {
    const tpl = TEMPLATES.find((t) => t.id === templateId)!;
    if (!name.trim()) return toast("Give the project a name");
    if (await call<boolean>("path_exists", { path: target })) return toast(`A project named “${name.trim()}” already exists there`);
    if (!(await confirmDiscard("Create a new project"))) return;
    setBusy(true);
    await act("new_project");
    const [num, den] = sig.split("/").map(Number);
    await act("set_tempo", { bpm: tempo });
    await act("set_signature", { num, den });
    await act("set_key", { key });
    for (const t of tpl.tracks) {
      const snap = await act("add_tracks", {
        kind: t.kind,
        count: 1,
        input: t.input ?? { type: "none" },
        ascending: false,
        instrument: null,
        armed: true,
        monitor: false,
      });
      const added = snap?.project.tracks[snap.project.tracks.length - 1];
      if (added && added.name !== t.name) await act("update_track", { id: added.id, patch: { name: t.name } });
    }
    // The setup above went through ordinary edits; none of it should be undoable.
    await act("clear_undo_history");
    const saved = await act("save_project", { path: target });
    const first = saved?.project.tracks[0];
    if (first) await act("select_track", { id: first.id });
    setState({ selectedRegions: [] });
    setBusy(false);
    if (saved) close();
  };

  const openPath = async (path: string) => {
    if (!(await confirmDiscard("Open another project"))) return;
    setBusy(true);
    const snap = await openProjectPath(path);
    setBusy(false);
    if (snap) {
      setState({ selectedRegions: [] });
      close();
    } else {
      call<RecentProject[]>("recent_projects").then((r) => setRecent(r ?? []));
    }
  };

  const browse = async () => {
    const dir = await open({ directory: true, title: "Open Project (select a .fermata folder)", defaultPath: location || undefined });
    if (typeof dir === "string") openPath(dir);
  };

  const chooseLocation = async () => {
    const dir = await open({ directory: true, title: "Choose where to save the project", defaultPath: location || undefined });
    if (typeof dir === "string") setLocation(dir);
  };

  const forget = async (path: string) => {
    await call("forget_recent", { path });
    setRecent((r) => r?.filter((p) => p.path !== path) ?? null);
  };

  const hasProject = (getState().snap?.project.tracks.length ?? 0) > 0 || getState().snap?.saved;

  return (
    <div className="chooser-backdrop">
      <div className="chooser" onMouseDown={(e) => e.stopPropagation()} onKeyDown={(e) => sheetKey(e, close)}>
        <aside className="chooser-side">
          <div className="chooser-brand">
            <svg width="22" height="22" viewBox="0 0 22 22" fill="none">
              <path d="M3 15a8 8 0 0 1 16 0" stroke="var(--cycle)" strokeWidth="2.4" strokeLinecap="round" />
              <circle cx="11" cy="14" r="2.2" fill="var(--text)" />
            </svg>
            <span>Fermata</span>
          </div>
          <button className={"chooser-nav" + (tab === "new" ? " on" : "")} onClick={() => setState({ chooserTab: "new" })}>
            New Project
          </button>
          <button className={"chooser-nav" + (tab === "recent" ? " on" : "")} onClick={() => setState({ chooserTab: "recent" })}>
            Recent
            {recent && recent.length > 0 && <span className="chooser-count">{recent.length}</span>}
          </button>
          <button className="chooser-nav" onClick={browse}>
            Open…
          </button>
          <div className="chooser-side-foot">
            <label className="check">
              <input
                type="checkbox"
                checked={atLaunch}
                onChange={(e) => {
                  setAtLaunch(e.target.checked);
                  setChooserAtLaunch(e.target.checked, launchWas);
                }}
              />
              {launchWas === "last_project" && !atLaunch ? "Show at launch, not the last project" : "Show at launch"}
            </label>
          </div>
        </aside>

        <section className="chooser-main">
          {tab === "new" ? (
            <>
              <div className="chooser-title">Choose a template</div>
              <div className="tpl-grid">
                {TEMPLATES.map((t) => (
                  <button
                    key={t.id}
                    className={"tpl" + (template === t.id ? " on" : "")}
                    onClick={() => setTemplate(t.id)}
                    onDoubleClick={() => {
                      setTemplate(t.id);
                      create(t.id);
                    }}
                  >
                    <TemplateArt id={t.id} />
                    <span className="tpl-name">{t.name}</span>
                    <span className="tpl-desc">{t.description}</span>
                  </button>
                ))}
              </div>
              <div className="chooser-details">
                <label>Name</label>
                <input
                  className="text-field"
                  value={name}
                  autoFocus
                  onChange={(e) => setName(e.target.value)}
                  onFocus={(e) => e.target.select()}
                  onKeyDown={(e) => e.key === "Enter" && create()}
                />
                <label>Location</label>
                <div className="row-inline">
                  <span className="path-field" title={target}>
                    <bdi>{location}</bdi>
                  </span>
                  <button className="push" onClick={chooseLocation}>
                    Choose…
                  </button>
                </div>
                <label>Tempo</label>
                <div className="row-inline">
                  <NumberField value={tempo} min={5} max={990} resolution={0.01} onChange={(v) => setTempo(Math.round(v * 100) / 100)} width={96} />
                  <span className="dim">BPM</span>
                  <span className="detail-gap" />
                  <span className="inline-label">Time Signature</span>
                  <Select value={sig} options={SIGNATURES} onChange={setSig} width={80} />
                  <span className="inline-label">Key</span>
                  <Select
                    value={key}
                    options={KEYS.flatMap((k) => [[`${k} major`, `${k} major`] as [string, string], [`${k} minor`, `${k} minor`] as [string, string]])}
                    onChange={setKey}
                    width={110}
                  />
                </div>
              </div>
            </>
          ) : (
            <>
              <div className="chooser-title">Recent projects</div>
              <div className="recent-list">
                {recent === null ? null : recent.length === 0 ? (
                  <div className="recent-empty">
                    Projects you save or open appear here.
                    <button className="push" onClick={() => setState({ chooserTab: "new" })}>
                      Create a Project
                    </button>
                  </div>
                ) : (
                  recent.map((p) => (
                    <div
                      key={p.path}
                      className={"recent" + (selected === p.path ? " on" : "") + (p.exists ? "" : " missing")}
                      onMouseDown={() => p.exists && setSelected(p.path)}
                      onDoubleClick={() => p.exists && openPath(p.path)}
                    >
                      <div className="recent-icon">
                        <svg width="18" height="18" viewBox="0 0 18 18" fill="none" stroke="currentColor" strokeWidth="1.4">
                          <path d="M2 5.5h5l1.5 1.7H16v8H2z" />
                        </svg>
                      </div>
                      <div className="recent-text">
                        <span className="recent-name">{p.name}</span>
                        <span className="recent-path">{p.path}</span>
                      </div>
                      <div className="recent-meta">
                        {p.exists ? (
                          <>
                            <span>
                              {p.tracks} track{p.tracks === 1 ? "" : "s"}
                              {p.tempo ? ` · ${Math.round(p.tempo)} BPM` : ""}
                            </span>
                            <span className="dim">{ago(p.modified)}</span>
                          </>
                        ) : (
                          <span className="warn-text">Missing</span>
                        )}
                      </div>
                      <button className="recent-forget" title="Remove from list" onMouseDown={(e) => e.stopPropagation()} onClick={() => forget(p.path)}>
                        ×
                      </button>
                    </div>
                  ))
                )}
              </div>
            </>
          )}

          <div className="chooser-foot">
            <span className="dim">{tab === "new" ? target : selected ?? ""}</span>
            <div className="sheet-buttons">
              <button className="push" onClick={close}>
                {hasProject ? "Cancel" : "Close"}
              </button>
              {tab === "new" ? (
                <button className="push default" disabled={busy} onClick={() => create()}>
                  {busy ? "Creating…" : "Create"}
                </button>
              ) : (
                <button className="push default" disabled={busy || !selected} onClick={() => selected && openPath(selected)}>
                  {busy ? "Opening…" : "Open"}
                </button>
              )}
            </div>
          </div>
        </section>
      </div>
    </div>
  );
}
