//! Errors returned to the UI. `message` is plain English and shown as-is (CLAUDE.md conventions).

use std::fmt;

use serde::Serialize;

use crate::template::TemplateError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub message: String,
}

impl AppError {
    pub fn new(message: impl Into<String>) -> Self {
        AppError {
            message: message.into(),
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for AppError {}

impl From<TemplateError> for AppError {
    fn from(e: TemplateError) -> Self {
        AppError::new(e.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;

// ── Words that differ between the two OSes, for messages (locked #1). ──

/// "Finder" on a Mac, "Explorer" on Windows.
pub fn file_manager() -> &'static str {
    if cfg!(target_os = "macos") {
        "Finder"
    } else {
        "Explorer"
    }
}

/// "Trash" on a Mac, "Recycle Bin" on Windows.
pub fn bin_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "Trash"
    } else {
        "Recycle Bin"
    }
}

/// The refresh shortcut: "⌘R" on a Mac, "Ctrl+R" on Windows.
pub fn refresh_keys() -> &'static str {
    if cfg!(target_os = "macos") {
        "⌘R"
    } else {
        "Ctrl+R"
    }
}

/// Short, readable reason for an io error.
pub fn io_reason(e: &std::io::Error) -> String {
    use std::io::ErrorKind;
    match e.kind() {
        ErrorKind::PermissionDenied => "permission denied".into(),
        ErrorKind::AlreadyExists => "it already exists".into(),
        ErrorKind::NotFound => "it wasn't found".into(),
        _ => e.to_string(),
    }
}
