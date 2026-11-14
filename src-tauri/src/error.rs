//! App-wide error type. Every message that can reach logs or the UI is
//! scrubbed of credentials (see `net::redact`).

use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{}", unauthorized_text(*.0))]
    Unauthorized(u16),
    #[error("Rate limited by the imageboard. Wait a bit and try again.")]
    RateLimited,
    #[error("The imageboard returned HTTP {0}.")]
    Http(u16),
    #[error("Network error: {0}")]
    Network(String),
    #[error("The imageboard rejected the request: {0}")]
    Rejected(String),
    #[error("Cancelled.")]
    Cancelled,
    #[error("Unexpected response format: {0}")]
    Parse(String),
    #[error("No credentials saved for {0}. Add them in Settings.")]
    MissingCredentials(String),
    #[error("Credential store error: {0}")]
    Credentials(String),
    #[error("Settings error: {0}")]
    Settings(String),
}

pub type AppResult<T> = Result<T, AppError>;

fn unauthorized_text(status: u16) -> String {
    if status == 401 {
        "Not authorized by the imageboard (HTTP 401). Check the account name and API key in Settings.".into()
    } else {
        format!(
            "The imageboard refused access (HTTP {status}). If you saved an account for this board, check it; \
             otherwise the site is probably blocking apps (bot protection)."
        )
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        // reqwest errors embed the request URL, which contains the API key.
        AppError::Network(crate::net::redact(&e.without_url().to_string()))
    }
}

impl From<keyring::Error> for AppError {
    fn from(e: keyring::Error) -> Self {
        AppError::Credentials(e.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Settings(e.to_string())
    }
}

/// Errors cross the Tauri IPC boundary as plain (redacted) strings.
impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&crate::net::redact(&self.to_string()))
    }
}
