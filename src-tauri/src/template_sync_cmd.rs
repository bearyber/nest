//! Template sync in the app (v0.5; the rules are in template_sync.rs). Runs inside each Devices
//! sync (launch, every minute, window focus, after changes); on quit it only writes. Holds the
//! templates lock for the whole tick, so it never races a save, import or delete.

use std::collections::{HashMap, HashSet};

use tauri::{AppHandle, Emitter, Manager};

use crate::commands::{lock, reload_templates, AppState};
use crate::error::bin_name;
use crate::template_io::{self, BUILT_IN_IDS};
use crate::template_sync::{self, Action, Book, Ctx, Got};

fn now() -> String {
    chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, false)
}

/// Remember a save, import or duplicate made on this computer (version seen, saved now). Kept
/// even while sync is off, so turning it on later knows what's newest. `fresh` = a brand-new
/// template (New, duplicate, from a folder). The caller holds the templates lock.
pub fn note_saved(state: &AppState, id: &str, version: u32, fresh: bool) {
    let at = now();
    if let Err(e) = crate::settings_cmd::edit_settings_quietly(state, |s| {
        let mut book = Book {
            seen: std::mem::take(&mut s.template_versions_seen),
            meta: std::mem::take(&mut s.template_sync_meta),
            copied: std::mem::take(&mut s.template_conflict_copies),
        };
        template_sync::note_local_save(&mut book, id, version, fresh, &at);
        s.template_versions_seen = book.seen;
        s.template_sync_meta = book.meta;
        s.template_conflict_copies = book.copied;
        Ok(())
    }) {
        log::warn!("couldn't note a template save for sync: {}", e.message);
    }
}

/// The highest version this computer has had of each template id (for the editor's save).
pub fn versions_seen(state: &AppState) -> std::collections::BTreeMap<String, u32> {
    state
        .settings
        .lock()
        .map(|s| s.template_versions_seen.clone())
        .unwrap_or_default()
}

/// One template sync: read and install (unless `read` is false: quitting), then write.
pub fn tick(app: &AppHandle, read: bool) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    if state.settings_error.is_some() {
        return;
    }
    let Ok(settings) = state.settings.lock().map(|s| s.clone()) else {
        return;
    };
    let device_id = settings
        .device_id
        .as_deref()
        .and_then(crate::devices::valid_device_id);
    let (Some(device_id), Some(shared)) = (device_id, settings.shared_folder.clone()) else {
        set_problem(app, &state, None);
        return;
    };
    if !settings.sync_templates {
        set_problem(app, &state, None);
        return;
    }
    let Ok(_one) = lock(&state.templates_lock) else {
        return;
    };
    let names: HashMap<String, String> = state
        .devices
        .runtime
        .lock()
        .map(|rt| {
            rt.cache
                .values()
                .map(|d| (d.device_id.clone(), d.name.clone()))
                .collect()
        })
        .unwrap_or_default();
    let busy: HashSet<String> = state
        .editor_sessions
        .lock()
        .map(|s| {
            s.values()
                .filter_map(|e| e.source.as_ref())
                .map(|t| t.loaded.template.id.clone())
                .collect()
        })
        .unwrap_or_default();
    let at = now();
    let own_name = settings
        .device_name
        .clone()
        .filter(|n| !n.trim().is_empty())
        .or_else(|| names.get(&device_id).cloned())
        .unwrap_or_else(crate::devices_cmd::default_name);
    let ctx = Ctx {
        enabled: true,
        shared: &shared,
        dev_build: cfg!(debug_assertions),
        device_id: &device_id,
        own_name: &own_name,
        user_dir: &state.user_templates,
        bundled_dir: &state.bundled_templates,
        built_in_ids: BUILT_IN_IDS,
        forgotten: &settings.forgotten_devices,
        names: &names,
        busy: &busy,
        now: &at,
    };
    let mut book = Book {
        seen: settings.template_versions_seen.clone(),
        meta: settings.template_sync_meta.clone(),
        copied: settings.template_conflict_copies.clone(),
    };
    let bin = |t: &template_io::Installed| template_io::delete(t, &state.user_templates);
    let mut save = |b: &Book| {
        crate::settings_cmd::edit_settings_quietly(&state, |s| {
            s.template_versions_seen = b.seen.clone();
            s.template_sync_meta = b.meta.clone();
            s.template_conflict_copies = b.copied.clone();
            Ok(())
        })
        .map_err(|e| e.message)
    };
    let report = template_sync::tick(&ctx, &mut book, read, &bin, &mut save);
    if !report.got.is_empty() {
        if let Err(e) = reload_templates(&state) {
            log::warn!("couldn't reload templates after sync: {}", e.message);
        }
        let _ = app.emit(crate::settings_cmd::CHANGED_EVENT, ());
        if let Ok(mut n) = state.notice.lock() {
            *n = Some(notice(&report.got));
        }
        let _ = app.emit("notice", ());
    }
    set_problem(app, &state, report.problem);
}

fn set_problem(app: &AppHandle, state: &AppState, problem: Option<String>) {
    let changed = state
        .devices
        .runtime
        .lock()
        .map(|mut rt| {
            let changed = rt.template_problem != problem;
            rt.template_problem = problem;
            changed
        })
        .unwrap_or(false);
    if changed {
        let _ = app.emit(crate::devices_cmd::CHANGED_EVENT, ());
    }
}

/// The main window's note: "Got “Video” from MacBook. Your older copy is in the Bin." When the
/// same template was changed on both computers: "“Wedding” was changed on both computers. Kept
/// both: “Wedding (MacBook)” is the one from this computer."
pub fn notice(got: &[Got]) -> String {
    let both: Vec<String> = got
        .iter()
        .filter_map(|g| g.kept_both.as_ref())
        .map(|(name, copy)| {
            format!(
                "\u{201c}{name}\u{201d} was changed on both computers. Kept both: \u{201c}{copy}\u{201d} is the one from this computer."
            )
        })
        .collect();
    let rest: Vec<&Got> = got.iter().filter(|g| g.kept_both.is_none()).collect();
    let mut parts = both;
    if !rest.is_empty() {
        parts.push(notice_got(&rest));
    }
    parts.join(" ")
}

fn notice_got(got: &[&Got]) -> String {
    let bin = bin_name();
    let binned: usize = got.iter().map(|g| g.binned).sum();
    let older = match binned {
        0 => String::new(),
        1 => format!(" Your older copy is in the {bin}."),
        _ => format!(" Your older copies are in the {bin}."),
    };
    match got {
        [one] => {
            let copy = if one.action == Action::Copy {
                " It shares a name with one of yours, so it was added next to it."
            } else {
                ""
            };
            format!(
                "Got \u{201c}{}\u{201d} from {}.{copy}{older}",
                one.name, one.from
            )
        }
        many => format!(
            "Got {} templates from your other computers.{older}",
            many.len()
        ),
    }
}
