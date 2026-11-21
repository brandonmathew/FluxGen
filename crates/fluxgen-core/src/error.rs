//! Typed application errors.
//!
//! Every failure that can reach the user is represented here (or
//! wrapped into this type) so the UI can display the full reason
//! instead of a generic failure message.

use std::path::PathBuf;

/// The single error type surfaced across FluxGen crate boundaries.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// A generation setting or other user-supplied value is invalid.
    #[error("invalid value for {field}: {reason}")]
    Validation {
        /// Name of the field that failed validation.
        field: String,
        /// Why the value was rejected.
        reason: String,
    },

    /// A required file does not exist.
    #[error("file not found: {0}")]
    FileNotFound(PathBuf),

    /// A file exists but its contents do not match expectations.
    #[error("file verification failed for {path}: {reason}")]
    VerificationFailed {
        /// File that failed verification.
        path: PathBuf,
        /// What did not match (size, hash, format).
        reason: String,
    },

    /// The engine binary is missing or cannot be launched.
    #[error("engine unavailable: {0}")]
    EngineUnavailable(String),

    /// The engine process exited before or during work.
    #[error("engine exited unexpectedly (code: {code:?}): {output}")]
    EngineExited {
        /// Exit code, when the operating system reported one.
        code: Option<i32>,
        /// Captured engine output, included so the reason is visible.
        output: String,
    },

    /// A request to the running engine failed or returned an
    /// error.
    #[error("engine request failed: {0}")]
    EngineRequest(String),

    /// The engine did not become ready within the allowed time.
    #[error("engine did not become ready within {timeout_secs}s: {output}")]
    EngineTimeout {
        /// Seconds waited before giving up.
        timeout_secs: u64,
        /// Captured engine output up to the timeout.
        output: String,
    },

    /// A network transfer failed.
    #[error("transfer failed: {0}")]
    Transfer(String),

    /// Storage (filesystem or database) failed.
    #[error("storage failed: {0}")]
    Storage(String),

    /// Any other I/O failure, with the OS message preserved.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl AppError {
    /// Build a validation error for a named field.
    pub fn validation(field: impl Into<String>, reason: impl Into<String>) -> Self {
        AppError::Validation {
            field: field.into(),
            reason: reason.into(),
        }
    }
}

/// Convenience result alias used across FluxGen crates.
pub type Result<T> = std::result::Result<T, AppError>;
