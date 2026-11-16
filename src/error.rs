use std::fmt;

/// Errors returned by Joplin API calls and tool handlers.
#[derive(Debug)]
pub enum JoplinError {
    /// Missing or invalid configuration (e.g. `JOPLIN_TOKEN`).
    Config(String),
    /// Network or HTTP transport error.
    Http(String),
    /// Joplin API returned a non-success status code.
    Api { status: u16, body: String },
    /// Failed to serialize or deserialize JSON.
    Json(String),
    /// Invalid tool arguments or user input.
    InvalidInput(String),
    /// Requested entity was not found.
    NotFound(String),
}

impl fmt::Display for JoplinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(msg) => write!(f, "Configuration error: {msg}"),
            Self::Http(msg) => write!(f, "HTTP error: {msg}"),
            Self::Api { status, body } => write!(f, "Joplin API error (HTTP {status}): {body}"),
            Self::Json(msg) => write!(f, "JSON error: {msg}"),
            Self::InvalidInput(msg) => write!(f, "Invalid input: {msg}"),
            Self::NotFound(msg) => write!(f, "Not found: {msg}"),
        }
    }
}

impl std::error::Error for JoplinError {}

pub type Result<T> = std::result::Result<T, JoplinError>;
