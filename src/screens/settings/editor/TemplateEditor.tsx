import { useCallback, useEffect, useRef, useState } from "react";
import Icon from "../../../components/Icon";
import PreviewTree from "../../../components/PreviewTree";
import { Button } from "../../../components/ui";
import { previewTemplate, saveTemplate } from "../../../lib/commands";
import { asPlan, fromNodes, toNodes, type FolderNode } from "../../../lib/templateDraft";
import { errorMessage, type SaveMode, type Saved, type Template, type TemplatePreview } from "../../../lib/types";
import FoldersStep from "./FoldersStep";
import FormPreview from "./FormPreview";
import NameStep from "./NameStep";
import QuestionsStep from "./QuestionsStep";
import styles from "./editor.module.css";

export type EditorStep = "name" | "questions" | "folders";

// Questions first: the folder name is built from the answers.
const STEPS: { id: EditorStep; label: string }[] = [
  { id: "questions", label: "Questions" },
  { id: "name", label: "Folder name" },
  { id: "folders", label: "Folders" },
];

interface Props {
  /** The template the editor started from (Rust copies its starter files). */
  sourceKey: string;
  mode: SaveMode;
  start: Template;
  starterFiles: string[];
  step: EditorStep;
  onClose: () => void;
  onSaved: (saved: Saved) => void;
}

/** The template editor (spec §12a #19): a sheet over the Settings window. Name at the top, then
 *  3 steps: Questions, Folder name, Folders. Rust
 *  previews every change with the real planner and does the saving. */
