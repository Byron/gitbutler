//! Client for the GitButler web API authentication endpoints.
//!
//! These functions make server-side HTTP calls to `app.gitbutler.com` (or staging)
//! so that browser-based frontends don't need to make cross-origin requests.
//! They are also usable from the CLI (`but auth`) without any web framework dependency.
//!
//! The public API is synchronous — async HTTP calls are executed on a dedicated
//! thread with a short-lived Tokio runtime, following the same pattern as `but-forge`.

use std::time::Duration;

use anyhow::{Context, Result};
use but_path::AppChannel;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .expect("failed to build HTTP client")
}

/// Error returned when the upstream API rejects a request with an HTTP error status.
#[derive(Debug, thiserror::Error)]
#[error("API request failed ({status}): {body}")]
pub struct ApiHttpError {
    pub status: StatusCode,
    pub body: String,
}

/// Returns the GitButler API base URL.
///
/// Resolution order:
/// 1. `GITBUTLER_API_URL` env var at runtime (backend-specific escape hatch)
/// 2. `PUBLIC_API_BASE_URL` env var at runtime (shared with the desktop frontend)
/// 3. Compile-time [`AppChannel`]:
///    - `Release` / `Nightly` → `https://app.gitbutler.com`
///    - `Dev` → `https://app.staging.gitbutler.com`
pub fn default_api_url() -> String {
    if let Some(url) = api_url_override_from_env(|key| std::env::var(key).ok()) {
        return url;
    }
    match AppChannel::new() {
        AppChannel::Release | AppChannel::Nightly => "https://app.gitbutler.com",
        AppChannel::Dev => "https://app.staging.gitbutler.com",
    }
    .to_string()
}

fn normalize_api_url(url: String) -> String {
    url.trim_end_matches('/').to_string()
}

fn api_url_override_from_env(mut get_var: impl FnMut(&str) -> Option<String>) -> Option<String> {
    get_var("GITBUTLER_API_URL")
        .or_else(|| get_var("PUBLIC_API_BASE_URL"))
        .map(normalize_api_url)
}

/// Response from `POST /api/login/token.json`.
#[cfg_attr(feature = "export-schema", derive(schemars::JsonSchema))]
#[derive(Debug, Deserialize, Serialize)]
pub struct LoginToken {
    /// Polling token returned by the API for completing login.
    ///
    /// This value is sensitive. Although it is obtained via a server-side HTTP
    /// call, this struct may be returned from backend commands to browser-based
    /// frontends, so callers must avoid exposing or logging it unnecessarily.
    pub token: String,
    /// Token shown to the user after authentication on gitbutler.com.
    pub browser_token: String,
    /// Expiration timestamp.
    pub expires: String,
    /// The full URL to redirect the user's browser to for login.
    pub url: String,
}
#[cfg(feature = "export-schema")]
but_schemars::register_sdk_type!(LoginToken);

/// Request a new login token from the GitButler API.
///
/// The returned [`LoginToken::url`] should be opened in the user's browser.
/// After the user authenticates, they receive a token that can be validated
/// with [`fetch_user_by_token`].
pub fn fetch_login_token() -> Result<LoginToken> {
    let api_url = default_api_url();
    run_async(async move {
        let url = format!("{api_url}/api/login/token.json");
        let client = http_client();
        let resp = client
            .post(&url)
            .send()
            .await
            .context("Failed to reach GitButler API for login token")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ApiHttpError { status, body }.into());
        }
        resp.json()
            .await
            .context("Failed to parse login token response")
    })
}

/// Validate an access token and return the user info from the GitButler API.
///
/// Calls `GET /api/login/whoami` with the given token. On success the user
/// object is returned as a [`serde_json::Value`] so callers can deserialize
/// into whatever type they need.
pub fn fetch_user_by_token(token: &str) -> Result<serde_json::Value> {
    let api_url = default_api_url();
    let token = token.to_string();
    run_async(async move {
        let url = format!("{api_url}/api/login/whoami");
        let client = http_client();
        let resp = client
            .get(&url)
            .header("X-Auth-Token", &token)
            .send()
            .await
            .context("Failed to reach GitButler API for token validation")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ApiHttpError { status, body }.into());
        }
        resp.json().await.context("Failed to parse whoami response")
    })
}

/// Execute an async future on a dedicated thread with its own Tokio runtime.
///
/// This keeps the crate's public API synchronous while still using async HTTP
/// internally, following the same pattern as `but-forge`.
fn run_async<F, T>(future: F) -> Result<T>
where
    F: std::future::Future<Output = Result<T>> + Send + 'static,
    T: Send + 'static,
{
    std::thread::spawn(move || {
        tokio::runtime::Runtime::new()
            .expect("failed to create tokio runtime")
            .block_on(future)
    })
    .join()
    .map_err(|e| anyhow::anyhow!("thread panicked: {e:?}"))?
}

#[cfg(test)]
mod tests {
    use super::api_url_override_from_env;

    #[test]
    fn prefers_backend_specific_override() {
        let url = api_url_override_from_env(|key| match key {
            "GITBUTLER_API_URL" => Some("https://backend.example.com".to_string()),
            "PUBLIC_API_BASE_URL" => Some("https://frontend.example.com".to_string()),
            _ => None,
        });

        assert_eq!(url.as_deref(), Some("https://backend.example.com"));
    }

    #[test]
    fn falls_back_to_shared_frontend_override() {
        let url = api_url_override_from_env(|key| match key {
            "PUBLIC_API_BASE_URL" => Some("https://frontend.example.com".to_string()),
            _ => None,
        });

        assert_eq!(url.as_deref(), Some("https://frontend.example.com"));
    }
}
