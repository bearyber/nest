import { useEffect, useMemo, useRef, useState } from "react";
import { Button } from "../../components/ui";
import { resolveExportToEditor, resolveLibraries, resolveProjects, resolveStatus } from "../../lib/commands";
import { libraryKey, sameLibrary } from "../../lib/resolve";
import {
  errorMessage,
  type Field,
  type ResolveLibraries,
  type ResolveLibrary,
  type ResolveProject,
  type ResolveStarter,
  type ResolveStatus,
} from "../../lib/types";
import NameField, { NameView } from "./NameField";
import styles from "./editor.module.css";

/** What to do when Resolve can't answer, in plain words. */
export const STATUS_LINE: Record<Exclude<ResolveStatus, "ready">, string> = {
  notRunning: "Open DaVinci Resolve first.",
  scriptingOff:
    "In Resolve: Preferences → System → General → External scripting: Local, then restart Resolve. If Resolve has just opened, wait a few seconds.",
  notInstalled: "DaVinci Resolve isn't installed on this computer. You can still drop a .drp file.",
  starting: "Resolve is starting. Try again in a few seconds.",
  noAnswer: "Resolve didn't answer. If a window is open in Resolve (Preferences, a render), close it and try again.",
  noHelper: "Nest couldn't talk to Resolve's script runner. You can still drop a .drp file.",
};

/** `files/2/Grade.drp` → `Grade`. */
export const drpName = (drp: string) => (drp.split("/").pop() ?? drp).replace(/\.drp$/i, "");

/** A `.drp` file (dropped from Finder / Explorer). */
export const isDrp = (path: string) => /\.drp$/i.test(path);

/** Where a new Resolve project goes in Resolve: the client's folder, or the top level when New
 *  Project doesn't ask for a client (Rust's own default would name an answer that isn't there). */
export const defaultFolder = (fields: Field[]) => {
  const client = fields.find((f) => f.type === "client");
  return client ? `{${client.key}}` : "";
};

interface Props {
  sessionId: string;
  value: ResolveStarter | undefined;
  fields: Field[];
  /** The project folder's name pattern (what the Resolve name is when left empty). */
  folderName: string;
  /** One undoable change; `undefined` = no Resolve project. */
  onChange: (next: ResolveStarter | undefined) => void;
  onRemove: () => void;
  onNewBlank: (label: string) => string;
  onChip: (key: string) => void;
  /** A drop is being added (from the editor). */
  adding?: string | null;
  /** The pointer is over the editor with a .drp. */
  dropping?: boolean;
}

/** "Choose from Resolve…" list: the projects of one library (`from`; null = the open one). */
type Pick = {
  kind: "pick";
  projects: ResolveProject[];
  libraries: ResolveLibraries;
  from: ResolveLibrary | null;
  /** Looking inside another library (Resolve switches and back: may take a moment). */
  loading: boolean;
  /** Why that library's projects can't be shown (e.g. close your project first). */
  note: string | null;
};

type Step =
  | { kind: "idle" }
  | { kind: "asking" }
  | { kind: "line"; text: string }
  | Pick
  | { kind: "exporting"; name: string };

/** A row in the "Choose from Resolve…" list: a folder (heading) or a project. */
type PickRow = { kind: "folder"; name: string; depth: number; key: string } | { kind: "project"; project: ResolveProject; depth: number; key: string };

/** Projects under their folders, folders after the projects at their level, as Resolve shows them. */
export function pickRows(projects: ResolveProject[], filter: string): PickRow[] {
  const f = filter.trim().toLowerCase();
  const shown = projects.filter((p) => !f || p.name.toLowerCase().includes(f));
  const sorted = [...shown].sort((a, b) => {
    const fa = a.folder.split("/").filter(Boolean);
    const fb = b.folder.split("/").filter(Boolean);
    for (let i = 0; i < Math.min(fa.length, fb.length); i++) {
      const c = fa[i].localeCompare(fb[i]);
      if (c) return c;
    }
    if (fa.length !== fb.length) return fa.length - fb.length;
    return a.name.localeCompare(b.name);
  });
  const rows: PickRow[] = [];
  let open: string[] = [];
  for (const p of sorted) {
    const parts = p.folder.split("/").filter(Boolean);
    let same = 0;
    while (same < parts.length && same < open.length && parts[same] === open[same]) same++;
    for (let i = same; i < parts.length; i++)
      rows.push({ kind: "folder", name: parts[i], depth: i, key: `folder:${parts.slice(0, i + 1).join("/")}` });
    open = parts;
    rows.push({ kind: "project", project: p, depth: parts.length, key: `project:${p.folder}/${p.name}` });
  }
  return rows;
}

