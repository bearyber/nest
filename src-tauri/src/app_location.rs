//! macOS: is Nest running from somewhere updates and open-at-login can't work? Opened straight
//! from the disk image (`/Volumes/Nest/Nest.app`, read-only) or from Downloads without being
//! moved (App Translocation: macOS runs a read-only copy under `…/AppTranslocation/…`).
//!
//! At start Nest offers to copy itself to /Applications and open from there. It never replaces
//! a Nest already in /Applications (it offers to open that one), and never removes the copy it
//! was started from (locked decision #4). Windows: nothing to do.

use std::path::{Component, Path, PathBuf};

/// Where Nest runs from, when that place breaks updates and opening at login.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Misplaced {
    /// Gatekeeper's read-only copy of an app that was never moved out of Downloads.
    Translocated,
    /// Opened from a mounted volume (the disk image), not from an Applications folder.
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

/// Is this bundle somewhere updates and opening at login break? Pure: looks at the path only.
/// `/Volumes/<drive>/Applications/Nest.app` (an Applications folder on another drive) is fine;
/// `/Volumes/<anything else>` is taken to be the disk image.
pub fn misplaced(bundle: &Path) -> Option<Misplaced> {
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
    let in_applications = bundle
        .parent()
        .and_then(|p| p.file_name())
        .is_some_and(|n| n == "Applications");
    if on_volume && !in_applications {
        return Some(Misplaced::DiskImage);
    }
    None
}

/// This running copy's bundle, if it's misplaced (release builds on macOS only).
pub fn misplaced_now() -> Option<(PathBuf, Misplaced)> {
    if !cfg!(target_os = "macos") || cfg!(debug_assertions) {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let exe = exe.canonicalize().unwrap_or(exe);
    let bundle = bundle_of(&exe)?;
    misplaced(&bundle).map(|m| (bundle, m))
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

    const APPLICATIONS: &str = "/Applications";

    /// Launch: if Nest runs from the disk image or a translocated copy, offer to move it.
    pub fn start(app: &AppHandle) {
        let Some((bundle, how)) = super::misplaced_now() else {
            return;
        };
        log::info!("running from {} ({how:?})", bundle.display());
        let app = app.clone();
        // Blocking dialogs: off the main thread (they're shown on it by the dialog plugin).
        std::thread::spawn(move || offer_move(&app, &bundle));
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

    fn offer_move(app: &AppHandle, bundle: &Path) {
        if !ask(
            app,
            "Move Nest to Applications?",
            "Move Nest to your Applications folder: updates and opening at login need it.".into(),
            "Move to Applications",
            "Not now",
        ) {
            return;
        }
        let name = bundle
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_else(|| "Nest.app".into());
        let target = Path::new(APPLICATIONS).join(&name);
        // Never replace what's there: offer to open that one instead.
        if target.exists() {
            if ask(
                app,
                "Nest is already in Applications",
                "There's already a Nest in your Applications folder. Open that one? This copy stays where it is.".into(),
                "Open it",
                "Not now",
            ) {
                relaunch_from(app, &target);
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

    /// Copy the whole bundle with `ditto` (keeps the signature, links and attributes; drops the
    /// quarantine note so the copy doesn't get translocated again). Into a hidden name first,
    /// then renamed, so a half copy never looks like Nest. A failed copy removes only what this
    /// run made.
    fn copy_bundle(from: &Path, to: &Path) -> Result<(), String> {
        let partial: PathBuf = to.with_file_name(format!(".Nest-moving-{}", uuid::Uuid::new_v4()));
        let out = Command::new("/usr/bin/ditto")
            .arg("--noqtn")
            .arg(from)
            .arg(&partial)
            .output()
            .map_err(|e| e.to_string());
        let result = match out {
            Ok(o) if o.status.success() => {
                if to.exists() {
                    Err("a Nest appeared in Applications meanwhile".to_string())
                } else {
                    std::fs::rename(&partial, to).map_err(|e| e.to_string())
                }
            }
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
        if result.is_err() && partial.exists() {
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

    #[test]
    fn disk_image_and_translocation_are_misplaced() {
        assert_eq!(
            misplaced(&p("/Volumes/Nest/Nest.app")),
            Some(Misplaced::DiskImage)
        );
        assert_eq!(
            misplaced(&p("/Volumes/Nest 0.7.1/Nest.app")),
            Some(Misplaced::DiskImage)
        );
        assert_eq!(
            misplaced(&p(
                "/private/var/folders/xy/abc123/T/AppTranslocation/0F3E-11AA/d/Nest.app"
            )),
            Some(Misplaced::Translocated)
        );
        // Translocation wins even when the original sat on another drive.
        assert_eq!(
            misplaced(&p("/Volumes/Work/x/AppTranslocation/1/d/Nest.app")),
            Some(Misplaced::Translocated)
        );
    }

    #[test]
    fn applications_and_other_folders_are_fine() {
        assert_eq!(misplaced(&p("/Applications/Nest.app")), None);
        assert_eq!(misplaced(&p("/Users/bernard/Applications/Nest.app")), None);
        assert_eq!(
            misplaced(&p("/Volumes/Work SSD/Applications/Nest.app")),
            None
        );
        // Moved out of Downloads by hand (not translocated): updates work there.
        assert_eq!(misplaced(&p("/Users/bernard/Downloads/Nest.app")), None);
        // Words that only look alike.
        assert_eq!(misplaced(&p("/Users/b/VolumesOfWork/Nest.app")), None);
        assert_eq!(
            misplaced(&p("/Users/b/AppTranslocationNotes/Nest.app")),
            None
        );
        assert_eq!(misplaced(&p("Volumes/Nest/Nest.app")), None);
    }

    #[test]
    fn korean_chinese_and_odd_names() {
        assert_eq!(
            misplaced(&p("/Volumes/네스트 설치/Nest.app")),
            Some(Misplaced::DiskImage)
        );
        assert_eq!(
            misplaced(&p("/Volumes/외장 디스크/Applications/Nest.app")),
            None
        );
        assert_eq!(misplaced(&p("/Users/버나드/다운로드/Nest.app")), None);
        assert_eq!(
            misplaced(&p(
                "/private/var/folders/T/AppTranslocation/中文/d/巢 Nest.app"
            )),
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
