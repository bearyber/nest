import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import FirstRun from "./screens/FirstRun";
import Main from "./screens/Main";
import Settings from "./screens/Settings";
import { getSettings, materialApplied } from "./lib/commands";
import { applyTheme } from "./lib/theme";

/** The same page serves two windows; the Settings window is the one labelled "settings". */
const isSettingsWindow = (() => {
  try {
    return getCurrentWindow().label === "settings";
  } catch {
    return false; // outside Tauri (tests)
  }
})();

export default function App() {
  const [firstRun, setFirstRun] = useState<boolean | null>(isSettingsWindow ? false : null);

  const syncSettings = useCallback(
    () =>
      getSettings()
        .then((v) => {
          applyTheme(v.settings.theme);
          if (!isSettingsWindow) setFirstRun(v.firstRun);
        })
        .catch(() => setFirstRun(false)),
    [],
  );

  useEffect(() => {
    // Solid background unless Rust confirms Mica/vibrancy is active.
    materialApplied()
      .then((ok) => {
        document.documentElement.dataset.material = ok ? "native" : "none";
      })
      .catch(() => {
        document.documentElement.dataset.material = "none";
      });
    void syncSettings();
    // Appearance changes in the Settings window apply here too.
    let unlisten: (() => void) | undefined;
    listen("settings-changed", () => void syncSettings())
      .then((u) => (unlisten = u))
      .catch(() => {});
    return () => unlisten?.();
  }, [syncSettings]);

  if (isSettingsWindow) return <Settings />;
  if (firstRun === null) return null;
  if (firstRun) return <FirstRun onDone={() => setFirstRun(false)} />;
  return <Main />;
}
