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
 *  when not; `each` picked option, with `{item}` in the name). `kit`: the client's kit is copied
 *  into this folder (one per template, never with `when` / `each`). */
export type TreeEntry = string | { name: string; when?: string; each?: string; kit?: boolean };

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
  /** Resolve starter project (Rust `ResolveStarter`); absent = none. */
  resolve?: ResolveStarter;
}

/** A Resolve starter project in a template: Create adds a copy of `drp` to DaVinci Resolve. */
export interface ResolveStarter {
  /** The `.drp` in the template folder, e.g. `files/Grade.drp`. */
  drp: string;
  /** The project's name in Resolve, with blanks; absent = the same as the folder name. */
  name?: string;
  /** Resolve's project folder, with blanks; `/` = folders inside folders; "" = none.
   *  Rust fills in "{client}" when absent. */
  folder: string;
  /** Ticked in New Project by default (Rust fills in true when absent). */
  on: boolean;
  /** The Resolve library it goes into; absent = the library open in Resolve. A library this
   *  computer's Resolve doesn't have: the open one instead (the project's note says so). */
  library?: ResolveLibrary;
}

/** A Resolve project library (Resolve calls it a database), exactly as Resolve names it. */
export interface ResolveLibrary {
  dbType: string;
  dbName: string;
  /** A network (PostgreSQL) library's server address, as Resolve reports it; keep it when
   *  passing a library back (absent for disk libraries). */
  ipAddress?: string | null;
}

/** The Resolve project Create will add (Rust `PlannedResolve`). Preview line:
 *  "Resolve project: <name> in <folder>". */
export interface PlannedResolve {
  drp: string;
  name: string;
  /** `/` between folders inside folders; "" = the top level. */
  folder: string;
  /** The template's library; null = the one open in Resolve. */
  library: ResolveLibrary | null;
  /** The library's name, or "the library open in Resolve". */
  libraryLabel: string;
}

/** ready · starting (Resolve opened moments ago) · scriptingOff (Resolve runs but External
 *  scripting isn't Local) · noAnswer (didn't answer in time, may be busy) · noHelper (Nest
 *  couldn't run Resolve's script runner) · notRunning · notInstalled. */
export type ResolveStatus =
  | "ready"
  | "starting"
  | "scriptingOff"
  | "noAnswer"
  | "noHelper"
  | "notRunning"
  | "notInstalled";

/** Resolve's libraries (Rust `resolve::Libraries`). */
export interface ResolveLibraries {
  /** The library open in Resolve now. */
  current: ResolveLibrary | null;
  libraries: ResolveLibrary[];
}

/** A project in Resolve's open library ("Choose from Resolve…"). */
export interface ResolveProject {
  /** `/` between folders inside folders; "" = the top level. */
  folder: string;
  name: string;
}

/** A project's Resolve project from its `.project.json` (Rust `resolve::ProjectResolve`). */
export interface ProjectResolve {
  name: string;
  /** `/` between folders inside folders; "" = the top level. */
  folder: string;
  /** The library's name; null = the one open in Resolve. */
  library: string | null;
  /** waiting · added · exists · skipped · failed. */
  status: string;
  /** RFC 3339 of the last change; "" when unknown. */
  at: string;
  /** The project file's note (it went into another library, why Nest stopped…); null = none. */
  note: string | null;
  /** On this computer's waiting list (Try now / Don't add apply). */
  waiting: boolean;
  /** What stops it going in right now, in plain words (only while it waits); null = nothing. */
  problem: string | null;
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
  /** v0.5 client kits; absent = no client, or no kit and no kit folder. */
  kit?: PlanKit;
  /** The Resolve project Create adds; absent = none, or the box is off. */
  resolve?: PlannedResolve;
}

/** ready: copied on Create · missing: the template has a kit folder, the client no kit here ·
 *  noPlace: the client has a kit, the template no kit folder · unreadable: created without it. */
export type KitState = "ready" | "missing" | "noPlace" | "unreadable";

/** The client kit in a Plan (Rust `PlanKit`): a summary, not every file. */
export interface PlanKit {
  state: KitState;
  /** The client's name and code. */
  client: string;
  code: string;
  /** The kit folder on this computer. */
  source?: string;
  /** Where it goes, relative to the project folder. */
  folder: string;
  count: number;
  bytes: number;
  /** Sent back with Create, which refuses if the kit changed since. */
  fingerprint?: string;
  /** The first few files' paths in the project (the rest are counted, not listed). */
  sample: string[];
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
  /** Create only: the preview's `plan.kit.fingerprint` (null = no kit in the preview). */
  kitFingerprint?: string | null;
  /** "Add to Resolve" box; null/absent = the template's default. */
  resolve?: boolean | null;
}

