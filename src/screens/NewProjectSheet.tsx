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
  setTemplateKitFolder,
} from "../lib/commands";
import { isMac } from "../lib/platform";
import { isTopDialog } from "../lib/dialogStack";
import { isShortcut } from "../lib/shortcuts";
import { formatBytes } from "../lib/search";
import { alsoMakeFor } from "../lib/alsoMake";
import { useDialogFocus } from "../lib/useDialogFocus";
import { plannedLine } from "../lib/resolve";
import { defaultsFor, todayIso } from "../lib/tree";
import {
  canCreate,
  errorMessage,
  type ClientValue,
  type Field,
  type Created,
  type CreateProgress,
  type FieldValue,
  type NewProjectContext,
  type Plan,
  type PlanKit,
  type TreeEntry,
  type Values,
} from "../lib/types";
import styles from "./NewProjectSheet.module.css";

const PREVIEW_DEBOUNCE_MS = 150;
/** "Where it goes" open or closed, remembered while Nest runs (this window's session). */
const WHERE_OPEN_KEY = "nest.newProject.whereOpen";

function readWhereOpen(): boolean {
  try {
    return sessionStorage.getItem(WHERE_OPEN_KEY) === "1";
  } catch {
    return false; // storage blocked: start closed
  }
}

function saveWhereOpen(open: boolean) {
  try {
    sessionStorage.setItem(WHERE_OPEN_KEY, open ? "1" : "0");
  } catch {
    /* storage blocked: it just isn't remembered */
  }
}

/** Client files inside Google Drive: online-only files are downloaded while Create copies them. */
const inGoogleDrive = (path: string) => /Google Drive|GoogleDrive-|My Drive/.test(path);

/** The template's folders that are always made (no "only if", no "one per"), top-level and
 *  nested, parents first: where "Put them in" can put a client's files. Same rule as Rust's
 *  `with_kit_folder`. */
export function clientFilePlaces(tree: TreeEntry[] = []): string[] {
  const out: string[] = [];
  for (const e of tree) {
    if (typeof e !== "string" && (e.when || e.each)) continue;
    const parts = (typeof e === "string" ? e : e.name).split("/").filter(Boolean);
    parts.forEach((_, i) => {
      const path = parts.slice(0, i + 1).join("/");
      if (!out.includes(path)) out.push(path);
    });
  }
  return out;
}

interface ClientFilesProps {
  kit: PlanKit;
  places: string[];
  saving: boolean;
  onChoose: () => void;
  onPut: (folder: string) => void;
}

/** Under the Client field: the client's files that come along, or how to add them. */
function ClientFilesLine({ kit, places, saving, onChoose, onPut }: ClientFilesProps) {
  const whose = `${kit.client}'s files`;
  const link = (label: string) => (
    <button type="button" className={styles.link} onClick={onChoose}>
      {label}
    </button>
  );
  if (kit.state === "ready") {
    const files = `${kit.count} file${kit.count === 1 ? "" : "s"}`;
    return (
      <p className={styles.kitLine} title={kit.source}>
        <span>
          {whose}: {files} · {formatBytes(kit.bytes)} · {link("Change")}
        </span>
        {kit.source && inGoogleDrive(kit.source) && (
          <span className={styles.kitNote}>Online-only files will be downloaded while copying.</span>
        )}
      </p>
    );
  }
  if (kit.state === "missing") {
    return (
      <p className={`${styles.kitLine} ${styles.muted}`}>
        <span>
          {whose}: none · {link("Add…")}
        </span>
      </p>
    );
  }
  if (kit.state === "noPlace") {
    if (places.length === 0) {
      return <p className={`${styles.kitLine} ${styles.muted}`}>{whose}: this template has no folders to put them in</p>;
    }
    return (
      <p className={styles.kitLine} title={kit.source}>
        <label className={styles.putIn}>
          {whose} · Put them in:
          <select
            className={styles.select}
            value=""
            disabled={saving}
            onChange={(e) => e.target.value && onPut(e.target.value)}
          >
            <option value="">{saving ? "Saving…" : "Choose a folder"}</option>
            {places.map((p) => (
              <option key={p} value={p}>
                {p}
              </option>
            ))}
          </select>
        </label>
      </p>
    );
  }
  return (
    <p className={styles.kitLine} title={kit.source}>
      <span>
        Couldn't read {whose} · {link("Choose a folder…")}
      </span>
    </p>
  );
}

/** "D:\Jobs\260901_X" → "D:\Jobs" (either separator). */
export function parentOf(path: string): string {
  const cut = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return cut > 0 ? path.slice(0, cut) : path;
}

/** A long folder path, shortened for one line: "D:\…\Clients\Jobs" (the full path is the
 *  tooltip). Keeps the drive (or the leading /) and the last two folders. */
export function shortPath(path: string): string {
  if (path.length <= 32) return path;
  const sep = path.includes("\\") ? "\\" : "/";
  const parts = path.split(/[\\/]/);
  const tail = parts.filter(Boolean).slice(-2);
  if (parts.filter(Boolean).length <= 3) return path;
  const head = parts[0]; // "D:" on Windows, "" for a path starting with /
  return [head, "…", ...tail].join(sep);
}

