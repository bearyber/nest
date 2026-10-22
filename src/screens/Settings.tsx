import { useCallback, useEffect, useRef, useState } from "react";
import { listen, type Event } from "@tauri-apps/api/event";
import { useTauriListener } from "../lib/useTauriListener";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getSettings, settingsReady } from "../lib/commands";
import { closesSettings } from "../lib/shortcuts";
import { applyTheme } from "../lib/theme";
import { errorMessage, type SettingsView } from "../lib/types";
import AboutTab from "./settings/AboutTab";
import AppearanceTab from "./settings/AppearanceTab";
import CodesTab from "./settings/CodesTab";
import DevicesTab from "./settings/DevicesTab";
import GeneralTab from "./settings/GeneralTab";
import KitsTab from "./settings/KitsTab";
import ShortcutsTab from "./settings/ShortcutsTab";
import SpacesTab from "./settings/SpacesTab";
import styles from "./settings/settings.module.css";

/** Sent by Rust when the window is closed (hidden). */
const RESET_EVENT = "settings-reset";

type Tab = "general" | "codes" | "spaces" | "kits" | "computers" | "appearance" | "shortcuts" | "about";

const TABS: { id: Tab; label: string }[] = [
  { id: "general", label: "General" },
  { id: "codes", label: "Job codes" },
  { id: "spaces", label: "Spaces" },
  { id: "kits", label: "Client files" },
  { id: "computers", label: "Computers" },
  { id: "appearance", label: "Appearance" },
  { id: "shortcuts", label: "Shortcuts" },
  { id: "about", label: "About" },
];

/** The tab a window opens on: Help → Keyboard Shortcuts, or the "same letter" note's button
 *  (Computers, with the letter field focused). */
export function startTab(name: string | null): Tab {
  return name === "shortcuts" || name === "codes" || name === "computers" ? name : "general";
}

/** What each tab gets: the current settings and a way to run a change. */
export interface TabProps {
  view: SettingsView;
  /** Switch to another tab (the "Change in Computers" link). */
  goTo?: (tab: "computers") => void;
  /** Put the cursor in the letter field when the Computers tab shows (the same-letter note's button). */
  focusLetter?: boolean;
  /** Run a settings command; shows its error, keeps the returned settings. */
  run: <T>(p: Promise<T>, ok?: string) => Promise<T | undefined>;
}

/** The Settings window (build spec §3.3): native preferences style, tabs on the left. */
export default function Settings() {
  const [view, setView] = useState<SettingsView | null>(null);
  // Help → Keyboard Shortcuts / F1 open the window on the Shortcuts tab (?tab=shortcuts).
  const [tab, setTab] = useState<Tab>(() => startTab(new URLSearchParams(window.location.search).get("tab")));
  // Opened on Computers by a request (not by clicking the tab): the letter field gets the cursor.
  const [focusLetter, setFocusLetter] = useState(() => new URLSearchParams(window.location.search).get("tab") === "computers");
  // Bumped when the window is closed (hidden): everything inside starts fresh next time.
  const [epoch, setEpoch] = useState(0);
  const [message, setMessage] = useState<{
    text: string;
    error: boolean;
  } | null>(null);

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
    // A hidden (pre-warmed) window may not run animation frames: don't wait for one forever.
    let sent = false;
    const send = () => {
      if (sent) return;
      sent = true;
      void settingsReady().catch(() => {});
    };
    requestAnimationFrame(send);
    setTimeout(send, 150);
  }, [view]);

  useEffect(() => {
    void refresh();
  }, [refresh]);
  useTauriListener(
    (cb) => listen("settings-changed", cb),
    () => void refresh(),
  );

  // Already open: the same request arrives as an event.
  useTauriListener<Event<string>>(
    (cb) => listen<string>("settings-tab", cb),
    (e) => {
      if (e.payload === "shortcuts" || e.payload === "codes" || e.payload === "computers") {
        setTab(e.payload);
        setFocusLetter(e.payload === "computers");
        setMessage(null);
      }
    },
  );

  // The window was closed, which only hides it: forget the tab, messages, dialogs and scroll.
  useTauriListener(
    (cb) => listen(RESET_EVENT, cb),
    () => {
      setTab("general");
      setFocusLetter(false);
      setMessage(null);
      setEpoch((n) => n + 1);
    },
  );

  // Windows: Escape (when you're not typing) and Ctrl+W close the window, like other Windows
  // settings windows. macOS has ⌘W in the menu bar; Escape doesn't close a Mac settings window.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!closesSettings(e)) return;
      e.preventDefault();
      void getCurrentWindow()
        .close()
        .catch(() => {});
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

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

  const goTo = useCallback((t: "computers") => {
    setTab(t);
    setFocusLetter(false);
    setMessage(null);
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
              setFocusLetter(false);
              setMessage(null);
            }}
          >
            {t.label}
          </button>
        ))}
      </nav>
      <main className={styles.content} key={epoch}>
        <h2 className={styles.title}>{TABS.find((t) => t.id === tab)?.label}</h2>
        {view?.settingsError && (
          <div className={styles.banner} role="alert">
            {view.settingsError}
          </div>
        )}
        {message?.error && (
          <div className={styles.banner} role="alert">
            {message.text}
          </div>
        )}
        {/* Always there (empty), so a "saved" message put into it is announced. */}
        <div role="status" aria-live="polite">
          {message && !message.error && <div className={styles.note}>{message.text}</div>}
        </div>
        {view && tab === "general" && <GeneralTab view={view} run={run} />}
        {view && tab === "codes" && <CodesTab view={view} run={run} goTo={goTo} />}
        {view && tab === "spaces" && <SpacesTab view={view} run={run} />}
        {view && tab === "kits" && <KitsTab view={view} run={run} />}
        {view && tab === "computers" && <DevicesTab view={view} run={run} focusLetter={focusLetter} />}
        {view && tab === "appearance" && <AppearanceTab view={view} run={run} />}
        {view && tab === "shortcuts" && <ShortcutsTab view={view} run={run} />}
        {view && tab === "about" && <AboutTab view={view} run={run} />}
      </main>
    </div>
  );
}
