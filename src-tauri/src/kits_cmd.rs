//! Client kits (v0.5): the Settings commands, and finding + reading the kit for New Project.
//! Kit folders are per computer (`kits_folder`, `client_kits` in settings) and only ever read.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, State, WebviewWindow};

use crate::commands::{lock, AppState, PlanRequest};
use crate::error::{AppError, AppResult};
use crate::kits::{find_kit, list_kit, KitListing, KitSource};
use crate::names::normalize_client_code;
use crate::plan::{FieldValue, KitFound, KitInput};
use crate::settings::Settings;
use crate::settings_cmd::{edit_settings, example, pick_folder, templates_changed, SettingsView};
use crate::template::{FieldKind, Template};
use crate::template_edit;
use crate::template_io::{self, BUILT_IN_IDS};

#[derive(Default)]
pub struct Kits {
    /// Kit listings by kit folder. The preview re-plans on every keystroke, so a big kit is read
    /// once; it's read again on Create, when New Project opens, and after a kit setting changes.
    pub cache: Mutex<HashMap<PathBuf, Arc<KitListing>>>,
    /// Cancel was clicked while a Create runs: it stops and undoes what it made.
    pub cancel: AtomicBool,
}

impl Kits {
    pub fn forget(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }
}

/// The chosen client and their kit, for the planner. `None` = no client (none picked yet, a
/// no-client space, or "No client" ticked). `fresh` = read the kit again (Create).
pub(crate) fn kit_input(
    kits: &Kits,
    settings: &Settings,
    template: &Template,
    request: &PlanRequest,
    fresh: bool,
) -> AppResult<Option<KitInput>> {
    if request.no_client || settings.no_client_spaces.contains(&request.space) {
        return Ok(None);
    }
    let client = template
        .fields
        .iter()
        .find(|f| f.kind == FieldKind::Client)
        .and_then(|f| match request.values.get(&f.key) {
            Some(FieldValue::Client(c)) => Some(c),
            _ => None,
        });
    let Some(client) = client else {
        return Ok(None);
    };
    let name = client.name.split_whitespace().collect::<Vec<_>>().join(" ");
    let Ok(code) = normalize_client_code(&client.code) else {
        return Ok(None); // the planner reports the bad code
    };
    if name.is_empty() {
        return Ok(None);
    }
    let found = match find_kit(
        settings.kits_folder.as_deref(),
        &settings.client_kits,
        &code,
        &name,
    ) {
        None => KitFound::None,
        // A template without a kit folder never reads the kit.
        Some(kit) if !template.tree.iter().any(|e| e.kit()) => KitFound::NotRead(kit.dir),
        Some(kit) => {
            let cached = if fresh {
                None
            } else {
                lock(&kits.cache)?.get(&kit.dir).cloned()
            };
            match cached {
                Some(listing) => KitFound::Listed(listing),
                // Read without holding the lock: a big kit on a slow drive takes a while.
                None => match list_kit(&kit.dir) {
                    Ok(listing) => {
                        let listing = Arc::new(listing);
                        lock(&kits.cache)?.insert(kit.dir.clone(), listing.clone());
                        KitFound::Listed(listing)
                    }
                    Err(message) => KitFound::Unreadable {
                        dir: kit.dir,
                        message,
                    },
                },
            }
        }
    };
    Ok(Some(KitInput {
        client: name,
        code,
        found,
    }))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KitsView {
    pub kits_folder: Option<PathBuf>,
    /// The Kits folder is set but can't be found (e.g. its drive isn't connected).
    pub kits_folder_missing: bool,
    pub clients: Vec<KitClient>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KitClient {
    pub name: String,
    pub code: String,
    /// The kit Nest would copy for this client on this computer (None = none).
    pub kit: Option<PathBuf>,
    pub source: Option<KitSource>,
    /// A folder was chosen for this client, but it can't be found.
    pub missing: bool,
}

/// Settings → Client kits: the Kits folder and every client Nest knows, with their kit.
#[tauri::command(async)]
pub fn kits_view(state: State<'_, AppState>) -> AppResult<KitsView> {
    let settings = lock(&state.settings)?.clone();
    let rows = lock(&state.index)?
        .list()
        .map_err(|e| AppError::new(format!("Couldn't read the project list: {e}")))?;
    // One per client code (named as in the first project found), never your own code.
    let mut clients: BTreeMap<String, String> = BTreeMap::new();
    for row in rows {
        let Ok(code) = normalize_client_code(&row.client.code) else {
            continue;
        };
        let name = row.client.name.trim();
        if name.is_empty() || code.eq_ignore_ascii_case(&settings.own_code) {
            continue;
        }
        clients.entry(code).or_insert_with(|| name.to_string());
    }
    // A folder chosen for a client with no projects here (yet): still listed, so it can be undone.
    for code in settings.client_kits.keys() {
        let code = code.to_ascii_uppercase();
        clients.entry(code.clone()).or_insert(code);
    }
    let mut clients: Vec<KitClient> = clients
        .into_iter()
        .map(|(code, name)| {
            let found = find_kit(
                settings.kits_folder.as_deref(),
                &settings.client_kits,
                &code,
                &name,
            );
            KitClient {
                missing: found
                    .as_ref()
                    .is_some_and(|k| k.source == KitSource::Chosen && !k.dir.is_dir()),
                kit: found.as_ref().map(|k| k.dir.clone()),
                source: found.map(|k| k.source),
                name,
                code,
            }
        })
        .collect();
    clients.sort_by_key(|c| c.name.to_lowercase());
    Ok(KitsView {
        kits_folder_missing: settings.kits_folder.as_ref().is_some_and(|d| !d.is_dir()),
        kits_folder: settings.kits_folder,
        clients,
    })
}

/// Choose the Kits folder (a folder holding one folder per client). `None` = cancelled.
#[tauri::command(async)]
pub fn choose_kits_folder(
    app: AppHandle,
    window: WebviewWindow,
    kits: State<'_, Kits>,
) -> AppResult<Option<SettingsView>> {
    let Some(path) = pick_folder(&window, "Choose your Kits folder (one folder per client)")?
    else {
        return Ok(None);
    };
    let v = edit_settings(&app, |s| s.set_kits_folder(Some(path)))?;
    kits.forget();
    Ok(Some(v))
}

/// Stop using the Kits folder. Nothing on disk changes.
#[tauri::command(async)]
pub fn clear_kits_folder(app: AppHandle, kits: State<'_, Kits>) -> AppResult<SettingsView> {
    let v = edit_settings(&app, |s| s.set_kits_folder(None))?;
    kits.forget();
    Ok(v)
}

/// "Choose a folder…" for one client (Settings, or New Project when a client has no kit).
/// `None` = cancelled.
#[tauri::command(async)]
pub fn choose_client_kit(
    app: AppHandle,
    window: WebviewWindow,
    kits: State<'_, Kits>,
    code: String,
    name: String,
) -> AppResult<Option<SettingsView>> {
    let title = match name.trim() {
        "" => "Choose the folder with the client's files".to_string(),
        name => format!("Choose the folder with {name}'s files"),
    };
    let Some(path) = pick_folder(&window, &title)? else {
        return Ok(None);
    };
    let v = edit_settings(&app, |s| s.set_client_kit(&code, Some(path)))?;
    kits.forget();
    Ok(Some(v))
}

/// "Use the Kits folder" for one client: forget the folder chosen for them.
#[tauri::command(async)]
pub fn clear_client_kit(
    app: AppHandle,
    kits: State<'_, Kits>,
    code: String,
) -> AppResult<SettingsView> {
    let v = edit_settings(&app, |s| s.set_client_kit(&code, None))?;
    kits.forget();
    Ok(v)
}

/// New Project's "Put them in": the template gets a place for client files (`folder`, a
/// template tree path) and keeps it. Saved as a new version through the normal save (one copy
/// per template: the older copy goes to the Recycle Bin / Trash); one of Nest's own becomes
/// yours with the same id, like Customize. `template_key` is a template's key, or the id of the
/// template New Project offers. The sheet plans again on `settings-changed`.
#[tauri::command(async)]
pub fn set_template_kit_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    template_key: String,
    folder: String,
) -> AppResult<()> {
    let _one = lock(&state.templates_lock)?; // one change to your templates at a time
    let installed = lock(&state.templates)?.clone();
    let source = installed
        .iter()
        .find(|t| t.key() == template_key)
        .or_else(|| {
            installed
                .iter()
                .find(|t| !t.superseded && t.loaded.template.id == template_key)
        })
        .cloned()
        .ok_or_else(|| AppError::new("That template isn't installed any more"))?;
    let saved = template_edit::save_kit_folder(
        &source,
        &folder,
        &installed,
        &state.user_templates,
        BUILT_IN_IDS,
        &example(&state)?,
    )
    .map_err(AppError::new)?;
    crate::template_sync_cmd::note_saved(&state, &saved.template.id, saved.template.version, false);
    templates_changed(&app, &state)?;
    // Decision T-A: one copy per template. If the older copy can't go to the bin, both stay.
    let installed = lock(&state.templates)?.clone();
    let bin = |t: &template_io::Installed| template_io::delete(t, &state.user_templates);
    let retired =
        template_edit::retire_older_copies(&saved.template.id, &saved.dir, &installed, &bin);
    templates_changed(&app, &state)?;
    retired.map(|_| ()).map_err(AppError::new)
}

/// Cancel a running Create: it stops at the next file and undoes what it made.
#[tauri::command]
pub fn cancel_create(state: State<'_, AppState>, kits: State<'_, Kits>) {
    if state.creating.load(std::sync::atomic::Ordering::SeqCst) {
        kits.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}
