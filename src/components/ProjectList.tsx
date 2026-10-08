import { useEffect, useRef, useState } from "react";
import { formatBytes, formatDate, isDone, type Sort, type SortKey } from "../lib/search";
import type { ProjectRow } from "../lib/types";
import Icon from "./Icon";
import styles from "./ProjectList.module.css";

interface Props {
  rows: ProjectRow[];
  selected: string | null;
  sort: Sort;
  onSort: (key: SortKey) => void;
  onSelect: (key: string) => void;
  onOpen: (row: ProjectRow) => void;
  onContextMenu: (row: ProjectRow) => void;
}

const COLUMNS: { key: SortKey; label: string }[] = [
  { key: "code", label: "Code" },
  { key: "title", label: "Title" },
  { key: "client", label: "Client" },
  { key: "size", label: "Size" },
  { key: "created", label: "Created" },
];

// Resizable columns: Title takes what's left; the others have a width you can drag.
type Widths = Record<"code" | "client" | "size" | "created", number>;
const DEFAULT_WIDTHS: Widths = { code: 110, client: 140, size: 84, created: 104 };
const MIN_W = 44;
const MAX_W = 420;
const WIDTHS_KEY = "nest.listColumns";

function loadWidths(): Widths {
  try {
    const saved = JSON.parse(localStorage.getItem(WIDTHS_KEY) ?? "{}") as Partial<Widths>;
    const w = { ...DEFAULT_WIDTHS };
    for (const k of Object.keys(w) as (keyof Widths)[]) {
      const n = saved[k];
      if (typeof n === "number" && n >= MIN_W && n <= MAX_W) w[k] = n;
    }
    return w;
  } catch {
    return { ...DEFAULT_WIDTHS };
  }
}

function saveWidths(w: Widths) {
  try {
    localStorage.setItem(WIDTHS_KEY, JSON.stringify(w));
  } catch {
    /* storage unavailable: widths just aren't remembered */
  }
}

/** The project list (build spec §3.1): click a header to sort, double-click a row to open,
 *  drag a header border to resize a column (double-click it to reset). */
export default function ProjectList({ rows, selected, sort, onSort, onSelect, onOpen, onContextMenu }: Props) {
  const selectedRef = useRef<HTMLDivElement>(null);
  const [widths, setWidths] = useState<Widths>(loadWidths);
  // Each column grows to its width when there's room and shrinks (down to MIN_W) when the window
  // is narrow; Title keeps at least 100px and takes whatever is left.
  const col = (w: number) => `minmax(${MIN_W}px, ${w}px)`;
  const cols = `28px ${col(widths.code)} minmax(100px, 1fr) ${col(widths.client)} ${col(widths.size)} ${col(widths.created)}`;

  // Code's border is on its right (drag right = wider); the columns after Title have theirs on
  // the left (drag left = wider), so Title always gives or takes the space.
  const startResize = (col: keyof Widths, e: React.PointerEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const startX = e.clientX;
    const start = widths[col];
    const dir = col === "code" ? 1 : -1;
    let latest = widths;
    const move = (ev: PointerEvent) => {
      const w = Math.min(MAX_W, Math.max(MIN_W, start + dir * (ev.clientX - startX)));
      latest = { ...latest, [col]: w };
      setWidths(latest);
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      saveWidths(latest);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };
  const resetWidth = (col: keyof Widths) => {
    const next = { ...widths, [col]: DEFAULT_WIDTHS[col] };
    setWidths(next);
    saveWidths(next);
  };

  // Keep the selected row in view when moving with the arrow keys.
  useEffect(() => {
    selectedRef.current?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  return (
    <div
      className={styles.table}
      role="grid"
      aria-label="Projects"
      aria-rowcount={rows.length}
      style={{ "--cols": cols } as React.CSSProperties}
    >
      <div className={styles.header} role="row">
        <span role="columnheader" aria-label="Status" />
        {COLUMNS.map((c) => {
          const resizable = c.key !== "title" ? (c.key as keyof Widths) : null;
          return (
            <div key={c.key} className={styles.headerWrap}>
              <button
                type="button"
                role="columnheader"
                className={styles.headerCell}
                aria-sort={sort.key === c.key ? (sort.dir === "asc" ? "ascending" : "descending") : "none"}
                onClick={() => onSort(c.key)}
              >
                {c.label}
                {sort.key === c.key && (
                  <Icon name="down" className={sort.dir === "asc" ? styles.sortAsc : styles.sortDesc} />
                )}
              </button>
              {resizable && (
                <span
                  role="separator"
                  aria-orientation="vertical"
                  aria-label={`Resize ${c.label} column`}
                  title="Drag to resize · double-click to reset"
                  className={resizable === "code" ? styles.resizeRight : styles.resizeLeft}
                  onPointerDown={(e) => startResize(resizable, e)}
                  onDoubleClick={() => resetWidth(resizable)}
                />
              )}
            </div>
          );
        })}
      </div>
      <div className={styles.body}>
        {rows.map((r) => {
          const isSelected = r.key === selected;
          const warning = r.error ?? (r.duplicateCode ? `Another project also uses ${r.jobCode}` : null);
          return (
            <div
              key={r.key}
              ref={isSelected ? selectedRef : undefined}
              role="row"
              aria-selected={isSelected}
              className={`${styles.row} ${r.offline || r.device || r.archived ? styles.offline : ""}`}
              onClick={() => onSelect(r.key)}
              onDoubleClick={() => onOpen(r)}
              onContextMenu={(e) => {
                e.preventDefault();
                onSelect(r.key);
                onContextMenu(r);
              }}
            >
              <span role="gridcell" className={styles.statusCell}>
                <span className={isDone(r) ? styles.dotDone : styles.dotActive} title={isDone(r) ? "Done" : "Active"} />
              </span>
              <span role="gridcell" className={styles.code}>
                {r.jobCode || "—"}
                {warning && (
                  <span title={warning} className={styles.warn}>
                    <Icon name="warning" />
                  </span>
                )}
              </span>
              <span role="gridcell" className={styles.title}>
                {r.title || "Untitled"}
                {r.error && <span className={styles.pill}>Unreadable</span>}
                {r.archived && <span className={styles.pill}>Archived</span>}
                {r.offline && <span className={styles.pill}>Offline</span>}
                {r.device && <span className={styles.pill}>On {r.device.name}</span>}
                {r.device && r.pending && r.pending !== r.status && (
                  <span className={styles.pill}>
                    {r.pending === "done" ? "Done" : "Active"}, waiting for {r.device.name}
                  </span>
                )}
                {!r.device && (r.alsoOn?.length ?? 0) > 0 && (
                  <span className={styles.pill} title={`Also on ${r.alsoOn!.join(", ")}`}>
                    Also on {r.alsoOn!.join(", ")}
                  </span>
                )}
              </span>
              <span role="gridcell" className={styles.muted}>
                {r.client.name || r.client.code}
              </span>
              <span
                role="gridcell"
                className={`${styles.muted} ${styles.size}`}
                title={r.size === undefined ? "Not measured yet: select the project to measure it" : undefined}
              >
                {r.size === undefined ? "—" : formatBytes(r.size)}
              </span>
              <span role="gridcell" className={styles.muted}>
                {formatDate(r.createdAt)}
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}
