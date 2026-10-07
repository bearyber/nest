import type { StatusFilter } from "../lib/search";
import Icon from "./Icon";
import styles from "./Sidebar.module.css";

interface Props {
  spaces: string[];
  /** Projects per space for the current status filter; key "" = all spaces. */
  counts: Record<string, number>;
  space: string | null;
  status: StatusFilter;
  statusCounts: Record<StatusFilter, number>;
  onSpace: (space: string | null) => void;
  onStatus: (status: StatusFilter) => void;
  onSettings: () => void;
  /** Devices: your other computers (none = the section is hidden). */
  computers?: Computer[];
  /** null = all computers, "" = this computer, or another computer's name. */
  computer?: string | null;
  onComputer?: (computer: string | null) => void;
  /** Projects on this computer (for the count). */
  hereCount?: number;
  allCount?: number;
}

export interface Computer {
  name: string;
  count: number;
  /** Its list was updated in the last 24 hours. */
  fresh: boolean;
}

// "Everything" first, like "All projects" and "All computers".
const STATUSES: { value: StatusFilter; label: string }[] = [
  { value: "all", label: "All statuses" },
  { value: "active", label: "Active" },
  { value: "done", label: "Done" },
  // Only shown once there is something archived (or while it's the chosen filter).
  { value: "archived", label: "Archived" },
];

/** Spaces and Status filters (build spec §3.1). The sidebar stays see-through over Mica. */
export default function Sidebar({
  spaces,
  counts,
  space,
  status,
  statusCounts,
  onSpace,
  onStatus,
  onSettings,
  computers = [],
  computer = null,
  onComputer,
  hereCount = 0,
  allCount = 0,
}: Props) {
  return (
    <nav className={styles.sidebar} aria-label="Filters">
      <div className={styles.section}>
        <h2 className={styles.heading}>Spaces</h2>
        <Item label="All projects" count={counts[""] ?? 0} selected={space === null} onClick={() => onSpace(null)} />
        {spaces.map((s) => (
          <Item key={s} label={s} count={counts[s] ?? 0} selected={space === s} onClick={() => onSpace(s)} />
        ))}
      </div>
      <div className={styles.section}>
        <h2 className={styles.heading}>Status</h2>
        {STATUSES.filter((s) => s.value !== "archived" || statusCounts.archived > 0 || status === "archived").map(
          (s) => (
            <Item
              key={s.value}
              label={s.label}
              count={statusCounts[s.value]}
              selected={status === s.value}
              onClick={() => onStatus(s.value)}
              dot={s.value === "active" || s.value === "done" ? s.value : undefined}
            />
          ),
        )}
      </div>
      {computers.length > 0 && onComputer && (
        <div className={styles.section}>
          <h2 className={styles.heading}>Computer</h2>
          <Item label="All computers" count={allCount} selected={computer === null} onClick={() => onComputer(null)} />
          <Item label="This computer" count={hereCount} selected={computer === ""} onClick={() => onComputer("")} />
          {computers.map((c) => (
            <Item
              key={c.name}
              label={c.name}
              count={c.count}
              selected={computer === c.name}
              onClick={() => onComputer(c.name)}
              fresh={c.fresh}
            />
          ))}
        </div>
      )}
      <button type="button" className={styles.settings} onClick={onSettings}>
        <Icon name="gear" />
        Settings
      </button>
    </nav>
  );
}

interface ItemProps {
  label: string;
  count: number;
  selected: boolean;
  onClick: () => void;
  dot?: "active" | "done";
  /** Devices: a dot for how fresh another computer's list is. */
  fresh?: boolean;
}

function Item({ label, count, selected, onClick, dot, fresh }: ItemProps) {
  return (
    <button type="button" className={styles.item} aria-current={selected ? "true" : undefined} onClick={onClick}>
      {dot && <span className={dot === "active" ? styles.dotActive : styles.dotDone} aria-hidden="true" />}
      {fresh !== undefined && (
        <span
          className={fresh ? styles.dotFresh : styles.dotStale}
          title={fresh ? "Updated in the last 24 hours" : "Not updated for over a day"}
          aria-hidden="true"
        />
      )}
      <span className={styles.label}>{label}</span>
      <span className={styles.count}>{count}</span>
    </button>
  );
}
