import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useTauriListener } from "../../lib/useTauriListener";
import { Button, PathText } from "../../components/ui";
import {
  devicesClaim,
  devicesDetect,
  devicesDisable,
  devicesEnable,
  devicesForget,
  devicesPickFolder,
  devicesSetName,
  devicesSetSyncTemplates,
  devicesView,
  setCodeSettings,
} from "../../lib/commands";
import { isMac } from "../../lib/platform";
import { ago, isFresh } from "../../lib/search";
import type { DevicesView, OtherComputer, SyncedFolder } from "../../lib/types";
import type { TabProps } from "../Settings";
import { Warning, computersWithLetter, previewCode, yourComputers } from "./CodesTab";
import styles from "./settings.module.css";

/** Computers: this computer's name and job-code letter (always editable, sharing or not), then
 *  Devices (spec §12a #16): see which of your computers has which project, through a folder
 *  your cloud app (Google Drive…) or NAS already syncs. */
export default function DevicesTab({ view: settingsView, run, focusLetter }: TabProps) {
  const st = settingsView.settings;
  const [view, setView] = useState<DevicesView | null>(null);
  const [found, setFound] = useState<SyncedFolder[] | null>(null);
  const [name, setName] = useState("");
  const [folder, setFolder] = useState<string | null>(null);
  const [custom, setCustom] = useState<string | null>(null);
  const [letter, setLetter] = useState(st.localMarker);
  /** "Keep W": the warning for that letter stays quiet until the letter changes. */
  const [kept, setKept] = useState<string | null>(null);
  // The saved letter changed (saved here, or by the same-letter note): the box follows it.
  const [seenMarker, setSeenMarker] = useState(st.localMarker);
  if (seenMarker !== st.localMarker) {
    setSeenMarker(st.localMarker);
    setLetter(st.localMarker);
  }
  const letterBox = useRef<HTMLInputElement>(null);

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
    devicesDetect()
      .then((f) => {
        setFound(f);
        setFolder((cur) => cur ?? f[0]?.path ?? null);
      })
      .catch(() => setFound([]));
  }, []);
  useTauriListener(
    (cb) => listen("devices-changed", cb),
    () => void load(),
  );
  useEffect(() => {
    void load();
  }, [st.localMarker, load]);
  // Opened by the same-letter note: the cursor goes to the letter.
  const ready = view !== null;
  useEffect(() => {
    if (focusLetter && ready) letterBox.current?.focus();
  }, [focusLetter, ready]);

  if (!view) return null;
  const thisOs = isMac() ? "macos" : "windows";
  const choices: SyncedFolder[] = [
    ...(found ?? []),
    ...(custom && !(found ?? []).some((f) => f.path === custom) ? [{ label: "Chosen folder", path: custom }] : []),
    ...(view.folder && ![...(found ?? []), ...(custom ? [{ path: custom }] : [])].some((f) => f.path === view.folder)
      ? [{ label: "Current folder", path: view.folder }]
      : []),
  ];
  const changed = folder !== view.folder;

  const trimmedName = name.trim();
  const nameOk = trimmedName.length > 0 && trimmedName.length <= 40;
  const letterProblem = letter.startsWith("J")
    ? "J is kept for jobs from your job tracker, so a letter can't start with J. Pick another."
    : letter.length > 2
      ? `Older versions allowed "${letter}". Use 1 or 2 letters to save changes.`
      : letter.length === 0
        ? "Every computer needs one letter."
        : null;
  const nameChanged = trimmedName !== view.name;
  const letterChanged = letter !== st.localMarker;
  const takenBy = letterProblem ? [] : computersWithLetter(view.letters, letter);
  const suggested = view.suggestedLetter;
  const savedNote = (l: string) => `Saved. Your next code will look like ${previewCode(st.codePattern, "ACME", l, 4)}.`;
  /** One button, one change: only the letter is saved (the name stays as typed). */
  const saveLetter = (l: string) => run(setCodeSettings(st.codePattern, l, st.ownCode), savedNote(l));
  const saveThisComputer = async () => {
    if (nameChanged) {
      const v = await run(devicesSetName(trimmedName), letterChanged ? undefined : "Saved.");
      if (!v) return;
      setView(v);
      setName(v.name);
    }
    if (letterChanged) await saveLetter(letter);
  };

  const pick = async () => {
    const p = await run(devicesPickFolder());
    if (p) {
      setCustom(p);
      setFolder(p);
    }
  };

  return (
    <>
      <section className={styles.section} aria-labelledby="this-computer-title">
        <h3 className={styles.sectionTitle} id="this-computer-title">
          This computer
        </h3>
        <div className={styles.card}>
          <div className={styles.field}>
            <label className={styles.name} htmlFor="device-name">
              Name
            </label>
            <input
              id="device-name"
              className={styles.input}
              style={{ width: 240 }}
              value={name}
              maxLength={40}
              aria-describedby="device-name-hint"
              onChange={(e) => setName(e.target.value)}
            />
            <p className={styles.hint} id="device-name-hint">
              Shown on your other computers, like “On {trimmedName || "Studio PC"}”.
            </p>
          </div>
          <div className={styles.field}>
            <label className={styles.name} htmlFor="marker">
              Letter
            </label>
            <input
              id="marker"
              ref={letterBox}
              className={`${styles.input} ${styles.codeInput}`}
              value={letter}
              maxLength={letter.length > 2 ? undefined : 2}
              aria-describedby="marker-hint"
              onChange={(e) => setLetter(e.target.value.toUpperCase().replace(/[^A-Z]/g, ""))}
            />
            <p className={styles.hint} id="marker-hint">
              Every computer has its own letter, so two never make the same code. Use 1 or 2 letters, for example W on your
              PC and M on your Mac. e.g.{" "}
              <b className={styles.mono}>{previewCode(st.codePattern, "ACME", letter || "MB", 4)}</b>
            </p>
            <p className={styles.problem} role="status" hidden={!letterProblem}>
              {letterProblem}
            </p>
            {!letterProblem && letterChanged && (
              <p className={styles.hint}>
                Your numbers carry on from the old letter ({st.localMarker}05 is followed by {letter}06). Nothing already made
                is renamed.
              </p>
            )}
            <Warning
              text={
                takenBy.length > 0 && kept !== letter
                  ? `${letter} ${takenBy.some((l) => l.guessed) ? "seems to be used" : "is used"} by ${yourComputers(takenBy.map((l) => l.name))}.${suggested ? ` Use ${suggested} instead?` : ""}`
                  : null
              }
            >
              {suggested && (
                <Button onClick={() => void saveLetter(suggested).then((r) => r !== undefined && setLetter(suggested))}>
                  Use {suggested}
                </Button>
              )}
              <Button onClick={() => setKept(letter)}>Keep {letter}</Button>
            </Warning>
            {!letterProblem && st.localMarker === "L" && letter === "L" && (
              <p className={styles.note}>L was the old default. If another of your computers also uses L, change one of them.</p>
            )}
          </div>
        </div>
        <div className={styles.actions}>
          <Button
            variant="primary"
            disabled={!nameOk || !!letterProblem || (!nameChanged && !letterChanged)}
            onClick={() => void saveThisComputer()}
          >
            Save
          </Button>
          <Button
            disabled={!nameChanged && !letterChanged}
            onClick={() => {
              setName(view.name);
              setLetter(st.localMarker);
            }}
          >
            Undo changes
          </Button>
        </div>
      </section>

      <hr className={styles.rule} />

      <h3 className={styles.sectionTitle}>Share with your other computers</h3>
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
          disabled={!folder || (view.enabled && !changed)}
          onClick={async () => {
            if (!folder) return;
            const v = await run(
              devicesEnable(view.name, folder),
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
          <label className={styles.check}>
            <input
              type="checkbox"
              checked={view.syncTemplates}
              onChange={async (e) => {
                const v = await run(devicesSetSyncTemplates(e.target.checked));
                if (v) setView(v);
              }}
            />
            Keep my templates the same on all my computers
          </label>
          <p className={styles.hint}>
            Templates you save show up on your other computers through the same synced folder; deleting one only removes
            it here.
          </p>
          {view.syncTemplates && view.templateProblem && <p className={styles.problem}>{view.templateProblem}</p>}
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
