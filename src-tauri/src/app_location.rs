//! macOS: is Nest running from somewhere updates and open-at-login can't work? Opened straight
//! from the disk image (`/Volumes/Nest/Nest.app`, a read-only volume) or from Downloads without
//! being moved (App Translocation: macOS runs a read-only copy under `…/AppTranslocation/…`).
//! A Nest kept on a writable drive (`/Volumes/Work/Nest.app`) is fine: it updates there.
//!
//! At start Nest offers to copy itself to /Applications and open from there. It never replaces
//! a Nest already in /Applications (it offers to open that one), and never removes the copy it
//! was started from (locked decision #4). "Not now" is remembered for that place, so it doesn't
//! ask on every launch. Windows: nothing to do.

use std::path::{Component, Path, PathBuf};

/// Where Nest runs from, when that place breaks updates and opening at login.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Misplaced {
    /// Gatekeeper's read-only copy of an app that was never moved out of Downloads.
    Translocated,
    /// Opened from a read-only volume under /Volumes (the mounted disk image).
    DiskImage,
}

/// The `.app` bundle a program inside it runs from: the outermost path component ending in
/// `.app` (`/Volumes/Nest/Nest.app/Contents/MacOS/nest` → `/Volumes/Nest/Nest.app`).
pub fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in exe.components() {
        out.push(c);
        if let Component::Normal(name) = c {
            if Path::new(name)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("app"))
            {
                return Some(out);
            }
        }
    }
    None
}

/// Is this bundle somewhere updates and opening at login break? Pure: the path, plus whether
/// the volume it's on is read-only. Only a translocated copy or a read-only volume under
/// /Volumes (the disk image) counts; a Nest on a writable drive is where you put it.
pub fn misplaced(bundle: &Path, read_only_volume: bool) -> Option<Misplaced> {
    let names: Vec<&std::ffi::OsStr> = bundle
        .components()
        .filter_map(|c| match c {
            Component::Normal(n) => Some(n),
            _ => None,
        })
        .collect();
    if names.iter().any(|n| *n == "AppTranslocation") {
        return Some(Misplaced::Translocated);
    }
    let on_volume = bundle.is_absolute() && names.first().is_some_and(|n| *n == "Volumes");
    if on_volume && read_only_volume {
        return Some(Misplaced::DiskImage);
    }
    None
}

/// Offer the move? Not when "Not now" was said for this very place. Pure.
pub fn should_offer(bundle: &Path, declined: Option<&Path>) -> bool {
    declined != Some(bundle)
}

/// Is the volume holding `path` mounted read-only (statfs `MNT_RDONLY`)? Unknown → no.
#[cfg(target_os = "macos")]
pub fn read_only_volume(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    let mut fs = std::mem::MaybeUninit::<libc::statfs>::zeroed();
    // SAFETY: `c` is a valid C string and `fs` is a buffer of the right type for statfs.
    let rc = unsafe { libc::statfs(c.as_ptr(), fs.as_mut_ptr()) };
    if rc != 0 {
        return false;
    }
    // SAFETY: statfs succeeded, so it filled the buffer.
    let fs = unsafe { fs.assume_init() };
    fs.f_flags & (libc::MNT_RDONLY as u32) != 0
}

#[cfg(not(target_os = "macos"))]
pub fn read_only_volume(_path: &Path) -> bool {
    false
}

