//! Shared HTTP client: honest User-Agent, global rate limit, polite retries,
//! and credential redaction. Never retries around 401/403.

use std::time::Duration;

use reqwest::{header, StatusCode};
use serde_json::Value;
use tokio::sync::Mutex;
use tokio::time::{sleep, Instant};

use crate::error::{AppError, AppResult};

pub const USER_AGENT: &str = concat!(
    "VIBDL/",
    env!("CARGO_PKG_VERSION"),
    " (personal desktop library client)"
);

const MAX_BACKOFF: Duration = Duration::from_secs(30);
const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);

/// Serializes requests so at most one starts per `min_interval`.
pub struct RateLimiter {
    min_interval: Duration,
    next_slot: Mutex<Instant>,
}

impl RateLimiter {
    pub fn new(min_interval: Duration) -> Self {
        Self {
            min_interval,
            next_slot: Mutex::new(Instant::now()),
        }
    }

    pub async fn acquire(&self) {
        let mut next = self.next_slot.lock().await;
        let now = Instant::now();
        if *next > now {
            sleep(*next - now).await;
        }
        *next = Instant::now() + self.min_interval;
    }
}

pub struct ApiClient {
    http: reqwest::Client,
    limiter: RateLimiter,
    max_retries: u32,
}

/// Plain HTTP client with VIBDL's User-Agent and sane timeouts (used for
/// static files like thumbnails; API calls go through `ApiClient`).
pub fn build_http() -> AppResult<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()?)
}

impl ApiClient {
    pub fn new(min_interval: Duration) -> AppResult<Self> {
        let http = build_http()?;
        Ok(Self {
            http,
            limiter: RateLimiter::new(min_interval),
            max_retries: 3,
        })
    }

    /// GET a JSON API endpoint. An empty body is returned as `Value::Null`.
    pub async fn get_json(&self, url: &str, query: &[(&str, String)]) -> AppResult<Value> {
        let mut attempt = 0u32;
        loop {
            self.limiter.acquire().await;
            let started = Instant::now();
            let result = self.http.get(url).query(query).send().await;

            match result {
                Err(e) => {
                    if attempt < self.max_retries && (e.is_timeout() || e.is_connect()) {
                        tracing::warn!(attempt, "network error, retrying: {}", AppError::from(e));
                        sleep(backoff(attempt)).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(e.into());
                }
                Ok(resp) => {
                    let status = resp.status();
                    tracing::debug!(
                        status = status.as_u16(),
                        ms = started.elapsed().as_millis() as u64,
                        "GET {}",
                        redact(resp.url().as_str())
                    );
                    if status.is_success() {
                        let text = resp.text().await?;
                        return parse_json_body(&text);
                    }
                    match status {
                        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                            return Err(AppError::Unauthorized(status.as_u16()));
                        }
                        s if (s == StatusCode::TOO_MANY_REQUESTS || s.is_server_error())
                            && attempt < self.max_retries =>
                        {
                            let wait = retry_after(&resp).unwrap_or_else(|| backoff(attempt));
                            tracing::warn!(status = s.as_u16(), wait_s = wait.as_secs(), "backing off");
                            sleep(wait).await;
                            attempt += 1;
                        }
                        StatusCode::TOO_MANY_REQUESTS => return Err(AppError::RateLimited),
                        s => return Err(AppError::Http(s.as_u16())),
                    }
                }
            }
        }
    }
}

fn parse_json_body(text: &str) -> AppResult<Value> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(trimmed).map_err(|e| {
        let preview: String = trimmed.chars().take(120).collect();
        AppError::Parse(format!("{e}; body starts with: {}", redact(&preview)))
    })
}

fn backoff(attempt: u32) -> Duration {
    Duration::from_secs(1u64 << attempt.min(5)).min(MAX_BACKOFF)
}

fn retry_after(resp: &reqwest::Response) -> Option<Duration> {
    resp.headers()
        .get(header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(|s| Duration::from_secs(s).min(MAX_RETRY_AFTER))
}

/// Mask the values of credential-bearing query parameters anywhere in `s`.
pub fn redact(s: &str) -> String {
    const KEYS: [&str; 2] = ["api_key=", "user_id="];
    let mut out = s.to_string();
    for key in KEYS {
        let mut search_from = 0;
        while let Some(pos) = out[search_from..].find(key) {
            let start = search_from + pos + key.len();
            let end = out[start..]
                .find(['&', ' ', '"', '\'', ')'])
                .map(|i| start + i)
                .unwrap_or(out.len());
            out.replace_range(start..end, "REDACTED");
            search_from = start + "REDACTED".len();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_credentials() {
        let url = "https://x/index.php?page=dapi&api_key=SECRET123&user_id=42&tags=a";
        let r = redact(url);
        assert!(!r.contains("SECRET123"));
        assert!(!r.contains("=42"));
        assert!(r.contains("api_key=REDACTED&user_id=REDACTED&tags=a"));
        assert_eq!(redact("api_key=abc"), "api_key=REDACTED");
        assert_eq!(redact("no secrets here"), "no secrets here");
    }

    #[test]
    fn empty_body_is_null() {
        assert_eq!(parse_json_body("  \n").unwrap(), Value::Null);
        assert!(parse_json_body("<html>").is_err());
    }

    #[test]
    fn backoff_grows_and_caps() {
        assert_eq!(backoff(0), Duration::from_secs(1));
        assert_eq!(backoff(2), Duration::from_secs(4));
        assert_eq!(backoff(10), MAX_BACKOFF);
    }

    #[tokio::test]
    async fn rate_limiter_spaces_requests() {
        let rl = RateLimiter::new(Duration::from_millis(60));
        let t0 = Instant::now();
        rl.acquire().await;
        rl.acquire().await;
        rl.acquire().await;
        assert!(t0.elapsed() >= Duration::from_millis(115));
    }
}
