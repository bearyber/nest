// Plain words for Resolve starter projects (New Project, details, Settings).
import type { PlannedResolve, ProjectResolve, ResolveLibrary, ResolveStatus } from "./types";

/** "17 Oct" in this computer's language (empty if the time can't be read). */
export function shortDay(iso: string): string {
  const d = new Date(iso);
  if (!iso || Number.isNaN(d.getTime())) return "";
  return d.toLocaleDateString(undefined, { day: "numeric", month: "short" });
}

/** New Project's preview: "Resolve project: Grade in Acme / 2026" ("" folder = the top level). */
export function plannedLine(r: PlannedResolve): string {
  const folder = r.folder ? r.folder.split("/").join(" / ") : "the top level";
  return `Resolve project: ${r.name} in ${folder}`;
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

/** The waiting list's "last result" (Rust `Outcome::key`). */
export function outcomeWords(key: string | null): string {
  switch (key) {
    case null:
    case "":
      return "Not tried yet";
    case "added":
      return "Added";
    case "exists":
      return "Already in Resolve";
    case "busy":
      return "A project was open in another library";
    case "noFolder":
      return "Couldn't make the folder in Resolve";
    case "noLibrary":
      return "The library isn't in Resolve";
    case "missing":
      return "Nest's copy is gone";
    case "scriptingOff":
      return "Resolve didn't let Nest in";
    case "notInstalled":
      return "Resolve isn't installed";
    case "notRunning":
      return "Resolve wasn't open";
    case "failed":
      return "Resolve didn't do it";
    default:
      return key;
  }
}

/** Settings' status line. `ok` = the ✓ look. */
export function statusLine(s: ResolveStatus): { text: string; ok: boolean } {
  switch (s) {
    case "ready":
      return { text: "✓ Resolve is ready", ok: true };
    case "scriptingOff":
      return {
        text:
          "Resolve isn't letting Nest in. In Resolve, open Preferences → System → General and set " +
          "“External scripting using” to Local. If Resolve has just opened, wait a few seconds.",
        ok: false,
      };
    case "notRunning":
      return { text: "Open Resolve to check that Nest can reach it.", ok: false };
    case "notInstalled":
      return { text: "DaVinci Resolve isn't installed on this computer.", ok: false };
  }
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
