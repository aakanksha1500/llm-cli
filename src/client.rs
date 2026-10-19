//! A small client for the Anthropic Messages API.
//!
//! Request and response types are plain structs; `serde` generates the JSON
//! conversion code at compile time from the `derive` attributes.

use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::AppError;

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";

// ---------- Request types (Rust -> JSON) ----------

#[derive(Debug, Serialize)]
struct Message<'a> {
    role: &'a str,
    content: &'a str,
}

/// The `'a` lifetime says: this struct borrows its strings from somewhere
/// else and must not outlive them. No copying of the prompt is needed.
#[derive(Debug, Serialize)]
struct Request<'a> {
    model: &'a str,
    max_tokens: u32,
    messages: Vec<Message<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<&'a str>,
}

// ---------- Response types (JSON -> Rust) ----------

/// Content blocks are tagged by their `type` field. Anything we don't model
/// lands in `Other` instead of failing the whole parse.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ContentBlock {
    Text {
        text: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct ApiResponse {
    content: Vec<ContentBlock>,
    model: String,
    stop_reason: Option<String>,
    usage: Usage,
}

#[derive(Debug, Deserialize)]
struct ApiErrorBody {
    error: ApiErrorDetail,
}

#[derive(Debug, Deserialize)]
struct ApiErrorDetail {
    message: String,
}

/// What callers get back: owned data, no references into the HTTP body.
#[derive(Debug)]
pub struct Completion {
    pub text: String,
    pub model: String,
    pub stop_reason: Option<String>,
    pub usage: Usage,
}

pub struct Client {
    http: reqwest::blocking::Client,
    api_key: String,
    max_retries: u32,
}

impl Client {
    /// Takes ownership of `api_key`: the client now owns the String and
    /// frees it when the client is dropped.
    pub fn new(api_key: String, timeout_secs: u64, max_retries: u32) -> Result<Self, AppError> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()?; // `?` converts reqwest::Error into AppError::Network
        Ok(Self {
            http,
            api_key,
            max_retries,
        })
    }

    /// Borrows `self`, `prompt` and `model` immutably (`&`). Nothing is moved,
    /// so the caller can keep using them afterwards.
    pub fn complete(
        &self,
        prompt: &str,
        model: &str,
        max_tokens: u32,
        system: Option<&str>,
    ) -> Result<Completion, AppError> {
        let request = Request {
            model,
            max_tokens,
            messages: vec![Message {
                role: "user",
                content: prompt,
            }],
            system,
        };

        let mut attempt = 0;
        loop {
            match self.send(&request) {
                Ok(c) => return Ok(c),
                Err(e) if e.is_retryable() && attempt < self.max_retries => {
                    attempt += 1;
                    let wait = Duration::from_millis(500 * 2u64.pow(attempt)); // 1s, 2s, 4s...
                    eprintln!(
                        "warning: {e}; retrying in {wait:?} ({attempt}/{})",
                        self.max_retries
                    );
                    thread::sleep(wait);
                }
                Err(e) => return Err(e),
            }
        }
    }

    fn send(&self, request: &Request) -> Result<Completion, AppError> {
        let response = self
            .http
            .post(API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .json(request)
            .send()?;

        let status = response.status();
        let body = response.text()?;

        if !status.is_success() {
            return Err(api_error(status.as_u16(), &body));
        }
        parse_completion(&body)
    }
}

/// Pull a readable message out of an error body, falling back to the raw text.
fn api_error(status: u16, body: &str) -> AppError {
    let message = serde_json::from_str::<ApiErrorBody>(body)
        .map(|b| b.error.message)
        .unwrap_or_else(|_| body.trim().to_string());
    AppError::Api { status, message }
}

/// Kept separate from the HTTP call so it can be unit tested without a network.
fn parse_completion(body: &str) -> Result<Completion, AppError> {
    let parsed: ApiResponse = serde_json::from_str(body)?;

    // Iterator chain: keep only text blocks, then join them.
    let text: Vec<String> = parsed
        .content
        .into_iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text),
            ContentBlock::Other => None,
        })
        .collect();

    if text.is_empty() {
        return Err(AppError::NoContent);
    }

    Ok(Completion {
        text: text.join("\n"),
        model: parsed.model,
        stop_reason: parsed.stop_reason,
        usage: parsed.usage,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_normal_response() {
        let body = r#"{
            "id": "msg_1", "type": "message", "role": "assistant",
            "model": "claude-test",
            "content": [{"type": "text", "text": "Hello!"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 3}
        }"#;
        let c = parse_completion(body).unwrap();
        assert_eq!(c.text, "Hello!");
        assert_eq!(c.model, "claude-test");
        assert_eq!(c.usage.output_tokens, 3);
    }

    #[test]
    fn ignores_unknown_block_types() {
        let body = r#"{
            "model": "m", "stop_reason": null,
            "content": [{"type": "thinking", "thinking": "..."}, {"type": "text", "text": "ok"}],
            "usage": {"input_tokens": 1, "output_tokens": 1}
        }"#;
        assert_eq!(parse_completion(body).unwrap().text, "ok");
    }

    #[test]
    fn no_text_blocks_is_an_error() {
        let body = r#"{"model": "m", "stop_reason": null, "content": [],
                       "usage": {"input_tokens": 1, "output_tokens": 0}}"#;
        assert!(matches!(parse_completion(body), Err(AppError::NoContent)));
    }

    #[test]
    fn malformed_json_is_a_parse_error() {
        assert!(matches!(
            parse_completion("not json"),
            Err(AppError::Parse(_))
        ));
    }

    #[test]
    fn extracts_message_from_api_error_body() {
        let body = r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#;
        match api_error(401, body) {
            AppError::Api { status, message } => {
                assert_eq!(status, 401);
                assert_eq!(message, "invalid x-api-key");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn falls_back_to_raw_body_when_error_is_not_json() {
        match api_error(502, "Bad Gateway\n") {
            AppError::Api { message, .. } => assert_eq!(message, "Bad Gateway"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn request_omits_system_when_none() {
        let req = Request {
            model: "m",
            max_tokens: 10,
            messages: vec![Message {
                role: "user",
                content: "hi",
            }],
            system: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("system"));
    }
}