/// This running copy's bundle, if it's misplaced (release builds on macOS only).
pub fn misplaced_now() -> Option<(PathBuf, Misplaced)> {
    if !cfg!(target_os = "macos") || cfg!(debug_assertions) {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let exe = exe.canonicalize().unwrap_or(exe);
    let bundle = bundle_of(&exe)?;
    misplaced(&bundle, read_only_volume(&bundle)).map(|m| (bundle, m))
}

#[cfg(target_os = "macos")]
pub use mac::start;

/// Windows (and dev builds): nothing to check.
#[cfg(not(target_os = "macos"))]
pub fn start(_app: &tauri::AppHandle) {}

#[cfg(target_os = "macos")]
mod mac {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use tauri::{AppHandle, Manager};
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

    use crate::commands::AppState;

    const APPLICATIONS: &str = "/Applications";

    /// Launch (release builds): tidy Nest's own half-made copies in /Applications, then, if Nest
    /// runs from the disk image or a translocated copy, offer to move it. One thread, in that
    /// order, so the tidy never meets a copy this run is making.
    pub fn start(app: &AppHandle) {
        if cfg!(debug_assertions) {
            return;
        }
        let app = app.clone();
        // Blocking dialogs: off the main thread (they're shown on it by the dialog plugin).
        std::thread::spawn(move || {
            for p in crate::tidy::applications_folder(
                Path::new(APPLICATIONS),
                std::time::SystemTime::now(),
            ) {
                log::info!("tidy: removed Nest's leftover {}", p.display());
            }
            let Some((bundle, how)) = super::misplaced_now() else {
                return;
            };
            log::info!("running from {} ({how:?})", bundle.display());
            let declined = app
                .try_state::<AppState>()
                .and_then(|s| s.settings.lock().ok().and_then(|s| s.move_declined.clone()));
            if !super::should_offer(&bundle, declined.as_deref()) {
                log::info!("\"Not now\" was said for this place before: not asking again");
                return;
            }
            offer_move(&app, &bundle);
        });
    }

    /// "Not now": remembered for this place, so it isn't asked on every launch.
    fn remember_not_now(app: &AppHandle, bundle: &Path) {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        let place = bundle.to_path_buf();
        if let Err(e) = crate::settings_cmd::edit_settings_quietly(&state, |s| {
            s.move_declined = Some(place);
            Ok(())
        }) {
            log::warn!("couldn't remember \"Not now\": {}", e.message);
        }
    }

    fn ask(app: &AppHandle, title: &str, message: String, yes: &str, no: &str) -> bool {
        let mut d = app
            .dialog()
            .message(message)
            .title(title)
            .kind(MessageDialogKind::Info)
            .buttons(MessageDialogButtons::OkCancelCustom(yes.into(), no.into()));
        // A sheet on the window when it shows; on its own when Nest started in the menu bar.
        if let Some(w) = app
            .get_webview_window("main")
            .filter(|w| w.is_visible().unwrap_or(false))
        {
            d = d.parent(&w);
        }
        d.blocking_show()
    }

    fn tell(app: &AppHandle, title: &str, message: String) {
        let mut d = app
            .dialog()
            .message(message)
            .title(title)
            .kind(MessageDialogKind::Warning);
        if let Some(w) = app
            .get_webview_window("main")
            .filter(|w| w.is_visible().unwrap_or(false))
        {
            d = d.parent(&w);
        }
        d.blocking_show();
    }

    /// Is something (anything: a folder, a file, a link, even a broken one) at `p`? Links are
    /// not followed. Can't tell → yes, so nothing is ever replaced.
    pub(super) fn there(p: &Path) -> bool {
        match std::fs::symlink_metadata(p) {
            Ok(_) => true,
            Err(e) => e.kind() != std::io::ErrorKind::NotFound,
        }
    }

    fn offer_move(app: &AppHandle, bundle: &Path) {
        if !ask(
            app,
            "Move Nest to Applications?",
            "Move Nest to your Applications folder: updates and opening at login need it.".into(),
            "Move to Applications",
            "Not now",
        ) {
            remember_not_now(app, bundle);
            return;
        }
        let name = bundle
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_else(|| "Nest.app".into());
        let target = Path::new(APPLICATIONS).join(&name);
        // Never replace what's there: offer to open a real Nest folder, leave anything else.
        if there(&target) {
            let real_folder = std::fs::symlink_metadata(&target).is_ok_and(|m| m.is_dir());
            if !real_folder {
                tell(
                    app,
                    "Couldn't move Nest",
                    format!(
                        "Something called {} is already in your Applications folder. Nest left it alone. Quit Nest, then drag it to Applications in Finder.",
                        name.to_string_lossy()
                    ),
                );
            } else if ask(
                app,
                "Nest is already in Applications",
                "There's already a Nest in your Applications folder. Open that one? This copy stays where it is.".into(),
                "Open it",
                "Not now",
            ) {
                relaunch_from(app, &target);
            } else {
                remember_not_now(app, bundle);
            }
            return;
        }
        match copy_bundle(bundle, &target) {
            Ok(()) => {
                log::info!("copied Nest to {}", target.display());
                relaunch_from(app, &target);
            }
            Err(e) => {
                log::warn!("couldn't copy Nest to Applications: {e}");
                tell(
                    app,
                    "Couldn't move Nest",
                    format!("Nest couldn't copy itself to your Applications folder ({e}). Quit Nest, then drag it to Applications in Finder."),
                );
            }
        }
    }

    /// Rename `from` to `to` only if nothing is at `to` (not even a link): `renamex_np` with
    /// `RENAME_EXCL`, one step, so nothing that appeared meanwhile is ever replaced.
    pub(super) fn rename_new(from: &Path, to: &Path) -> std::io::Result<()> {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let f = CString::new(from.as_os_str().as_bytes())?;
        let t = CString::new(to.as_os_str().as_bytes())?;
        // SAFETY: both are valid, NUL-terminated C strings that outlive the call.
        let rc = unsafe { libc::renamex_np(f.as_ptr(), t.as_ptr(), libc::RENAME_EXCL) };
        if rc == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }

    /// Copy the whole bundle with `ditto` (keeps the signature, links and attributes; drops the
    /// quarantine note so the copy doesn't get translocated again). Into a hidden name first,
    /// then renamed (never over anything), so a half copy never looks like Nest. A failed copy
    /// removes only what this run made; a crash leaves it for the start-up tidy (tidy.rs).
    fn copy_bundle(from: &Path, to: &Path) -> Result<(), String> {
        let partial: PathBuf = to.with_file_name(format!(
            "{}{}",
            crate::tidy::MOVING_PREFIX,
            uuid::Uuid::new_v4()
        ));
        let out = Command::new("/usr/bin/ditto")
            .arg("--noqtn")
            .arg(from)
            .arg(&partial)
            .output()
            .map_err(|e| e.to_string());
        let result = match out {
            Ok(o) if o.status.success() => rename_new(&partial, to).map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    "something called Nest appeared in Applications meanwhile; it was left alone"
                        .to_string()
                } else {
                    e.to_string()
                }
            }),
            Ok(o) => {
                let said = String::from_utf8_lossy(&o.stderr).trim().to_string();
                Err(if said.is_empty() {
                    "the copy failed".to_string()
                } else {
                    said
                })
            }
            Err(e) => Err(e),
        };
        if result.is_err() && there(&partial) {
            // Our own uuid-named copy; remove_dir_all never follows links.
            let _ = std::fs::remove_dir_all(&partial);
        }
        result
    }

    /// Open `target` once this copy has quit (a second Nest would only hand over to this one
    /// and close), then quit.
    fn relaunch_from(app: &AppHandle, target: &Path) {
        let pid = std::process::id().to_string();
        let spawned = Command::new("/bin/sh")
            .arg("-c")
            .arg(r#"while kill -0 "$1" 2>/dev/null; do sleep 0.2; done; exec /usr/bin/open "$2""#)
            .arg("sh")
            .arg(&pid)
            .arg(target)
            .spawn();
        match spawned {
            Ok(_) => app.exit(0),
            Err(e) => {
                log::warn!("couldn't open {}: {e}", target.display());
                tell(
                    app,
                    "Couldn't open Nest",
                    format!("Open Nest from your Applications folder ({e})."),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn finds_the_bundle_of_the_running_program() {
        assert_eq!(
            bundle_of(&p("/Volumes/Nest/Nest.app/Contents/MacOS/nest")),
            Some(p("/Volumes/Nest/Nest.app"))
        );
        assert_eq!(
            bundle_of(&p("/Applications/Nest.app/Contents/MacOS/nest")),
            Some(p("/Applications/Nest.app"))
        );
        // The outermost .app, not a helper inside it.
        assert_eq!(
            bundle_of(&p(
                "/Applications/Nest.app/Contents/Helpers/X.app/Contents/MacOS/x"
            )),
            Some(p("/Applications/Nest.app"))
        );
        assert_eq!(bundle_of(&p("/usr/local/bin/nest")), None);
        assert_eq!(bundle_of(&p("C:\\Program Files\\Nest\\nest.exe")), None);
    }

    /// Read-only / writable volume, for the pure check.
    const RO: bool = true;
    const RW: bool = false;

    // Mac paths: the feature is Mac-only, and Windows reads "/Volumes/…" differently.
    #[cfg(target_os = "macos")]
    #[test]
    fn disk_image_and_translocation_are_misplaced() {
        assert_eq!(
            misplaced(&p("/Volumes/Nest/Nest.app"), RO),
            Some(Misplaced::DiskImage)
        );
        assert_eq!(
            misplaced(&p("/Volumes/Nest 0.7.1/Nest.app"), RO),
            Some(Misplaced::DiskImage)
        );
        assert_eq!(
            misplaced(
                &p("/private/var/folders/xy/abc123/T/AppTranslocation/0F3E-11AA/d/Nest.app"),
                RO
            ),
            Some(Misplaced::Translocated)
        );
        // Translocation wins even when the original sat on another drive.
        assert_eq!(
            misplaced(&p("/Volumes/Work/x/AppTranslocation/1/d/Nest.app"), RW),
            Some(Misplaced::Translocated)
        );
    }

    // Mac paths: the feature is Mac-only, and Windows reads "/Volumes/…" differently.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_nest_kept_on_a_writable_drive_is_fine() {
        assert_eq!(misplaced(&p("/Volumes/Work/Nest.app"), RW), None);
        assert_eq!(misplaced(&p("/Volumes/Work/Tools/Nest.app"), RW), None);
        assert_eq!(
            misplaced(&p("/Volumes/Work SSD/Applications/Nest.app"), RW),
            None
        );
        // The same place read-only (a mounted image, a locked drive): misplaced.
        assert_eq!(
            misplaced(&p("/Volumes/Work/Nest.app"), RO),
            Some(Misplaced::DiskImage)
        );
        // Only /Volumes counts: the system volume being sealed says nothing about /Applications.
        assert_eq!(misplaced(&p("/Applications/Nest.app"), RO), None);
    }

    #[test]
    fn not_now_is_remembered_for_that_place_only() {
        let here = p("/Volumes/Nest/Nest.app");
        assert!(should_offer(&here, None));
        assert!(!should_offer(&here, Some(&here)));
        assert!(should_offer(
            &here,
            Some(&p("/Volumes/Nest 0.7.2/Nest.app"))
        ));
        assert!(should_offer(&p("/Volumes/네스트/Nest.app"), Some(&here)));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_system_volume_check_runs() {
        // The temp folder is on a writable volume; a path that doesn't exist is "don't know".
        let tmp = tempfile::tempdir().unwrap();
        assert!(!read_only_volume(tmp.path()));
        assert!(!read_only_volume(&tmp.path().join("missing")));
    }

    /// Renaming into Applications never replaces anything: a folder, a file or a link (even a
    /// broken one) already there makes it fail, and the existence check doesn't follow links.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_final_rename_never_replaces_anything() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let made = |n: &str| {
            let d = dir.join(n);
            std::fs::create_dir(&d).unwrap();
            std::fs::write(d.join("Info.plist"), "x").unwrap();
            d
        };
        let existing = made("Nest.app");
        let a = made(".Nest-moving-a");
        let err = mac::rename_new(&a, &existing).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert!(a.join("Info.plist").is_file() && existing.join("Info.plist").is_file());

        let link = dir.join("Linked.app");
        std::os::unix::fs::symlink(dir.join("nowhere"), &link).unwrap();
        assert!(mac::there(&link), "a broken link still counts");
        assert!(mac::rename_new(&a, &link).is_err());
        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());

        let fresh = dir.join("네스트 巢.app");
        assert!(!mac::there(&fresh));
        mac::rename_new(&a, &fresh).unwrap();
        assert!(fresh.join("Info.plist").is_file() && !mac::there(&a));
    }

    #[test]
    fn applications_and_other_folders_are_fine() {
        for rw in [RO, RW] {
            assert_eq!(misplaced(&p("/Applications/Nest.app"), rw), None);
            assert_eq!(
                misplaced(&p("/Users/bernard/Applications/Nest.app"), rw),
                None
            );
            // Moved out of Downloads by hand (not translocated): updates work there.
            assert_eq!(misplaced(&p("/Users/bernard/Downloads/Nest.app"), rw), None);
            // Words that only look alike.
            assert_eq!(misplaced(&p("/Users/b/VolumesOfWork/Nest.app"), rw), None);
            assert_eq!(
                misplaced(&p("/Users/b/AppTranslocationNotes/Nest.app"), rw),
                None
            );
            assert_eq!(misplaced(&p("Volumes/Nest/Nest.app"), rw), None);
        }
    }

    // Mac paths: the feature is Mac-only, and Windows reads "/Volumes/…" differently.
    #[cfg(target_os = "macos")]
    #[test]
    fn korean_chinese_and_odd_names() {
        assert_eq!(
            misplaced(&p("/Volumes/네스트 설치/Nest.app"), RO),
            Some(Misplaced::DiskImage)
        );
        assert_eq!(misplaced(&p("/Volumes/외장 디스크/Nest.app"), RW), None);
        assert_eq!(misplaced(&p("/Users/버나드/다운로드/Nest.app"), RW), None);
        assert_eq!(
            misplaced(
                &p("/private/var/folders/T/AppTranslocation/中文/d/巢 Nest.app"),
                RW
            ),
            Some(Misplaced::Translocated)
        );
        assert_eq!(
            bundle_of(&p(
                "/Volumes/巢 \"Nest\" (1)/Nest 2.app/Contents/MacOS/nest"
            )),
            Some(p("/Volumes/巢 \"Nest\" (1)/Nest 2.app"))
        );
        assert_eq!(bundle_of(&p("/Users/b/My.APP.notes/x")), None);
        assert_eq!(
            bundle_of(&p("/Volumes/X/Nest.APP/Contents/MacOS/nest")),
            Some(p("/Volumes/X/Nest.APP"))
        );
    }
}
