//! All the ways this program can fail, in one enum.
//!
//! Rust has no exceptions. Functions that can fail return `Result<T, AppError>`,
//! and the compiler forces every caller to deal with the error case.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("missing API key: set ANTHROPIC_API_KEY or pass --api-key")]
    MissingApiKey,

    #[error("prompt is empty: pass a prompt as an argument or pipe one on stdin")]
    EmptyPrompt,

    /// Network-level failure (DNS, TLS, timeout). `#[from]` lets `?` convert
    /// a `reqwest::Error` into this variant automatically.
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),

    /// The API answered, but with a non-success HTTP status.
    #[error("API returned {status}: {message}")]
    Api { status: u16, message: String },

    /// The API answered 200, but the body was not the JSON shape we expected.
    #[error("could not parse API response: {0}")]
    Parse(#[from] serde_json::Error),

    #[error("API response contained no text content")]
    NoContent,

    #[error("failed to read stdin: {0}")]
    Io(#[from] std::io::Error),
}

impl AppError {
    /// Rate limits (429) and server errors (5xx) are worth retrying.
    /// Bad requests or auth failures (4xx) will fail the same way again.
    pub fn is_retryable(&self) -> bool {
        match self {
            AppError::Api { status, .. } => *status == 429 || *status >= 500,
            AppError::Network(e) => e.is_timeout() || e.is_connect(),
            _ => false,
        }
    }

    /// Distinct exit codes make the tool scriptable from bash.
    pub fn exit_code(&self) -> i32 {
        match self {
            AppError::MissingApiKey | AppError::EmptyPrompt => 2,
            AppError::Api { .. } => 3,
            AppError::Network(_) => 4,
            AppError::Parse(_) | AppError::NoContent => 5,
            AppError::Io(_) => 6,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api(status: u16) -> AppError {
        AppError::Api {
            status,
            message: String::new(),
        }
    }

    #[test]
    fn rate_limit_and_server_errors_are_retryable() {
        assert!(api(429).is_retryable());
        assert!(api(500).is_retryable());
        assert!(api(529).is_retryable());
    }

    #[test]
    fn client_errors_are_not_retryable() {
        assert!(!api(400).is_retryable());
        assert!(!api(401).is_retryable());
        assert!(!AppError::MissingApiKey.is_retryable());
    }
}
