import { Fragment, useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import FieldInput from "../components/fields/FieldInput";
import Icon from "../components/Icon";
import PreviewTree from "../components/PreviewTree";
import TemplatePicker from "../components/TemplatePicker";
import { Button, IconButton, Kbd, Segmented } from "../components/ui";
import {
  cancelCreate,
  chooseClientKit,
  chooseJobsRoot,
  choosePersonalRoot,
  createProject,
  newProjectContext,
  planProject,
} from "../lib/commands";
import { hasCommandKey, isMac } from "../lib/platform";
import { formatBytes } from "../lib/search";
import { defaultsFor } from "../lib/tree";
import {
  canCreate,
  errorMessage,
  type Created,
  type CreateProgress,
  type FieldValue,
  type NewProjectContext,
  type Plan,
  type PlanKit,
  type Values,
} from "../lib/types";
import styles from "./NewProjectSheet.module.css";

const PREVIEW_DEBOUNCE_MS = 150;

/** A kit inside Google Drive: its online-only files are downloaded while Create copies them. */
const inGoogleDrive = (path: string) => /Google Drive|GoogleDrive-|My Drive/.test(path);

/** Under the preview: what the client kit adds, or why there isn't one. */
function KitLine({ kit, onChoose }: { kit: PlanKit; onChoose: () => void }) {
  if (kit.state === "noPlace") {
    return (
      <p className={styles.kitLine}>
        This template has no place for a client kit (right-click a folder in the template editor)
      </p>
    );
  }
  if (kit.state !== "ready") {
    return (
      <p className={styles.kitLine}>
        {kit.state === "missing" ? `No kit for ${kit.client}` : `Couldn't read ${kit.client}'s kit`} ·{" "}
        <Button onClick={onChoose}>Choose a folder…</Button>
      </p>
    );
  }
  const files = `${kit.count} file${kit.count === 1 ? "" : "s"}`;
  return (
    <p className={styles.kitLine} title={kit.source}>
      Client kit: {kit.client} · {files} · {formatBytes(kit.bytes)}
      {kit.folder && <span className={styles.note}> into {kit.folder}</span>}
      {kit.source && inGoogleDrive(kit.source) && (
        <span className={styles.kitNote}>Online-only files will be downloaded while copying.</span>
      )}
    </p>
  );
}

/** "D:\Jobs\260901_X" → "D:\Jobs" (either separator). */
function parentOf(path: string): string {
  const cut = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return cut > 0 ? path.slice(0, cut) : path;
}

interface Props {
  /** Start with this template (e.g. "New project from this" in Templates). */
  initialTemplate?: string;
  onClose: () => void;
  onCreated: (created: Created) => void;
}

/** New Project sheet (build spec §3.2). Every preview comes from Rust's Plan. */
export default function NewProjectSheet({ initialTemplate, onClose, onCreated }: Props) {
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
  const [progress, setProgress] = useState<CreateProgress | null>(null);
  /** Bumped to plan again without a change in the form (e.g. a kit was chosen). */
  const [replan, setReplan] = useState(0);
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
        const asked = c.templates.find((t) => t.id === initialTemplate);
        const last = c.templates.find((t) => t.id === c.lastTemplate);
        const first = asked ?? last ?? c.templates[0];
        if (first) chooseTemplate(first.id, c);
      })
      .catch((e) => setError(errorMessage(e)));
  }, [chooseTemplate, initialTemplate]);

  // Settings changed while the sheet is open (e.g. a new Personal folder or a client's kit):
  // reload what it shows, and plan again.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen("settings-changed", () => {
      newProjectContext()
        .then((c) => {
          setCtx(c);
          setReplan((n) => n + 1);
        })
        .catch((e) => setError(errorMessage(e)));
    })
      .then((u) => (unlisten = u))
      .catch(() => {});
    return () => unlisten?.();
  }, []);

  // New jobs folder (or Personal folder, when that's the one shown): reload the context
  // (rescans that folder); the preview effect re-plans.
  const changeLocation = () => {
    (personalShown ? choosePersonalRoot() : chooseJobsRoot())
      .then((picked) => (picked ? newProjectContext() : null))
      .then((c) => {
        if (c) {
          setCtx(c);
          setError(null);
        }
      })
      .catch((e) => setError(errorMessage(e)));
  };

  // A no-client space (Personal) never has a client; elsewhere it's the tick box.
  const spaceHasNoClient = !!ctx?.noClientSpaces.includes(space);
  const noClient = noClientTicked || spaceHasNoClient;
  // Personal and "No client" projects go to the Personal folder, if one is chosen (Settings).
  // The folder shown is the one the Plan uses, so it always matches where Create puts it.
  const personalShown = noClient && !!ctx?.personalRoot;
  const planFolder = plan?.folderName ? parentOf(plan.root) : null;
  const jobsRoot = planFolder ?? (personalShown ? ctx?.personalRoot : ctx?.jobsRoot);
  const rootMissing = personalShown ? ctx?.personalMissing : ctx?.rootMissing;

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
  }, [templateId, values, space, codeOverride, noClient, jobsRoot, replan]);

  const ready = canCreate(plan) && !creating;
  // Create copies the kit the preview showed, or refuses if it changed meanwhile.
  const kitFingerprint = plan?.kit?.state === "ready" ? (plan.kit.fingerprint ?? null) : null;

  // While Create copies a client kit, Rust says how far it got.
  useEffect(() => {
    if (!creating) return;
    let unlisten: (() => void) | undefined;
    let live = true;
    listen<CreateProgress>("create-progress", (e) => live && setProgress(e.payload))
      .then((u) => (live ? (unlisten = u) : u()))
      .catch(() => {});
    return () => {
      live = false;
      unlisten?.();
      setProgress(null);
    };
  }, [creating]);

  const create = useCallback(() => {
    if (!ready) return;
    setCreating(true);
    setError(null);
    createProject({ templateId, values, space, codeOverride, noClient, kitFingerprint })
      .then(onCreated)
      .catch((e) => {
        setError(errorMessage(e));
        setCreating(false);
        // E.g. the kit changed: show what's there now.
        setReplan((n) => n + 1);
      });
  }, [ready, templateId, values, space, codeOverride, noClient, kitFingerprint, onCreated]);

  // Cancel while creating stops the copy (Rust undoes what it made); otherwise it closes.
  const cancel = useCallback(() => {
    if (creating) void cancelCreate().catch(() => {});
    else onClose();
  }, [creating, onClose]);

  const chooseKit = () => {
    const kit = plan?.kit;
    if (!kit) return;
    chooseClientKit(kit.code, kit.client)
      .then((v) => v && setReplan((n) => n + 1))
      .catch((e) => setError(errorMessage(e)));
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        cancel();
      } else if (e.key === "Enter" && hasCommandKey(e) && !e.repeat) {
        e.preventDefault();
        create();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [cancel, create]);

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
  const copying = creating && progress && progress.total > 0;
  const percent = copying ? Math.round((progress.done / progress.total) * 100) : 0;
  const footHint = copying ? (
    `Copying ${plan?.kit?.client ? `${plan.kit.client}'s kit` : "files"}… ${percent}%. Cancel undoes everything.`
  ) : !ready && missing.length > 0 ? (
      missing[0]
    ) : plan?.folderName && jobsRoot ? (
      <>
        Creates <b>{plan.folderName}</b> in {jobsRoot}
      </>
    ) : null;

  return (
    <div className={styles.backdrop} onMouseDown={(e) => e.target === e.currentTarget && !creating && onClose()}>
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
          {rootMissing && jobsRoot && (
            <div className={styles.banner} role="alert">
              {personalShown ? "Personal folder" : "Jobs folder"} not found: {jobsRoot}
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
                          <span className={styles.label}>{noClient ? "Client" : ""}</span>
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
                      <div className={styles.path} title={jobsRoot ?? undefined}>
                        <Icon name="folder" className={styles.pathIcon} />
                        <span className={styles.pathText}>{jobsRoot ?? "No jobs folder yet"}</span>
                      </div>
                      {personalShown && <span className={styles.note}>Your Personal folder</span>}
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
              {plan?.kit && <KitLine kit={plan.kit} onChoose={chooseKit} />}
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
          <Button onClick={cancel}>Cancel</Button>
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
