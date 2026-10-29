import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";
import { applyTheme, savedThemeId } from "./themes";
import { applyUiScale, savedUiScale } from "./uiScale";
import { savedWaveStyle } from "./waveform";
import { setState } from "./store";
import { savedShowOverview } from "./components/Overview";

applyTheme(savedThemeId(), false);
const scale = savedUiScale();
if (scale !== 1) applyUiScale(scale, false);
setState({ waveStyle: savedWaveStyle() });
setState({ showOverview: savedShowOverview() });

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
