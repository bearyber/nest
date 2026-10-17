// Typed wrappers around Tauri commands. The UI never calls `invoke` directly.
// Project actions take a project key; Rust looks up the folder itself.
import { invoke } from "@tauri-apps/api/core";
import type {
  AddedFiles,
  DropKind,
  Created,
  KitsView,
  NewProjectContext,
  Plan,
  PlanRequest,
  ProjectList,
  ProjectResolve,
  ResolveLibraries,
  ResolveLibrary,
  ResolveProject,
  ResolveStatus,
  RescanSummary,
  UpdateInfo,
  WhatsNew,
  FolderInfo,
  Settings,
  SettingsView,
  Sizes,
  DevicesView,
  EditorOpened,
  FolderDraft,
  SaveMode,
  Saved,
  SyncedFolder,
  Template,
  TemplateDraft,
  TemplateInfo,
  TemplatePreview,
} from "./types";

export const materialApplied = () => invoke<boolean>("material_applied");

export const newProjectContext = () => invoke<NewProjectContext>("new_project_context");

export const planProject = (request: PlanRequest) => invoke<Plan>("plan_project", { request });

export const createProject = (request: PlanRequest) =>
  invoke<Created>("create_project", { request });

/** Stop a running Create; it undoes what it made and rejects with "Create was cancelled…". */
export const cancelCreate = () => invoke<void>("cancel_create");

// ── Client kits (v0.5) ──
export const kitsView = () => invoke<KitsView>("kits_view");
/** Folder picker for the Kits folder; null when cancelled. */
export const chooseKitsFolder = () => invoke<SettingsView | null>("choose_kits_folder");
export const clearKitsFolder = () => invoke<SettingsView>("clear_kits_folder");
/** Folder picker for one client's kit; null when cancelled. */
export const chooseClientKit = (code: string, name: string) =>
  invoke<SettingsView | null>("choose_client_kit", { code, name });
/** Use the Kits folder again for this client. */
export const clearClientKit = (code: string) => invoke<SettingsView>("clear_client_kit", { code });
/** New Project's "Put them in": the template keeps `folder` (a template tree path) as the place
 *  for client files. Saved as its next version (a built-in becomes yours); `templateKey` is a
 *  template key or the id New Project offers. */
export const setTemplateKitFolder = (templateKey: string, folder: string) =>
  invoke<void>("set_template_kit_folder", { templateKey, folder });

/** Native folder picker; resolves to the new jobs folder, or null if cancelled. */
export const chooseJobsRoot = () => invoke<string | null>("choose_jobs_root");

/** The indexed project list (instant, no disk access). */
export const listProjects = () => invoke<ProjectList>("list_projects");

/** Walk the jobs folders and refresh the index. */
export const rescanProjects = () => invoke<RescanSummary>("rescan_projects");

export const openProject = (key: string) => invoke<void>("open_project", { key });

export const revealProject = (key: string) => invoke<void>("reveal_project", { key });
/** Folder sizes: the last measurement (null = never measured). */
export const projectSizes = (key: string) => invoke<Sizes | null>("project_sizes", { key });
/** Counts every file in the project folder now; a few seconds for big footage folders. */
export const measureProject = (key: string) => invoke<Sizes>("measure_project", { key });

export const setStatus = (key: string, status: "active" | "done") =>
  invoke<void>("set_status", { key, status });

export const copyText = (text: string) => invoke<void>("copy_text", { text });

/** An update found by the background check (release builds only), or null. */
export const pendingUpdate = () => invoke<UpdateInfo | null>("pending_update");

/** Ask for a newer version now; null = already up to date. */
export const checkForUpdates = () => invoke<UpdateInfo | null>("check_for_updates");

/** Download, verify, install and restart Nest. */
export const installUpdate = () => invoke<void>("install_update");

// ── Settings window + first run ──
const s = <T,>(cmd: string, args?: Record<string, unknown>) => invoke<T>(cmd, args);

