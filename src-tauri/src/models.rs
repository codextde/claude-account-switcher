use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A Claude account known to the switcher. Credentials live in `Backup`, not here.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: Uuid,
    pub email: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub organization_name: Option<String>,
    #[serde(default)]
    pub subscription_type: Option<String>,
    #[serde(default)]
    pub rate_limit_tier: Option<String>,
    /// Unix milliseconds.
    pub added_at: i64,
    #[serde(default)]
    pub last_active_at: Option<i64>,
}

/// Everything needed to make an account the active one again:
/// the credential JSON the CLI stores plus the `oauthAccount` block of `~/.claude.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Backup {
    pub token_json: String,
    pub oauth_account: serde_json::Value,
    pub saved_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TrayMode {
    /// Progress bar icon plus percentage text (text only on macOS).
    #[default]
    Both,
    /// Progress bar icon only.
    Bar,
    /// Percentage text only (falls back to the bar on platforms without tray titles).
    Percent,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TrayWindow {
    FiveHour,
    SevenDay,
    /// Whichever window is closer to its limit.
    #[default]
    Max,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub auto_switch: bool,
    /// Switch away from the active account once its binding utilization reaches this (0-100).
    pub threshold: f64,
    /// A candidate must sit at least this far below the threshold to be eligible.
    pub hysteresis: f64,
    pub poll_interval_secs: u64,
    pub switch_cooldown_secs: u64,
    pub launch_at_login: bool,
    pub notifications: bool,
    /// Download new releases in the background and install them when the app is idle.
    pub auto_update: bool,
    pub tray_mode: TrayMode,
    pub tray_window: TrayWindow,
    /// Optional explicit path to the `claude` executable.
    pub cli_path: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_switch: true,
            threshold: 90.0,
            hysteresis: 10.0,
            poll_interval_secs: 60,
            switch_cooldown_secs: 600,
            launch_at_login: false,
            notifications: true,
            auto_update: true,
            tray_mode: TrayMode::Both,
            tray_window: TrayWindow::Max,
            cli_path: None,
        }
    }
}

impl Settings {
    pub fn sanitized(mut self) -> Self {
        self.threshold = self.threshold.clamp(50.0, 100.0);
        self.hysteresis = self.hysteresis.clamp(0.0, 50.0);
        self.poll_interval_secs = self.poll_interval_secs.clamp(30, 3600);
        self.switch_cooldown_secs = self.switch_cooldown_secs.clamp(0, 86_400);
        self.cli_path = self.cli_path.map(|p| p.trim().to_string()).filter(|p| !p.is_empty());
        self
    }
}

/// One rate-limit window as returned by `/api/oauth/usage`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    /// Percent, 0-100.
    pub utilization: Option<f64>,
    /// RFC 3339 timestamp.
    #[serde(alias = "resets_at")]
    pub resets_at: Option<String>,
}

impl UsageWindow {
    pub fn resets_at_ms(&self) -> Option<i64> {
        let raw = self.resets_at.as_deref()?;
        chrono::DateTime::parse_from_rfc3339(raw)
            .ok()
            .map(|d| d.timestamp_millis())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExtraUsage {
    #[serde(alias = "is_enabled")]
    pub is_enabled: Option<bool>,
    #[serde(alias = "monthly_limit")]
    pub monthly_limit: Option<f64>,
    #[serde(alias = "used_credits")]
    pub used_credits: Option<f64>,
    pub utilization: Option<f64>,
}

/// Parsed `/api/oauth/usage` response. Unknown windows are ignored.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    #[serde(alias = "five_hour")]
    pub five_hour: Option<UsageWindow>,
    #[serde(alias = "seven_day")]
    pub seven_day: Option<UsageWindow>,
    #[serde(alias = "seven_day_opus")]
    pub seven_day_opus: Option<UsageWindow>,
    #[serde(alias = "seven_day_sonnet")]
    pub seven_day_sonnet: Option<UsageWindow>,
    #[serde(alias = "extra_usage")]
    pub extra_usage: Option<ExtraUsage>,
}

/// Latest usage sample for one account, plus fetch bookkeeping.
#[derive(Debug, Clone, Default)]
pub struct AccountUsage {
    pub usage: Option<Usage>,
    pub error: Option<String>,
    pub fetched_at: Option<i64>,
    /// Do not poll again before this time (429 back-off, dead refresh token, ...).
    pub backoff_until: Option<i64>,
    /// The stored refresh token was rejected; only a fresh login helps.
    pub needs_reauth: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CliInfo {
    pub available: bool,
    pub path: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LoginState {
    pub in_progress: bool,
    /// Login URL printed by the CLI, if the browser did not open by itself.
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEvent {
    pub kind: String,
    pub message: String,
    pub at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateStage {
    /// Nothing pending. `version` is `None`.
    #[default]
    Idle,
    /// Fetching the release manifest.
    Checking,
    /// A newer build is being downloaded and verified.
    Downloading,
    /// A verified build is on disk and installs on the next idle moment or on request.
    Ready,
    /// The build is being written in place; the app restarts right after.
    Installing,
}

/// Auto-update progress, shown in the tray menu and the settings window.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub stage: UpdateStage,
    /// Version of the pending update, once one is known.
    pub version: Option<String>,
    pub last_checked_at: Option<i64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    #[serde(flatten)]
    pub account: Account,
    pub is_active: bool,
    pub has_backup: bool,
    pub needs_reauth: bool,
    pub usage: Option<Usage>,
    pub usage_error: Option<String>,
    pub usage_fetched_at: Option<i64>,
    pub token_expires_at: Option<i64>,
}

/// Full UI state. Sent on request and pushed as the `snapshot` event on every change.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub accounts: Vec<AccountView>,
    pub active_id: Option<Uuid>,
    /// Email found in `~/.claude.json` when it belongs to no known account.
    pub unknown_active_email: Option<String>,
    pub settings: Settings,
    pub cli: CliInfo,
    pub refreshing: bool,
    pub last_refresh_at: Option<i64>,
    pub login: LoginState,
    pub last_event: Option<ActivityEvent>,
    pub update: UpdateInfo,
    pub platform: &'static str,
    pub version: &'static str,
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
