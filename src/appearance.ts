// Settings › Appearance (theme, interface size, waveform style) are saved in settings.json
// with the other settings. localStorage keeps a copy only so the window opens in the right
// theme before the settings have loaded (main.tsx applies it).
import { useEffect } from "react";
import { getSettings, updateSettings } from "./prefs";
import { getState, useStore } from "./store";
import { applyTheme } from "./themes";
import { applyUiScale } from "./uiScale";
import { applyWaveStyle, WAVE_STYLES } from "./waveform";

let loaded = false;

/** Saves the current appearance to settings.json if it differs from what's there. */
function persist() {
  if (!loaded) return;
  const { themeId: theme, uiScale: ui_scale, waveStyle: wave_style } = getState();
  const ui = getSettings().ui;
  if (ui.theme === theme && ui.ui_scale === ui_scale && ui.wave_style === wave_style) return;
  updateSettings((s) => ({ ...s, ui: { ...s.ui, theme, ui_scale, wave_style } }));
}

/**
 * Applies the appearance from settings.json, once loaded (`loadSettings`). Settings written
 * before it was saved there have none yet: they take what this machine's copy had.
 */
export function restoreAppearance() {
  const ui = getSettings().ui;
  const s = getState();
  if (ui.theme && ui.theme !== s.themeId) applyTheme(ui.theme);
  if (ui.ui_scale && Math.abs(ui.ui_scale - s.uiScale) > 0.001) applyUiScale(ui.ui_scale);
  const wave = WAVE_STYLES.find((w) => w.id === ui.wave_style);
  if (wave && wave.id !== s.waveStyle) applyWaveStyle(wave.id);
  loaded = true;
  persist();
}

/** Keeps settings.json up to date as the appearance changes (call once, in App). */
export function useAppearancePersistence() {
  const theme = useStore((s) => s.themeId);
  const scale = useStore((s) => s.uiScale);
  const wave = useStore((s) => s.waveStyle);
  useEffect(persist, [theme, scale, wave]);
}
