import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getSettings, settingsReady } from "../lib/commands";
import { applyTheme } from "../lib/theme";
import { errorMessage, type SettingsView } from "../lib/types";
import AboutTab from "./settings/AboutTab";
import AppearanceTab from "./settings/AppearanceTab";
import CodesTab from "./settings/CodesTab";
import DevicesTab from "./settings/DevicesTab";
import GeneralTab from "./settings/GeneralTab";
import SpacesTab from "./settings/SpacesTab";
import TemplatesTab from "./settings/TemplatesTab";
import styles from "./settings/settings.module.css";

type Tab = "general" | "codes" | "spaces" | "templates" | "devices" | "appearance" | "about";

const TABS: { id: Tab; label: string }[] = [
  { id: "general", label: "General" },
  { id: "codes", label: "Job codes" },
  { id: "spaces", label: "Spaces" },
  { id: "templates", label: "Templates" },
  { id: "devices", label: "Devices" },
  { id: "appearance", label: "Appearance" },
  { id: "about", label: "About" },
];

/** What each tab gets: the current settings and a way to run a change. */
export interface TabProps {
  view: SettingsView;
  /** Run a settings command; shows its error, keeps the returned settings. */
  run: <T>(p: Promise<T>, ok?: string) => Promise<T | undefined>;
}

/** The Settings window (build spec §3.3): native preferences style, tabs on the left. */
export default function Settings() {
  const [view, setView] = useState<SettingsView | null>(null);
  const [tab, setTab] = useState<Tab>("general");
  const [message, setMessage] = useState<{ text: string; error: boolean } | null>(null);

  const refresh = useCallback(
    () =>
      getSettings()
        .then((v) => {
          setView(v);
          applyTheme(v.settings.theme);
        })
        .catch((e) => setMessage({ text: errorMessage(e), error: true })),
    [],
  );

  // The window is created hidden: show it once the page has drawn, so it appears in one go.
  const shown = useRef(false);
  useEffect(() => {
    if (!view || shown.current) return;
    shown.current = true;
    requestAnimationFrame(() => void settingsReady().catch(() => {}));
  }, [view]);

  useEffect(() => {
    void refresh();
    let unlisten: (() => void) | undefined;
    listen("settings-changed", () => void refresh())
      .then((u) => (unlisten = u))
      .catch(() => {});
    return () => unlisten?.();
  }, [refresh]);

  const run = useCallback(async <T,>(p: Promise<T>, ok?: string) => {
    try {
      const result = await p;
      if (result && typeof result === "object" && "settings" in result) setView(result as unknown as SettingsView);
      // null = cancelled (e.g. a dialog): no "done" message.
      setMessage(ok && result !== null ? { text: ok, error: false } : null);
      return result;
    } catch (e) {
      setMessage({ text: errorMessage(e), error: true });
      return undefined;
    }
  }, []);

  return (
    <div className={styles.window}>
      <nav className={styles.nav} aria-label="Settings sections">
        <h1 className={styles.navTitle}>Settings</h1>
        {TABS.map((t) => (
          <button
            key={t.id}
            type="button"
            className={styles.navItem}
            aria-current={tab === t.id ? "page" : undefined}
            onClick={() => {
              setTab(t.id);
              setMessage(null);
            }}
          >
            {t.label}
          </button>
        ))}
      </nav>
      <main className={styles.content}>
        <h2 className={styles.title}>{TABS.find((t) => t.id === tab)?.label}</h2>
        {view?.settingsError && (
          <div className={styles.banner} role="alert">
            {view.settingsError}
          </div>
        )}
        {message && (
          <div className={message.error ? styles.banner : styles.note} role={message.error ? "alert" : "status"}>
            {message.text}
          </div>
        )}
        {view && tab === "general" && <GeneralTab view={view} run={run} />}
        {view && tab === "codes" && <CodesTab view={view} run={run} />}
        {view && tab === "spaces" && <SpacesTab view={view} run={run} />}
        {view && tab === "templates" && <TemplatesTab view={view} run={run} />}
        {view && tab === "devices" && <DevicesTab view={view} run={run} />}
        {view && tab === "appearance" && <AppearanceTab view={view} run={run} />}
        {view && tab === "about" && <AboutTab view={view} run={run} />}
      </main>
    </div>
  );
}
