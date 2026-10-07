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
