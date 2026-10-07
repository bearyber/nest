import { Fragment, useCallback, useEffect, useRef, useState } from "react";
import FieldInput from "../components/fields/FieldInput";
import Icon from "../components/Icon";
import PreviewTree from "../components/PreviewTree";
import TemplatePicker from "../components/TemplatePicker";
import { Button, IconButton, Kbd, Segmented } from "../components/ui";
import { chooseJobsRoot, createProject, newProjectContext, planProject } from "../lib/commands";
import { hasCommandKey, isMac } from "../lib/platform";
import { defaultsFor } from "../lib/tree";
import {
  canCreate,
  errorMessage,
  type Created,
  type FieldValue,
  type NewProjectContext,
  type Plan,
  type Values,
} from "../lib/types";
import styles from "./NewProjectSheet.module.css";

const PREVIEW_DEBOUNCE_MS = 150;

interface Props {
  onClose: () => void;
  onCreated: (created: Created) => void;
}

/** New Project sheet (build spec §3.2). Every preview comes from Rust's Plan. */
export default function NewProjectSheet({ onClose, onCreated }: Props) {
  const [ctx, setCtx] = useState<NewProjectContext | null>(null);
  const [templateId, setTemplateId] = useState("");
  const [values, setValues] = useState<Values>({});
  const [space, setSpace] = useState("");
  const [codeOverride, setCodeOverride] = useState<string | null>(null);
  const [noClientTicked, setNoClientTicked] = useState(false);
  const [plan, setPlan] = useState<Plan | null>(null);
  const [touched, setTouched] = useState<Set<string>>(new Set());
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const planRequestId = useRef(0);

  const template = ctx?.templates.find((t) => t.id === templateId);

  const chooseTemplate = useCallback((id: string, context: NewProjectContext) => {
    const t = context.templates.find((x) => x.id === id);
    setTemplateId(id);
    setValues(t ? defaultsFor(t.fields) : {});
    setTouched(new Set());
    setCodeOverride(null);
  }, []);

  useEffect(() => {
    newProjectContext()
      .then((c) => {
        setCtx(c);
        setSpace(c.defaultSpace);
        const last = c.templates.find((t) => t.id === c.lastTemplate);
        const first = last ?? c.templates[0];
        if (first) chooseTemplate(first.id, c);
      })
      .catch((e) => setError(errorMessage(e)));
  }, [chooseTemplate]);

  // New jobs folder: reload the context (rescans that folder); the preview effect re-plans.
  const changeLocation = () => {
    chooseJobsRoot()
      .then((picked) => (picked ? newProjectContext() : null))
      .then((c) => {
        if (c) {
          setCtx(c);
          setError(null);
        }
      })
      .catch((e) => setError(errorMessage(e)));
  };

  const jobsRoot = ctx?.jobsRoot;
  // A no-client space (Personal) never has a client; elsewhere it's the tick box.
  const spaceHasNoClient = !!ctx?.noClientSpaces.includes(space);
  const noClient = noClientTicked || spaceHasNoClient;

  // Live preview: re-plan in Rust shortly after each change; ignore stale answers.
  useEffect(() => {
    if (!templateId || !space) return;
    const id = ++planRequestId.current;
    const timer = setTimeout(() => {
      planProject({ templateId, values, space, codeOverride, noClient })
        .then((p) => {
          if (id === planRequestId.current) setPlan(p);
        })
        .catch((e) => {
          if (id === planRequestId.current) setError(errorMessage(e));
        });
    }, PREVIEW_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [templateId, values, space, codeOverride, noClient, jobsRoot]);

  const ready = canCreate(plan) && !creating;

  const create = useCallback(() => {
    if (!ready) return;
    setCreating(true);
    setError(null);
    createProject({ templateId, values, space, codeOverride, noClient })
      .then(onCreated)
      .catch((e) => {
        setError(errorMessage(e));
        setCreating(false);
      });
  }, [ready, templateId, values, space, codeOverride, noClient, onCreated]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      } else if (e.key === "Enter" && hasCommandKey(e) && !e.repeat) {
        e.preventDefault();
        create();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, create]);

  const setValue = (key: string, v: FieldValue) => {
    setValues((prev) => ({ ...prev, [key]: v }));
    setTouched((prev) => new Set(prev).add(key));
  };
  const touch = (key: string) => setTouched((prev) => new Set(prev).add(key));
  const fieldError = (key: string) =>
    touched.has(key) ? plan?.issues.find((i) => i.field === key)?.message : undefined;
  const generalIssues = plan?.issues.filter((i) => !i.field) ?? [];
  const missing =
    plan?.issues.filter((i) => i.field && !touched.has(i.field)).map((i) => i.message) ?? [];


  const copyFolderName = () => {
    if (!plan?.folderName) return;
    navigator.clipboard?.writeText(plan.folderName).catch(() => {
      /* clipboard refused: nothing to do, the name is selectable */
    });
  };
  const codeInName = plan ? plan.folderName.includes(plan.jobCode) : true;
  const footHint =
    !ready && missing.length > 0 ? (
      missing[0]
    ) : plan?.folderName && ctx?.jobsRoot ? (
      <>
        Creates <b>{plan.folderName}</b> in {ctx.jobsRoot}
      </>
    ) : null;

  return (
    <div className={styles.backdrop} onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className={styles.sheet} role="dialog" aria-modal="true" aria-labelledby="new-project-title">
        <header className={styles.head}>
          <h2 id="new-project-title" className={styles.title}>
            New Project
          </h2>
          {ctx && ctx.templates.length > 0 && (
            <TemplatePicker
              templates={ctx.templates}
              value={templateId}
              onChange={(id) => chooseTemplate(id, ctx)}
            />
          )}
        </header>

        <div className={styles.body}>
          {ctx?.rootMissing && ctx.jobsRoot && (
            <div className={styles.banner} role="alert">
              Jobs folder not found: {ctx.jobsRoot}
            </div>
          )}

          {ctx && template && (
            <>
              <section className={styles.group} aria-labelledby="group-project">
                <h3 id="group-project" className={styles.groupTitle}>
                  Project
                </h3>
                <div className={styles.card}>
                  {template.fields.map((f) =>
                    f.type === "client" ? (
                      <Fragment key={`${templateId}:${f.key}`}>
                        {!noClient && (
                          <FieldInput
                            field={f}
                            value={values[f.key]}
                            onChange={(v) => setValue(f.key, v)}
                            onBlur={() => touch(f.key)}
                            error={fieldError(f.key)}
                            clients={ctx.clients}
                          />
                        )}
                        <div className={styles.row}>
                          <span className={styles.label}>{noClient ? "Billed to" : ""}</span>
                          <div>
                            <label className={styles.check}>
                              <input
                                type="checkbox"
                                checked={noClient}
                                disabled={spaceHasNoClient}
                                onChange={(e) => setNoClientTicked(e.target.checked)}
                              />
                              No client (personal or passion project)
                            </label>
                            {noClient && (
                              <p className={styles.noClientNote}>
                                {spaceHasNoClient
                                  ? `${space} never has a client. `
                                  : ""}
                                The job code uses your own code. Every question is optional.
                              </p>
                            )}
                          </div>
                        </div>
                      </Fragment>
                    ) : (
                      <FieldInput
                        key={`${templateId}:${f.key}`}
                        field={noClient ? { ...f, required: false } : f}
                        value={values[f.key]}
                        onChange={(v) => setValue(f.key, v)}
                        onBlur={() => touch(f.key)}
                        error={fieldError(f.key)}
                        clients={ctx.clients}
                      />
                    ),
                  )}
                </div>
              </section>

              <section className={styles.group} aria-labelledby="group-where">
                <h3 id="group-where" className={styles.groupTitle}>
                  Where it goes
                </h3>
                <div className={styles.card}>
                  <div className={styles.row}>
                    <span className={styles.label}>Location</span>
                    <div className={styles.inline}>
                      <div className={styles.path} title={ctx.jobsRoot ?? undefined}>
                        <Icon name="folder" className={styles.pathIcon} />
                        <span className={styles.pathText}>{ctx.jobsRoot ?? "No jobs folder yet"}</span>
                      </div>
                      <Button onClick={changeLocation}>Change…</Button>
                    </div>
                  </div>

                  <div className={styles.row}>
                    <span className={styles.label}>Space</span>
                    <div>
                      <Segmented label="Space" options={ctx.spaces} value={space} onChange={setSpace} />
                    </div>
                  </div>

                  <div className={styles.row}>
                    <span className={styles.label}>Folder name</span>
                    <div className={styles.inline}>
                      <span className={styles.result}>{plan?.folderName || "…"}</span>
                      <IconButton icon="copy" label="Copy folder name" onClick={copyFolderName} />
                    </div>
                  </div>

                  <div className={styles.row}>
                    <span className={styles.label}>Job code</span>
                    <div className={styles.inline}>
                      {codeOverride === null ? (
                        <>
                          <span className={styles.codePill}>{plan?.jobCode ?? "…"}</span>
                          <IconButton
                            icon="pencil"
                            label="Edit job code"
                            onClick={() => setCodeOverride(plan?.jobCode ?? "")}
                          />
                        </>
                      ) : (
                        <>
                          <input
                            aria-label="Job code"
                            className={`${styles.input} ${styles.codeInput}`}
                            value={codeOverride}
                            spellCheck={false}
                            autoFocus
                            onChange={(e) => setCodeOverride(e.target.value.toUpperCase())}
                          />
                          <Button onClick={() => setCodeOverride(null)}>Auto</Button>
                        </>
                      )}
                      {!codeInName && <span className={styles.note}>Kept in Nest, not in the folder name</span>}
                    </div>
                  </div>
                  {plan?.issues
                    .filter((i) => i.field === "jobCode")
                    .map((i) => (
                      <div key={i.message} className={styles.inlineError} role="alert">
                        {i.message}
                      </div>
                    ))}
                </div>
              </section>

              {plan && <PreviewTree plan={plan} />}
            </>
          )}

          {(generalIssues.length > 0 || error) && (
            <ul className={styles.issues}>
              {error && <li className={styles.errorText}>{error}</li>}
              {generalIssues.map((i) => (
                <li key={i.message} className={i.level === "error" ? styles.errorText : styles.warningText}>
                  {i.message}
                </li>
              ))}
            </ul>
          )}
        </div>

        <footer className={styles.foot}>
          <span className={styles.hint}>{footHint}</span>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" disabled={!ready} onClick={create}>
            {creating ? (
              "Creating…"
            ) : (
              <>
                Create <Kbd>{isMac() ? "⌘↵" : "Ctrl ↵"}</Kbd>
              </>
            )}
          </Button>
        </footer>
      </div>
    </div>
  );
}