/** The libraries to offer: what Resolve listed, plus `saved` if Resolve didn't list it (closed,
 *  or not on this computer), so a choice is never lost. */
export function libraryChoices(listed: ResolveLibrary[], saved: ResolveLibrary | null | undefined): ResolveLibrary[] {
  return saved && !listed.some((l) => sameLibrary(l, saved)) ? [...listed, saved] : listed;
}

/** A pattern (Name in Resolve, the folder): shown with chips; click to change it. */
function PatternValue({
  label,
  value,
  empty,
  fields,
  allowSlash,
  placeholder,
  onCommit,
  onNewBlank,
  onChip,
}: {
  label: string;
  value: string;
  /** What shows when the pattern is empty. */
  empty: React.ReactNode;
  fields: Field[];
  allowSlash?: boolean;
  placeholder?: string;
  onCommit: (v: string) => void;
  onNewBlank: (label: string) => string;
  onChip: (key: string) => void;
}) {
  const [editing, setEditing] = useState(false);
  return editing ? (
    <NameField
      value={value}
      fields={fields}
      label={label}
      allowSlash={allowSlash}
      placeholder={placeholder}
      onCommit={(v) => {
        setEditing(false);
        if (v !== value) onCommit(v);
      }}
      onCancel={() => setEditing(false)}
      onNewBlank={onNewBlank}
      onChip={(key) => onChip(key)}
    />
  ) : (
    // Not a <button>: the name's chips are buttons themselves.
    <div
      role="button"
      tabIndex={0}
      className={styles.resolveValue}
      aria-label={label}
      onClick={() => setEditing(true)}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          setEditing(true);
        }
      }}
    >
      {value ? <NameView name={value} fields={fields} onChip={() => setEditing(true)} /> : empty}
    </div>
  );
}

/** "Resolve project" under the files (v0.8): a DaVinci Resolve starter project; Create adds a
 *  copy of it to Resolve for each new job, into the library and folder set here. Chosen from
 *  Resolve (any of its libraries), or a dropped .drp. */
