// Mirrors of the Rust types (serde, camelCase). Update together with src-tauri/src.

export type FieldKind = "text" | "longtext" | "bool" | "select" | "multi" | "date" | "client";

export interface Field {
  key: string;
  label: string;
  type: FieldKind;
  required?: boolean;
  max?: number;
  options?: string[];
  default?: FieldValue;
}

/** A folder in a template: a plain `/` path, or one with rules (`when` a box is ticked, `!key`
 *  when not; `each` picked option, with `{item}` in the name). */
export type TreeEntry = string | { name: string; when?: string; each?: string };

export interface FileEntry {
  from: string;
  to: string;
  fill?: boolean;
}

export interface Template {
  schema: number;
  id: string;
  version: number;
  name: string;
  description: string;
  title?: string;
  fields: Field[];
  folderName: string;
  tree?: TreeEntry[];
  files?: FileEntry[];
}

export interface ClientValue {
  name: string;
  code: string;
}

export type FieldValue = boolean | string[] | ClientValue | string;
export type Values = Record<string, FieldValue>;

export interface Issue {
  level: "error" | "warning";
  field?: string;
  message: string;
}

export interface PlannedFile {
  from: string;
  to: string;
  fill: boolean;
}

export interface Plan {
  jobCode: string;
  folderName: string;
  title: string;
  root: string;
  folders: string[];
  files: PlannedFile[];
  manifest: unknown;
  fill: Record<string, string>;
  issues: Issue[];
}

export interface NewProjectContext {
  templates: Template[];
  spaces: string[];
  /** Spaces where the Client field is hidden (e.g. Personal). */
  noClientSpaces: string[];
  defaultSpace: string;
  lastTemplate: string | null;
  /** null until a jobs folder is chosen. */
  jobsRoot: string | null;
  rootMissing: boolean;
  /** v0.4.2: the Personal folder, when it's separate from `jobsRoot`. */
  personalRoot: string | null;
  personalMissing: boolean;
  clients: ClientValue[];
  settingsError: string | null;
}

export interface PlanRequest {
  templateId: string;
  values: Values;
  space: string;
  codeOverride: string | null;
  /** "No client (personal or passion project)" ticked. */
  noClient?: boolean;
}

export interface Created {
  /** The new project's key in the list (its manifest id). */
  key: string;
  jobCode: string;
  title: string;
  root: string;
  bumped: boolean;
}

/** One project in the list (Rust `index::ProjectRow`). */
export interface ProjectRow {
  /** Manifest id, or `path:<folder>` when the manifest can't be read. */
  key: string;
  id: string | null;
  jobCode: string;
  title: string;
  client: ClientValue;
  space: string;
  status: string;
  templateId: string;
  createdAt: string;
  fields: Record<string, unknown> | null;
  /** Why the manifest can't be read (null when fine). */
  error: string | null;
  /** More than one = the same project exists in two places. */
  paths: string[];
  offline: boolean;
  duplicateCode: boolean;
  /** Devices: set when the project lives on another computer (greyed, read-only). */
  device?: RemoteDevice;
  /** Devices: other computers that also list this project. */
  alsoOn?: string[];
  /** The template's name ("Grading: PV"). */
  templateName: string;
  /** Devices: a status asked of the owning computer, still waiting. */
  pending?: "active" | "done";
  /** Devices: why the owning computer refused this computer's request. */
  requestProblem?: string;
  /** Devices: this project's status was last changed from another computer. */
  changedBy?: { name: string; status: string; at: string };
  /** Archive: its folder is in the archive folder (read-only in Nest, behind the Archived filter). */
  archived: boolean;
  /** Done since, ms since 1970 (absent while active). */
  doneAt?: number;
  /** Archive: done for longer than Settings' "suggest after N days", and not dismissed. */
  readyToArchive?: boolean;
}

/** Where a project from another computer lives (Rust `RemoteDevice`). */
export interface RemoteDevice {
  deviceId: string;
  name: string;
  /** RFC 3339; display only. */
  updatedAt: string;
  /** The folder on that computer, as text. */
  path: string;
}

/** Folder sizes (Rust `sizes::Sizes`). Bytes. */
export interface Sizes {
  total: number;
  files: number;
  /** Biggest first; the entry with an empty name is loose files in the project folder. */
  folders: FolderSize[];
  unreadable: number;
  /** ms since 1970. */
  measuredAt: number;
}

