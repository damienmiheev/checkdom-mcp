//! Error type shared across the CheckDom client and MCP tools.

use thiserror::Error;

/// Errors from talking to the CheckDom dashboard.
#[derive(Debug, Error)]
pub enum CheckDomError {
    /// No session token is stored locally.
    #[error("not logged in; run `checkdom-mcp login`")]
    NotLoggedIn,

    /// The stored session token is no longer valid (API code 205).
    #[error("session expired; run `checkdom-mcp login` again")]
    SessionExpired,

    /// The API returned a logical error (`code` other than `000`).
    #[error("CheckDom API error [{code}]: {message}")]
    Api { code: String, message: String },

    /// Login did not succeed (bad credentials, missing CSRF token, no session cookie).
    #[error("login failed: {0}")]
    Login(String),

    /// The response was shaped in a way we did not expect.
    #[error("unexpected response from CheckDom: {0}")]
    Unexpected(String),

    /// Transport-level HTTP error.
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// The body could not be (de)serialized as JSON.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Convenience alias for results that fail with [`CheckDomError`].
pub type Result<T> = std::result::Result<T, CheckDomError>;
