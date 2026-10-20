// Plain words for Resolve starter projects (template editor, New Project, details).
import type { PlannedResolve, ProjectResolve, ResolveLibrary } from "./types";

/** "17 Oct" in this computer's language (empty if the time can't be read). */
export function shortDay(iso: string): string {
  const d = new Date(iso);
  if (!iso || Number.isNaN(d.getTime())) return "";
  return d.toLocaleDateString(undefined, { day: "numeric", month: "short" });
}

/** New Project's preview: "Adds “261020_TEST” to DaVinci Resolve, in Client Work › Acme › 2026".
 *  The library is named only when the template picks one; no folder = no "in …". */
export function plannedLine(r: PlannedResolve): string {
  const where = [...(r.library ? [r.library.dbName] : []), ...r.folder.split("/").filter(Boolean)];
  return `Adds “${r.name}” to DaVinci Resolve${where.length ? `, in ${where.join(" › ")}` : ""}`;
}

/** The details panel: "added 17 Oct", "waiting for Resolve", "already in Resolve"… */
export function projectStatusWords(r: ProjectResolve): string {
  switch (r.status) {
    case "added": {
      const day = shortDay(r.at);
      return day ? `added ${day}` : "added";
    }
    case "waiting":
      return "waiting for Resolve";
    case "exists":
      return "already in Resolve";
    case "skipped":
      return "not added (taken off the waiting list)";
    case "failed":
      return "not added (Nest stopped trying)";
    default:
      return "";
  }
}

/** "Resolve project: Grade · added 17 Oct". */
export function projectLine(r: ProjectResolve): string {
  const words = projectStatusWords(r);
  return `Resolve project: ${r.name}${words ? ` · ${words}` : ""}`;
}

/** One string per library (for a <select> value). "" = the library open in Resolve. */
export function libraryKey(l: ResolveLibrary | null): string {
  return l ? `${l.dbType}\n${l.dbName}` : "";
}

export function sameLibrary(a: ResolveLibrary | null, b: ResolveLibrary | null): boolean {
  return libraryKey(a) === libraryKey(b);
}

/** The notice Rust sends when a project went into Resolve (`resolve::after`). */
export const addedNotice = (name: string) => `Added to Resolve: ${name}`;

/** The success line after Create: "Created VX-L01", plus where its Resolve project is
 *  (`resolve`: null = none; "waiting" until the first try's notice says it was added). */
export function createdMessage(jobCode: string, bumped: boolean, resolve: null | "waiting" | "added"): string {
  const created = bumped ? `Created ${jobCode} (the previewed code was taken)` : `Created ${jobCode}`;
  if (resolve === "added") return `${created} · Added to Resolve`;
  if (resolve === "waiting") return `${created} · Ready when Resolve opens`;
  return created;
}

/** A notice arrived. If it says the just-created project went into Resolve while its success
 *  line is still showing, that line says so instead of a second toast. Otherwise: the notice.
 *  `T` is the toast shape ({ message, … }). */
export function toastForNotice<T extends { message: string }>(
  current: T | null,
  pending: { toast: T; name: string; added: T } | null,
  notice: string,
): T | { message: string } {
  if (pending && current === pending.toast && notice === addedNotice(pending.name)) return pending.added;
  return { message: notice };
}
