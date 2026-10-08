import { useState } from "react";
import { isMac } from "../lib/platform";
import { ago, formatBytes, formatDate, isDone } from "../lib/search";
import type { FolderSize, ProjectRow, Sizes } from "../lib/types";
import Icon from "./Icon";
import { Button, IconButton, PathText, Segmented } from "./ui";
import styles from "./Inspector.module.css";

interface Props {
  project: ProjectRow | null;
  onOpen: () => void;
  onReveal: () => void;
  onCopyPath: () => void;
  onStatus: (status: "active" | "done") => void;
  /** Archive (M5 P2): an archive folder is set, and Archive… / Unarchive… was clicked. */
  archiveOn: boolean;
  onArchive: () => void;
  /** Archive: "Not this one" on a project that's ready to archive. */
  onDismissArchive: () => void;
  /** Folder sizes: the last measurement (null = none yet), and whether one is running. */
  sizes: Sizes | null;
  measuring: boolean;
  onMeasure: () => void;
  /** Open a folder inside the project in Finder/Explorer ([] = the project folder). */
  onOpenFolder: (path: string[]) => void;
  /** What's inside one folder, one level down (the ▸ under Size). */
  onMeasureFolder: (path: string[]) => Promise<Sizes>;
}

type Inside = Sizes | "loading" | "error";

/** Size rows: each folder opens in Finder/Explorer, and ▸ shows what's inside it, as deep as
 *  you like. "Loose files" opens the folder they're in. */
function FolderRows({
  folders,
  parent,
  depth,
  disabled,
  inside,
  onToggle,
  onOpen,
}: {
  folders: FolderSize[];
  parent: string[];
  depth: number;
  disabled: boolean;
  inside: Record<string, Inside>;
  onToggle: (path: string[]) => void;
  onOpen: (path: string[]) => void;
}) {
  const app = isMac() ? "Finder" : "Explorer";
  return (
    <ul className={styles.folderList}>
      {folders
        .filter((f) => f.total > 0)
        .map((f) => {
          const path = f.name ? [...parent, f.name] : parent;
          const id = path.join("/");
          const open = f.name ? inside[id] : undefined;
          const split = typeSplit(f);
          return (
            <li key={f.name || "/"}>
              <div className={styles.folderRow} style={{ paddingLeft: depth * 16 }}>
                {f.name ? (
                  <button
                    type="button"
                    className={styles.expand}
                    aria-expanded={!!open}
                    aria-label={`${open ? "Hide" : "Show"} what's inside ${f.name}`}
                    disabled={disabled}
                    onClick={() => onToggle(path)}
                  >
                    {open ? "▾" : "▸"}
                  </button>
                ) : (
                  <span className={styles.expandGap} />
                )}
                <div className={styles.folderText}>
                  <button
                    type="button"
                    className={styles.folderLink}
                    disabled={disabled}
                    title={`Open ${f.name || "this folder"} in ${app}`}
                    onClick={() => onOpen(path)}
                  >
                    <span className={styles.folderName}>{f.name || "Loose files"}</span>
                    <Icon name="open" className={styles.folderLinkIcon} />
                  </button>
                  {split && <span className={styles.folderSplit}>{split}</span>}
                </div>
                <span className={styles.folderSize}>{formatBytes(f.total)}</span>
              </div>
              {open === "loading" && (
                <p className={styles.insideNote} style={{ paddingLeft: (depth + 1) * 16 + 28 }}>
                  Counting…
                </p>
              )}
              {open === "error" && (
                <p className={styles.insideNote} style={{ paddingLeft: (depth + 1) * 16 + 28 }}>
                  Couldn't look inside.
                </p>
              )}
              {open && typeof open === "object" && (
                <FolderRows
                  folders={open.folders}
                  parent={path}
                  depth={depth + 1}
                  disabled={disabled}
                  inside={inside}
                  onToggle={onToggle}
                  onOpen={onOpen}
                />
              )}
            </li>
          );
        })}
    </ul>
  );
}

/** The Size breakdown with its own unfolded state (reset per project and per measurement). */
function FolderTree({
  sizes,
  disabled,
  onOpenFolder,
  onMeasureFolder,
}: {
  sizes: Sizes;
  disabled: boolean;
  onOpenFolder: (path: string[]) => void;
  onMeasureFolder: (path: string[]) => Promise<Sizes>;
}) {
  const [inside, setInside] = useState<Record<string, Inside>>({});
  const toggle = (path: string[]) => {
    const id = path.join("/");
    if (inside[id]) {
      setInside((s) => {
        const next = { ...s };
        delete next[id];
        return next;
      });
      return;
    }
    setInside((s) => ({ ...s, [id]: "loading" }));
    onMeasureFolder(path)
      .then((found) => setInside((s) => (s[id] ? { ...s, [id]: found } : s)))
      .catch(() => setInside((s) => (s[id] ? { ...s, [id]: "error" } : s)));
  };
  return (
    <FolderRows
      folders={sizes.folders}
      parent={[]}
      depth={0}
      disabled={disabled}
      inside={inside}
      onToggle={toggle}
      onOpen={onOpenFolder}
    />
  );
}

