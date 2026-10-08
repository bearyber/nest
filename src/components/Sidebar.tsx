import { useState, type ReactNode } from "react";
import { isMac } from "../lib/platform";
import type { StatusFilter } from "../lib/search";
import Icon from "./Icon";
import { IconButton } from "./ui";
import styles from "./Sidebar.module.css";

interface Props {
  /** Hide the sidebar (its button sits at the top, like other sidebars on Windows and Mac). */
  onHide: () => void;
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
  onHide,
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
  const [collapsed, setCollapsed] = useState<Set<string>>(loadCollapsed);
  const toggle = (id: string) =>
    setCollapsed((c) => {
      const next = new Set(c);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      saveCollapsed(next);
      return next;
    });

  return (
    <nav className={styles.sidebar} aria-label="Filters">
      <div className={styles.top}>
        <IconButton icon="sidebar" label={`Hide sidebar (${isMac() ? "⌘B" : "Ctrl+B"})`} onClick={onHide} />
      </div>
      <Section id="spaces" title="Spaces" collapsed={collapsed} onToggle={toggle}>
        {(open) => (
          <>
            {(open || space === null) && (
              <Item label="All projects" count={counts[""] ?? 0} selected={space === null} onClick={() => onSpace(null)} />
            )}
            {spaces
              .filter((s) => open || space === s)
              .map((s) => (
                <Item key={s} label={s} count={counts[s] ?? 0} selected={space === s} onClick={() => onSpace(s)} />
              ))}
          </>
        )}
      </Section>
      <Section id="status" title="Status" collapsed={collapsed} onToggle={toggle}>
        {(open) =>
          STATUSES.filter((s) => s.value !== "archived" || statusCounts.archived > 0 || status === "archived")
            .filter((s) => open || status === s.value)
            .map((s) => (
              <Item
                key={s.value}
                label={s.label}
                count={statusCounts[s.value]}
                selected={status === s.value}
                onClick={() => onStatus(s.value)}
                dot={s.value === "active" || s.value === "done" ? s.value : undefined}
              />
            ))
        }
      </Section>
      {computers.length > 0 && onComputer && (
        <Section id="computer" title="Computer" collapsed={collapsed} onToggle={toggle}>
          {(open) => (
            <>
              {(open || computer === null) && (
                <Item label="All computers" count={allCount} selected={computer === null} onClick={() => onComputer(null)} />
              )}
              {(open || computer === "") && (
                <Item label="This computer" count={hereCount} selected={computer === ""} onClick={() => onComputer("")} />
              )}
              {computers
                .filter((c) => open || computer === c.name)
                .map((c) => (
                  <Item
                    key={c.name}
                    label={c.name}
                    count={c.count}
                    selected={computer === c.name}
                    onClick={() => onComputer(c.name)}
                    fresh={c.fresh}
                  />
                ))}
            </>
          )}
        </Section>
      )}
      <button type="button" className={styles.settings} onClick={onSettings}>
        <Icon name="gear" />
        Settings
      </button>
    </nav>
  );
}

// Folded sections, remembered per computer (a convenience: lost storage just means unfolded).
const COLLAPSED_KEY = "nest.sidebarCollapsed";

function loadCollapsed(): Set<string> {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(COLLAPSED_KEY) ?? "[]");
    return new Set(Array.isArray(saved) ? saved.filter((x): x is string => typeof x === "string") : []);
  } catch {
    return new Set();
  }
}

function saveCollapsed(c: Set<string>) {
  try {
    localStorage.setItem(COLLAPSED_KEY, JSON.stringify([...c]));
  } catch {
    /* storage unavailable: just not remembered */
  }
}

/** A sidebar section whose heading folds it away. Folded, it still shows the chosen row (so
 *  you can see what's filtering the list). */
function Section({
  id,
  title,
  collapsed,
  onToggle,
  children,
}: {
  id: string;
  title: string;
  collapsed: Set<string>;
  onToggle: (id: string) => void;
  children: (open: boolean) => ReactNode;
}) {
  const open = !collapsed.has(id);
  return (
    <div className={styles.section}>
      <h2 className={styles.headingWrap}>
        <button type="button" className={styles.heading} aria-expanded={open} onClick={() => onToggle(id)}>
          {title}
          <Icon name="chevron" className={open ? styles.chevronOpen : styles.chevron} />
        </button>
      </h2>
      {children(open)}
    </div>
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
