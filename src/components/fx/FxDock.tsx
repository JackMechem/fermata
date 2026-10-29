// The bottom section while it shows docked plug-in panels (BottomPane "fx"): a tab per docked
// panel, the shown panel's power, presets and side chain, buttons to float or close it, and the
// panel filling the rest of the section's height (the splitter above sets it).
import { dockedPanels, type PanelKey } from "../../fxDock";
import { closePanel, floatPanel, selectDocked } from "../../fxDockState";
import { useStore } from "../../store";
import { FloatGlyph, PanelContent, PanelLead, PanelTools, usePanelTitle } from "./FxPanel";

export function FxDock() {
  const panels = useStore((s) => s.fxDock);
  const tabs = dockedPanels(panels);
  const active = panels.active;
  return (
    <div className="fx-dock">
      <div className="local-bar fx-dock-bar">
        <div className="fx-tabs" role="tablist">
          {tabs.map((k) => (
            <DockTab key={k} panelKey={k} on={k === active} />
          ))}
        </div>
        {active !== null && (
          <div className="fx-dock-tools">
            <PanelLead panelKey={active} />
            <PanelTools panelKey={active} />
          </div>
        )}
        {active !== null && (
          <div className="local-right">
            <button className="lb-btn" title="Float in its own window" onClick={() => floatPanel(active)}>
              <FloatGlyph />
            </button>
            <button className="eq-close fx-dock-close" title="Close (Esc)" onClick={() => closePanel(active)}>
              ×
            </button>
          </div>
        )}
      </div>
      {active !== null && <PanelContent key={active} panelKey={active} />}
    </div>
  );
}

function DockTab({ panelKey, on }: { panelKey: PanelKey; on: boolean }) {
  const title = usePanelTitle(panelKey);
  if (!title) return null;
  return (
    <button role="tab" aria-selected={on} className={"fx-tab" + (on ? " on" : "")} title={`${title.name} — ${title.track}`} onClick={() => selectDocked(panelKey)}>
      <span className="fx-tab-name">{title.name}</span>
      <span className="fx-tab-track">{title.track}</span>
      <span
        className="fx-tab-close"
        title="Close"
        onClick={(e) => {
          e.stopPropagation();
          closePanel(panelKey);
        }}
      >
        ×
      </span>
    </button>
  );
}