/** "video 390 GB, images 5 GB": only the types that are there, biggest first. */
function typeSplit(f: FolderSize): string {
  return [
    ["video", f.video],
    ["audio", f.audio],
    ["images", f.images],
    ["other", f.other],
  ]
    .filter(([, n]) => (n as number) > 0)
    .sort((a, b) => (b[1] as number) - (a[1] as number))
    .map(([k, n]) => `${k} ${formatBytes(n as number)}`)
    .join(", ");
}

/** "artist" → "Artist", "startDate" → "Start date". */
function labelFor(key: string): string {
  const words = key.replace(/([a-z])([A-Z])/g, "$1 $2").replace(/_/g, " ").toLowerCase();
  return words.charAt(0).toUpperCase() + words.slice(1);
}

function display(v: unknown): string {
  if (Array.isArray(v)) return v.join(", ");
  if (typeof v === "boolean") return v ? "Yes" : "No";
  if (v === null || v === undefined) return "";
  return String(v);
}

/** Why the status can't be changed right now, or null if it can. */
function statusBlocked(p: ProjectRow): string | null {
  if (p.error) return "The project file can't be read";
  if (p.archived) return "Archived: unarchive it to change it";
  if (p.offline) return "Its jobs folder is offline";
  if (p.paths.length > 1) return "It exists in two places; remove the extra copy first";
  return null;
}

