import { useEffect, useState } from "react";
import Icon from "../components/Icon";
import { Button } from "../components/ui";
import { completeFirstRun, firstRunCreateFolder, firstRunPickFolder, getSettings, listTemplates } from "../lib/commands";
import { isMac } from "../lib/platform";
import { errorMessage, type FolderInfo, type TemplateInfo } from "../lib/types";
import styles from "./FirstRun.module.css";

type Step = "welcome" | "folder" | "templates" | "spaces" | "letter";
const STEPS: Step[] = ["welcome", "folder", "templates", "spaces", "letter"];

interface SpaceDraft {
  name: string;
  noClient: boolean;
}

/** First run (build spec §3.4): shown when there's no settings file yet. */
export default function FirstRun({ onDone }: { onDone: () => void }) {
  const [step, setStep] = useState<Step>("welcome");
  const [folder, setFolder] = useState<FolderInfo | null>(null);
  const [templates, setTemplates] = useState<TemplateInfo[]>([]);
  const [hidden, setHidden] = useState<Set<string>>(new Set());
  const [spaces, setSpaces] = useState<SpaceDraft[]>([
    { name: "Work", noClient: false },
    { name: "Personal", noClient: true },
  ]);
  const [newSpace, setNewSpace] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [marker, setMarker] = useState("");

  useEffect(() => {
    listTemplates()
      .then((all) => setTemplates(all.filter((t) => !t.superseded)))
      .catch(() => {});
    getSettings()
      .then((v) => setMarker(v.suggestedMarker))
      .catch(() => setMarker(isMac() ? "M" : "W"));
  }, []);

  const go = (s: Step) => {
    setError(null);
    setStep(s);
  };
  const tryFolder = (p: Promise<FolderInfo | null>) =>
    p.then((f) => f && setFolder(f)).catch((e) => setError(errorMessage(e)));

  const finish = () => {
    if (!folder) return;
    setSaving(true);
    completeFirstRun(
      folder.path,
      spaces.map((s) => s.name),
      spaces.filter((s) => s.noClient).map((s) => s.name),
      [...hidden],
      marker,
    )
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

        {step === "welcome" && (
          <>
            <Icon name="folder" className={styles.bigIcon} />
            <h1>Welcome to Nest</h1>
            <p>Every job gets a home: one click makes the folders, starter files and job code, the same way every time.</p>
            <div className={styles.buttons}>
              <Button variant="primary" onClick={() => go("folder")}>
                Get started
              </Button>
            </div>
          </>
        )}

        {step === "folder" && (
          <>
            <h1>Where do your jobs live?</h1>
            <p>Pick the folder new projects go in. You can add more folders later in Settings.</p>
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
              <Button onClick={() => tryFolder(firstRunPickFolder())}>Choose folder…</Button>
              <Button onClick={() => tryFolder(firstRunCreateFolder())}>Create one in Documents</Button>
            </div>
            <div className={styles.nav}>
              <Button onClick={() => go("welcome")}>Back</Button>
              <Button variant="primary" disabled={!folder} onClick={() => go("templates")}>
                Continue
              </Button>
            </div>
          </>
        )}

        {step === "templates" && (
          <>
            <h1>Starter templates</h1>
            <p>Untick any you won't use. You can show them again in Settings.</p>
            <div className={styles.list}>
              {templates.map((t) => (
                <label key={t.key} className={styles.check}>
                  <input
                    type="checkbox"
                    checked={!hidden.has(t.id)}
                    onChange={(e) =>
                      setHidden((h) => {
                        const next = new Set(h);
                        if (e.target.checked) next.delete(t.id);
                        else next.add(t.id);
                        return next;
                      })
                    }
                  />
                  <span>
                    <b>{t.name}</b> <span className={styles.sub}>{t.description}</span>
                  </span>
                </label>
              ))}
            </div>
            <div className={styles.nav}>
              <Button onClick={() => go("folder")}>Back</Button>
              <Button variant="primary" onClick={() => go("spaces")}>
                Continue
              </Button>
            </div>
          </>
        )}

        {step === "spaces" && (
          <>
            <h1>Spaces</h1>
            <p>Spaces group your projects. In a "no client" space the Billed to field is hidden.</p>
            <div className={styles.list}>
              {spaces.map((s, i) => (
                <div key={i} className={styles.spaceRow}>
                  <input
                    className={styles.input}
                    value={s.name}
                    aria-label="Space name"
                    onChange={(e) => setSpaces((all) => all.map((x, j) => (j === i ? { ...x, name: e.target.value } : x)))}
                  />
                  <label className={styles.check}>
                    <input
                      type="checkbox"
                      checked={s.noClient}
                      onChange={(e) =>
                        setSpaces((all) => all.map((x, j) => (j === i ? { ...x, noClient: e.target.checked } : x)))
                      }
                    />
                    No client
                  </label>
                  <Button disabled={spaces.length === 1} onClick={() => setSpaces((all) => all.filter((_, j) => j !== i))}>
                    Remove
                  </Button>
                </div>
              ))}
              <div className={styles.spaceRow}>
                <input
                  className={styles.input}
                  placeholder="Add a space"
                  value={newSpace}
                  aria-label="New space"
                  onChange={(e) => setNewSpace(e.target.value)}
                />
                <Button
                  disabled={!newSpace.trim()}
                  onClick={() => {
                    setSpaces((all) => [...all, { name: newSpace.trim(), noClient: false }]);
                    setNewSpace("");
                  }}
                >
                  Add
                </Button>
              </div>
            </div>
            <div className={styles.nav}>
              <Button onClick={() => go("templates")}>Back</Button>
              <Button variant="primary" disabled={spaces.every((s) => !s.name.trim())} onClick={() => go("letter")}>
                Continue
              </Button>
            </div>
          </>
        )}

        {step === "letter" && (
          <>
            <h1>A letter for this computer</h1>
            <p>
              Every job gets a short code like <b className={styles.mono}>ACME-{marker || "W"}04</b>. The letter shows which
              computer started it, so two of your computers never make the same code. Use a different letter on each one.
            </p>
            <label className={styles.letterRow}>
              <span>Letter for this computer</span>
              <input
                className={`${styles.input} ${styles.letter}`}
                value={marker}
                maxLength={1}
                aria-label="Letter for this computer"
                onChange={(e) => setMarker(e.target.value.toUpperCase().replace(/[^A-Z]/g, ""))}
              />
            </label>
            {marker === "J" && <p className={styles.sub}>J is kept for jobs from your job tracker. Pick another letter.</p>}
            <div className={styles.nav}>
              <Button onClick={() => go("spaces")}>Back</Button>
              <Button variant="primary" disabled={saving || !marker || marker === "J"} onClick={finish}>
                {saving ? "Setting up…" : "Start using Nest"}
              </Button>
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
