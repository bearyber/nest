import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Button, PathText } from "../../components/ui";
import {
  devicesClaim,
  devicesDetect,
  devicesDisable,
  devicesEnable,
  devicesForget,
  devicesPickFolder,
  devicesView,
} from "../../lib/commands";
import { isMac } from "../../lib/platform";
import { ago, isFresh } from "../../lib/search";
import type { DevicesView, OtherComputer, SyncedFolder } from "../../lib/types";
import type { TabProps } from "../Settings";
import styles from "./settings.module.css";

/** Devices (spec §12a #16): see which of your computers has which project, through a folder
 *  your cloud app (Google Drive…) or NAS already syncs. */
export default function DevicesTab({ run }: TabProps) {
  const [view, setView] = useState<DevicesView | null>(null);
  const [found, setFound] = useState<SyncedFolder[] | null>(null);
  const [name, setName] = useState("");
  const [folder, setFolder] = useState<string | null>(null);
  const [custom, setCustom] = useState<string | null>(null);

  const load = useCallback(
    () =>
      devicesView()
        .then((v) => {
          setView(v);
          setName((n) => n || v.name);
          setFolder((f) => f ?? v.folder);
        })
        .catch(() => {}),
    [],
  );

  useEffect(() => {
    void load();
    devicesDetect()
      .then((f) => {
        setFound(f);
        setFolder((cur) => cur ?? f[0]?.path ?? null);
      })
      .catch(() => setFound([]));
    let unlisten: (() => void) | undefined;
    listen("devices-changed", () => void load())
      .then((u) => (unlisten = u))
      .catch(() => {});
    return () => unlisten?.();
  }, [load]);

  if (!view) return null;
  const thisOs = isMac() ? "macos" : "windows";
  const choices: SyncedFolder[] = [
    ...(found ?? []),
    ...(custom && !(found ?? []).some((f) => f.path === custom) ? [{ label: "Chosen folder", path: custom }] : []),
    ...(view.folder && ![...(found ?? []), ...(custom ? [{ path: custom }] : [])].some((f) => f.path === view.folder)
      ? [{ label: "Current folder", path: view.folder }]
      : []),
  ];
  const changed = name.trim() !== view.name || folder !== view.folder;

  const pick = async () => {
    const p = await run(devicesPickFolder());
    if (p) {
      setCustom(p);
      setFolder(p);
    }
  };

  return (
    <>
      <p className={styles.hint}>
        See which of your computers has which project. Each computer keeps a small list of its projects in a folder your
        cloud app already syncs (like Google Drive), and reads the others' lists from there. Nest never copies your
        project files, and there's no account or server.
      </p>

      <div className={styles.how}>
        <div>
          <b>1 · Pick a synced folder</b>
          <span>The same service on each computer, like Google Drive.</span>
        </div>
        <div>
          <b>2 · Each computer writes its own list</b>
          <span>Job codes, project names and where they are. A few KB. Never footage.</span>
        </div>
        <div>
          <b>3 · Your cloud app copies it over</b>
          <span>Usually within a minute or two. Nest shows how fresh each list is.</span>
        </div>
      </div>

      <section className={styles.section}>
        <label className={styles.sectionTitle} htmlFor="device-name">
          This computer's name
        </label>
        <div className={styles.actions}>
          <input
            id="device-name"
            className={styles.input}
            style={{ width: 240 }}
            value={name}
            maxLength={40}
            onChange={(e) => setName(e.target.value)}
          />
        </div>
        <p className={styles.hint}>Shown on your other computers, like “On {name.trim() || "Studio PC"}”.</p>
      </section>

      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Synced folder</h3>
        {found === null ? (
          <p className={styles.hint}>Looking for Google Drive…</p>
        ) : choices.length === 0 ? (
          <p className={styles.hint}>
            No Google Drive found on this computer. Install Google Drive for desktop (free), or choose any folder that
            syncs between your computers (Dropbox, iCloud Drive, OneDrive, a NAS).
          </p>
        ) : null}
        <div className={styles.choices} role="radiogroup" aria-label="Synced folder">
          {choices.map((c) => (
            <label key={c.path} className={styles.choice}>
              <input type="radio" name="devices-folder" checked={folder === c.path} onChange={() => setFolder(c.path)} />
              <b>{c.label}</b>
              <small className={styles.mono}>
                <PathText path={c.path} />
              </small>
              {c.path === custom && (
                <button
                  type="button"
                  className={styles.choiceRemove}
                  aria-label="Remove this folder from the list"
                  title="Remove from the list"
                  onClick={(e) => {
                    // Inside the card's label: don't also select the card.
                    e.preventDefault();
                    e.stopPropagation();
                    setCustom(null);
                    if (folder === c.path) setFolder(found?.[0]?.path ?? view.folder ?? null);
                  }}
                >
                  ×
                </button>
              )}
            </label>
          ))}
        </div>
        <div className={styles.actions}>
          <Button onClick={() => void pick()}>Choose another folder…</Button>
        </div>
      </section>

      <div className={styles.actions}>
        <Button
          variant="primary"
          disabled={!folder || !name.trim() || (view.enabled && !changed)}
          onClick={async () => {
            if (!folder) return;
            const v = await run(
              devicesEnable(name.trim(), folder),
              view.enabled ? "Saved." : "Devices is on. Do the same on your other computers.",
            );
            if (v) setView(v);
          }}
        >
          {view.enabled ? "Save changes" : "Turn on Devices"}
        </Button>
        {view.enabled && (
          <Button
            onClick={async () => {
              const v = await run(devicesDisable(), "This computer stopped sharing its list.");
              if (v) setView(v);
            }}
          >
            Stop sharing this computer's list
          </Button>
        )}
      </div>
      {/* Said next to the button, before anything is shared. */}
      <p className={styles.note}>
        The lists include client names and folder paths. Use your own cloud folder, not one you share with other people.
      </p>

      {view.enabled && (
        <section className={styles.section}>
          <h3 className={styles.sectionTitle}>Your computers</h3>
          {view.problem && <p className={styles.problem}>{view.problem}</p>}
          <div className={styles.card}>
            <div className={styles.dev}>
              <span className={styles.devDotFresh} />
              <div>
                <b>{view.name}</b> <span className={styles.pill}>This computer</span>
                <div className={styles.sub}>
                  {view.thisProjectCount} project{view.thisProjectCount === 1 ? "" : "s"}
                  {view.savedAt ? ` · list saved ${ago(view.savedAt)}` : ""}
                </div>
              </div>
            </div>
            {view.others.map((d) => (
              <Other
                key={d.deviceId}
                d={d}
                canClaim={d.os === thisOs}
                onForget={async () => {
                  const v = await run(devicesForget(d.deviceId, true), `${d.name} is hidden on this computer.`);
                  if (v) setView(v);
                }}
                onClaim={async () => {
                  const v = await run(devicesClaim(d.deviceId), `This computer is ${d.name} again.`);
                  if (v) {
                    setView(v);
                    setName(v.name);
                  }
                }}
              />
            ))}
            {view.others.length === 0 && (
              <div className={styles.dev}>
                <span className={styles.devDotOff} />
                <div className={styles.sub}>
                  No other computers yet. Turn on Devices on your other computer, with the same Google Drive.
                </div>
              </div>
            )}
          </div>
          {view.forgotten.map((d) => (
            <div key={d.deviceId} className={styles.actions}>
              <span className={styles.sub}>{d.name} is hidden on this computer.</span>
              <Button
                onClick={async () => {
                  const v = await run(devicesForget(d.deviceId, false));
                  if (v) setView(v);
                }}
              >
                Show again
              </Button>
            </div>
          ))}
        </section>
      )}
    </>
  );
}

