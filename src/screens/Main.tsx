import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { listen } from "@tauri-apps/api/event";
import { Menu } from "@tauri-apps/api/menu";
import { getCurrentWindow } from "@tauri-apps/api/window";
import Icon from "../components/Icon";
import Inspector from "../components/Inspector";
import ProjectList from "../components/ProjectList";
import Sidebar, { type Computer } from "../components/Sidebar";
import Toast, { type ToastData } from "../components/Toast";
import { Button, IconButton } from "../components/ui";
import {
  archiveProject,
  checkForUpdates,
  copyText,
  devicesRefresh,
  devicesView,
  dismissArchiveSuggestion,
  getSettings,
  installUpdate,
  listProjects,
  measureProject,
  openSettings,
  projectSizes,
  pendingUpdate,
  openProject,
  rescanProjects,
  revealProject,
  requestStatus,
  setStatus,
  takeNotice,
} from "../lib/commands";
import { hasCommandKey, isMac } from "../lib/platform";
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
  type ProjectList as List,
  type ProjectRow,
  type Sizes,
  type UpdateInfo,
} from "../lib/types";
import NewProjectSheet from "./NewProjectSheet";
import styles from "./Main.module.css";

/** Rescan when the window regains focus after this long (build spec §7). */
const FOCUS_RESCAN_MS = 10 * 60 * 1000;
const DEFAULT_SPACES = ["Work", "Personal"];

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
  const [showSidebar, setShowSidebar] = useState(true);
  const [showInspector, setShowInspector] = useState(true);
  const [sheetOpen, setSheetOpen] = useState(false);
  const [toast, setToast] = useState<ToastData | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const scanningRef = useRef(false);
  const scannedAtRef = useRef<number | null>(null);

  const fail = useCallback((e: unknown) => setToast({ message: errorMessage(e) }), []);

  const [devices, setDevices] = useState<DevicesView | null>(null);
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
            s.removed ? `${s.removed} removed` : "",
            s.duplicates.length ? `duplicate codes: ${s.duplicates.join(", ")}` : "",
            s.offline.length ? `${s.offline.length} jobs folder offline` : "",
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
  useEffect(() => {
    refresh().then(() => rescan(false));
    const loadSpaces = () =>
      getSettings()
        .then((v) => {
          setSpaces(v.settings.spaces);
          setArchiveDays(v.settings.archiveAfterDays);
          setArchiveOn(v.settings.archiveFolder !== null);
        })
        .catch(() => {});
    void loadSpaces();
    // Settings window changed something (spaces, jobs folders…): refresh and rescan.
    let unlisten: (() => void) | undefined;
    listen("settings-changed", () => {
      void loadSpaces();
      rescan(false);
    })
      .then((u) => (unlisten = u))
      .catch(() => {});
    // Devices: another computer's list changed (or this one's was saved): reload the list.
    let unlistenDevices: (() => void) | undefined;
    listen("devices-changed", () => void refresh())
      .then((u) => (unlistenDevices = u))
      .catch(() => {});
    // A one-off note from the app (e.g. the template tidy on first start of v0.3.0): asked
    // now, and again if it arrives later.
    const showNotice = () =>
      takeNotice()
        .then((n) => n && setToast({ message: n }))
        .catch(() => {});
    void showNotice();
    let unlistenNotice: (() => void) | undefined;
    listen("notice", () => void showNotice())
      .then((u) => (unlistenNotice = u))
      .catch(() => {});
    return () => {
      unlisten?.();
      unlistenDevices?.();
      unlistenNotice?.();
    };
  }, [refresh, rescan]);

  // Updates (release builds): ask once in case the check finished before we listened.
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [installing, setInstalling] = useState(false);
  const [version, setVersion] = useState("");
  useEffect(() => {
    pendingUpdate().then(setUpdate).catch(() => {});
    getVersion().then(setVersion).catch(() => {});
    let unlisten: (() => void) | undefined;
    listen<UpdateInfo>("update-available", (e) => setUpdate(e.payload))
      .then((u) => (unlisten = u))
      .catch(() => {});
    return () => unlisten?.();
  }, []);
  const [checking, setChecking] = useState(false);
  const checkUpdates = useCallback(() => {
    setChecking(true);
    checkForUpdates()
      .then((u) => (u ? setUpdate(u) : setToast({ message: `Nest is up to date${version ? ` (${version})` : ""}` })))
      .catch(fail)
      .finally(() => setChecking(false));
  }, [fail, version]);
  const restartToUpdate = () => {
    setInstalling(true);
    installUpdate().catch((e) => {
      setInstalling(false);
      fail(e);
    });
  };

  // Window focus: rescan if the last one is older than 10 minutes.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    getCurrentWindow()
      .onFocusChanged(({ payload: focused }) => {
        // Devices: read the other computers' lists again (some drives don't announce changes).
        if (focused) void devicesRefresh().catch(() => {});
        const last = scannedAtRef.current;
        if (focused && (last === null || Date.now() - last > FOCUS_RESCAN_MS)) rescan(false);
      })
      .then((u) => (unlisten = u))
      .catch(() => {});
    return () => unlisten?.();
  }, [rescan]);

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
        .then((sizes) => setSizeInfo({ key, sizes, measuring: false }))
        .catch((e) => {
          setSizeInfo((s) => (s?.key === key ? { ...s, measuring: false } : s));
          fail(e);
        })
        .finally(() => {
          if (measuringKey.current === key) measuringKey.current = null;
        });
    },
    [fail],
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
  const offlineRoots = list?.roots.filter((r) => !r.online) ?? [];

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
    async (r: ProjectRow) => {
      const canStatus = !r.error && !r.offline && !r.archived && (r.device ? true : r.paths.length === 1);
      const canArchive =
        archiveOn && !r.error && !r.offline && !r.device && r.paths.length === 1 && (r.archived || isDone(r));
      const shownStatus = r.pending ?? r.status;
      const here = !r.offline && !r.device;
      const path = r.device?.path ?? r.paths[0] ?? "";
      try {
        const menu = await Menu.new({
          items: [
            { id: "open", text: "Open folder", enabled: here, action: () => void open(r) },
            {
              id: "reveal",
              text: isMac() ? "Reveal in Finder" : "Show in Explorer",
              enabled: here,
              action: () => void reveal(r),
            },
            { item: "Separator" },
            { id: "copy-path", text: "Copy path", enabled: path !== "", action: () => void copy(path, "path") },
            {
              id: "copy-code",
              text: "Copy job code",
              enabled: r.jobCode !== "",
              action: () => void copy(r.jobCode, r.jobCode),
            },
            { item: "Separator" },
            {
              id: "status",
              text: shownStatus === "done" ? "Mark active" : "Mark done",
              enabled: canStatus,
              action: () => void changeStatus(r, shownStatus === "done" ? "active" : "done"),
            },
            ...(archiveOn
              ? [
                  {
                    id: "archive",
                    text: r.archived ? "Unarchive…" : "Archive…",
                    enabled: canArchive,
                    action: () => void archive(r),
                  },
                ]
              : []),
          ],
        });
        await menu.popup();
      } catch (e) {
        fail(e);
      }
    },
    [open, reveal, copy, changeStatus, archive, archiveOn, fail],
  );

  const onCreated = useCallback(
    (c: Created) => {
      setSheetOpen(false);
      // Make sure the new project is visible, then select it.
      setFilters((f) => ({ space: null, query: "", status: f.status === "done" ? "active" : f.status }));
      refresh().then(() => setSelected(c.key));
      setToast({
        message: c.bumped ? `Created ${c.jobCode} (the previewed code was taken)` : `Created ${c.jobCode}`,
        action: { label: "Open folder", run: () => void openProject(c.key).catch(fail) },
      });
    },
    [refresh, fail],
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
    setSort((s) => (s.key === key ? { key, dir: s.dir === "asc" ? "desc" : "asc" } : { key, dir: key === "created" ? "desc" : "asc" }));

  // ── Menu bar (macOS) and keyboard (build spec §8) ──
  const runMenu = useCallback(
    (id: string) => {
      if (id === "new-project") setSheetOpen(true);
      else if (id === "rescan") rescan(true);
      else if (id === "search") searchRef.current?.focus();
      else if (id === "toggle-sidebar") setShowSidebar((v) => !v);
      else if (id === "toggle-inspector") setShowInspector((v) => !v);
      else if (id === "check-updates") checkUpdates();
    },
    [rescan, checkUpdates],
  );

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<string>("menu", (e) => runMenu(e.payload))
      .then((u) => (unlisten = u))
      .catch(() => {});
    return () => unlisten?.();
  }, [runMenu]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (sheetOpen) return; // the sheet has its own keys
      const cmd = hasCommandKey(e);
      // On macOS the menu bar owns these (its accelerators fire first); elsewhere JS does.
      const menuOwned = isMac();
      const target = e.target as HTMLElement;
      const inSearch = target === searchRef.current;
      const typing = !inSearch && (target.tagName === "INPUT" || target.tagName === "TEXTAREA");
      const key = e.key.toLowerCase();

      // Block the webview's own reload / find so they never fire.
      if (e.key === "F5" || (cmd && key === "r")) {
        e.preventDefault();
        if (e.key === "F5" || !menuOwned) rescan(true);
        return;
      }
      if (cmd && (key === "f" || key === "k")) {
        e.preventDefault();
        if (key === "f" || !menuOwned) searchRef.current?.focus();
        return;
      }
      if (cmd && e.key === ",") {
        e.preventDefault();
        if (!menuOwned) void openSettings().catch(fail);
        return;
      }
      if (cmd && !e.shiftKey && !e.altKey && key === "n") {
        e.preventDefault();
        if (!menuOwned) setSheetOpen(true);
        return;
      }
      if (cmd && e.altKey && e.code === "KeyI") {
        e.preventDefault();
        if (!menuOwned) setShowInspector((v) => !v);
        return;
      }
      if (typing) return;
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
  }, [sheetOpen, rescan, moveSelection, selectedRow, open, fail]);

  const closeSheet = useCallback(() => setSheetOpen(false), []);
  const dismissToast = useCallback(() => setToast(null), []);

  const firstScan = list === null || (list.scannedAt === null && projects.length === 0);
  const empty = (() => {
    if (firstScan)
      return <p className={styles.emptyHint}>{scanning ? "Looking for projects in your jobs folders…" : "Loading…"}</p>;
    if (projects.length === 0)
      return (
        <>
          <Icon name="folder" className={styles.emptyIcon} />
          <p className={styles.emptyTitle}>No projects yet</p>
          <p className={styles.emptyHint}>
            Projects you create with Nest, and any folder with a Nest project file inside your jobs folders, show up
            here.
          </p>
          <Button variant="primary" onClick={() => setSheetOpen(true)}>
            Create your first project
          </Button>
        </>
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

  const columns = [showSidebar ? "200px" : "", "minmax(0, 1fr)", showInspector ? "280px" : ""]
    .filter(Boolean)
    .join(" ");

  return (
    <div className={styles.window} style={{ gridTemplateColumns: columns }}>
      {showSidebar && (
        <Sidebar
          spaces={spaces}
          counts={counts}
          space={filters.space}
          status={filters.status}
          statusCounts={statusCounts}
          onSpace={(space) => setFilters((f) => ({ ...f, space }))}
          onStatus={(status) => setFilters((f) => ({ ...f, status }))}
          onSettings={() => void openSettings().catch(fail)}
          computers={computers}
          computer={computerGone ? null : (filters.computer ?? null)}
          onComputer={(computer) => setFilters((f) => ({ ...f, computer }))}
          hereCount={hereCount}
          allCount={allCount}
        />
      )}

      <main className={styles.list}>
        <header className={styles.toolbar}>
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
          <div className={styles.update} role="status">
            <span>
              <b>Nest {update.version}</b> is ready.
            </span>
            <Button variant="primary" onClick={restartToUpdate} disabled={installing}>
              {installing ? "Updating…" : "Restart to update"}
            </Button>
          </div>
        )}

        {offlineRoots.map((r) => (
          <div key={r.path} className={styles.banner} role="status">
            <Icon name="warning" />
            Jobs folder not found: {r.path}. Its projects are shown greyed out until it's back.
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
              onContextMenu={(r) => void showContextMenu(r)}
            />
          </div>
        ) : (
          <div className={`${styles.empty} ${styles.fill}`}>{empty}</div>
        )}

        <footer className={styles.status}>
          {scanning
            ? "Scanning your jobs folders…"
            : `${list?.roots.length ?? 0} jobs folder${list?.roots.length === 1 ? "" : "s"} watched`}
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

      {showInspector && (
        <Inspector
          project={selectedRow}
          onOpen={() => selectedRow && void open(selectedRow)}
          onReveal={() => selectedRow && void reveal(selectedRow)}
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
          sizes={selectedRow && sizeInfo?.key === selectedRow.key ? sizeInfo.sizes : null}
          measuring={!!selectedRow && sizeInfo?.key === selectedRow.key && sizeInfo.measuring}
          onMeasure={() => selectedRow && measure(selectedRow.key)}
        />
      )}

      {sheetOpen && <NewProjectSheet onClose={closeSheet} onCreated={onCreated} />}
      {toast && <Toast toast={toast} onDismiss={dismissToast} />}
    </div>
  );
}
