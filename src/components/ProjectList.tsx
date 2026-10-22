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
  /** `option`: ⌥ (Alt) was held, which on a Mac asks for Show in Finder instead of Open. */
  onContextMenu: (row: ProjectRow, option: boolean) => void;
}

const COLUMNS: { key: SortKey; label: string }[] = [
  { key: "code", label: "Code" },
  { key: "title", label: "Title" },
  { key: "client", label: "Client" },
  { key: "size", label: "Size" },
  { key: "created", label: "Created" },
];

// Resizable columns, like Explorer and Finder: each divider resizes the column on its left, and
// an empty filler after the last column takes what's left.
type Widths = Record<"code" | "title" | "client" | "size" | "created", number>;
const DEFAULT_WIDTHS: Widths = {
  code: 110,
  title: 280,
  client: 140,
  size: 84,
  created: 104,
};
const MIN_W = 44;
const MIN_TITLE = 120;
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
  const [dragging, setDragging] = useState<keyof Widths | null>(null);
  // Each column grows to its width when there's room and shrinks (down to its minimum) when the
  // window is narrow; the filler at the end takes whatever is left.
  const col = (w: number, min = MIN_W) => `minmax(${min}px, ${w}px)`;
  const cols = `28px ${col(widths.code)} ${col(widths.title, MIN_TITLE)} ${col(widths.client)} ${col(widths.size)} ${col(widths.created)} minmax(0, 1fr)`;

  // Drag right = wider. The drag starts from the width on screen (a narrow window may have
  // squeezed the column below its saved width), so the divider follows the mouse from the start.
  const startResize = (col: keyof Widths, e: React.PointerEvent<HTMLElement>) => {
    e.preventDefault();
    e.stopPropagation();
    const min = col === "title" ? MIN_TITLE : MIN_W;
    const onScreen = e.currentTarget.parentElement?.getBoundingClientRect().width ?? widths[col];
    const start = Math.min(MAX_W, Math.max(min, Math.round(onScreen)));
    const startX = e.clientX;
    let latest = { ...widths, [col]: start };
    setWidths(latest);
    setDragging(col);
    document.body.classList.add(styles.resizing);
    const move = (ev: PointerEvent) => {
      const w = Math.min(MAX_W, Math.max(min, start + ev.clientX - startX));
      latest = { ...latest, [col]: w };
      setWidths(latest);
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
      document.body.classList.remove(styles.resizing);
      setDragging(null);
      saveWidths(latest);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
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
      data-area="list"
      aria-rowcount={rows.length}
      style={{ "--cols": cols } as React.CSSProperties}
    >
      <div className={styles.header} role="row">
        <span role="columnheader" aria-label="Status" />
        {COLUMNS.map((c) => {
          const resizable = c.key as keyof Widths;
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
              <span
                role="separator"
                aria-orientation="vertical"
                aria-label={`Resize ${c.label} column`}
                title="Drag to resize · double-click to reset"
                className={`${styles.resize} ${dragging === resizable ? styles.resizeActive : ""}`}
                onPointerDown={(e) => startResize(resizable, e)}
                onDoubleClick={() => resetWidth(resizable)}
              />
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
                onContextMenu(r, e.altKey);
              }}
            >
              <span role="gridcell" className={styles.statusCell}>
                <span className={isDone(r) ? styles.dotDone : styles.dotActive} title={isDone(r) ? "Done" : "Active"} />
              </span>
              <span role="gridcell" className={styles.code}>
                {r.jobCode || "—"}
                {warning && (
                  <span title={warning} role="img" aria-label={`Warning: ${warning}`} className={styles.warn}>
                    <Icon name="warning" />
                  </span>
                )}
              </span>
              <span role="gridcell" className={styles.title}>
                <span className={styles.titleText}>{r.title || "Untitled"}</span>
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
