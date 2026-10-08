import { useState } from "react";
import Icon from "../../components/Icon";
import { Button, IconButton, PathText } from "../../components/ui";
import {
  addJobsRoot,
  clearArchiveFolder,
  forgetPastArchiveFolder,
  moveJobsRoot,
  removeJobsRoot,
  setArchiveAfterDays,
  setArchiveFolder,
  setDefaultSpace,
  setLaunchAtLogin,
  setPersonalRoot,
  reusePastArchiveFolder,
} from "../../lib/commands";

const SUGGESTED_DAYS = 90;
import type { TabProps } from "../Settings";
import styles from "./settings.module.css";

/** General: jobs folders, default space, launch at login. */
export default function GeneralTab({ view, run }: TabProps) {
  const s = view.settings;
  const roots = s.jobsRoots;
  // Said after changing / turning off the archive folder, so its address is never lost.
  const oldArchiveNote = s.archiveFolder
    ? `The old one (${s.archiveFolder}) is under "Previous archive folders".`
    : undefined;
  // The days box saves when you leave it (not on every keystroke).
  const [days, setDays] = useState<string>(String(s.archiveAfterDays ?? SUGGESTED_DAYS));
  const commitDays = () => {
    const n = Number(days);
    if (Number.isInteger(n) && n >= 1 && n <= 3650) {
      if (n !== s.archiveAfterDays) void run(setArchiveAfterDays(n));
    } else {
      setDays(String(s.archiveAfterDays ?? SUGGESTED_DAYS));
    }
  };

  return (
    <>
      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Jobs folders</h3>
        <p className={styles.hint}>
          Nest finds your projects in these folders. New ones go in the first. Removing a folder doesn't delete anything.
        </p>
        {roots.length > 0 && (
          <div className={styles.card}>
            {roots.map((path, i) => (
              <div key={path} className={styles.row}>
                <Icon name="folder" className={styles.folderIcon} />
                <span className={`${styles.grow} ${styles.mono}`}>
                  <PathText path={path} />
                </span>
                {i === 0 && <span className={styles.pill}>New projects go here</span>}
                {path === s.personalRoot && <span className={styles.pill}>Personal projects go here</span>}
                <IconButton
                  icon="down"
                  label="Move up"
                  disabled={i === 0}
                  style={{ transform: "rotate(180deg)" }}
                  onClick={() => run(moveJobsRoot(path, i - 1))}
                />
                <IconButton
                  icon="down"
                  label="Move down"
                  disabled={i === roots.length - 1}
                  onClick={() => run(moveJobsRoot(path, i + 1))}
                />
                <Button onClick={() => run(removeJobsRoot(path), `Removed ${path} from Nest. Your files weren't touched.`)}>Remove</Button>
              </div>
            ))}
          </div>
        )}
        <div className={styles.actions}>
          <Button onClick={() => run(addJobsRoot())}>Add folder…</Button>
        </div>
      </section>

      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>New projects</h3>
        <div className={styles.formRow}>
          <label htmlFor="default-space">Default space</label>
          <select
            id="default-space"
            className={styles.select}
            value={s.defaultSpace}
            onChange={(e) => run(setDefaultSpace(e.target.value))}
          >
            {s.spaces.map((sp) => (
              <option key={sp} value={sp}>
                {sp}
              </option>
            ))}
          </select>
        </div>
        <div className={styles.formRow}>
          <label htmlFor="personal-root">Personal projects go in</label>
          <select
            id="personal-root"
            className={styles.select}
            value={s.personalRoot ?? ""}
            onChange={(e) => run(setPersonalRoot(e.target.value === "" ? null : e.target.value))}
          >
            <option value="">The same folder as other new projects</option>
            {roots.map((path) => (
              <option key={path} value={path}>
                {path}
              </option>
            ))}
          </select>
        </div>
        <p className={styles.hint}>
          For the {s.noClientSpaces[0] ?? "Personal"} space and “No client” projects. The folder must be in the list
          above.
        </p>
      </section>

      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Archive</h3>
        <p className={styles.hint}>
          A folder for finished projects. Select a Done project and click Archive… to move it there. Nothing is ever
          deleted.
        </p>
        {s.archiveFolder ? (
          <div className={styles.card}>
            <div className={styles.row}>
              <Icon name="folder" className={styles.folderIcon} />
              <span className={`${styles.grow} ${styles.mono}`}>
                <PathText path={s.archiveFolder} />
              </span>
              <Button onClick={() => run(setArchiveFolder(), oldArchiveNote)}>Change…</Button>
              <Button onClick={() => run(clearArchiveFolder(), `Archive is off. Nothing on disk changed. ${oldArchiveNote}`)}>
                Turn off
              </Button>
            </div>
            <div className={styles.row}>
              <label className={styles.check}>
                <input
                  type="checkbox"
                  checked={s.archiveAfterDays !== null}
                  onChange={(e) => {
                    const n = Number(days);
                    const next = Number.isInteger(n) && n >= 1 && n <= 3650 ? n : SUGGESTED_DAYS;
                    setDays(String(next));
                    void run(setArchiveAfterDays(e.target.checked ? next : null));
                  }}
                />
                Suggest archiving projects that have been done for
              </label>
              <input
                className={styles.input}
                style={{ width: 64, textAlign: "right" }}
                type="number"
                min={1}
                max={3650}
                aria-label="Days"
                value={days}
                disabled={s.archiveAfterDays === null}
                onChange={(e) => setDays(e.target.value)}
                onBlur={commitDays}
                onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
              />
              <span>days</span>
            </div>
          </div>
        ) : (
          <div className={styles.actions}>
            <Button onClick={() => run(setArchiveFolder())}>Choose archive folder…</Button>
          </div>
        )}
        {s.pastArchiveFolders.length > 0 && (
          <>
            <p className={styles.hint}>Previous archive folders (their projects are still in them):</p>
            <div className={styles.card}>
              {s.pastArchiveFolders.map((path) => (
                <div key={path} className={styles.row}>
                  <Icon name="folder" className={styles.folderIcon} />
                  <span className={`${styles.grow} ${styles.mono}`}>
                    <PathText path={path} />
                  </span>
                  <Button onClick={() => run(reusePastArchiveFolder(path), `Using ${path} as the archive folder again.`)}>
                    Use again
                  </Button>
                  <Button
                    onClick={() => run(forgetPastArchiveFolder(path), "Removed from this list. Nothing on disk changed.")}
                  >
                    Forget
                  </Button>
                </div>
              ))}
            </div>
          </>
        )}
      </section>

      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Startup</h3>
        <label className={styles.check}>
          <input
            type="checkbox"
            checked={s.launchAtLogin}
            disabled={view.devBuild}
            onChange={(e) => run(setLaunchAtLogin(e.target.checked))}
          />
          Open Nest when you log in
        </label>
        {view.devBuild && <p className={styles.hint}>Only available in the installed app.</p>}
      </section>
    </>
  );
}