export interface FolderSize {
  name: string;
  total: number;
  video: number;
  audio: number;
  images: number;
  other: number;
}

export interface RootState {
  path: string;
  online: boolean;
}

export interface ProjectList {
  projects: ProjectRow[];
  roots: RootState[];
  /** ms since 1970 of the last finished rescan, null before the first. */
  scannedAt: number | null;
}

export interface UpdateInfo {
  version: string;
  notes: string | null;
}

export interface RescanSummary {
  found: number;
  removed: number;
  duplicates: string[];
  offline: string[];
}

/** Rust commands reject with `{ message }` (AppError). */
export function errorMessage(e: unknown): string {
  if (typeof e === "object" && e !== null && "message" in e) {
    return String((e as { message: unknown }).message);
  }
  return String(e);
}

export function canCreate(plan: Plan | null): boolean {
  return plan !== null && !plan.issues.some((i) => i.level === "error");
}

/** Rust `settings::Settings` (camelCase). Unknown keys may also be present. */
export interface Settings {
  jobsRoots: string[];
  spaces: string[];
  defaultSpace: string;
  codePattern: string;
  localMarker: string;
  lastTemplate: string | null;
  noClientSpaces: string[];
  ownCode: string;
  hiddenTemplates: string[];
  theme: "system" | "light" | "dark";
  launchAtLogin: boolean;
  /** Archive (M5): null = off. */
  archiveFolder: string | null;
  /** Suggest archiving projects done for more than this many days; null = no suggestions. */
  archiveAfterDays: number | null;
  /** v0.4.2: where Personal-space and "No client" projects go; null = the first jobs folder. */
  personalRoot: string | null;
}

export interface SettingsView {
  settings: Settings;
  settingsError: string | null;
  firstRun: boolean;
  templatesFolder: string;
  devBuild: boolean;
  /** First run suggests this job-code letter (W on Windows, M on a Mac). */
  suggestedMarker: string;
}

export interface TemplateInfo {
  /** `user:<folder>` or `builtin:<folder>`. */
  key: string;
  id: string;
  name: string;
  version: number;
  description: string;
  builtIn: boolean;
  /** An older version of one of yours (T1), or a built-in yours replaces: kept, not offered. */
  superseded: boolean;
  /** A built-in that one of yours (same id) replaces: listed as the original. */
  replaced: boolean;
  hidden: boolean;
  folder: string;
}

export interface FolderInfo {
  path: string;
  /** Nest projects already in it. */
  projects: number;
}

/** What the template editor opens (Rust `TemplateDraft`). */
export interface TemplateDraft {
  template: Template;
  builtIn: boolean;
  replaced: boolean;
  /** Starter files that come along (read-only in the editor). */
  starterFiles: string[];
}

/** What New Project would make from a draft, with example answers (Rust `TemplatePreview`). */
export interface TemplatePreview {
  folderName: string;
  /** The job code the example project would get, in this computer's format. */
  jobCode: string;
  title: string;
  folders: string[];
  files: string[];
  /** Plain-language reasons the draft can't be saved yet. */
  problems: string[];
}

/** edit = one of yours (next version); customize = a built-in (yours replaces it); new = new id. */
export type SaveMode = "edit" | "customize" | "new";

/** A saved template and what happened to its previous copy (decision T-A). */
export interface Saved {
  info: TemplateInfo;
  /** Older copies moved to the Recycle Bin / Trash. */
  binned: number;
  binProblem: string | null;
}

/** A synced folder Nest found (Google Drive). */
export interface SyncedFolder {
  label: string;
  path: string;
}

export interface OtherComputer {
  deviceId: string;
  name: string;
  os: string;
  updatedAt: string;
  projectCount: number;
  problem: string | null;
  notDownloaded: boolean;
}

/** Settings → Devices (Rust `DevicesView`). */
export interface DevicesView {
  enabled: boolean;
  deviceId: string | null;
  name: string;
  folder: string | null;
  savedAt: string | null;
  problem: string | null;
  thisProjectCount: number;
  others: OtherComputer[];
  forgotten: OtherComputer[];
}
