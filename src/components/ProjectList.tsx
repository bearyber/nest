import { useEffect, useRef } from "react";
import { formatDate, isDone, type Sort, type SortKey } from "../lib/search";
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
  { key: "client", label: "Billed to" },
  { key: "created", label: "Created" },
];

/** The project list (build spec §3.1): click a header to sort, double-click a row to open. */
export default function ProjectList({ rows, selected, sort, onSort, onSelect, onOpen, onContextMenu }: Props) {
  const selectedRef = useRef<HTMLDivElement>(null);

  // Keep the selected row in view when moving with the arrow keys.
  useEffect(() => {
    selectedRef.current?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  return (
    <div className={styles.table} role="grid" aria-label="Projects" aria-rowcount={rows.length}>
      <div className={styles.header} role="row">
        <span role="columnheader" aria-label="Status" />
        {COLUMNS.map((c) => (
          <button
            key={c.key}
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
        ))}
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
