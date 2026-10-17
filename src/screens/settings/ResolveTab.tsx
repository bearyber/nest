import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Button } from "../../components/ui";
import {
  listTemplates,
  resolveLibraries,
  resolveRemoveWaiting,
  resolveSetLibrary,
  resolveStatus,
  resolveTryNow,
  resolveWaitingList,
} from "../../lib/commands";
import { libraryKey, outcomeWords, sameLibrary, shortDay, statusLine } from "../../lib/resolve";
import { ago } from "../../lib/search";
import type { ResolveLibrary, ResolveStatus, ResolveWaitingView } from "../../lib/types";
import type { TabProps } from "../Settings";
import styles from "./settings.module.css";

/** DaVinci Resolve: is it reachable, which library new projects go to, and what's waiting.
 *  Only on this computer. */
export default function ResolveTab({ view, run }: TabProps) {
  /** null = still finding out. */
  const [used, setUsed] = useState<boolean | null>(null);
  const [status, setStatus] = useState<ResolveStatus | null>(null);
  const [libraries, setLibraries] = useState<ResolveLibrary[]>([]);
  const [waiting, setWaiting] = useState<ResolveWaitingView | null>(null);
  const [trying, setTrying] = useState(false);

  const check = useCallback(() => {
    resolveStatus()
      .then((s) => {
        setStatus(s);
        if (s !== "ready") return;
        resolveLibraries()
          .then((l) => setLibraries(l.libraries))
          .catch(() => setLibraries([]));
      })
      .catch(() => setStatus("notRunning"));
  }, []);

  const loadWaiting = useCallback(
    () =>
      resolveWaitingList()
        .then(setWaiting)
        .catch(() => {}),
    [],
  );

  useEffect(() => {
    let live = true;
    listTemplates()
      .then((ts) => live && setUsed(ts.some((t) => t.resolve && !t.superseded)))
      .catch(() => live && setUsed(false));
    void loadWaiting();
    let unlisten: (() => void) | undefined;
    listen("resolve-changed", () => void loadWaiting())
      .then((u) => (live ? (unlisten = u) : u()))
      .catch(() => {});
    return () => {
      live = false;
      unlisten?.();
    };
  }, [loadWaiting]);

  const entries = waiting?.entries ?? [];
  // Projects still waiting count too (their template may be gone).
  const shown = used === true || entries.length > 0;
  useEffect(() => {
    if (shown) check();
  }, [shown, check]);

  if (used === null && !waiting) return null;
  if (!shown) {
    return <p className={styles.hint}>No template uses a Resolve project yet.</p>;
  }

  const saved = view.settings.resolveLibrary ?? null;
  // The saved library stays a choice while Resolve is closed (or no longer lists it).
  const choices = [...libraries, ...(saved && !libraries.some((l) => sameLibrary(l, saved)) ? [saved] : [])];
  const line = status ? statusLine(status) : null;

  const chooseLibrary = (key: string) => {
    const library = choices.find((l) => libraryKey(l) === key) ?? null;
    void run(resolveSetLibrary(library));
  };

  const tryNow = () => {
    setTrying(true);
    run(resolveTryNow())
      .then((v) => v && setWaiting(v))
      .finally(() => setTrying(false));
  };

  const remove = (id: string) => {
    void run(resolveRemoveWaiting(id)).then((v) => v && setWaiting(v));
  };

  return (
    <>
      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Resolve</h3>
        <div className={styles.card}>
          <div className={styles.row}>
            <span className={styles.grow} role="status">
              {line ? (
                <span className={line.ok ? styles.name : undefined}>{line.text}</span>
              ) : (
                <span className={styles.sub}>Checking…</span>
              )}
            </span>
            {status && status !== "ready" && status !== "notInstalled" && (
              <Button
                onClick={() => {
                  setStatus(null);
                  check();
                }}
              >
                Check again
              </Button>
            )}
          </div>
        </div>
      </section>

      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Library</h3>
        <div className={styles.formRow}>
          <label htmlFor="resolve-library">Add projects to</label>
          <div>
            <select
              id="resolve-library"
              className={styles.select}
              value={libraryKey(saved)}
              onChange={(e) => chooseLibrary(e.target.value)}
            >
              <option value="">The library open in Resolve</option>
              {choices.map((l) => (
                <option key={libraryKey(l)} value={libraryKey(l)}>
                  {l.dbName}
                </option>
              ))}
            </select>
          </div>
        </div>
        <p className={styles.hint}>Nest only switches library when no project is open in Resolve.</p>
      </section>

      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Waiting for Resolve</h3>
        {entries.length === 0 ? (
          <p className={styles.hint}>Nothing is waiting.</p>
        ) : (
          <>
            {waiting?.hint && <p className={styles.hint}>{waiting.hint}</p>}
            <div className={styles.card}>
              {entries.map((e) => (
                <div key={e.id} className={styles.row}>
                  <span className={styles.grow}>
                    <span className={styles.name}>{e.name}</span> <span className={styles.sub}>{e.jobCode}</span>
                    <br />
                    <span className={styles.sub}>
                      Since {shortDay(e.createdAt)} · {outcomeWords(e.lastOutcome)}
                      {e.lastTry ? ` · ${ago(e.lastTry)}` : ""}
                    </span>
                  </span>
                  <Button onClick={() => remove(e.id)} disabled={trying} title="Resolve won't get this project">
                    Remove
                  </Button>
                </div>
              ))}
            </div>
            <div className={styles.actions}>
              <Button onClick={tryNow} disabled={trying}>
                {trying ? "Trying…" : "Try now"}
              </Button>
            </div>
          </>
        )}
      </section>
    </>
  );
}