/** Progress while Create copies a client kit (event `create-progress`). */
export interface CreateProgress {
  done: number;
  total: number;
}

/** Settings → Client kits (Rust `KitsView`). */
export interface KitsView {
  kitsFolder: string | null;
  kitsFolderMissing: boolean;
  clients: KitClient[];
}

export interface KitClient {
  name: string;
  code: string;
  /** The kit Nest copies for this client on this computer; null = none. */
  kit: string | null;
  /** chosen: a folder picked for this client · kitsFolder: found in the Kits folder. */
  source: "chosen" | "kitsFolder" | null;
  /** The folder chosen for this client can't be found. */
  missing: boolean;
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
  /** Folder size in bytes from the last measurement; missing = not measured yet. */
  size?: number;
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
  /** v0.7: keep running with an icon in the menu bar / notification area when the window closes. */
  showInMenuBar: boolean;
  /** v0.7: the one-time "Nest is still running" note was shown. */
  closeNoteShown: boolean;
  /** Archive (M5): null = off. */
  archiveFolder: string | null;
  /** Suggest archiving projects done for more than this many days; null = no suggestions. */
  archiveAfterDays: number | null;
  /** v0.4.2: where Personal-space and "No client" projects go; null = the first jobs folder. */
  personalRoot: string | null;
  /** Archive folders used before, newest first. */
  pastArchiveFolders: string[];
  /** v0.5: keep your templates the same on all your computers (needs Devices). */
  syncTemplates: boolean;
  /** v0.5 client kits: a folder with one folder per client (by code or name); null = none. */
  kitsFolder: string | null;
  /** v0.5 client kits: a kit folder chosen for one client, by client code (wins over kitsFolder). */
  clientKits: Record<string, string>;
  /** v0.7: the note after your very first project was shown (once). */
  firstProjectTipSeen: boolean;
}

export interface SettingsView {
  settings: Settings;
  settingsError: string | null;
  firstRun: boolean;
  templatesFolder: string;
  devBuild: boolean;
  /** First run suggests this job-code letter (W on Windows, M on a Mac). */
  suggestedMarker: string;
  /** Opening at login as the system has it now (mirrors login_item.rs `LoginItem`). */
  loginItem: LoginItem;
}

/** "on" / "off"; "needsApproval" = Mac, allow it in System Settings; "moveFirst" = Mac, Nest
 *  runs from the disk image or Downloads; "devBuild" = only in the installed app. */
export type LoginItem = "off" | "on" | "needsApproval" | "moveFirst" | "devBuild";

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
  /** How many folders a new project gets (Templates list). */
  folders: number;
  /** It has a Resolve starter project (Settings → DaVinci Resolve). */
  resolve?: boolean;
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

/** Why something in a folder didn't come along into a template (Rust `SkipReason`). */
/** Mirrors `template_io::DropKind`: what something dropped on the Templates screen is. */
export type DropKind = "folder" | "template" | "other";

export type SkipReason = "clutter" | "cache" | "tooBig" | "link" | "unreadable" | "badName";

export interface Skipped {
  /** `/`-separated, relative to the folder. */
  path: string;
  reason: SkipReason;
}

/** An open editor (Rust `EditorOpened`): `key` goes to preview / save / add files. */
export interface EditorOpened {
  key: string;
  draft: TemplateDraft;
}

/** Files added in the editor (Rust `AddedFiles`): new folders (template paths) and file entries. */
export interface AddedFiles {
  folders: string[];
  files: FileEntry[];
  skipped: Skipped[];
}

/** A template read from a folder (Rust `FolderDraft`). `key` goes to preview / save. */
export interface FolderDraft {
  key: string;
  draft: TemplateDraft;
  skipped: Skipped[];
  /** The folder was a Nest project: its answers became blanks. */
  fromProject: boolean;
}

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
  /** v0.5: keep my templates the same on all my computers. */
  syncTemplates: boolean;
  /** Plain words when template sync couldn't run. */
  templateProblem: string | null;
}

/** The "What's new" note shown once after an update (whats_new.rs). */
export interface WhatsNew {
  version: string;
  lines: string[];
}

/** Up to `max` plain-text lines from an update's release notes: leading "- " / "* " removed. */
export function updateNoteLines(notes: string | null | undefined, max = 4): string[] {
  return (notes ?? "")
    .split(/\r?\n/)
    .map((l) => l.trim().replace(/^[-*]\s+/, "").trim())
    .filter((l) => l.length > 0)
    .slice(0, max);
}
