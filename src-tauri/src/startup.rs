//! When Nest can't start: a native message box in plain words instead of a silent crash.
//!
//! "Nest couldn't start: <reason>. Your projects and templates are safe." with **Open log
//! folder** / **Quit**. The full error goes to the log (and to `startup-error.log` in the
//! same folder, in case the log itself never started). Shown with `rfd` directly, because the
//! app handle (and so the dialog plugin) may not exist yet, and setup runs on the main thread
//! where the plugin's blocking dialogs can't be used.

use std::fmt::Display;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

pub const OPEN_LOG: &str = "Open log folder";
pub const QUIT: &str = "Quit";
const NOTE_FILE: &str = "startup-error.log";

/// Set once setup has finished: after that, a crash is logged but no start-up box is shown.
static STARTED: AtomicBool = AtomicBool::new(false);
static LOG_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// The part of start-up that failed. Each has one plain reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Nest's own settings folder can't be found or created.
    ConfigFolder,
    /// Files that come with Nest (built-in templates) can't be found.
    OwnFiles,
    /// The main window couldn't be made.
    Window,
    /// The macOS menu bar couldn't be set up.
    MenuBar,
    /// Anything else.
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupError {
    pub step: Step,
    /// The full technical error, for the log only.
    pub detail: String,
}

impl StartupError {
    pub fn new(step: Step, detail: impl Display) -> Self {
        StartupError {
            step,
            detail: detail.to_string(),
        }
    }
}

/// The plain reason for a step, completing "Nest couldn't start: …".
pub fn reason(step: Step) -> &'static str {
    match step {
        Step::ConfigFolder => "it can't create or open its own settings folder",
        Step::OwnFiles => "some of its own files are missing (installing Nest again fixes this)",
        Step::Window => "its window couldn't be opened",
        Step::MenuBar => "its menu bar couldn't be set up",
        Step::Other => "something unexpected went wrong",
    }
}

/// The message box text.
pub fn message(reason: &str) -> String {
    format!("Nest couldn't start: {reason}. Your projects and templates are safe.")
}

/// Which step a crash during start-up most likely came from, from its text.
pub fn step_for_crash(text: &str) -> Step {
    let t = text.to_lowercase();
    if t.contains("webview") || t.contains("window") {
        Step::Window
    } else {
        Step::Other
    }
}

/// Where the logs go when Tauri can't tell yet (same place as Tauri's `app_log_dir`).
pub fn fallback_log_dir(identifier: &str) -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        let home = std::env::var_os("HOME")?;
        Some(PathBuf::from(home).join("Library/Logs").join(identifier))
    } else {
        let local = std::env::var_os("LOCALAPPDATA")?;
        Some(PathBuf::from(local).join(identifier).join("logs"))
    }
}

pub fn set_log_dir(dir: PathBuf) {
    if let Ok(mut d) = LOG_DIR.lock() {
        *d = Some(dir);
    }
}

/// The log folder. `try_lock`: the crash hook must never wait on a lock.
fn log_dir() -> Option<PathBuf> {
    LOG_DIR.try_lock().ok().and_then(|d| d.clone())
}

/// Setup finished: from now on crashes are only logged.
pub fn started() {
    STARTED.store(true, Ordering::SeqCst);
}

/// Crashes are always noted. A crash on the main thread before setup finished (e.g. Tauri
/// couldn't make the window) gets the start-up message box.
///
/// The hook never waits on the logger: a crash inside the logger (while it holds its lock)
/// would otherwise hang Nest instead of showing the box. It writes `startup-error.log`
/// directly first, then hands the line to the logger from another thread, waiting at most a
/// second for it.
pub fn install_crash_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let text = info.to_string();
        let line = format!("Nest stopped unexpectedly: {text}");
        if let Some(dir) = log_dir() {
            append_note(&dir, &line);
        }
        eprintln!("{line}");
        let on_main = std::thread::current().name() == Some("main");
        if on_main && !STARTED.swap(true, Ordering::SeqCst) {
            show_and_exit(&StartupError::new(step_for_crash(&text), text));
        }
        log_without_waiting(line);
        default(info);
    }));
}

/// Pass a line to the logger from a helper thread; give up after a second.
fn log_without_waiting(line: String) {
    let (done, wait) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().spawn(move || {
        log::error!("{line}");
        log::logger().flush();
        let _ = done.send(()); // the hook may have stopped waiting: nothing to do then
    });
    if spawned.is_ok() {
        let _ = wait.recv_timeout(std::time::Duration::from_secs(1));
    }
}