export const getSettings = () => s<SettingsView>("get_settings");
export const openSettings = () => s<void>("open_settings");
export const addJobsRoot = () => s<SettingsView | null>("add_jobs_root");
export const removeJobsRoot = (path: string) => s<SettingsView>("remove_jobs_root", { path });
export const moveJobsRoot = (path: string, to: number) => s<SettingsView>("move_jobs_root", { path, to });
export const setDefaultSpace = (name: string) => s<SettingsView>("set_default_space", { name });
export const setLaunchAtLogin = (on: boolean) => s<SettingsView>("set_launch_at_login", { on });
/** v0.7: the menu bar (macOS) / notification area (Windows) icon. */
export const setShowInMenuBar = (on: boolean) => s<SettingsView>("set_show_in_menu_bar", { on });
/** v0.7: tells Rust while the New Project sheet is open (an update never installs then). */
export const setNewProjectOpen = (open: boolean) => s<void>("set_new_project_open", { open });
/** Opens the folder picker; null when cancelled. */
export const setArchiveFolder = () => s<SettingsView | null>("set_archive_folder");
/** Rust asks first if the archive folder holds projects; null = cancelled. */
export const clearArchiveFolder = () => s<SettingsView | null>("clear_archive_folder");
export const reusePastArchiveFolder = (path: string) => s<SettingsView | null>("use_past_archive_folder", { path });
export const forgetPastArchiveFolder = (path: string) => s<SettingsView>("forget_past_archive_folder", { path });
export const setArchiveAfterDays = (days: number | null) => s<SettingsView>("set_archive_after_days", { days });
/** Where Personal-space and "No client" projects go (one of the jobs folders; null = the first). */
export const setPersonalRoot = (path: string | null) => s<SettingsView>("set_personal_root", { path });
/** New Project's Change… while it shows the Personal folder. Null when cancelled. */
export const choosePersonalRoot = () => s<SettingsView | null>("choose_personal_root");
/** Open a folder inside a project in Finder/Explorer. `path` = folder names from the project
 *  down ([] = the project folder). */
export const openProjectSubfolder = (key: string, path: string[]) =>
  invoke<void>("open_project_subfolder", { key, path });
/** What's inside one folder of a project, one level down (for ▸ under Size). */
export const measureProjectFolder = (key: string, path: string[]) =>
  invoke<Sizes>("measure_project_folder", { key, path });
/** Move a Done project into the archive folder (or back). Rust asks first; null = cancelled. */
export const archiveProject = (key: string, unarchive: boolean) =>
  s<string | null>("archive_project", { key, unarchive });
/** "Not this one": never suggest archiving this project again. */
export const dismissArchiveSuggestion = (key: string) => s<void>("dismiss_archive_suggestion", { key });
export const setCodeSettings = (pattern: string, marker: string, ownCode: string) =>
  s<SettingsView>("set_code_settings", { pattern, marker, ownCode });
export const addSpace = (name: string) => s<SettingsView>("add_space", { name });
export const renameSpace = (old: string, next: string) => s<SettingsView>("rename_space", { old, new: next });
export const deleteSpace = (name: string) => s<SettingsView>("delete_space", { name });
export const moveSpace = (name: string, to: number) => s<SettingsView>("move_space", { name, to });
export const setSpaceNoClient = (name: string, on: boolean) => s<SettingsView>("set_space_no_client", { name, on });
export const spacesInUse = () => s<string[]>("spaces_in_use");
export const setTheme = (theme: Settings["theme"]) => s<SettingsView>("set_theme", { theme });
export const listTemplates = () => s<TemplateInfo[]>("list_templates");
export const importTemplate = () => s<TemplateInfo | null>("import_template");
/** Install a dropped `.nesttemplate` / `.json` (same checks and messages as Import…). */
export const importTemplateFrom = (path: string) => s<TemplateInfo | null>("import_template_from", { path });
/** What each dropped path is: a folder, a template file, or neither. */
export const dropKinds = (paths: string[]) => s<DropKind[]>("drop_kinds", { paths });
export const exportTemplate = (key: string) => s<boolean>("export_template", { key });
export const duplicateTemplate = (key: string) => s<void>("duplicate_template", { key });
export const deleteTemplate = (key: string) => s<boolean>("delete_template", { key });
/** Templates list → Remove: hides one of Nest's, or moves yours to the bin (after a confirm). `false` if cancelled. */
export const removeTemplate = (key: string) => s<boolean>("remove_template", { key });
/** Bring back Nest's templates: shows every hidden one of Nest's again. */
export const restoreBuiltinTemplates = () => s<SettingsView>("restore_builtin_templates");
export const setTemplateHidden = (id: string, hidden: boolean) => s<SettingsView>("set_template_hidden", { id, hidden });
export const revealTemplatesFolder = () => s<void>("reveal_templates_folder");
export const resetTemplate = (key: string) => s<boolean>("reset_template", { key });
export const templateDraft = (key: string) => s<TemplateDraft>("template_draft", { key });
export const previewTemplate = (key: string, draft: Template) =>
  s<TemplatePreview>("preview_template", { key, draft });
/** The editor's names (single folder / file names, as typed) as the example project would have
 *  them: the preview's example answers. Names that can't be written come back as typed. */
export const exampleNames = (key: string, draft: Template, names: string[]) =>
  s<string[]>("example_names", { key, draft, names });
export const saveTemplate =(key: string, draft: Template, mode: SaveMode) =>
  s<Saved>("save_template", { key, draft, mode });
/** Make a template from a folder (`null` = pick one). `null` back if cancelled. */
export const templateFromFolder = (path: string | null) =>
  s<FolderDraft | null>("template_from_folder", { path });
