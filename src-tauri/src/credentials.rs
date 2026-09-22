//! Reads and writes the credentials Claude Code keeps on disk.
//!
//! * macOS: the keychain item `Claude Code-credentials` (read through `/usr/bin/security`
//!   so the user can grant "Always Allow" once and never see a prompt again).
//! * Linux / Windows: `~/.claude/.credentials.json`.
//! * All platforms: the `oauthAccount` block in `~/.claude.json`, which tells the CLI
//!   which account the stored token belongs to.
//!
//! `CLAUDE_CONFIG_DIR` is honoured the same way the CLI honours it.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::error::{AppError, Result};

#[cfg(target_os = "macos")]
pub const KEYCHAIN_SERVICE: &str = "Claude Code-credentials";

#[derive(Debug, Clone)]
pub struct ClaudePaths {
    pub config_dir: PathBuf,
    pub claude_json: PathBuf,
    pub credentials_file: PathBuf,
}

pub fn paths() -> ClaudePaths {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let config_dir = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| home.join(".claude"));
    let claude_json = if std::env::var_os("CLAUDE_CONFIG_DIR").is_some() {
        config_dir.join(".claude.json")
    } else {
        home.join(".claude.json")
    };
    ClaudePaths {
        credentials_file: config_dir.join(".credentials.json"),
        config_dir,
        claude_json,
    }
}

/// Fields of the credential JSON we care about.
#[derive(Debug, Clone)]
pub struct TokenInfo {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Unix milliseconds.
    pub expires_at: Option<i64>,
    pub subscription_type: Option<String>,
    pub rate_limit_tier: Option<String>,
}

pub fn parse_token(token_json: &str) -> Option<TokenInfo> {
    let root: Value = serde_json::from_str(token_json).ok()?;
    let oauth = root.get("claudeAiOauth")?;
    Some(TokenInfo {
        access_token: oauth.get("accessToken")?.as_str()?.to_string(),
        refresh_token: oauth.get("refreshToken").and_then(Value::as_str).map(str::to_string),
        expires_at: oauth.get("expiresAt").and_then(Value::as_i64),
        subscription_type: oauth
            .get("subscriptionType")
            .and_then(Value::as_str)
            .map(str::to_string),
        rate_limit_tier: oauth.get("rateLimitTier").and_then(Value::as_str).map(str::to_string),
    })
}

/// Last 8 characters of the access token: enough to tell two tokens apart, safe to log.
pub fn fingerprint(token_json: &str) -> Option<String> {
    let info = parse_token(token_json)?;
    let n = info.access_token.len();
    Some(info.access_token[n.saturating_sub(8)..].to_string())
}

pub fn oauth_email(oauth_account: &Value) -> Option<String> {
    oauth_account
        .get("emailAddress")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
}

// ---------------------------------------------------------------------------
// Live token
// ---------------------------------------------------------------------------

/// Returns the credential JSON the CLI currently uses, or `None` when nobody is logged in.
pub fn read_live_token() -> Result<Option<String>> {
    if let Some(token) = platform_store_read()? {
        return Ok(Some(token));
    }
    // The CLI falls back to the file when the platform store is unavailable.
    read_credentials_file()
}

pub fn write_live_token(token_json: &str) -> Result<()> {
    // Validate before touching anything the CLI depends on.
    parse_token(token_json).ok_or_else(|| AppError::Credentials("credential JSON is malformed".into()))?;
    // Only use the file when the CLI itself is using the file on this machine.
    if force_file_mode() || (paths().credentials_file.exists() && platform_store_read()?.is_none()) {
        return write_credentials_file(token_json);
    }
    platform_store_write(token_json)
}

/// `CLAUDE_ACCOUNT_SWITCHER_FILE_CREDENTIALS=1` bypasses the platform store (tests, or a CLI
/// that was configured to use the credentials file).
fn force_file_mode() -> bool {
    std::env::var_os("CLAUDE_ACCOUNT_SWITCHER_FILE_CREDENTIALS").is_some_and(|v| !v.is_empty() && v != "0")
}

#[cfg(target_os = "macos")]
fn platform_store_read() -> Result<Option<String>> {
    if force_file_mode() {
        return Ok(None);
    }
    keychain::read()
}

#[cfg(target_os = "macos")]
fn platform_store_write(token_json: &str) -> Result<()> {
    keychain::write(token_json)
}

#[cfg(not(target_os = "macos"))]
fn platform_store_read() -> Result<Option<String>> {
    Ok(None)
}

#[cfg(not(target_os = "macos"))]
fn platform_store_write(token_json: &str) -> Result<()> {
    write_credentials_file(token_json)
}

fn read_credentials_file() -> Result<Option<String>> {
    let p = paths().credentials_file;
    if !p.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&p)?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    Ok(Some(trimmed.to_string()))
}

fn write_credentials_file(token_json: &str) -> Result<()> {
    let p = paths();
    fs::create_dir_all(&p.config_dir)?;
    atomic_write(&p.credentials_file, token_json.as_bytes())?;
    restrict_permissions(&p.credentials_file);
    Ok(())
}

// ---------------------------------------------------------------------------
// ~/.claude.json oauthAccount
// ---------------------------------------------------------------------------

pub fn read_oauth_account() -> Result<Option<Value>> {
    let p = paths().claude_json;
    if !p.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&p)?;
    let root: Value = serde_json::from_str(&raw)
        .map_err(|e| AppError::Credentials(format!("~/.claude.json is not valid JSON: {e}")))?;
    Ok(root.get("oauthAccount").filter(|v| v.is_object()).cloned())
}