/** "Try an example" (first steps): the client and name it fills in. */
export const EXAMPLE_CLIENT = { name: "Acme", code: "ACME" };
export const EXAMPLE_NAME = "Summer Film";

/** The example's answers for a template: its defaults, plus client Acme, the project name
 *  "Summer Film" and today's date. Nothing is saved: it's only what the form starts with. */
export function exampleValues(fields: Field[], clients: ClientValue[] = [], today: string = todayIso()): Values {
  const values = defaultsFor(fields);
  // Acme already used in the jobs folder: keep its code.
  const known = clients.find((c) => c.name.toLowerCase() === EXAMPLE_CLIENT.name.toLowerCase());
  const texts = fields.filter((f) => f.type === "text");
  const nameField =
    texts.find((f) => ["name", "title", "project"].includes(f.key.toLowerCase())) ??
    texts.find((f) => f.required) ??
    texts[0];
  for (const f of fields) {
    if (f.type === "client") values[f.key] = { name: EXAMPLE_CLIENT.name, code: known?.code ?? EXAMPLE_CLIENT.code };
    else if (f.type === "date") values[f.key] = today;
  }
  if (nameField) values[nameField.key] = EXAMPLE_NAME;
  return values;
}

interface Props {
  /** Start with this template (e.g. "New project from this" in Templates). */
  initialTemplate?: string;
  /** "Try an example": the first template, filled in with Acme / Summer Film / today. Nothing
   *  is made until Create, like any other project. */
  example?: boolean;
  onClose: () => void;
  /** `resolveName`: the Resolve project Create queued (null = none). */
  onCreated: (created: Created, resolveName: string | null) => void;
}

