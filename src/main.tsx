import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";
import { applyTheme, savedThemeId } from "./themes";
import { applyUiScale, savedUiScale } from "./uiScale";
import { savedWaveStyle } from "./waveform";
import { setState } from "./store";

applyTheme(savedThemeId(), false);
const scale = savedUiScale();
if (scale !== 1) applyUiScale(scale, false);
setState({ waveStyle: savedWaveStyle() });

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