/// Log the error, show the message box, open the log folder if asked, and quit.
pub fn fail(err: &StartupError) -> ! {
    STARTED.store(true, Ordering::SeqCst);
    log::error!("Nest couldn't start ({:?}): {}", err.step, err.detail);
    log::logger().flush();
    if let Some(dir) = log_dir() {
        append_note(&dir, &note_line(err));
    }
    eprintln!("{}", note_line(err));
    show_and_exit(err)
}

/// The message box, then quit. Uses no logger and no lock it could wait on.
fn show_and_exit(err: &StartupError) -> ! {
    let dir = log_dir();
    let choice = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("Nest")
        .set_description(message(reason(err.step)))
        .set_buttons(rfd::MessageButtons::OkCancelCustom(
            OPEN_LOG.into(),
            QUIT.into(),
        ))
        .show();
    if matches!(&choice, rfd::MessageDialogResult::Custom(c) if c == OPEN_LOG) {
        if let Some(dir) = &dir {
            if let Err(e) = std::fs::create_dir_all(dir)
                .map_err(|e| e.to_string())
                .and_then(|_| {
                    tauri_plugin_opener::open_path(dir, None::<&str>).map_err(|e| e.to_string())
                })
            {
                eprintln!("couldn't open the log folder {}: {e}", dir.display());
            }
        }
    }
    std::process::exit(1)
}

fn note_line(err: &StartupError) -> String {
    format!("Nest couldn't start ({:?}): {}", err.step, err.detail)
}

/// One dated line in `startup-error.log`, next to the log (in case the log plugin never
/// started, or is the thing that broke).
fn append_note(dir: &std::path::Path, line: &str) {
    let line = format!("{} {line}\n", chrono::Local::now().to_rfc3339());
    let written = std::fs::create_dir_all(dir).and_then(|_| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(NOTE_FILE))?
            .write_all(line.as_bytes())
    });
    if let Err(e) = written {
        eprintln!("couldn't write {}: {e}", dir.join(NOTE_FILE).display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_is_plain_and_reassuring() {
        assert_eq!(
            message(reason(Step::Window)),
            "Nest couldn't start: its window couldn't be opened. Your projects and templates are safe."
        );
        for step in [
            Step::ConfigFolder,
            Step::OwnFiles,
            Step::Window,
            Step::MenuBar,
            Step::Other,
        ] {
            let m = message(reason(step));
            assert!(m.starts_with("Nest couldn't start: "), "{m}");
            assert!(
                m.ends_with(". Your projects and templates are safe."),
                "{m}"
            );
            // Plain words only: no error codes, paths or Rust debug text.
            assert!(
                !m.contains("Error") && !m.contains("::") && !m.contains('{'),
                "{m}"
            );
        }
        assert_eq!(OPEN_LOG, "Open log folder");
        assert_eq!(QUIT, "Quit");
    }

    #[test]
    fn crash_text_picks_the_step() {
        assert_eq!(
            step_for_crash("Failed to setup app: error encountered during setup hook: failed to create webview: WebView2 not found"),
            Step::Window
        );
        assert_eq!(step_for_crash("index out of bounds"), Step::Other);
    }

    #[test]
    fn note_is_appended_next_to_the_log() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("logs");
        let err = StartupError::new(Step::ConfigFolder, "permission denied");
        append_note(&dir, &note_line(&err));
        append_note(&dir, "Nest stopped unexpectedly: second");
        let text = std::fs::read_to_string(dir.join(NOTE_FILE)).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text.contains("Nest couldn't start (ConfigFolder): permission denied"));
        assert!(text.contains("stopped unexpectedly: second"));
    }

    /// The crash hook's helpers never wait: not on the log folder's lock, not on the logger.
    #[test]
    fn crash_path_never_waits() {
        let held = LOG_DIR.lock().unwrap();
        let start = std::time::Instant::now();
        assert_eq!(log_dir(), None, "a held lock means no folder, not a wait");
        drop(held);
        // Logging goes through a helper thread with a one-second limit.
        log_without_waiting("test line".into());
        assert!(start.elapsed() < std::time::Duration::from_secs(5));
    }
}