function Other({
  d,
  canClaim,
  onForget,
  onClaim,
}: {
  d: OtherComputer;
  canClaim: boolean;
  onForget: () => void;
  onClaim: () => void;
}) {
  const [sure, setSure] = useState(false);
  const [claimSure, setClaimSure] = useState(false);
  const status = d.notDownloaded
    ? "List not downloaded yet: let your cloud app finish syncing"
    : d.problem
      ? `${d.problem} Showing the last good one.`
      : `${d.projectCount} project${d.projectCount === 1 ? "" : "s"}${d.updatedAt ? ` · updated ${ago(d.updatedAt)}` : ""}`;
  return (
    <div className={styles.dev}>
      <span className={d.notDownloaded ? styles.devDotOff : isFresh(d.updatedAt) ? styles.devDotFresh : styles.devDotStale} />
      <div>
        <b>{d.name}</b>
        <div className={styles.sub}>{status}</div>
        {canClaim && !claimSure && (
          <button type="button" className={styles.linkish} onClick={() => setClaimSure(true)}>
            This is this computer (after reinstalling Nest)
          </button>
        )}
        {claimSure && (
          <div className={styles.confirmRow}>
            <span className={styles.sub}>
              Only if you reinstalled Nest on this computer. This computer will take over “{d.name}”'s list.
            </span>
            <Button onClick={() => setClaimSure(false)}>Cancel</Button>
            <Button onClick={onClaim}>Yes, this is {d.name}</Button>
          </div>
        )}
      </div>
      <Button onClick={() => (sure ? onForget() : setSure(true))}>{sure ? "Click again to hide" : "Hide"}</Button>
    </div>
  );
}
