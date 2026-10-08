// Typed wrappers around Tauri commands. The UI never calls `invoke` directly.
// Project actions take a project key; Rust looks up the folder itself.
import { invoke } from "@tauri-apps/api/core";
import type {
  Created,
  NewProjectContext,
  Plan,
  PlanRequest,
  ProjectList,
  RescanSummary,
  UpdateInfo,
  FolderInfo,
  Settings,
  SettingsView,
  Sizes,
  DevicesView,
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
/** Opens the folder picker; null when cancelled. */
export const setArchiveFolder = () => s<SettingsView | null>("set_archive_folder");
export const clearArchiveFolder = () => s<SettingsView>("clear_archive_folder");
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
export const exportTemplate = (key: string) => s<boolean>("export_template", { key });
export const duplicateTemplate = (key: string) => s<void>("duplicate_template", { key });
export const deleteTemplate = (key: string) => s<boolean>("delete_template", { key });
export const setTemplateHidden = (id: string, hidden: boolean) => s<SettingsView>("set_template_hidden", { id, hidden });
export const revealTemplatesFolder = () => s<void>("reveal_templates_folder");
export const resetTemplate = (key: string) => s<boolean>("reset_template", { key });
export const templateDraft = (key: string) => s<TemplateDraft>("template_draft", { key });
export const previewTemplate = (key: string, draft: Template) =>
  s<TemplatePreview>("preview_template", { key, draft });
export const saveTemplate = (key: string, draft: Template, mode: SaveMode) =>
  s<Saved>("save_template", { key, draft, mode });
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
export const openLogsFolder = () => s<void>("open_logs_folder");
export const firstRunPickFolder = () => s<FolderInfo | null>("first_run_pick_folder");
export const firstRunCreateFolder = () => s<FolderInfo>("first_run_create_folder");
export const completeFirstRun = (
  jobsRoot: string,
  spaces: string[],
  noClientSpaces: string[],
  hiddenTemplates: string[],
  marker: string,
) => s<SettingsView>("complete_first_run", { jobsRoot, spaces, noClientSpaces, hiddenTemplates, marker });
export const settingsReady = () => invoke<void>("settings_ready");
export const requestStatus = (key: string, status: "active" | "done") => invoke<void>("request_status", { key, status });
