//! Talks to Anthropic's OAuth usage endpoint and refreshes access tokens.

use std::time::Duration;

use serde_json::Value;

use crate::credentials;
use crate::models::{now_ms, Usage};

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const TOKEN_URL: &str = "https://console.anthropic.com/v1/oauth/token";
/// Claude Code's public OAuth client id (PKCE public client, not a secret).
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const BETA_HEADER: &str = "oauth-2025-04-20";

#[derive(Debug)]
pub enum UsageError {
    /// 401: the access token is expired or revoked.
    Expired,
    /// 403: usually no active Pro/Max subscription on this account.
    Forbidden,
    /// 429 with an optional Retry-After in seconds.
    RateLimited(Option<u64>),
    Network(String),
    Decode(String),
}

impl std::fmt::Display for UsageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UsageError::Expired => write!(f, "Token expired"),
            UsageError::Forbidden => write!(f, "No active subscription for this account"),
            UsageError::RateLimited(Some(s)) => write!(f, "Rate limited, retry in {s}s"),
            UsageError::RateLimited(None) => write!(f, "Rate limited"),
            UsageError::Network(m) => write!(f, "Network error: {m}"),
            UsageError::Decode(m) => write!(f, "Unexpected response: {m}"),
        }
    }
}

pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(format!("claude-account-switcher/{}", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(30))
        .build()
        .expect("reqwest client")
}

pub async fn fetch_usage(client: &reqwest::Client, access_token: &str) -> Result<Usage, UsageError> {
    let resp = client
        .get(USAGE_URL)
        .bearer_auth(access_token)
        .header("anthropic-beta", BETA_HEADER)
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|e| UsageError::Network(e.to_string()))?;

    let status = resp.status();
    let retry_after = resp
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok());
    let body = resp.text().await.map_err(|e| UsageError::Network(e.to_string()))?;

    match status.as_u16() {
        200 => serde_json::from_str::<Usage>(&body).map_err(|e| UsageError::Decode(e.to_string())),
        401 => Err(UsageError::Expired),
        403 => Err(UsageError::Forbidden),
        429 => Err(UsageError::RateLimited(retry_after)),
        _ if body.contains("token_expired") => Err(UsageError::Expired),
        code => Err(UsageError::Network(format!("HTTP {code}"))),
    }
}

pub enum RefreshResult {
    /// New credential JSON, ready to store.
    Success(String),
    /// The refresh token is dead; only a new login helps.
    Rejected,
    /// Network or server trouble; the token's state is unknown.
    Transient,
}

/// Exchanges the refresh token inside `token_json` for a new access token.
/// Never touches the CLI's own storage; the caller decides where the result goes.
pub async fn refresh(client: &reqwest::Client, token_json: &str) -> RefreshResult {
    let Some(info) = credentials::parse_token(token_json) else {
        return RefreshResult::Rejected;
    };
    let Some(refresh_token) = info.refresh_token.filter(|t| !t.is_empty()) else {
        log::warn!("[refresh] stored credential has no refresh token");
        return RefreshResult::Rejected;
    };

    let resp = client
        .post(TOKEN_URL)
        .json(&serde_json::json!({
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
            "client_id": CLIENT_ID,
        }))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            log::warn!("[refresh] request failed: {e}");
            return RefreshResult::Transient;
        }
    };
    let status = resp.status().as_u16();
    let body: Value = match resp.json().await {
        Ok(v) => v,
        Err(_) => {
            return if status == 200 {
                RefreshResult::Transient
            } else {
                RefreshResult::Rejected
            }
        }
    };

    if status != 200 {
        // Only `invalid_grant` means the refresh token itself is dead (RFC 6749 §5.2).
        let code = body.get("error").and_then(Value::as_str).unwrap_or("none");
        log::warn!("[refresh] not applied (HTTP {status}, error={code})");
        return if code == "invalid_grant" {
            RefreshResult::Rejected
        } else {
            RefreshResult::Transient
        };
    }

    let (Some(access_token), Some(expires_in)) = (
        body.get("access_token").and_then(Value::as_str),
        body.get("expires_in").and_then(Value::as_f64),
    ) else {
        // A 200 we cannot parse is a schema change, not a dead grant.
        log::warn!("[refresh] 200 with unexpected shape");
        return RefreshResult::Transient;
    };

    let mut root: Value = match serde_json::from_str(token_json) {
        Ok(v) => v,
        Err(_) => return RefreshResult::Rejected,
    };
    let Some(oauth) = root.get_mut("claudeAiOauth").and_then(Value::as_object_mut) else {
        return RefreshResult::Rejected;
    };
    oauth.insert("accessToken".into(), Value::String(access_token.to_string()));
    oauth.insert("expiresAt".into(), Value::from(now_ms() + (expires_in * 1000.0) as i64));
    if let Some(new_refresh) = body
        .get("refresh_token")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        oauth.insert("refreshToken".into(), Value::String(new_refresh.to_string()));
    }
    // `scopes` is deliberately kept from login time: the CLI only recognises a stored login
    // whose scopes contain `user:inference`, and refresh responses may narrow the scope string.
    log::info!(
        "[refresh] access token refreshed, valid for {:.0} min",
        expires_in / 60.0
    );
    match serde_json::to_string(&root) {
        Ok(json) => RefreshResult::Success(json),
        Err(_) => RefreshResult::Transient,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_usage_payload() {
        let body = r#"{
          "five_hour": {"utilization": 42.5, "resets_at": "2026-09-22T15:00:00.000000+00:00"},
          "seven_day": {"utilization": 12, "resets_at": "2026-09-27T10:00:00Z"},
          "seven_day_opus": null,
          "extra_usage": {"is_enabled": false, "monthly_limit": null, "used_credits": null, "utilization": null},
          "iguana_necktie": {"utilization": 0}
        }"#;
        let usage: Usage = serde_json::from_str(body).unwrap();
        assert_eq!(usage.five_hour.as_ref().unwrap().utilization, Some(42.5));
        assert!(usage.five_hour.as_ref().unwrap().resets_at_ms().is_some());
        assert_eq!(usage.seven_day.as_ref().unwrap().utilization, Some(12.0));
        assert!(usage.seven_day_opus.is_none());
    }
}
