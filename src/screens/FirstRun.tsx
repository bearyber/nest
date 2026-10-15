import { useEffect, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import Icon from "../components/Icon";
import { Button } from "../components/ui";
import {
  completeFirstRun,
  firstRunCreateFolder,
  firstRunPickFolder,
  firstRunPickTemplateFolder,
  getSettings,
} from "../lib/commands";
import { isMac } from "../lib/platform";
import { errorMessage, type FolderInfo } from "../lib/types";
import styles from "./FirstRun.module.css";

type Step = "folder" | "template";
const STEPS: Step[] = ["folder", "template"];

/** The last part of a folder path, for showing its name (works for / and \). */
function folderName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

/** First run: two screens. Where jobs live, then (optionally) the folder you copy for every job. */
export default function FirstRun({ onDone }: { onDone: () => void }) {
  const [step, setStep] = useState<Step>("folder");
  const [folder, setFolder] = useState<FolderInfo | null>(null);
  const [template, setTemplate] = useState<string | null>(null);
  const [over, setOver] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [marker, setMarker] = useState(isMac() ? "M" : "W");

  useEffect(() => {
    getSettings()
      .then((v) => v.suggestedMarker && setMarker(v.suggestedMarker))
      .catch(() => {});
  }, []);

  // A folder dropped anywhere on the window counts, on the second screen.
  useEffect(() => {
    if (step !== "template") return;
    let unlisten: (() => void) | undefined;
    getCurrentWebview()
      .onDragDropEvent((e) => {
        const t = e.payload.type;
        if (t === "enter" || t === "over") setOver(true);
        else if (t === "leave") setOver(false);
        else if (t === "drop") {
          setOver(false);
          if (e.payload.paths.length > 0) {
            setError(null);
            setTemplate(e.payload.paths[0]);
          }
        }
      })
      .then((u) => (unlisten = u))
      .catch(() => {});
    return () => unlisten?.();
  }, [step]);

  const tryFolder = (p: Promise<FolderInfo | null>) =>
    p.then((f) => f && setFolder(f)).catch((e) => setError(errorMessage(e)));
  const pickTemplate = () =>
    firstRunPickTemplateFolder()
      .then((p) => p && setTemplate(p))
      .catch((e) => setError(errorMessage(e)));

  const finish = (withTemplate: boolean) => {
    if (!folder) return;
    setSaving(true);
    // Spaces (Work, and Personal with no client), all starter templates and this computer's
    // letter are the defaults. All of them can be changed in Settings.
    completeFirstRun(folder.path, ["Work", "Personal"], ["Personal"], [], marker, withTemplate ? template : null)
      .then(onDone)
      .catch((e) => {
        setError(errorMessage(e));
        setSaving(false);
      });
  };

  const index = STEPS.indexOf(step);

  return (
    <div className={styles.screen}>
      <div className={styles.card}>
        <ol className={styles.dots} aria-label={`Step ${index + 1} of ${STEPS.length}`}>
          {STEPS.map((s, i) => (
            <li key={s} className={i <= index ? styles.dotOn : styles.dot} />
          ))}
        </ol>

        {step === "folder" && (
          <>
            <p className={styles.welcome}>Welcome to Nest</p>
            <h1>Where do your jobs live?</h1>
            <p>Every job gets a home: one click makes the folders, starter files and job code.</p>
            {folder && (
              <div className={styles.picked}>
                <Icon name="folder" className={styles.folderIcon} />
                <span className={styles.mono}>{folder.path}</span>
                {folder.projects > 0 && (
                  <span className={styles.found}>
                    Found {folder.projects} project{folder.projects === 1 ? "" : "s"}
                  </span>
                )}
              </div>
            )}
            <div className={styles.buttons}>
              <Button onClick={() => tryFolder(firstRunCreateFolder())}>Make a "Nest Jobs" folder in Documents</Button>
              <Button onClick={() => tryFolder(firstRunPickFolder())}>Choose folder…</Button>
            </div>
            <div className={styles.nav}>
              <span />
              <Button
                variant="primary"
                disabled={!folder}
                onClick={() => {
                  setError(null);
                  setStep("template");
                }}
              >
                Continue
              </Button>
            </div>
          </>
        )}

        {step === "template" && (
          <>
            <h1>Got a folder you copy for every job?</h1>
            <p>Drop it here and Nest makes the same folders for every new project. Your folder isn't changed.</p>
            <div className={over ? `${styles.drop} ${styles.dropOver}` : styles.drop}>
              {template ? (
                <>
                  <Icon name="folder" className={styles.folderIcon} />
                  <b>{folderName(template)}</b>
                  <button type="button" className={styles.link} onClick={() => void pickTemplate()}>
                    Change
                  </button>
                </>
              ) : (
                <>
                  <span className={styles.sub}>Drop a folder here</span>
                  <Button onClick={() => void pickTemplate()}>Choose a folder…</Button>
                </>
              )}
            </div>
            <div className={styles.nav}>
              <Button disabled={saving} onClick={() => setStep("folder")}>
                Back
              </Button>
              <span className={styles.navRight}>
                {!template && (
                  <Button disabled={saving} onClick={() => finish(false)}>
                    Skip, use Nest's templates
                  </Button>
                )}
                <Button variant="primary" disabled={saving || !template} onClick={() => finish(true)}>
                  {saving ? "Setting up…" : "Finish"}
                </Button>
              </span>
            </div>
          </>
        )}

        {error && (
          <p className={styles.error} role="alert">
            {error}
          </p>
        )}
      </div>
    </div>
  );
}