/** New Project sheet (build spec §3.2). Every preview comes from Rust's Plan. */
export default function NewProjectSheet({ initialTemplate, example = false, onClose, onCreated }: Props) {
  const sheetRef = useRef<HTMLDivElement>(null);
  // Focus goes to the first field (once the template has loaded), stays in the sheet, and goes back.
  useDialogFocus(sheetRef);
  const [ctx, setCtx] = useState<NewProjectContext | null>(null);
  const [templateId, setTemplateId] = useState("");
  const [values, setValues] = useState<Values>({});
  const [space, setSpace] = useState("");
  const [codeOverride, setCodeOverride] = useState<string | null>(null);
  const [noClientTicked, setNoClientTicked] = useState(false);
  /** "Also make: Resolve project"; null = the template's default (`resolve.on`). */
  const [resolveOn, setResolveOn] = useState<boolean | null>(null);
  const [plan, setPlan] = useState<Plan | null>(null);
  const [touched, setTouched] = useState<Set<string>>(new Set());
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useState<CreateProgress | null>(null);
  /** Bumped to plan again without a change in the form (e.g. a kit was chosen). */
  const [replan, setReplan] = useState(0);
  /** "Where it goes" shows its controls (Change…), or just the one summary line. */
  const [whereOpen, setWhereOpen] = useState(readWhereOpen);
  /** "Put them in" is saving the template. */
  const [savingPlace, setSavingPlace] = useState(false);
  const planRequestId = useRef(0);

  const template = ctx?.templates.find((t) => t.id === templateId);

  const chooseTemplate = useCallback((id: string, context: NewProjectContext) => {
    const t = context.templates.find((x) => x.id === id);
    setTemplateId(id);
    setValues(t ? defaultsFor(t.fields) : {});
    setTouched(new Set());
    setCodeOverride(null);
    setResolveOn(null);
  }, []);

  useEffect(() => {
    newProjectContext()
      .then((c) => {
        setCtx(c);
        setSpace(c.defaultSpace);
        const asked = c.templates.find((t) => t.id === initialTemplate);
        const last = c.templates.find((t) => t.id === c.lastTemplate);
        const first = example ? c.templates[0] : (asked ?? last ?? c.templates[0]);
        if (first) chooseTemplate(first.id, c);
        if (first && example) setValues(exampleValues(first.fields, c.clients));
      })
      .catch((e) => setError(errorMessage(e)));
  }, [chooseTemplate, initialTemplate, example]);

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
      planProject({ templateId, values, space, codeOverride, noClient, resolve: resolveOn })
        .then((p) => {
          if (id === planRequestId.current) setPlan(p);
        })
        .catch((e) => {
          if (id === planRequestId.current) setError(errorMessage(e));
        });
    }, PREVIEW_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [templateId, values, space, codeOverride, noClient, resolveOn, jobsRoot, replan]);

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
    const resolveName = plan?.resolve?.name ?? null;
    createProject({ templateId, values, space, codeOverride, noClient, kitFingerprint, resolve: resolveOn })
      .then((c) => onCreated(c, resolveName))
      .catch((e) => {
        setError(errorMessage(e));
        setCreating(false);
        // E.g. the kit changed: show what's there now.
        setReplan((n) => n + 1);
      });
  }, [ready, templateId, values, space, codeOverride, noClient, kitFingerprint, resolveOn, plan, onCreated]);

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

  // "Put them in": the template keeps this folder for client files (saved in Rust as its next
  // version); then plan again with the new template.
  const putClientFiles = (folder: string) => {
    if (!templateId) return;
    setSavingPlace(true);
    setError(null);
    setTemplateKitFolder(templateId, folder)
      .then(() => newProjectContext())
      .then((c) => {
        setCtx(c);
        setReplan((n) => n + 1);
      })
      .catch((e) => setError(errorMessage(e)))
      .finally(() => setSavingPlace(false));
  };

  const toggleWhere = () => {
    saveWhereOpen(!whereOpen);
    setWhereOpen(!whereOpen);
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // Only when it's the top dialog (a dialog over it keeps its own keys).
      if (!isTopDialog(sheetRef.current)) return;
      if (e.key === "Escape") {
        // A picker or list inside that closed itself already handled it.
        if (e.defaultPrevented) return;
        e.preventDefault();
        cancel();
      } else if (isShortcut(e, "create") && !e.repeat) {
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
  // Problems about where it goes show without Change…; an error opens the controls.
  const codeIssues = plan?.issues.filter((i) => i.field === "jobCode") ?? [];
  const whereError = !jobsRoot || !!rootMissing || codeIssues.some((i) => i.level === "error");
  const showWhere = whereOpen || whereError;
  const kitPlaces = clientFilePlaces(template?.tree);
  const alsoMake = template ? alsoMakeFor(template.fields, template.tree) : [];
  const alsoMakeKeys = new Set(alsoMake.map((a) => a.field.key));
  const starter = template?.resolve;

  const footHint = copying ? (
    `Copying ${plan?.kit?.client ? `${plan.kit.client}'s files` : "files"}… ${percent}%. Cancel undoes everything.`
  ) : !ready && missing.length > 0 ? (
      missing[0]
    ) : plan?.folderName && jobsRoot ? (
      <>
        Creates <b>{plan.folderName}</b> in {jobsRoot}
      </>
    ) : null;

  return (
    <div className={styles.backdrop} onMouseDown={(e) => e.target === e.currentTarget && !creating && onClose()}>
      <div
        ref={sheetRef}
        tabIndex={-1}
        className={styles.sheet}
        role="dialog"
        aria-modal="true"
        aria-labelledby="new-project-title"
      >
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
          {example && (
            <p className={styles.exampleNote}>
              An example to try. Change anything you like. Nothing is made until you press Create.
            </p>
          )}
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
                  {template.fields
                    .filter((f) => !alsoMakeKeys.has(f.key))
                    .map((f) =>
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
                        {!noClient && plan?.kit && (
                          <div className={`${styles.row} ${styles.under}`}>
                            <span />
                            <ClientFilesLine
                              kit={plan.kit}
                              places={kitPlaces}
                              saving={savingPlace}
                              onChoose={chooseKit}
                              onPut={putClientFiles}
                            />
                          </div>
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

              {(alsoMake.length > 0 || starter) && (
                <section className={styles.group} aria-labelledby="group-also-make">
                  <h3 id="group-also-make" className={styles.groupTitle}>
                    Also make:
                  </h3>
                  <div className={styles.alsoMake}>
                    {alsoMake.map((a) => (
                      <label key={`${templateId}:${a.field.key}`} className={styles.check} title={a.paths.join("\n")}>
                        <input
                          type="checkbox"
                          checked={values[a.field.key] === true}
                          onChange={(e) => setValue(a.field.key, e.target.checked)}
                        />
                        {a.label}
                      </label>
                    ))}
                    {starter && (
                      <label
                        key={`${templateId}:resolve`}
                        className={styles.check}
                        title="Adds a copy of the template's Resolve project to DaVinci Resolve"
                      >
                        <input
                          type="checkbox"
                          checked={resolveOn ?? starter.on}
                          onChange={(e) => setResolveOn(e.target.checked)}
                        />
                        Resolve project
                      </label>
                    )}
                  </div>
                </section>
              )}

              <section className={styles.group} aria-label="Where it goes">
                <div className={styles.summary}>
                  <span className={styles.summaryText}>
                    Goes in{" "}
                    <span className={styles.summaryPath} title={jobsRoot ?? undefined}>
                      {jobsRoot ? shortPath(jobsRoot) : "no jobs folder yet"}
                    </span>
                    {" · "}
                    {space}
                    {" · "}
                    <span className={styles.summaryCode}>{plan?.jobCode ?? "…"}</span>
                  </span>
                  {!whereError && (
                    <button type="button" className={styles.link} aria-expanded={showWhere} onClick={toggleWhere}>
                      {showWhere ? "Done" : "Change…"}
                    </button>
                  )}
                </div>
                {codeIssues.map((i) => (
                  <div key={i.message} className={styles.whereIssue} role="alert">
                    {i.message}
                  </div>
                ))}
                {showWhere && (
                  <div className={`${styles.card} nest-unfold`}>
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
                  </div>
                )}
              </section>

              {plan && <PreviewTree plan={plan} />}
              {plan?.resolve && (
                <p className={styles.resolveLine} title={`Library: ${plan.resolve.libraryLabel}`}>
                  {plannedLine(plan.resolve)}
                </p>
              )}
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
