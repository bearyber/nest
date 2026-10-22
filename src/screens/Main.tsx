import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { listen, type Event } from "@tauri-apps/api/event";
import { useTauriListener } from "../lib/useTauriListener";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { setNewProjectOpen } from "../lib/commands";
import Icon from "../components/Icon";
import FirstSteps, { FirstProjectNote } from "../components/FirstSteps";
import LetterNote from "../components/LetterNote";
import Inspector from "../components/Inspector";
import ProjectList from "../components/ProjectList";
import Sidebar, { type Computer } from "../components/Sidebar";
import Toast, { type ToastData } from "../components/Toast";
import WhatsNew from "../components/WhatsNew";
import { Button, IconButton } from "../components/ui";
import {
  archiveProject,
  checkForUpdates,
  copyText,
  devicesRefresh,
  devicesView,
  dismissArchiveSuggestion,
  firstProjectTipSeen,
  getSettings,
  installUpdate,
  listProjects,
  measureProject,
  measureProjectFolder,
  openSettings,
  projectResolve,
  projectSizes,
  resolveSkipProject,
  resolveTryNow,
  pendingUpdate,
  whatsNew,
  whatsNewSeen,
  openProject,
  openProjectSubfolder,
  rescanProjects,
  revealProject,
  requestStatus,
  setMenuState,
  setStatus,
  takeNotice,
} from "../lib/commands";
import { fileManager, isMac } from "../lib/platform";
import { hasOpenDialog } from "../lib/dialogStack";
import { findShortcut, listenHere, type ShortcutId } from "../lib/shortcuts";
import { isListKey } from "../lib/listKeys";
import { popupMenu } from "../lib/popupMenu";
import { trackFocusArea } from "../lib/focusArea";
import { createdMessage, toastForNotice } from "../lib/resolve";
import {
  computerMatches,
  filterProjects,
  isDone,
  isFresh,
  sortProjects,
  statusMatches,
  type Filters,
  type Sort,
  type SortKey,
  type StatusFilter,
} from "../lib/search";
import {
  errorMessage,
  type Created,
  type DevicesView,
  type ProjectResolve,
  type ProjectList as List,
  type ProjectRow,
  type Sizes,
  type UpdateInfo,
  type WhatsNew as WhatsNewNote,
  updateNoteLines,
} from "../lib/types";
import NewProjectSheet, { parentOf, shortPath } from "./NewProjectSheet";
import Templates from "./templates/Templates";
import styles from "./Main.module.css";

/** Rescan when the window regains focus after this long (build spec §7). */
const FOCUS_RESCAN_MS = 10 * 60 * 1000;
const DEFAULT_SPACES = ["Work", "Personal"];
/** The main window's shortcuts (lib/shortcuts.ts). */
const MAIN_SHORTCUTS: readonly ShortcutId[] = [
  "new-project",
  "open-settings",
  "rescan",
  "search",
  "toggle-sidebar",
  "toggle-inspector",
  "open-project",
  "copy-path",
];
/** Commands that still act with a dialog open (see runMenu). */
const OVER_DIALOGS = new Set(["new-project", "open-settings", "check-updates"]);