/** Open a template in the editor: a session key (for preview, save, adding files) + the draft. */
export const editorOpen = (key: string) => s<EditorOpened>("editor_open", { key });
/** Add files dropped on (or picked for, when `paths` is null) the template folder `into`. */
export const editorAddFiles = (key: string, into: string, paths: string[] | null) =>
  s<AddedFiles | null>("editor_add_files", { key, into, paths });
/** Add a small ready-made text file (e.g. "Project info.txt") into the template folder `into`. */
export const editorAddTextFile = (key: string, into: string, name: string, contents: string) =>
  s<AddedFiles>("editor_add_text_file", { key, into, name, contents });
export const editorClose = (key: string) => s<void>("editor_close", { key });
/** Show the main window's Templates view (from Settings). */
export const openTemplates = () => s<void>("open_templates");
export const takeNotice = () => invoke<string | null>("take_notice");
export const devicesView = () => s<DevicesView>("devices_view");
export const devicesDetect = () => invoke<SyncedFolder[]>("devices_detect");
export const devicesPickFolder = () => s<string | null>("devices_pick_folder");
export const devicesEnable = (name: string, folder: string) => s<DevicesView>("devices_enable", { name, folder });
export const devicesDisable = () => s<DevicesView>("devices_disable");
export const devicesForget = (deviceId: string, forget: boolean) =>
  s<DevicesView>("devices_forget", { deviceId, forget });
export const devicesClaim = (deviceId: string) => s<DevicesView>("devices_claim", { deviceId });
export const devicesRefresh = () => invoke<void>("devices_refresh");
export const devicesSetSyncTemplates = (on: boolean) => s<DevicesView>("devices_set_sync_templates", { on });
export const openLogsFolder = () => s<void>("open_logs_folder");
export const firstRunPickFolder = () => s<FolderInfo | null>("first_run_pick_folder");
export const firstRunCreateFolder = () => s<FolderInfo>("first_run_create_folder");
export const completeFirstRun = (
  jobsRoot: string,
  spaces: string[],
  noClientSpaces: string[],
  hiddenTemplates: string[],
  marker: string,
  firstTemplate: string | null,
) => s<SettingsView>("complete_first_run", { jobsRoot, spaces, noClientSpaces, hiddenTemplates, marker, firstTemplate });
/** First run: pick the folder you copy for every job. `null` if cancelled. */
export const firstRunPickTemplateFolder = () => s<string | null>("first_run_pick_template_folder");
export const settingsReady = () => invoke<void>("settings_ready");
export const requestStatus = (key: string, status: "active" | "done") => invoke<void>("request_status", { key, status });

// ── Resolve starter projects ──
/** Is Resolve installed, open, and letting Nest in? */
export const resolveStatus = () => invoke<ResolveStatus>("resolve_status");
/** Resolve's libraries and the open one (rejects with plain words if Resolve can't answer). */
export const resolveLibraries = () => invoke<ResolveLibraries>("resolve_libraries");
/** The projects in one of Resolve's libraries ("Choose from Resolve…"); null = the open one.
 *  Another library only when no real project is open in Resolve (else it rejects with what
 *  to do); Resolve is always switched back. */
export const resolveProjects = (library: ResolveLibrary | null = null) =>
  invoke<ResolveProject[]>("resolve_projects", { library });
/** Export a project from Resolve into the open editor; resolves to the template path for
 *  `template.resolve.drp`. `folder` as in `ResolveProject` ("" = the top level); `library` as
 *  in `resolveProjects`. */
export const resolveExportToEditor = (
  sessionId: string,
  projectName: string,
  folder: string,
  library: ResolveLibrary | null = null,
) => invoke<string>("resolve_export_to_editor", { sessionId, projectName, folder, library });
/** A `.drp` dropped on the template editor becomes its Resolve project; resolves to the
 *  template path for `template.resolve.drp`. */
export const editorAddResolveDrp = (sessionId: string, path: string) =>
  invoke<string>("editor_add_resolve_drp", { sessionId, path });
/** The details panel: a project's Resolve project (null = none, or it can't be read here). */
export const projectResolve = (key: string) => invoke<ProjectResolve | null>("project_resolve", { key });
/** Try the waiting projects now (can take a while if Resolve is slow). */
export const resolveTryNow = () => invoke<void>("resolve_try_now");
/** "Don't add": take this project off the waiting list (its file then says "skipped"). */
export const resolveSkipProject = (key: string) => invoke<void>("resolve_skip_project", { key });

// ── What's new ──
/** The note to show after an update, or null. */
export const whatsNew = () => invoke<WhatsNew | null>("whats_new");

/** OK pressed on the note: remember this version has been seen. */
export const whatsNewSeen = () => invoke<void>("whats_new_seen");

/** The note after your very first project was shown: never show it again. */
export const firstProjectTipSeen = () => invoke<void>("first_project_tip_seen");