pub fn write_oauth_account(oauth_account: &Value) -> Result<()> {
    let p = paths().claude_json;
    let mut root: Map<String, Value> = if p.exists() {
        let raw = fs::read_to_string(&p)?;
        serde_json::from_str::<Value>(&raw)
            .ok()
            .and_then(|v| v.as_object().cloned())
            .ok_or_else(|| AppError::Credentials("~/.claude.json is not a JSON object".into()))?
    } else {
        Map::new()
    };
    root.insert("oauthAccount".into(), oauth_account.clone());
    let out = serde_json::to_vec_pretty(&Value::Object(root))?;
    atomic_write(&p, &out)
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(&tmp, bytes)?;
    restrict_permissions(&tmp);
    if let Err(e) = fs::rename(&tmp, path) {
        // Windows refuses to rename over an existing file in some situations.
        let _ = fs::remove_file(path);
        fs::rename(&tmp, path).map_err(|_| e)?;
    }
    Ok(())
}

#[cfg(unix)]
pub fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
pub fn restrict_permissions(_path: &Path) {}

/// Owner-only directory (needs the execute bit to stay traversable).
#[cfg(unix)]
pub fn restrict_dir_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
}

#[cfg(not(unix))]
pub fn restrict_dir_permissions(_path: &Path) {}

#[cfg(target_os = "macos")]
mod keychain {
    use super::*;
    use std::process::{Command, Stdio};

    fn account_name() -> String {
        if let Ok(user) = std::env::var("USER") {
            if !user.is_empty() {
                return user;
            }
        }
        Command::new("/usr/bin/whoami")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "unknown".into())
    }

    pub fn read() -> Result<Option<String>> {
        let out = Command::new("/usr/bin/security")
            .args([
                "find-generic-password",
                "-s",
                KEYCHAIN_SERVICE,
                "-a",
                &account_name(),
                "-w",
            ])
            .stdin(Stdio::null())
            .output()?;
        if !out.status.success() {
            // Item not found is the normal "logged out" state.
            return Ok(None);
        }
        let token = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if token.is_empty() {
            return Ok(None);
        }
        Ok(Some(token))
    }

    pub fn write(token_json: &str) -> Result<()> {
        let account = account_name();
        // Delete then add: `-U` alone does not always replace the ACL-owned item.
        let _ = Command::new("/usr/bin/security")
            .args(["delete-generic-password", "-s", KEYCHAIN_SERVICE, "-a", &account])
            .stdin(Stdio::null())
            .output();
        let out = Command::new("/usr/bin/security")
            .args([
                "add-generic-password",
                "-s",
                KEYCHAIN_SERVICE,
                "-a",
                &account,
                "-w",
                token_json,
                "-U",
            ])
            .stdin(Stdio::null())
            .output()?;
        if !out.status.success() {
            return Err(AppError::Credentials(format!(
                "keychain write failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-abcdefgh-XYZ12345","refreshToken":"sk-ant-ort01-r","expiresAt":1790000000000,"scopes":["user:inference"],"subscriptionType":"max","rateLimitTier":"default_claude_max_20x"}}"#;

    #[test]
    fn parses_token() {
        let t = parse_token(SAMPLE).unwrap();
        assert_eq!(t.access_token, "sk-ant-oat01-abcdefgh-XYZ12345");
        assert_eq!(t.expires_at, Some(1790000000000));
        assert_eq!(t.subscription_type.as_deref(), Some("max"));
    }

    #[test]
    fn fingerprint_is_last_eight() {
        assert_eq!(fingerprint(SAMPLE).as_deref(), Some("XYZ12345"));
        assert!(fingerprint("{}").is_none());
    }

    /// File-mode round trip through a scratch config dir: the same code path Linux and
    /// Windows use, and what macOS uses when the CLI stores credentials in a file.
    #[test]
    fn file_mode_round_trip() {
        let dir = std::env::temp_dir().join(format!("cas-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        std::env::set_var("CLAUDE_CONFIG_DIR", &dir);
        std::env::set_var("CLAUDE_ACCOUNT_SWITCHER_FILE_CREDENTIALS", "1");
        fs::write(
            dir.join(".claude.json"),
            r#"{"numStartups": 3, "projects": {"/x": {}}}"#,
        )
        .unwrap();

        assert_eq!(read_live_token().unwrap(), None);
        assert_eq!(read_oauth_account().unwrap(), None);

        write_live_token(SAMPLE).unwrap();
        assert_eq!(read_live_token().unwrap().as_deref(), Some(SAMPLE));
        assert!(write_live_token("{\"nope\": 1}").is_err());

        let account = serde_json::json!({"emailAddress": "a@b.c", "organizationName": "Org"});
        write_oauth_account(&account).unwrap();
        assert_eq!(read_oauth_account().unwrap(), Some(account));
        let root: Value = serde_json::from_str(&fs::read_to_string(dir.join(".claude.json")).unwrap()).unwrap();
        assert_eq!(root["numStartups"], 3, "other keys must survive");
        assert!(root["projects"].is_object());

        std::env::remove_var("CLAUDE_CONFIG_DIR");
        std::env::remove_var("CLAUDE_ACCOUNT_SWITCHER_FILE_CREDENTIALS");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn email_is_normalised() {
        let v = serde_json::json!({"emailAddress": "  Foo@Example.COM "});
        assert_eq!(oauth_email(&v).as_deref(), Some("foo@example.com"));
        assert!(oauth_email(&serde_json::json!({})).is_none());
    }
}
