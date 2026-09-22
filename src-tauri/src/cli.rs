//! Locates and runs the `claude` command line tool.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::error::{AppError, Result};
use crate::models::CliInfo;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct AuthStatus {
    #[serde(default)]
    pub logged_in: bool,
    #[serde(default)]
    pub auth_method: Option<String>,
    #[serde(default)]
    pub api_provider: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub org_id: Option<String>,
    #[serde(default)]
    pub org_name: Option<String>,
    #[serde(default)]
    pub subscription_type: Option<String>,
}

impl AuthStatus {
    pub fn email_normalised(&self) -> Option<String> {
        self.email
            .as_deref()
            .map(|e| e.trim().to_lowercase())
            .filter(|e| !e.is_empty())
    }
}

/// Directories that commonly hold `claude` but are missing from a GUI app's PATH.
fn extra_bin_dirs() -> Vec<PathBuf> {
    let mut dirs_ = Vec::new();
    if let Some(home) = dirs::home_dir() {
        for rel in [
            ".local/bin",
            ".claude/local",
            ".npm-global/bin",
            ".volta/bin",
            ".bun/bin",
            ".yarn/bin",
            ".fnm/aliases/default/bin",
            ".cargo/bin",
        ] {
            dirs_.push(home.join(rel));
        }
        // nvm keeps one bin dir per Node version.
        if let Ok(entries) = std::fs::read_dir(home.join(".nvm/versions/node")) {
            let mut versions: Vec<PathBuf> = entries.flatten().map(|e| e.path().join("bin")).collect();
            versions.sort();
            versions.reverse();
            dirs_.extend(versions);
        }
    }
    for abs in [
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/opt/local/bin",
        "/usr/bin",
        "/snap/bin",
    ] {
        dirs_.push(PathBuf::from(abs));
    }
    #[cfg(windows)]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            dirs_.push(PathBuf::from(appdata).join("npm"));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs_.push(PathBuf::from(local).join("Programs").join("claude"));
        }
    }
    dirs_
}

fn executable_names() -> &'static [&'static str] {
    #[cfg(windows)]
    {
        &["claude.cmd", "claude.exe", "claude"]
    }
    #[cfg(not(windows))]
    {
        &["claude"]
    }
}

fn is_executable(p: &Path) -> bool {
    if !p.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// A PATH that includes the usual installer locations, so subprocesses (and the CLI's own
/// `node` lookup) behave like they do in a terminal.
pub fn augmented_path() -> String {
    let mut parts: Vec<PathBuf> = extra_bin_dirs();
    if let Some(existing) = std::env::var_os("PATH") {
        let mut current: Vec<PathBuf> = std::env::split_paths(&existing).collect();
        current.append(&mut parts);
        parts = current;
    }
    let mut seen = std::collections::HashSet::new();
    parts.retain(|p| seen.insert(p.clone()));
    std::env::join_paths(parts)
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Finds the `claude` executable, honouring an explicit override first.
pub fn locate(override_path: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = override_path.map(str::trim).filter(|p| !p.is_empty()) {
        let p = PathBuf::from(p);
        if is_executable(&p) {
            return Some(p);
        }
    }
    let path_var = augmented_path();
    for dir in std::env::split_paths(&path_var) {
        for name in executable_names() {
            let candidate = dir.join(name);
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

fn build_command(exe: &Path, args: &[&str]) -> Command {
    #[cfg(windows)]
    let mut cmd = {
        // `.cmd` shims need the shell; plain executables do not.
        if exe.extension().map(|e| e.eq_ignore_ascii_case("cmd")).unwrap_or(false) {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(exe).args(args);
            c
        } else {
            let mut c = Command::new(exe);
            c.args(args);
            c
        }
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = Command::new(exe);
        c.args(args);
        c
    };
    cmd.env("PATH", augmented_path())
        .env("CLAUDE_CODE_DISABLE_AUTOUPDATE", "1")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

async fn run(exe: &Path, args: &[&str], timeout: Duration) -> Result<String> {
    let child = build_command(exe, args).output();
    let out = tokio::time::timeout(timeout, child)
        .await
        .map_err(|_| AppError::Cli(format!("`claude {}` timed out", args.join(" "))))??;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let detail = if stderr.is_empty() {
            stdout.trim().to_string()
        } else {
            stderr
        };
        return Err(AppError::Cli(format!(
            "`claude {}` exited with {}: {}",
            args.join(" "),
            out.status,
            detail.chars().take(300).collect::<String>()
        )));
    }
    Ok(stdout)
}

pub async fn detect(override_path: Option<&str>) -> CliInfo {
    let Some(exe) = locate(override_path) else {
        return CliInfo::default();
    };
    let version = run(&exe, &["--version"], Duration::from_secs(20))
        .await
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    CliInfo {
        available: true,
        path: Some(exe.to_string_lossy().into_owned()),
        version,
    }
}

/// `claude auth status` — read only, but it also makes the CLI refresh an expired token.
pub async fn auth_status(exe: &Path) -> Result<AuthStatus> {
    let out = run(exe, &["auth", "status"], Duration::from_secs(45)).await?;
    // The CLI prints JSON; tolerate a warning line before it.
    let start = out.find('{').unwrap_or(0);
    serde_json::from_str::<AuthStatus>(&out[start..])
        .map_err(|e| AppError::Cli(format!("could not parse `claude auth status`: {e}")))
}

/// Outcome of a `claude auth login` run.
pub struct LoginRun {
    pub child: tokio::process::Child,
    /// Receives the OAuth URL if the CLI prints one (browser did not open automatically).
    pub url_rx: tokio::sync::oneshot::Receiver<String>,
}

/// Starts `claude auth login`. The CLI notices it has no TTY, opens the system browser and
/// exits once the browser callback has completed. The caller awaits or kills `child`.
pub fn start_login(exe: &Path) -> Result<LoginRun> {
    let mut child = build_command(exe, &["auth", "login"]).spawn()?;
    let (tx, url_rx) = tokio::sync::oneshot::channel::<String>();
    let tx = std::sync::Arc::new(std::sync::Mutex::new(Some(tx)));
    if let Some(out) = child.stdout.take() {
        tokio::spawn(scan_for_url(out, tx.clone()));
    }
    if let Some(err) = child.stderr.take() {
        tokio::spawn(scan_for_url(err, tx));
    }
    Ok(LoginRun { child, url_rx })
}

/// Streams a CLI output pipe line by line and forwards the first OAuth URL it sees.
async fn scan_for_url<R: tokio::io::AsyncRead + Unpin>(
    reader: R,
    tx: std::sync::Arc<std::sync::Mutex<Option<tokio::sync::oneshot::Sender<String>>>>,
) {
    let url_re = regex::Regex::new(r#"https://[^\s'"]+/oauth/[^\s'"]*"#).expect("static regex");
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        log::debug!("[claude auth login] {line}");
        if let Some(m) = url_re.find(&line) {
            if let Some(sender) = tx.lock().ok().and_then(|mut guard| guard.take()) {
                let _ = sender.send(m.as_str().to_string());
            }
        }
    }
}