/** Details panel (build spec §3.1): read-only values, status, Open / Reveal / Copy path. */
export default function Inspector({
  project: p,
  onOpen,
  onReveal,
  onCopyPath,
  onStatus,
  archiveOn,
  onArchive,
  onDismissArchive,
  sizes,
  measuring,
  onMeasure,
  onOpenFolder,
  onMeasureFolder,
}: Props) {
  if (!p) {
    return (
      <aside className={`${styles.inspector} ${styles.emptyPane}`} aria-label="Details">
        <p className={styles.empty}>Select a project to see its details</p>
      </aside>
    );
  }

  const blocked = statusBlocked(p);
  // Archive… on a Done project here; Unarchive… on an archived one (Rust re-checks and asks).
  const archiveBlocked = p.error
    ? "The project file can't be read"
    : p.offline
      ? "Its folder is offline"
      : p.paths.length > 1
        ? "It exists in two places; remove the extra copy first"
        : !p.archived && !isDone(p)
          ? "Mark it done first"
          : null;
  const fields = Object.entries(p.fields ?? {}).filter(([, v]) => display(v) !== "");
  const revealLabel = isMac() ? "Reveal in Finder" : "Show in Explorer";

  return (
    <aside className={styles.inspector} aria-label="Details">
      <header className={styles.head}>
        <div className={styles.codeRow}>
          <span className={styles.code}>{p.jobCode || "No code"}</span>
          {p.archived && <span className={styles.pill}>Archived</span>}
          {p.offline && <span className={styles.pill}>Offline</span>}
        </div>
        <h2 className={styles.title}>{p.title || "Untitled"}</h2>
      </header>

      {p.device && (
        <div className={styles.lives}>
          <b>Lives on {p.device.name}</b>
          <span>
            {p.device.updatedAt ? `Updated ${ago(p.device.updatedAt)}. ` : ""}The folder isn't on this computer.
          </span>
        </div>
      )}
      {p.readyToArchive && (
        <div className={styles.lives}>
          <b>Ready to archive</b>
          <span>Done {p.doneAt ? ago(new Date(p.doneAt).toISOString()) : "a while ago"}.</span>
          <div className={styles.inlineActions}>
            {archiveOn && (
              <Button onClick={onArchive} disabled={!!archiveBlocked} title={archiveBlocked ?? undefined}>
                Archive…
              </Button>
            )}
            <Button onClick={onDismissArchive}>Not this one</Button>
          </div>
        </div>
      )}
      {!p.device && (p.alsoOn?.length ?? 0) > 0 && (
        <p className={styles.note}>Also on {p.alsoOn!.join(", ")}.</p>
      )}

      {p.error && (
        <div className={styles.alert} role="alert">
          <Icon name="warning" />
          <div>
            <b>Manifest unreadable.</b> Nest won't change or fix it. Open it in a text editor to repair it.
            <div className={styles.alertDetail}>{p.error}</div>
          </div>
        </div>
      )}
      {p.duplicateCode && (
        <div className={styles.alert}>
          <Icon name="warning" />
          <div>Another project also uses {p.jobCode}. Rename one of them by hand to keep codes unique.</div>
        </div>
      )}

      {!p.error && (
        <div className={styles.section}>
          <Segmented
            label="Status"
            options={["Active", "Done"]}
            value={(p.pending ?? p.status) === "done" ? "Done" : "Active"}
            onChange={(v) => !blocked && onStatus(v === "Done" ? "done" : "active")}
            disabled={blocked !== null}
            block
          />
          {blocked && (
            <p className={styles.note}>
              Status can't be changed: {blocked.charAt(0).toLowerCase() + blocked.slice(1)}.
            </p>
          )}
          {/* Devices: a project on another computer is changed there, by request. */}
          {p.device && p.pending && p.pending !== p.status && (
            <div className={styles.waiting}>
              <span>
                Waiting for {p.device.name}: it marks this {p.pending} next time Nest is open there.
              </span>
              <Button onClick={() => onStatus(p.status === "done" ? "done" : "active")}>Undo</Button>
            </div>
          )}
          {p.device && !(p.pending && p.pending !== p.status) && !p.requestProblem && (
            <p className={styles.note}>A change here is sent to {p.device.name} and applied there.</p>
          )}
          {p.requestProblem && (
            <p className={styles.problemNote}>
              Couldn't change it on {p.device?.name ?? "the other computer"}: {p.requestProblem}
            </p>
          )}
          {p.changedBy && (
            <p className={styles.note}>
              Marked {p.changedBy.status} from {p.changedBy.name} · {ago(p.changedBy.at)}
            </p>
          )}
        </div>
      )}

      <dl className={styles.details}>
        {p.client.name ? (
          <>
            <dt>Billed to</dt>
            <dd>
              {p.client.name} {p.client.code && <span className={styles.mono}>{p.client.code}</span>}
            </dd>
          </>
        ) : (
          p.client.code && (
            <>
              <dt>Billed to</dt>
              <dd>No client (personal)</dd>
            </>
          )
        )}
        {p.space && (
          <>
            <dt>Space</dt>
            <dd>{p.space}</dd>
          </>
        )}
        {p.createdAt && (
          <>
            <dt>Created</dt>
            <dd>{formatDate(p.createdAt)}</dd>
          </>
        )}
        {p.templateId && (
          <>
            <dt>Template</dt>
            <dd>{p.templateName || p.templateId}</dd>
          </>
        )}
        {fields.map(([k, v]) => (
          <div key={k} className={styles.pair}>
            <dt>{labelFor(k)}</dt>
            <dd>{display(v)}</dd>
          </div>
        ))}
      </dl>

      {!p.device && !p.error && (
        <div className={styles.section}>
          <div className={styles.subRow}>
            <h3 className={styles.sub}>Size</h3>
            <Button onClick={onMeasure} disabled={measuring || p.offline}>
              {measuring ? "Measuring…" : sizes ? "Measure again" : "Measure"}
            </Button>
          </div>
          {sizes ? (
            <>
              <dl className={styles.details}>
                <dt>Total</dt>
                <dd>
                  {formatBytes(sizes.total)}
                  <span className={styles.note}>
                    {" "}
                    · {ago(new Date(sizes.measuredAt).toISOString())}
                    {sizes.unreadable > 0 ? ` · ${sizes.unreadable} unreadable` : ""}
                  </span>
                </dd>
              </dl>
              <FolderTree
                key={`${p.key}:${sizes.measuredAt}`}
                sizes={sizes}
                disabled={p.offline}
                onOpenFolder={onOpenFolder}
                onMeasureFolder={onMeasureFolder}
              />
            </>
          ) : (
            <p className={styles.note}>{measuring ? "Counting every file…" : "Not measured yet."}</p>
          )}
        </div>
      )}

      <div className={styles.section}>
        <div className={styles.subRow}>
          <h3 className={styles.sub}>
            {p.device
              ? `Location on ${p.device.name}`
              : p.paths.length > 1
                ? "Locations (the same project in two places)"
                : "Location"}
          </h3>
          <IconButton icon="copy" label="Copy path" onClick={onCopyPath} />
        </div>
        {(p.device ? [p.device.path].filter(Boolean) : p.paths).map((path) => (
          <div key={path} className={styles.path} title={path}>
            <Icon name="folder" className={styles.folder} />
            <span>
              <PathText path={path} />
            </span>
          </div>
        ))}
      </div>

      <div className={styles.actions}>
        <Button variant="primary" onClick={onOpen} disabled={p.offline || !!p.device}>
          Open folder
        </Button>
        <Button onClick={onReveal} disabled={p.offline || !!p.device}>
          {revealLabel}
        </Button>
        {archiveOn && !p.device && (
          <Button onClick={onArchive} disabled={!!archiveBlocked} title={archiveBlocked ?? undefined}>
            {p.archived ? "Unarchive…" : "Archive…"}
          </Button>
        )}
      </div>
      {p.device && <p className={styles.note}>To open it, use Nest on {p.device.name}.</p>}
    </aside>
  );
}