export default function ResolveSection({
  sessionId,
  value,
  fields,
  folderName,
  onChange,
  onRemove,
  onNewBlank,
  onChip,
  adding,
  dropping,
}: Props) {
  const [step, setStep] = useState<Step>({ kind: "idle" });
  const [filter, setFilter] = useState("");
  const [active, setActive] = useState(0);
  /** Resolve's libraries (null = not known: Resolve closed, or not asked yet). */
  const [libs, setLibs] = useState<ResolveLibraries | null>(null);
  const pickerRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const live = useRef(true);
  // Set on every mount too: React's development double mount runs the cleanup once, and a
  // flag only ever cleared would ignore every answer from Resolve ("Asking Resolve…" forever).
  useEffect(() => {
    live.current = true;
    return () => {
      live.current = false;
    };
  }, []);

  // "Goes into" lists Resolve's libraries once there's a starter project (quietly: Resolve
  // may well be closed, and then the saved choice still shows).
  const hasStarter = !!value;
  useEffect(() => {
    if (!hasStarter) return;
    resolveLibraries()
      .then((l) => live.current && setLibs(l))
      .catch(() => {});
  }, [hasStarter]);

  const rows = useMemo(() => (step.kind === "pick" ? pickRows(step.projects, filter) : []), [step, filter]);
  const choices = rows.filter((r): r is Extract<PickRow, { kind: "project" }> => r.kind === "project");

  useEffect(() => {
    if (step.kind === "pick") listRef.current?.focus();
  }, [step.kind]);

  const choose = async () => {
    setStep({ kind: "asking" });
    try {
      const status = await resolveStatus();
      if (!live.current) return;
      if (status !== "ready") {
        setStep({ kind: "line", text: STATUS_LINE[status] });
        return;
      }
      const [projects, libraries] = await Promise.all([
        resolveProjects(null),
        resolveLibraries().catch((): ResolveLibraries => ({ current: null, libraries: [] })),
      ]);
      if (!live.current) return;
      if (libraries.libraries.length) setLibs(libraries);
      setFilter("");
      setActive(0);
      setStep(
        projects.length || libraries.libraries.length > 1
          ? { kind: "pick", projects, libraries, from: null, loading: false, note: null }
          : { kind: "line", text: "There are no projects in the library open in Resolve." },
      );
    } catch (e) {
      if (live.current) setStep({ kind: "line", text: errorMessage(e) });
    }
  };

  /** "From:" another library: its projects (Rust switches Resolve there and back). */
  const lookIn = async (key: string) => {
    if (step.kind !== "pick") return;
    const { current, libraries } = step.libraries;
    const chosen = libraries.find((l) => libraryKey(l) === key) ?? null;
    const from = chosen && current && sameLibrary(chosen, current) ? null : chosen;
    setFilter("");
    setActive(0);
    setStep({ ...step, from, loading: true, note: null });
    const still = (s: Step): s is Pick => s.kind === "pick" && libraryKey(s.from) === libraryKey(from);
    try {
      const projects = await resolveProjects(from);
      if (live.current) setStep((s) => (still(s) ? { ...s, projects, loading: false } : s));
    } catch (e) {
      if (live.current) setStep((s) => (still(s) ? { ...s, projects: [], loading: false, note: errorMessage(e) } : s));
    }
    listRef.current?.focus();
  };

  const pick = async (p: ResolveProject) => {
    const from = step.kind === "pick" ? step.from : null;
    setStep({ kind: "exporting", name: p.name });
    try {
      const drp = await resolveExportToEditor(sessionId, p.name, p.folder, from);
      if (!live.current) return;
      onChange(value ? { ...value, drp } : { drp, folder: defaultFolder(fields), on: true });
      setStep({ kind: "idle" });
    } catch (e) {
      if (live.current) setStep({ kind: "line", text: errorMessage(e) });
    }
  };

  const busy = step.kind === "asking" || step.kind === "exporting" || !!adding;
  const busyText =
    step.kind === "exporting"
      ? `Getting “${step.name}” from Resolve…`
      : adding
        ? `Adding ${adding}…`
        : "Asking Resolve…";

  const picker = step.kind === "pick" && (
    <div
      ref={pickerRef}
      className={`${styles.picker} ${styles.resolvePicker}`}
      // Clicking a project mustn't take the focus away first (WebKit doesn't focus buttons).
      onMouseDown={(e) => {
        if (!(e.target as HTMLElement).closest("input, select")) e.preventDefault();
      }}
      onBlur={(e) => {
        if (!pickerRef.current?.contains(e.relatedTarget as Node | null)) setStep({ kind: "idle" });
      }}
      onKeyDown={(e) => {
        // The library dropdown keeps its own keys.
        if ((e.target as HTMLElement).tagName === "SELECT" && e.key !== "Escape") return;
        if (e.key === "ArrowDown" || e.key === "ArrowUp") {
          e.preventDefault();
          const n = Math.max(1, choices.length);
          setActive((a) => (a + (e.key === "ArrowDown" ? 1 : n - 1)) % n);
        } else if (e.key === "Enter") {
          e.preventDefault();
          const c = choices[active];
          if (c && !step.loading) void pick(c.project);
        } else if (e.key === "Escape") {
          e.preventDefault();
          e.stopPropagation();
          setStep({ kind: "idle" });
        }
      }}
    >
      {step.libraries.libraries.length > 1 && (
        <label className={styles.resolveFrom}>
          From
          <select
            className={styles.select}
            value={libraryKey(step.from ?? step.libraries.current)}
            onChange={(e) => void lookIn(e.target.value)}
          >
            {step.libraries.libraries.map((l) => (
              <option key={libraryKey(l)} value={libraryKey(l)}>
                {step.libraries.current && sameLibrary(l, step.libraries.current) ? `${l.dbName} (open in Resolve)` : l.dbName}
              </option>
            ))}
          </select>
        </label>
      )}
      {step.projects.length > 8 && (
        <input
          className={styles.input}
          autoFocus
          value={filter}
          placeholder="Find a project"
          aria-label="Find a project"
          onChange={(e) => {
            setFilter(e.target.value);
            setActive(0);
          }}
        />
      )}
      <div ref={listRef} className={styles.resolveList} role="listbox" aria-label="Projects in Resolve" tabIndex={-1}>
        {step.loading ? (
          <div className={styles.resolveFolder} role="status">
            <span className={styles.spinner} aria-hidden="true" /> Looking inside “{step.from?.dbName}”…
          </div>
        ) : step.note ? (
          <div className={styles.resolveNote} role="status">
            {step.note}
          </div>
        ) : (
          rows.map((r) =>
            r.kind === "folder" ? (
              <div key={r.key} className={styles.resolveFolder} style={{ paddingLeft: 8 + r.depth * 14 }} role="presentation">
                {r.name}
              </div>
            ) : (
              <button
                key={r.key}
                type="button"
                role="option"
                aria-selected={choices[active] === r}
                className={`${styles.pickItem} ${choices[active] === r ? styles.pickActive : ""}`}
                style={{ paddingLeft: 8 + r.depth * 14 }}
                onMouseEnter={() => setActive(choices.indexOf(r))}
                onClick={() => void pick(r.project)}
              >
                {r.project.name}
              </button>
            ),
          )
        )}
        {!step.loading && !step.note && rows.length === 0 && (
          <div className={styles.resolveFolder}>
            {step.projects.length ? "No project is called that." : "There are no projects in this library."}
          </div>
        )}
      </div>
    </div>
  );

  const line =
    step.kind === "line" ? (
      <p className={styles.hint} role="status">
        {step.text}
      </p>
    ) : busy ? (
      <p className={styles.hint} role="status">
        <span className={styles.spinner} aria-hidden="true" /> {busyText}
      </p>
    ) : null;

  const saved = value?.library ?? null;
  const intoChoices = libraryChoices(libs?.libraries ?? [], saved);

  return (
    <section
      className={`${styles.resolve} ${dropping ? styles.resolveDrop : ""}`}
      aria-label="Resolve project"
      data-resolve-section
    >
      <h3 className={styles.resolveTitle}>Resolve project</h3>
      {!value ? (
        <>
          <p className={styles.hint}>Add a DaVinci Resolve starter project. New jobs get their own copy in Resolve.</p>
          <div className={styles.resolveLine}>
            <span className={styles.resolveAnchor}>
              <Button onClick={() => void choose()} disabled={busy}>
                Choose from Resolve…
              </Button>
              {picker}
            </span>
            <span className={styles.hint}>or drop a .drp file</span>
          </div>
          {line}
        </>
      ) : (
        <>
          <div className={styles.resolveGrid}>
            <span className={styles.resolveLabel}>Starter</span>
            <div className={styles.resolveLine}>
              <span className={styles.resolveName}>{drpName(value.drp)}</span>
              <span className={styles.resolveAnchor}>
                <button type="button" className={styles.fillLink} disabled={busy} onClick={() => void choose()}>
                  Change…
                </button>
                {picker}
              </span>
              <span aria-hidden="true" className={styles.hint}>
                ·
              </span>
              <button type="button" className={styles.fillLink} disabled={busy} onClick={onRemove}>
                Remove
              </button>
            </div>
            {line && (
              <>
                <span />
                {line}
              </>
            )}

            <span className={styles.resolveLabel}>Name in Resolve</span>
            <div>
              <PatternValue
                label="Name in Resolve"
                value={value.name ?? ""}
                empty={
                  <span className={styles.resolveEmpty}>
                    <NameView name={folderName} fields={fields} />
                    <span>same as the job folder</span>
                  </span>
                }
                placeholder="Same as the job folder"
                fields={fields}
                onCommit={(v) => {
                  const next: ResolveStarter = { ...value, name: v };
                  if (!v) delete next.name;
                  onChange(next);
                }}
                onNewBlank={onNewBlank}
                onChip={onChip}
              />
            </div>

            <span className={styles.resolveLabel}>Goes into</span>
            <div className={styles.resolveLine}>
              <select
                className={styles.select}
                aria-label="Library in Resolve"
                value={libraryKey(saved)}
                onChange={(e) => {
                  const library = intoChoices.find((l) => libraryKey(l) === e.target.value);
                  const next: ResolveStarter = { ...value, library };
                  if (!library) delete next.library;
                  onChange(next);
                }}
              >
                <option value="">The library open in Resolve</option>
                {intoChoices.map((l) => (
                  <option key={libraryKey(l)} value={libraryKey(l)}>
                    {l.dbName}
                  </option>
                ))}
              </select>
              <span aria-hidden="true" className={styles.resolveSep}>
                ›
              </span>
              <PatternValue
                label="Folder in Resolve"
                value={value.folder}
                empty={<span className={styles.resolveEmpty}>the top of the library</span>}
                fields={fields}
                allowSlash
                onCommit={(v) => onChange({ ...value, folder: v.replace(/^\/+|\/+$/g, "").replace(/\/{2,}/g, "/") })}
                onNewBlank={onNewBlank}
                onChip={onChip}
              />
            </div>
            <span />
            <p className={styles.hint}>
              Use / for folders inside folders. If this computer's Resolve doesn't have the library, the project goes
              into the open one.
            </p>

            <span />
            <label className={styles.resolveCheck}>
              <input type="checkbox" checked={value.on} onChange={(e) => onChange({ ...value, on: e.target.checked })} />
              Ticked in New Project
            </label>
          </div>
        </>
      )}
    </section>
  );
}