export default function TemplateEditor({ sourceKey, mode, start, starterFiles, step: firstStep, onClose, onSaved }: Props) {
  const [draft, setDraft] = useState<Template>(start);
  const [nodes, setNodes] = useState<FolderNode[]>(() => toNodes(start.tree));
  const [step, setStep] = useState<EditorStep>(firstStep);
  const [preview, setPreview] = useState<TemplatePreview | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmClose, setConfirmClose] = useState(false);
  const dirty = useRef(false);

  const labelOf = useCallback(
    (key: string) => draft.fields.find((f) => f.key === key)?.label ?? key,
    [draft.fields],
  );
  const treeResult = fromNodes(nodes, labelOf);
  const full: Template = { ...draft, tree: treeResult.tree };

  const change = (next: Template) => {
    dirty.current = true;
    setDraft(next);
  };
  const changeNodes = (next: FolderNode[], files?: Template["files"]) => {
    dirty.current = true;
    setNodes(next);
    if (files) setDraft((d) => ({ ...d, files }));
  };

  // Ask Rust what this draft would make (debounced). Only the reply to the latest draft counts,
  // and Save waits until it has arrived.
  const fullJson = JSON.stringify(full);
  const [previewFor, setPreviewFor] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    const t = setTimeout(() => {
      previewTemplate(sourceKey, JSON.parse(fullJson) as Template)
        .then((p) => {
          if (!live) return;
          setPreview(p);
          setPreviewFor(fullJson);
          setError(null);
        })
        .catch((e) => live && setError(errorMessage(e)));
    }, 200);
    return () => {
      live = false;
      clearTimeout(t);
    };
  }, [fullJson, sourceKey]);
  const fresh = previewFor === fullJson;
  const stepIndex = STEPS.findIndex((s) => s.id === step);

  const problems = [...treeResult.problems, ...(preview?.problems ?? [])];

  const close = () => {
    if (dirty.current && !confirmClose) setConfirmClose(true);
    else onClose();
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !(e.target as HTMLElement).closest("input, textarea, select")) close();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const save = () => {
    setSaving(true);
    setError(null);
    saveTemplate(sourceKey, full, mode)
      .then(onSaved)
      .catch((e) => {
        setError(errorMessage(e));
        setSaving(false);
      });
  };

  const title =
    mode === "customize" ? `Customize “${start.name}”` : mode === "edit" ? `Edit “${start.name}”` : "New template";

  return (
    <div className={styles.scrim}>
      <div className={styles.sheet} role="dialog" aria-modal="true" aria-label={title}>
        <header className={styles.head}>
          <h3 className={styles.title}>{title}</h3>
          <nav className={styles.steps} aria-label="Steps">
            {STEPS.map((s) => (
              <button
                key={s.id}
                type="button"
                className={styles.step}
                aria-current={step === s.id ? "step" : undefined}
                onClick={() => setStep(s.id)}
              >
                <span className={styles.stepNum}>{STEPS.indexOf(s) + 1}</span>
                {s.label}
              </button>
            ))}
          </nav>
          <div className={styles.basics}>
            <label>
              Template name
              <input
                className={styles.input}
                value={draft.name}
                maxLength={60}
                onChange={(e) => change({ ...draft, name: e.target.value })}
              />
            </label>
            <label>
              Short description
              <input
                className={styles.input}
                value={draft.description}
                maxLength={120}
                placeholder="Shown under the name in New Project"
                onChange={(e) => change({ ...draft, description: e.target.value })}
              />
            </label>
          </div>
        </header>

        <div className={styles.exampleBar} aria-live="polite">
          <span className={styles.exampleLabel}>Example</span>
          <Icon name="folder" className={styles.folderIcon} />
          <b className={`${styles.mono} ${styles.exampleName}`}>
            {preview && preview.problems.length === 0 ? preview.folderName : "…"}
          </b>
          {preview?.jobCode && preview.problems.length === 0 && (
            <span className={styles.exampleCode}>
              Job code <b className={styles.mono}>{preview.jobCode}</b>
            </span>
          )}
          <span className={styles.exampleNote}>Client Acme · project “Summer Campaign” · started today</span>
        </div>

        <div className={step === "folders" ? styles.bodyWide : styles.body}>
          <div className={styles.main} key={step}>
            {step === "name" && (
              <NameStep
                draft={draft}
                folderName={preview && preview.problems.length === 0 ? preview.folderName : ""}
                onChange={change}
              />
            )}
            {step === "questions" && <QuestionsStep draft={full} onChange={(t) => {
              // Removing a question can change folders too.
              if (JSON.stringify(t.tree) !== JSON.stringify(full.tree)) setNodes(toNodes(t.tree));
              change(t);
            }} />}
            {step === "folders" && (
              <FoldersStep
                draft={draft}
                nodes={nodes}
                starterFiles={starterFiles}
                folderName={preview?.folderName ?? ""}
                onChange={changeNodes}
                onFieldsChange={change}
              />
            )}
          </div>
          {step !== "folders" && (
            <aside className={styles.side}>
              {step === "name" ? (
                preview && preview.problems.length === 0 && <PreviewTree plan={asPlan(preview)} tall />
              ) : (
                <>
                  <h4 className={styles.sideTitle}>New Project will look like this</h4>
                  <p className={styles.hint}>Updates as you edit. Try it: this is what you'll fill in.</p>
                  <FormPreview fields={draft.fields} />
                </>
              )}
            </aside>
          )}
        </div>

        <footer className={styles.foot}>
          <div className={styles.footMsg} role={problems.length || error ? "alert" : undefined}>
            {confirmClose ? (
              <span>Close without saving? Your changes will be lost.</span>
            ) : error ? (
              <span className={styles.problem}>{error}</span>
            ) : problems.length ? (
              <span className={styles.problem}>
                {problems.length === 1 ? problems[0] : `${problems[0]} (and ${problems.length - 1} more)`}
              </span>
            ) : (
              <span className={styles.hint}>Changes apply to new projects only. Projects you already made are never touched.</span>
            )}
          </div>
          {confirmClose ? (
            <>
              <Button onClick={() => setConfirmClose(false)}>Keep editing</Button>
              <Button onClick={onClose}>Close without saving</Button>
            </>
          ) : (
            <>
              <Button onClick={close}>Cancel</Button>
              {stepIndex > 0 && (
                <Button onClick={() => setStep(STEPS[stepIndex - 1].id)}>← {STEPS[stepIndex - 1].label}</Button>
              )}
              {stepIndex < STEPS.length - 1 && (
                <Button onClick={() => setStep(STEPS[stepIndex + 1].id)}>{STEPS[stepIndex + 1].label} →</Button>
              )}
              <Button variant="primary" disabled={saving || !fresh || problems.length > 0} onClick={save}>
                {saving ? "Saving…" : "Save template"}
              </Button>
            </>
          )}
        </footer>
      </div>
    </div>
  );
}
