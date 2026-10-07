import type { ProjectRow } from "./types";

/** "archived" lists only archived projects; "active" and "done" never include them; "all" does. */
export type StatusFilter = "active" | "done" | "all" | "archived";
export type SortKey = "created" | "code" | "title" | "client";

export interface Sort {
  key: SortKey;
  dir: "asc" | "desc";
}

export interface Filters {
  /** null = all spaces */
  space: string | null;
  status: StatusFilter;
  query: string;
  /** Devices: null = all computers, "" = this computer, or another computer's name. */
  computer?: string | null;
  /** Archive: only projects that are ready to archive. */
  ready?: boolean;
}

/** Lowercase and strip accents, so "beyonce" finds "Beyoncé" (Korean/Chinese unchanged). */
export function fold(s: string): string {
  return s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
}

function flatten(v: unknown): string[] {
  if (typeof v === "string") return [v];
  if (typeof v === "number") return [String(v)];
  if (Array.isArray(v)) return v.flatMap(flatten);
  if (v && typeof v === "object") return Object.values(v).flatMap(flatten);
  return [];
}

/** Everything a project can be found by: code, title, client, space, and every field value. */
export function haystack(p: ProjectRow): string {
  return fold(
    [p.jobCode, p.title, p.client.name, p.client.code, p.space, p.templateId, ...flatten(p.fields)].join(
      " ",
    ),
  );
}

/** Every word in the query must appear somewhere (in any order). */
export function matches(p: ProjectRow, query: string): boolean {
  const words = fold(query).split(/\s+/).filter(Boolean);
  if (words.length === 0) return true;
  const hay = haystack(p);
  return words.every((w) => hay.includes(w));
}

export const isDone = (p: ProjectRow) => p.status === "done";

/** "412 GB", "2.5 MB", "0 B": one decimal below 10, none above (like Finder). */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = bytes;
  let i = 0;
  while (v >= 1000 && i < units.length - 1) {
    v /= 1000;
    i++;
  }
  const digits = i === 0 ? 0 : v < 10 ? 1 : 0;
  return `${v.toFixed(digits)} ${units[i]}`;
}

export function statusMatches(p: ProjectRow, status: StatusFilter): boolean {
  if (status === "all") return true;
  if (status === "archived") return p.archived;
  return !p.archived && (status === "done") === isDone(p);
}

/** Devices: which computer a project is on (a project on both matches both). */
export function computerMatches(p: ProjectRow, computer: string | null | undefined): boolean {
  if (computer === null || computer === undefined) return true;
  if (computer === "") return !p.device;
  return p.device?.name === computer || (p.alsoOn ?? []).includes(computer);
}

export function filterProjects(rows: ProjectRow[], f: Filters): ProjectRow[] {
  return rows.filter(
    (p) =>
      (f.space === null || p.space === f.space) &&
      statusMatches(p, f.status) &&
      computerMatches(p, f.computer) &&
      (!f.ready || p.readyToArchive === true) &&
      matches(p, f.query),
  );
}

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

export function sortProjects(rows: ProjectRow[], sort: Sort): ProjectRow[] {
  const value = (p: ProjectRow): string => {
    switch (sort.key) {
      case "code":
        return p.jobCode;
      case "title":
        return p.title;
      case "client":
        return p.client.name || p.client.code;
      case "created":
        return p.createdAt;
    }
  };
  const sign = sort.dir === "asc" ? 1 : -1;
  return [...rows].sort((a, b) => sign * collator.compare(value(a), value(b)) || collator.compare(a.key, b.key));
}

/** "28 Sep 2026" from an RFC 3339 timestamp; empty if missing. */
export function formatDate(iso: string): string {
  const d = new Date(iso);
  if (!iso || Number.isNaN(d.getTime())) return "";
  return d.toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" });
}

/** "just now", "5 min ago", "3 h ago", "2 days ago" from an RFC 3339 time. A time in the
 *  future (another computer's clock is ahead) reads "just now". Empty if unknown. */
export function ago(iso: string, now: number = Date.now()): string {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return "";
  const min = Math.floor((now - t) / 60000);
  if (min < 1) return "just now";
  if (min < 60) return `${min} min ago`;
  const h = Math.floor(min / 60);
  if (h < 24) return `${h} h ago`;
  const d = Math.floor(h / 24);
  return d === 1 ? "1 day ago" : `${d} days ago`;
}

/** Devices: a computer's list counts as fresh for 24 hours. */
export const isFresh = (iso: string, now: number = Date.now()) => {
  const t = Date.parse(iso);
  return !Number.isNaN(t) && now - t < 24 * 3600 * 1000;
};