// Three-pane main window (build spec §3.1).
export default function Main() {
  const [list, setList] = useState<List | null>(null);
  const [spaces, setSpaces] = useState<string[]>(DEFAULT_SPACES);
  const [archiveDays, setArchiveDays] = useState<number | null>(null);
  const [archiveOn, setArchiveOn] = useState(false);
  const [scanning, setScanning] = useState(false);
  const [filters, setFilters] = useState<Filters>({ space: null, status: "active", query: "", computer: null });
  const [sort, setSort] = useState<Sort>({ key: "created", dir: "desc" });
  const [selected, setSelected] = useState<string | null>(null);
  // The sidebar can be hidden (button at the top left, Ctrl/⌘+B); remembered per computer.
  const [showSidebar, setShowSidebar] = useState(() => {
    try {
      return localStorage.getItem("nest.sidebarHidden") !== "1";
    } catch {
      return true;
    }
  });
  const toggleSidebar = useCallback(
    () =>
      setShowSidebar((v) => {
        try {
          localStorage.setItem("nest.sidebarHidden", v ? "1" : "0");
        } catch {
          /* storage unavailable: just not remembered */
        }
        return !v;
      }),
    [],
  );
  const [showInspector, setShowInspector] = useState(true);
  const [sheetOpen, setSheetOpen] = useState(false);
  /** The list of projects, or your templates (sidebar → Templates). */
  const [view, setView] = useState<"projects" | "templates">("projects");
  /** New Project opened from a template ("New project from this"). */
  const [sheetTemplate, setSheetTemplate] = useState<string | undefined>(undefined);
  /** New Project opened with "Try an example" (first steps). */
  const [sheetExample, setSheetExample] = useState(false);
  /** The note after the very first project: shown once (Settings remembers it). Assumed seen
   *  until the settings say otherwise, so it never shows by mistake. */
  const [tipSeen, setTipSeen] = useState(true);
  const [firstTip, setFirstTip] = useState<Created | null>(null);
  const [toast, setToast] = useState<ToastData | null>(null);
  /** The success line of a Create that queued a Resolve project: the "Added to Resolve" notice
   *  changes that line while it shows (instead of a second toast). */
  const pendingResolve = useRef<{ toast: ToastData; name: string; added: ToastData } | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const scanningRef = useRef(false);
  const scannedAtRef = useRef<number | null>(null);

  const fail = useCallback((e: unknown) => setToast({ message: errorMessage(e) }), []);

  const [devices, setDevices] = useState<DevicesView | null>(null);
  /** "Later" on the same-letter note: that situation stays hidden until it changes or Nest restarts. */
  const [letterLater, setLetterLater] = useState<string | null>(null);
  const refresh = useCallback(
    () =>
      Promise.all([listProjects(), devicesView().then(setDevices).catch(() => {})])
        .then(([l]) => l)
        .then((l) => {
          setList(l);
          scannedAtRef.current = l.scannedAt;
        })
        .catch(fail),
    [fail],
  );

  const rescan = useCallback(
    (manual: boolean) => {
      if (scanningRef.current) return;
      scanningRef.current = true;
      setScanning(true);
      rescanProjects()
        .then(async (s) => {
          await refresh();
          if (!manual) return;
          const notes = [
            `${s.found} project${s.found === 1 ? "" : "s"} found`,
            s.removed ? `${s.removed} no longer in your folders` : "",
            s.duplicates.length ? `same job code used twice: ${s.duplicates.join(", ")}` : "",
            s.offline.length ? `${s.offline.length} folder${s.offline.length === 1 ? "" : "s"} not connected` : "",
          ].filter(Boolean);
          setToast({ message: `List refreshed · ${notes.join(" · ")}` });
        })
        .catch((e) => manual && fail(e))
        .finally(() => {
          scanningRef.current = false;
          setScanning(false);
        });
    },
    [refresh, fail],
  );

  // Launch: show the saved list instantly, then rescan in the background.
  const loadSpaces = useCallback(
    () =>
      getSettings()
        .then((v) => {
          setSpaces(v.settings.spaces);
          setArchiveDays(v.settings.archiveAfterDays);
          setArchiveOn(v.settings.archiveFolder !== null);
          setTipSeen(v.settings.firstProjectTipSeen === true);
        })
        .catch(() => {}),
    [],
  );
  // A one-off note from the app (e.g. the template tidy on first start of v0.3.0): asked
  // now, and again if it arrives later.
  const showNotice = useCallback(
    () =>
      takeNotice()
        .then((n) => n && setToast((cur) => toastForNotice(cur, pendingResolve.current, n)))
        .catch(() => {}),
    [],
  );
  useEffect(() => {
    refresh().then(() => rescan(false));
    void loadSpaces();
    void showNotice();
  }, [refresh, rescan, loadSpaces, showNotice]);
  // Settings window changed something (spaces, jobs folders…): refresh and rescan.
  useTauriListener(
    (cb) => listen("settings-changed", cb),
    () => {
      void loadSpaces();
      rescan(false);
      // A changed letter may end (or start) a clash.
      void devicesView().then(setDevices).catch(() => {});
    },
  );
  // Settings → Templates → Open templates.
  useTauriListener(
    (cb) => listen("open-templates", cb),
    () => setView("templates"),
  );
  // Devices: another computer's list changed (or this one's was saved): reload the list.
  useTauriListener(
    (cb) => listen("devices-changed", cb),
    () => void refresh(),
  );
  useTauriListener(
    (cb) => listen("notice", cb),
    () => void showNotice(),
  );

  // Updates (release builds): ask once in case the check finished before we listened.
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [installing, setInstalling] = useState(false);
  // "What's new" after an update: asked once at launch.
  const [note, setNote] = useState<WhatsNewNote | null>(null);
  useEffect(() => {
    whatsNew().then(setNote).catch(() => {});
  }, []);
  const [version, setVersion] = useState("");
  useEffect(() => {
    pendingUpdate().then(setUpdate).catch(() => {});
    getVersion().then(setVersion).catch(() => {});
  }, []);
  useTauriListener<Event<UpdateInfo>>(
    (cb) => listen<UpdateInfo>("update-available", cb),
    (e) => setUpdate(e.payload),
  );
  const [checking, setChecking] = useState(false);
  const checkUpdates = useCallback(() => {
    setChecking(true);
    checkForUpdates()
      .then((u) => (u ? setUpdate(u) : setToast({ message: `Nest is up to date${version ? ` (${version})` : ""}` })))
      .catch(fail)
      .finally(() => setChecking(false));
  }, [fail, version]);
  /** The update bar's notes, folded away until "What's new" is clicked. */
  const [showNotes, setShowNotes] = useState(false);
  const restartToUpdate = () => {
    setInstalling(true);
    installUpdate().catch((e) => {
      setInstalling(false);
      fail(e);
    });
  };

  // Window focus: rescan if the last one is older than 10 minutes.
  useTauriListener<Event<boolean>>(
    (cb) => getCurrentWindow().onFocusChanged(cb),
    ({ payload: focused }) => {
      // Hover highlights only while this window is the active one (like other Mac / Windows apps).
      document.documentElement.dataset.window = focused ? "active" : "inactive";
      // Devices: read the other computers' lists again (some drives don't announce changes).
      if (focused) void devicesRefresh().catch(() => {});
      const last = scannedAtRef.current;
      if (focused && (last === null || Date.now() - last > FOCUS_RESCAN_MS)) rescan(false);
    },
  );

  const projects = useMemo(() => list?.projects ?? [], [list]);
  const selectedRow = projects.find((p) => p.key === selected) ?? null;

  // Folder sizes (M5 P4): the remembered number shows at once; it's measured again in the
  // background when it's a day old, or on "Measure again".
  const [sizeInfo, setSizeInfo] = useState<{ key: string; sizes: Sizes | null; measuring: boolean } | null>(null);
  const measuringKey = useRef<string | null>(null);
  const measure = useCallback(
    (key: string) => {
      if (measuringKey.current === key) return;
      measuringKey.current = key;
      setSizeInfo((s) => ({ key, sizes: s?.key === key ? s.sizes : null, measuring: true }));
      measureProject(key)
        .then((sizes) => {
          setSizeInfo({ key, sizes, measuring: false });
          void refresh(); // the list's Size column shows it too
        })
        .catch((e) => {
          setSizeInfo((s) => (s?.key === key ? { ...s, measuring: false } : s));
          fail(e);
        })
        .finally(() => {
          if (measuringKey.current === key) measuringKey.current = null;
        });
    },
    [fail, refresh],
  );
  const selectedKey = selectedRow?.key ?? null;
  const measurable = !!selectedRow && !selectedRow.device && !selectedRow.offline && !selectedRow.error;
  useEffect(() => {
    if (!selectedKey || !measurable) return;
    let live = true;
    projectSizes(selectedKey)
      .then((sizes) => {
        if (!live) return;
        setSizeInfo((s) => (s?.key === selectedKey && s.measuring ? s : { key: selectedKey, sizes, measuring: false }));
        if (!sizes || Date.now() - sizes.measuredAt > 86_400_000) measure(selectedKey);
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [selectedKey, measurable, measure]);
  // The project's Resolve project (from its .project.json), read again when Resolve's
  // waiting list changes.
  const [resolveInfo, setResolveInfo] = useState<{ key: string; r: ProjectResolve | null } | null>(null);
  const [resolveTick, setResolveTick] = useState(0);
  useTauriListener(
    (cb) => listen("resolve-changed", cb),
    () => setResolveTick((n) => n + 1),
  );
  const readable = !!selectedRow && !selectedRow.device && !selectedRow.error;
  useEffect(() => {
    if (!selectedKey || !readable) return;
    let live = true;
    projectResolve(selectedKey)
      .then((r) => live && setResolveInfo({ key: selectedKey, r }))
      .catch(() => live && setResolveInfo({ key: selectedKey, r: null }));
    return () => {
      live = false;
    };
  }, [selectedKey, readable, resolveTick]);
  const offlineRoots = list?.roots.filter((r) => !r.online) ?? [];
  const letterClash = devices?.letterClash ?? [];
  const letterClashKey = `${devices?.thisLetter ?? ""}|${letterClash
    .map((c) => c.deviceId)
    .sort()
    .join(",")}`;

  const counts = useMemo(() => {
    const c: Record<string, number> = { "": 0 };
    for (const p of projects) {
      if (!statusMatches(p, filters.status)) continue;
      c[""] += 1;
      c[p.space] = (c[p.space] ?? 0) + 1;
    }
    return c;
  }, [projects, filters.status]);

  // Devices: your other computers, with how fresh each one's own list is.
  const computers = useMemo<Computer[]>(
    () =>
      (devices?.enabled ? devices.others : []).map((d) => ({
        name: d.name,
        count: projects.filter((p) => computerMatches(p, d.name) && statusMatches(p, filters.status)).length,
        fresh: !d.notDownloaded && isFresh(d.updatedAt),
      })),
    [devices, projects, filters.status],
  );
  // The chosen computer is gone (hidden, or Devices turned off): show all again.
  const computerGone =
    filters.computer !== null && filters.computer !== undefined && filters.computer !== "" &&
    !computers.some((c) => c.name === filters.computer);
  const rows = useMemo(
    () => sortProjects(filterProjects(projects, computerGone ? { ...filters, computer: null } : filters), sort),
    [projects, filters, sort, computerGone],
  );
  const readyCount = projects.filter((p) => p.readyToArchive).length;
  const hereCount = projects.filter((p) => computerMatches(p, "") && statusMatches(p, filters.status)).length;
  const allCount = projects.filter((p) => statusMatches(p, filters.status)).length;

  const statusCounts = useMemo(() => {
    const inSpace = projects.filter((p) => filters.space === null || p.space === filters.space);
    return {
      active: inSpace.filter((p) => !p.archived && !isDone(p)).length,
      done: inSpace.filter((p) => !p.archived && isDone(p)).length,
      all: inSpace.length,
      archived: inSpace.filter((p) => p.archived).length,
    } satisfies Record<StatusFilter, number>;
  }, [projects, filters.space]);

  // ── Actions ──
  const open = useCallback((r: ProjectRow) => openProject(r.key).catch(fail), [fail]);
  const reveal = useCallback((r: ProjectRow) => revealProject(r.key).catch(fail), [fail]);
  const copy = useCallback(
    (text: string, what: string) =>
      copyText(text)
        .then(() => setToast({ message: `Copied ${what}` }))
        .catch(fail),
    [fail],
  );
  const changeStatus = useCallback(
    (r: ProjectRow, status: "active" | "done") => {
      // On another computer: ask that computer (Devices); it applies it next time Nest is open there.
      if (r.device) {
        const name = r.device.name;
        return requestStatus(r.key, status)
          .then(refresh)
          .then(() =>
            setToast({
              message:
                status === r.status
                  ? `Undone. ${name} keeps it ${status}.`
                  : `Sent to ${name}: it marks this ${status} next time Nest is open there.`,
            }),
          )
          .catch(fail);
      }
      return setStatus(r.key, status).then(refresh).catch(fail);
    },
    [refresh, fail],
  );

  // Archive (M5 P2): Rust checks everything and asks first; null = you clicked Cancel.
  // One at a time: a second click while the question is open does nothing.
  const archiving = useRef(false);
  const archive = useCallback(
    (r: ProjectRow) => {
      if (archiving.current) return;
      archiving.current = true;
      return archiveProject(r.key, r.archived)
        .then((message) => {
          if (!message) return;
          setToast({ message });
          return refresh();
        })
        .catch(fail)
        .finally(() => {
          archiving.current = false;
        });
    },
    [refresh, fail],
  );

  const showContextMenu = useCallback(
    async (r: ProjectRow, option = false) => {
      const canStatus = !r.error && !r.offline && !r.archived && (r.device ? true : r.paths.length === 1);
      const canArchive =
        archiveOn && !r.error && !r.offline && !r.device && r.paths.length === 1 && (r.archived || isDone(r));
      const shownStatus = r.pending ?? r.status;
      const here = !r.offline && !r.device;
      const path = r.device?.path ?? r.paths[0] ?? "";
      try {
        await popupMenu([
            // Mac (Bernard, 2026-10-22): one item, Open; ⌥-right-click gives Show in Finder instead
            // (both are in the File menu). Windows lists both, like File Explorer.
            ...(isMac()
              ? [
                  option
                    ? { text: "Show in Finder", enabled: here, action: () => void reveal(r) }
                    : { text: "Open", enabled: here, action: () => void open(r) },
                ]
              : [
                  { text: "Open", enabled: here, action: () => void open(r) },
                  { text: `Show in ${fileManager()}`, enabled: here, action: () => void reveal(r) },
                ]),
            { item: "Separator" },
            { text: "Copy path", enabled: path !== "", action: () => void copy(path, "path") },
            {
              text: "Copy job code",
              enabled: r.jobCode !== "",
              action: () => void copy(r.jobCode, r.jobCode),
            },
            { item: "Separator" },
            {
              text: shownStatus === "done" ? "Mark active" : "Mark done",
              enabled: canStatus,
              action: () => void changeStatus(r, shownStatus === "done" ? "active" : "done"),
            },
            ...(archiveOn
              ? [
                  {
                    text: r.archived ? "Unarchive…" : "Archive…",
                    enabled: canArchive,
                    action: () => void archive(r),
                  },
                ]
              : []),
          ]);
      } catch (e) {
        fail(e);
      }
    },
    [open, reveal, copy, changeStatus, archive, archiveOn, fail],
  );

  // No projects at all before this one: the very first.
  const noProjectsYet = list !== null && projects.length === 0;
  const onCreated = useCallback(
    (c: Created, resolveName: string | null) => {
      setSheetOpen(false);
      // Make sure the new project is visible, then select it.
      setFilters((f) => ({ space: null, query: "", status: f.status === "done" ? "active" : f.status }));
      refresh().then(() => setSelected(c.key));
      if (noProjectsYet && !tipSeen) {
        // The very first project: a calm note says where it is (once, ever).
        setFirstTip(c);
        setTipSeen(true);
        firstProjectTipSeen().catch(() => {});
        return;
      }
      const action = { label: "Open folder", run: () => void openProject(c.key).catch(fail) };
      const toast = { message: createdMessage(c.jobCode, c.bumped, resolveName ? "waiting" : null), action };
      pendingResolve.current = resolveName
        ? { toast, name: resolveName, added: { message: createdMessage(c.jobCode, c.bumped, "added"), action } }
        : null;
      setToast(toast);
    },
    [refresh, fail, noProjectsYet, tipSeen],
  );

  const moveSelection = useCallback(
    (delta: number) => {
      if (rows.length === 0) return;
      const i = rows.findIndex((r) => r.key === selected);
      const next = i < 0 ? (delta > 0 ? 0 : rows.length - 1) : Math.min(Math.max(i + delta, 0), rows.length - 1);
      setSelected(rows[next].key);
    },
    [rows, selected],
  );

  const onSort = (key: SortKey) =>
    setSort((s) => (s.key === key ? { key, dir: s.dir === "asc" ? "desc" : "asc" } : { key, dir: key === "created" || key === "size" ? "desc" : "asc" }));

  // ── Menu bar (macOS) and keyboard (build spec §8); the keys are in lib/shortcuts.ts ──
  const runMenu = useCallback(
    (id: string) => {
      // With a dialog open (the template editor, New Project, What's new) the list's commands
      // wait: they'd act behind it. New Project still opens (over the editor, as from the menu
      // bar), and Settings opens in its own window.
      if (hasOpenDialog() && !OVER_DIALOGS.has(id)) return;
      if (id === "new-project") setSheetOpen(true);
      else if (id === "open-settings") void openSettings().catch(fail);
      else if (id === "rescan") rescan(true);
      else if (id === "search") searchRef.current?.focus();
      else if (id === "toggle-sidebar") toggleSidebar();
      else if (id === "toggle-inspector") setShowInspector((v) => !v);
      else if (id === "check-updates") checkUpdates();
      // File → Open / Show in Finder / Copy Path, and Ctrl+O / Ctrl+Shift+C: the selected project
      // (this computer's), only in the project list.
      else if (id === "open-project" || id === "reveal-project" || id === "copy-path") {
        if (view !== "projects" || !selectedRow || selectedRow.offline || selectedRow.device) return;
        if (id === "open-project") void open(selectedRow);
        else if (id === "reveal-project") void reveal(selectedRow);
        else if (selectedRow.paths[0]) void copy(selectedRow.paths[0], "path");
      }
    },
    [rescan, checkUpdates, toggleSidebar, fail, view, selectedRow, open, reveal, copy],
  );

  // The menu bar says what's true: "Hide / Show Sidebar", and Open / Show in Finder / Copy Path
  // only with one of this computer's projects selected (menu.rs; Windows has no menu bar).
  const canActOnSelected = view === "projects" && !!selectedRow && !selectedRow.offline && !selectedRow.device;
  useEffect(() => {
    setMenuState(showSidebar, showInspector, canActOnSelected).catch(() => {});
  }, [showSidebar, showInspector, canActOnSelected]);
  // Which area has the selection's blue (list or sidebar): see lib/focusArea.ts.
  useEffect(() => trackFocusArea(), []);

  // Menu bar commands, sent to this window only (lib.rs brings it forward first). Listened to
  // once, through useTauriListener (it calls the latest runMenu): re-listening whenever runMenu changed
  // (a refresh when the window comes forward gives a new selected row) left a gap where a menu click was lost.
  useTauriListener<Event<string>>(
    (cb) => listenHere<string>("menu", cb),
    (e) => runMenu(e.payload),
  );

  // Menu bar / notification area icon (tray.rs): "New Project…" and "New from ›".
  useTauriListener<Event<string | null>>(
    (cb) => listen<string | null>("open-new-project", cb),
    (e) => {
      setSheetTemplate(e.payload ?? undefined);
      setSheetOpen(true);
    },
  );
  // An update never installs by itself while the sheet is open (tray.rs).
  useEffect(() => {
    setNewProjectOpen(sheetOpen).catch(() => {});
  }, [sheetOpen]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      const inSearch = target === searchRef.current;
      const typing = !inSearch && (target.tagName === "INPUT" || target.tagName === "TEXTAREA");

      // Nest's shortcuts (exact keys: Ctrl+Shift+R or ⌃⌘F are left alone). On Windows the
      // webview's own reload / find / print are blocked for every window in main.tsx.
      const hit = findShortcut(e, MAIN_SHORTCUTS);
      if (hit) {
        if (typing && hit.shortcut.whileTyping === false) return;
        // macOS: the menu bar item owns this key and runs it (lib.rs sends it here as a menu
        // command). Leave the key alone: in the webview, preventDefault stops it reaching the menu.
        if (isMac() && hit.combo.menu) return;
        e.preventDefault();
        runMenu(hit.shortcut.id);
        return;
      }
      // The list's keys don't apply under a dialog, while typing, or while Templates is showing.
      if (hasOpenDialog() || typing || view === "templates") return;
      // Search keeps its shortcuts (arrows / Enter act on the list, Escape clears). Anywhere else,
      // a focused button, link, menu, dialog… keeps its own Enter / Space / arrows.
      if (!inSearch && !isListKey(e)) return;
      if (e.key === "Escape" && inSearch) {
        setFilters((f) => ({ ...f, query: "" }));
        searchRef.current?.blur();
      } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        moveSelection(e.key === "ArrowDown" ? 1 : -1);
      } else if (e.key === "Enter" && selectedRow) {
        e.preventDefault();
        void open(selectedRow);
      } else if (e.key === " " && !inSearch) {
        e.preventDefault();
        setShowInspector((v) => !v);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [runMenu, moveSelection, selectedRow, open, view]);

  const closeSheet = useCallback(() => setSheetOpen(false), []);
  const dismissToast = useCallback(() => setToast(null), []);

  const firstScan = list === null || (list.scannedAt === null && projects.length === 0);
  const empty = (() => {
    if (firstScan)
      return <p className={styles.emptyHint}>{scanning ? "Looking for projects in your jobs folders…" : "Loading…"}</p>;
    if (projects.length === 0 && !filters.query)
      return (
        <FirstSteps
          onStart={() => setSheetOpen(true)}
          onExample={() => {
            setSheetExample(true);
            setSheetOpen(true);
          }}
        />
      );
    if (filters.query)
      return (
        <>
          <Icon name="search" className={styles.emptyIcon} />
          <p className={styles.emptyTitle}>No matches for “{filters.query}”</p>
          <p className={styles.emptyHint}>Search looks at codes, titles, clients and every field.</p>
        </>
      );
    const which = filters.status === "all" ? "" : `${filters.status} `;
    return <p className={styles.emptyTitle}>No {which}projects{filters.space ? ` in ${filters.space}` : ""}</p>;
  })();

  const heading = filters.space ?? "All projects";
  const statusLabel = { active: "Active", done: "Done", all: "All statuses", archived: "Archived" }[filters.status];

  /** Templates' commands: errors and "done" notes show as toasts. */
  const runTemplates = async <T,>(p: Promise<T>, ok?: string): Promise<T | undefined> => {
    try {
      const result = await p;
      if (ok && result !== null) setToast({ message: ok });
      return result;
    } catch (e) {
      fail(e);
      return undefined;
    }
  };
  const inspector = showInspector && view === "projects";

  const columns = [showSidebar ? "200px" : "", "minmax(0, 1fr)", inspector ? "280px" : ""]
    .filter(Boolean)
    .join(" ");

  return (
    <div className={styles.window} style={{ gridTemplateColumns: columns }}>
      {showSidebar && (
        <Sidebar
          onHide={toggleSidebar}
          spaces={spaces}
          counts={counts}
          space={filters.space}
          status={filters.status}
          statusCounts={statusCounts}
          onSpace={(space) => {
            setView("projects");
            setFilters((f) => ({ ...f, space }));
          }}
          onStatus={(status) => {
            setView("projects");
            setFilters((f) => ({ ...f, status }));
          }}
          onSettings={() => void openSettings().catch(fail)}
          onTemplates={() => setView((v) => (v === "templates" ? "projects" : "templates"))}
          templatesOpen={view === "templates"}
          computers={computers}
          computer={computerGone ? null : (filters.computer ?? null)}
          onComputer={(computer) => {
            setView("projects");
            setFilters((f) => ({ ...f, computer }));
          }}
          hereCount={hereCount}
          allCount={allCount}
        />
      )}

      {view === "templates" ? (
        <main className={styles.list}>
          <header className={styles.toolbar}>
            {!showSidebar && (
              <IconButton
                icon="sidebar"
                label={`Show sidebar (${isMac() ? "⌃⌘S" : "Ctrl+B"})`}
                onClick={toggleSidebar}
              />
            )}
            <div className={styles.heading}>
              <h1>Templates</h1>
              <span>What a new project gets</span>
            </div>
          </header>
          <div className={`${styles.fill} ${styles.templates}`}>
            <Templates
              run={runTemplates}
              onNewProject={(id) => {
                setSheetTemplate(id);
                setSheetOpen(true);
              }}
            />
          </div>
        </main>
      ) : (
      <main className={styles.list}>
        <header className={styles.toolbar}>
          {/* While the sidebar is hidden, its button waits here (it lives in the sidebar otherwise). */}
          {!showSidebar && (
            <IconButton
              icon="sidebar"
              label={`Show sidebar (${isMac() ? "⌃⌘S" : "Ctrl+B"})`}
              onClick={toggleSidebar}
            />
          )}
          <div className={styles.heading}>
            <h1>{heading}</h1>
            <span>
              {statusLabel} · {rows.length} project{rows.length === 1 ? "" : "s"}
            </span>
          </div>
          <label className={styles.search}>
            <Icon name="search" className={styles.searchIcon} />
            <input
              ref={searchRef}
              type="search"
              placeholder={`Search  ${isMac() ? "⌘K" : "Ctrl+K"}`}
              aria-label="Search projects"
              value={filters.query}
              spellCheck={false}
              onChange={(e) => setFilters((f) => ({ ...f, query: e.target.value }))}
            />
          </label>
          <IconButton
            icon="refresh"
            label={`Refresh list (${isMac() ? "⌘R" : "Ctrl+R"})`}
            onClick={() => rescan(true)}
            className={scanning ? styles.spinning : undefined}
          />
          <Button variant="primary" onClick={() => setSheetOpen(true)}>
            <Icon name="plus" /> New
          </Button>
        </header>

        {update && (
          <div className={`${styles.updateBar} nest-in`} role="status">
            <div className={styles.updateRow}>
              <span className={styles.updateDot} aria-hidden="true" />
              <span className={styles.updateTitle}>
                <b>Nest {update.version}</b> is ready
              </span>
              {updateNoteLines(update.notes).length > 0 && (
                <button
                  type="button"
                  className={styles.linkButton}
                  aria-expanded={showNotes}
                  onClick={() => setShowNotes((v) => !v)}
                >
                  {showNotes ? "Hide" : "What's new"}
                </button>
              )}
              <span className={styles.spacer} />
              <Button variant="primary" className={styles.updateButton} onClick={restartToUpdate} disabled={installing}>
                {installing ? "Updating…" : "Restart to update"}
              </Button>
            </div>
            {showNotes && (
              <ul className={`${styles.updateNotes} nest-unfold`}>
                {updateNoteLines(update.notes).map((l, i) => (
                  <li key={i}>{l}</li>
                ))}
              </ul>
            )}
          </div>
        )}

        {firstTip && (
          <FirstProjectNote
            code={firstTip.jobCode}
            folder={shortPath(parentOf(firstTip.root))}
            fullPath={firstTip.root}
            onReveal={() => void revealProject(firstTip.key).catch(fail)}
            onDone={() => setFirstTip(null)}
          />
        )}

        {letterClash.length > 0 && letterLater !== letterClashKey && (
          <LetterNote
            clash={letterClash}
            letter={devices?.thisLetter ?? ""}
            mac={isMac()}
            onChange={() => void openSettings("codes").catch(fail)}
            onLater={() => setLetterLater(letterClashKey)}
          />
        )}

        {offlineRoots.map((r) => (
          <div key={r.path} className={styles.banner} role="status">
            <Icon name="warning" />
            Can't find {r.path}. Plug in the drive, or change folders in Settings → General. Its projects are greyed out until then.
          </div>
        ))}

        {readyCount > 0 && !filters.ready && (
          <div className={styles.update} role="status">
            <span>
              <b>
                {readyCount} project{readyCount === 1 ? "" : "s"}
              </b>{" "}
              ready to archive (done for over {archiveDays} days).
            </span>
            <Button onClick={() => setFilters((f) => ({ ...f, ready: true, status: "done", query: "" }))}>Review</Button>
          </div>
        )}
        {filters.ready && (
          <div className={styles.update} role="status">
            <span>
              Ready to archive. Select one and click Archive… in the details panel, or pick “Not this one”.
            </span>
            <Button onClick={() => setFilters((f) => ({ ...f, ready: false, status: "active" }))}>Show all</Button>
          </div>
        )}

        {rows.length > 0 ? (
          <div className={styles.fill}>
            <ProjectList
              rows={rows}
              selected={selected}
              sort={sort}
              onSort={onSort}
              onSelect={setSelected}
              onOpen={(r) => void open(r)}
              onContextMenu={(r, option) => void showContextMenu(r, option)}
            />
          </div>
        ) : (
          <div className={`${styles.empty} ${styles.fill}`}>{empty}</div>
        )}

        <footer className={styles.status}>
          {scanning
            ? "Refreshing the list…"
            : `Looking in ${list?.roots.length ?? 0} folder${list?.roots.length === 1 ? "" : "s"}`}
          {version && (
            <span className={styles.version}>
              Nest {version} ·{" "}
              <button type="button" className={styles.linkButton} onClick={checkUpdates} disabled={checking}>
                {checking ? "Checking…" : "Check for updates"}
              </button>
            </span>
          )}
        </footer>
      </main>
      )}

      {inspector && (
        <Inspector
          project={selectedRow}
          onOpen={() => selectedRow && void open(selectedRow)}
          onCopyPath={() => selectedRow && void copy(selectedRow.device?.path ?? selectedRow.paths[0], "path")}
          onStatus={(s) => selectedRow && void changeStatus(selectedRow, s)}
          archiveOn={archiveOn}
          onArchive={() => selectedRow && void archive(selectedRow)}
          onDismissArchive={() =>
            selectedRow &&
            dismissArchiveSuggestion(selectedRow.key)
              .then(() => refresh())
              .catch(fail)
          }
          resolve={readable && resolveInfo?.key === selectedKey ? resolveInfo.r : null}
          onResolveTryNow={() =>
            resolveTryNow()
              .catch(fail)
              .finally(() => setResolveTick((n) => n + 1))
          }
          onResolveSkip={() =>
            selectedRow
              ? resolveSkipProject(selectedRow.key)
                  .catch(fail)
                  .finally(() => setResolveTick((n) => n + 1))
              : Promise.resolve()
          }
          sizes={selectedRow && sizeInfo?.key === selectedRow.key ? sizeInfo.sizes : null}
          measuring={!!selectedRow && sizeInfo?.key === selectedRow.key && sizeInfo.measuring}
          onMeasure={() => selectedRow && measure(selectedRow.key)}
          onOpenFolder={(path) => selectedRow && void openProjectSubfolder(selectedRow.key, path).catch(fail)}
          onMeasureFolder={(path) =>
            selectedRow ? measureProjectFolder(selectedRow.key, path) : Promise.reject(new Error("No project selected"))
          }
        />
      )}

      {sheetOpen && (
        <NewProjectSheet
          initialTemplate={sheetTemplate}
          example={sheetExample}
          onClose={() => {
            setSheetTemplate(undefined);
            setSheetExample(false);
            closeSheet();
          }}
          onCreated={(c, resolveName) => {
            setSheetTemplate(undefined);
            setSheetExample(false);
            setView("projects");
            onCreated(c, resolveName);
          }}
        />
      )}
      {note && (
        <WhatsNew
          note={note}
          onOk={() => {
            setNote(null);
            whatsNewSeen().catch(() => {});
          }}
        />
      )}
      {toast && <Toast toast={toast} onDismiss={dismissToast} />}
    </div>
  );
}
