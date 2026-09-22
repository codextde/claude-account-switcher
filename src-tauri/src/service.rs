//! Orchestrates accounts, credentials, usage polling and automatic switching.
//!
//! Locking model: `state` is a plain mutex that is never held across an `.await`;
//! `op` serialises every mutating operation (refresh, switch, login) so two of them
//! can never interleave their reads and writes of the CLI's credential storage.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;
use uuid::Uuid;

use crate::cli;
use crate::credentials::{self, oauth_email};
use crate::engine;
use crate::error::{AppError, Result};
use crate::models::*;
use crate::store::Store;
use crate::tray;
use crate::usage::{self, RefreshResult, UsageError};

pub const SNAPSHOT_EVENT: &str = "snapshot";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(600);

pub struct AppState {
    pub store: Store,
    pub usage: HashMap<Uuid, AccountUsage>,
    pub cli: CliInfo,
    pub refreshing: bool,
    pub login: LoginState,
    pub last_refresh_at: Option<i64>,
    pub last_event: Option<ActivityEvent>,
    pub last_auto_switch_at: Option<i64>,
    pub unknown_active_email: Option<String>,
}

pub struct Service {
    app: AppHandle,
    client: reqwest::Client,
    state: Mutex<AppState>,
    op: tokio::sync::Mutex<()>,
    login_cancel: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

/// What a completed `claude auth login` left behind.
struct Captured {
    email: String,
    token_json: String,
    oauth_account: Value,
    status: Option<cli::AuthStatus>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SwitchReason {
    Manual,
    Auto,
}

impl Service {
    pub fn new(app: AppHandle, data_dir: PathBuf) -> Arc<Self> {
        let _ = std::fs::create_dir_all(&data_dir);
        credentials::restrict_dir_permissions(&data_dir);
        let store = Store::load(data_dir);
        Arc::new(Self {
            app,
            client: usage::client(),
            state: Mutex::new(AppState {
                store,
                usage: HashMap::new(),
                cli: CliInfo::default(),
                refreshing: false,
                login: LoginState::default(),
                last_refresh_at: None,
                last_event: None,
                last_auto_switch_at: None,
                unknown_active_email: None,
            }),
            op: tokio::sync::Mutex::new(()),
            login_cancel: Mutex::new(None),
        })
    }

    fn state(&self) -> std::sync::MutexGuard<'_, AppState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    // ------------------------------------------------------------------
    // Snapshot / events
    // ------------------------------------------------------------------

    pub fn settings(&self) -> Settings {
        self.state().store.data.settings.clone()
    }

    pub fn snapshot(&self) -> Snapshot {
        let s = self.state();
        let accounts = s
            .store
            .data
            .accounts
            .iter()
            .map(|a| {
                let u = s.usage.get(&a.id);
                let backup = s.store.backup(a.id);
                AccountView {
                    account: a.clone(),
                    is_active: s.store.data.active_id == Some(a.id),
                    has_backup: backup.is_some(),
                    needs_reauth: u.map(|u| u.needs_reauth).unwrap_or(false),
                    usage: u.and_then(|u| u.usage.clone()),
                    usage_error: u.and_then(|u| u.error.clone()),
                    usage_fetched_at: u.and_then(|u| u.fetched_at),
                    token_expires_at: backup
                        .and_then(|b| credentials::parse_token(&b.token_json))
                        .and_then(|t| t.expires_at),
                }
            })
            .collect();
        Snapshot {
            accounts,
            active_id: s.store.data.active_id,
            unknown_active_email: s.unknown_active_email.clone(),
            settings: s.store.data.settings.clone(),
            cli: s.cli.clone(),
            refreshing: s.refreshing,
            last_refresh_at: s.last_refresh_at,
            login: s.login.clone(),
            last_event: s.last_event.clone(),
            platform: std::env::consts::OS,
            version: env!("CARGO_PKG_VERSION"),
        }
    }

    /// Pushes the current state to every window and refreshes the tray.
    pub fn publish(&self) {
        let snap = self.snapshot();
        let _ = self.app.emit(SNAPSHOT_EVENT, &snap);
        tray::update(&self.app, &snap);
    }

    fn set_event(&self, kind: &str, message: impl Into<String>) {
        let message = message.into();
        log::info!("[{kind}] {message}");
        self.state().last_event = Some(ActivityEvent {
            kind: kind.into(),
            message,
            at: now_ms(),
        });
    }

    fn notify(&self, title: &str, body: &str) {
        if !self.settings().notifications {
            return;
        }
        let _ = self.app.notification().builder().title(title).body(body).show();
    }

    fn save(&self) {
        if let Err(e) = self.state().store.save() {
            log::error!("failed to save state: {e}");
        }
    }

    // ------------------------------------------------------------------
    // CLI
    // ------------------------------------------------------------------

    pub async fn detect_cli(&self) {
        let override_path = self.settings().cli_path;
        let info = cli::detect(override_path.as_deref()).await;
        log::info!(
            "claude cli: available={} path={:?} version={:?}",
            info.available,
            info.path,
            info.version
        );
        self.state().cli = info;
        self.publish();
    }

    fn exe(&self) -> Result<PathBuf> {
        let s = self.state();
        s.cli.path.as_deref().map(PathBuf::from).ok_or(AppError::CliNotFound)
    }

    // ------------------------------------------------------------------
    // Disk <-> state synchronisation
    // ------------------------------------------------------------------

    /// Reads what the CLI is logged in as and aligns the active account with it.
    /// Also refreshes the active account's backup when the CLI rotated its token.
    fn sync_active_from_disk(&self) {
        let oauth = credentials::read_oauth_account().unwrap_or_else(|e| {
            log::warn!("could not read ~/.claude.json: {e}");
            None
        });
        let live = credentials::read_live_token().unwrap_or_else(|e| {
            log::warn!("could not read live credentials: {e}");
            None
        });
        let email = oauth.as_ref().and_then(oauth_email);

        let mut s = self.state();
        let (Some(email), Some(live), Some(oauth)) = (email, live, oauth) else {
            if s.store.data.active_id.is_some() {
                log::info!("no live claude login found; clearing active account");
            }
            s.store.data.active_id = None;
            s.unknown_active_email = None;
            return;
        };

        let Some(account) = s.store.account_by_email(&email).cloned() else {
            // The CLI is logged in as an account we have never seen: adopt it. This only
            // copies credentials into our own store, so it is safe to do unprompted and it
            // means a fresh install shows the current account right away.
            drop(s);
            let (id, _) = self.upsert_captured(Captured {
                email: email.clone(),
                token_json: live,
                oauth_account: oauth,
                status: None,
            });
            self.set_event("login", format!("Found {email}"));
            log::info!("adopted existing login {email} as {id}");
            return;
        };
        s.unknown_active_email = None;
        if s.store.data.active_id != Some(account.id) {
            log::info!("active account changed outside the app: now {}", account.email);
            s.store.data.active_id = Some(account.id);
        }
        let live_fp = credentials::fingerprint(&live);
        let stored_fp = s
            .store
            .backup(account.id)
            .and_then(|b| credentials::fingerprint(&b.token_json));
        if live_fp.is_some() && live_fp != stored_fp {
            log::info!("live token for {} rotated; updating backup", account.email);
            s.store.set_backup(
                account.id,
                Backup {
                    token_json: live,
                    oauth_account: oauth,
                    saved_at: now_ms(),
                },
            );
            let _ = s.store.save();
        }
    }

    /// Saves the live credentials into the backup of whichever account they belong to.
    /// Never writes a backup whose identity could not be verified.
    fn backup_live_if_verified(&self) {
        let (Ok(Some(live)), Ok(Some(oauth))) = (credentials::read_live_token(), credentials::read_oauth_account())
        else {
            return;
        };
        let Some(email) = oauth_email(&oauth) else { return };
        let mut s = self.state();
        if let Some(account) = s.store.account_by_email(&email).cloned() {
            s.store.set_backup(
                account.id,
                Backup {
                    token_json: live,
                    oauth_account: oauth,
                    saved_at: now_ms(),
                },
            );
            let _ = s.store.save();
        } else {
            log::warn!("live credentials belong to unknown account {email}; not backing up");
        }
    }

    // ------------------------------------------------------------------
    // Usage polling
    // ------------------------------------------------------------------

    /// Polls every account, then evaluates auto-switch. Skips silently if busy.
    pub async fn refresh_all(&self) {
        let Ok(_guard) = self.op.try_lock() else {
            log::debug!("refresh skipped: another operation is running");
            return;
        };
        self.state().refreshing = true;
        self.publish();

        self.sync_active_from_disk();
        let accounts: Vec<Account> = self.state().store.data.accounts.clone();
        for account in &accounts {
            self.refresh_account_usage(account.id).await;
        }

        {
            let mut s = self.state();
            s.refreshing = false;
            s.last_refresh_at = Some(now_ms());
        }
        self.maybe_auto_switch_locked().await;
        self.publish();
    }

    /// Fetches usage for one account, refreshing its token when needed.
    async fn refresh_account_usage(&self, id: Uuid) {
        let now = now_ms();
        let (is_active, backup, backoff) = {
            let s = self.state();
            (
                s.store.data.active_id == Some(id),
                s.store.backup(id).map(|b| b.token_json.clone()),
                s.usage.get(&id).and_then(|u| u.backoff_until),
            )
        };
        if backoff.map(|b| b > now).unwrap_or(false) {
            return;
        }

        let mut token_json = if is_active {
            credentials::read_live_token().ok().flatten().or(backup)
        } else {
            backup
        };

        let Some(mut json) = token_json.take() else {
            self.set_usage_error(
                id,
                "No stored credentials. Re-authenticate this account.".into(),
                None,
                true,
            );
            return;
        };

        // Proactively refresh tokens that are about to expire.
        if let Some(info) = credentials::parse_token(&json) {
            if info.expires_at.map(|e| e <= now + 60_000).unwrap_or(false) {
                match self.refresh_token(id, is_active, &json).await {
                    Ok(Some(new_json)) => json = new_json,
                    Ok(None) => {}
                    Err(msg) => {
                        self.set_usage_error(id, msg, Some(now + 600_000), true);
                        return;
                    }
                }
            }
        }

        let Some(access_token) = credentials::parse_token(&json).map(|t| t.access_token) else {
            self.set_usage_error(id, "Stored credentials are malformed.".into(), None, true);
            return;
        };

        let mut attempt = usage::fetch_usage(&self.client, &access_token).await;
        if matches!(attempt, Err(UsageError::Expired)) {
            match self.refresh_token(id, is_active, &json).await {
                Ok(Some(new_json)) => {
                    if let Some(t) = credentials::parse_token(&new_json) {
                        attempt = usage::fetch_usage(&self.client, &t.access_token).await;
                    }
                }
                Ok(None) => {}
                Err(msg) => {
                    self.set_usage_error(id, msg, Some(now + 600_000), true);
                    return;
                }
            }
        }

        match attempt {
            Ok(u) => {
                let mut s = self.state();
                let entry = s.usage.entry(id).or_default();
                entry.usage = Some(u);
                entry.error = None;
                entry.fetched_at = Some(now_ms());
                entry.backoff_until = None;
                entry.needs_reauth = false;
            }
            Err(UsageError::RateLimited(secs)) => {
                let wait = secs.unwrap_or(60).clamp(10, 3600) as i64 * 1000;
                self.set_usage_error(id, UsageError::RateLimited(secs).to_string(), Some(now + wait), false);
            }
            Err(UsageError::Forbidden) => {
                self.set_usage_error(id, UsageError::Forbidden.to_string(), Some(now + 900_000), false);
            }
            Err(UsageError::Expired) => {
                self.set_usage_error(
                    id,
                    "Token expired. Re-authenticate this account.".into(),
                    Some(now + 300_000),
                    true,
                );
            }
            Err(e) => self.set_usage_error(id, e.to_string(), None, false),
        }
    }

    fn set_usage_error(&self, id: Uuid, message: String, backoff_until: Option<i64>, needs_reauth: bool) {
        log::warn!("usage for {id}: {message}");
        let mut s = self.state();
        let entry = s.usage.entry(id).or_default();
        entry.error = Some(message);
        entry.backoff_until = backoff_until;
        entry.needs_reauth = needs_reauth;
    }

    /// Refreshes a token. The active account goes through the CLI (Anthropic's own refresh
    /// logic, written straight to the CLI's storage); other accounts use the OAuth endpoint
    /// directly and only touch our backup. Returns the new credential JSON when one exists.
    async fn refresh_token(
        &self,
        id: Uuid,
        is_active: bool,
        token_json: &str,
    ) -> std::result::Result<Option<String>, String> {
        if is_active {
            if let Ok(exe) = self.exe() {
                match cli::auth_status(&exe).await {
                    Ok(status) if status.logged_in => {
                        if let Ok(Some(live)) = credentials::read_live_token() {
                            if credentials::fingerprint(&live) != credentials::fingerprint(token_json) {
                                self.sync_active_from_disk();
                                return Ok(Some(live));
                            }
                        }
                    }
                    Ok(_) => return Err("Claude CLI reports it is logged out. Re-authenticate this account.".into()),
                    Err(e) => log::warn!("cli refresh failed, falling back to direct refresh: {e}"),
                }
            }
        }
        match usage::refresh(&self.client, token_json).await {
            RefreshResult::Success(new_json) => {
                let oauth = self.state().store.backup(id).map(|b| b.oauth_account.clone());
                if let Some(oauth) = oauth {
                    self.state().store.set_backup(
                        id,
                        Backup {
                            token_json: new_json.clone(),
                            oauth_account: oauth,
                            saved_at: now_ms(),
                        },
                    );
                    self.save();
                }
                if is_active {
                    if let Err(e) = credentials::write_live_token(&new_json) {
                        log::error!("could not write refreshed live token: {e}");
                    }
                }
                Ok(Some(new_json))
            }
            RefreshResult::Rejected => Err("Session expired. Re-authenticate this account.".into()),
            RefreshResult::Transient => Ok(None),
        }
    }

    // ------------------------------------------------------------------
    // Switching
    // ------------------------------------------------------------------

    pub async fn switch_to(&self, id: Uuid, reason: SwitchReason) -> Result<()> {
        let _guard = self.op.lock().await;
        self.switch_locked(id, reason).await?;
        self.refresh_account_usage(id).await;
        self.publish();
        Ok(())
    }

    async fn switch_locked(&self, id: Uuid, reason: SwitchReason) -> Result<()> {
        let (target, target_backup, active) = {
            let s = self.state();
            let target = s
                .store
                .account(id)
                .cloned()
                .ok_or_else(|| AppError::State("Unknown account".into()))?;
            let backup = s.store.backup(id).cloned();
            (target, backup, s.store.active().cloned())
        };
        let target_backup = target_backup.ok_or_else(|| {
            AppError::State(format!(
                "No stored credentials for {}. Re-authenticate it first.",
                target.email
            ))
        })?;
        if active.as_ref().map(|a| a.id) == Some(id) && reason == SwitchReason::Manual {
            // Still re-apply on disk: repairs a desynced ~/.claude.json.
            log::info!("re-applying credentials for already active {}", target.email);
        }

        // Step 0: never swap in a token that another account also claims.
        let target_fp = credentials::fingerprint(&target_backup.token_json);
        {
            let s = self.state();
            for other in s.store.data.accounts.iter().filter(|a| a.id != id) {
                let fp = s
                    .store
                    .backup(other.id)
                    .and_then(|b| credentials::fingerprint(&b.token_json));
                if fp.is_some() && fp == target_fp {
                    return Err(AppError::State(format!(
                        "{} and {} share the same token. Re-authenticate one of them.",
                        target.email, other.email
                    )));
                }
            }
        }

        // Step 1: keep the outgoing account's freshest token, but only if it is really theirs.
        self.backup_live_if_verified();

        // Step 2: swap both halves of the CLI's identity.
        credentials::write_live_token(&target_backup.token_json)?;
        credentials::write_oauth_account(&target_backup.oauth_account)?;

        // Step 3: verify what landed on disk.
        let live_fp = credentials::read_live_token()?.and_then(|t| credentials::fingerprint(&t));
        if live_fp != target_fp {
            return Err(AppError::State("Credential write could not be verified".into()));
        }
        let disk_email = credentials::read_oauth_account()?.as_ref().and_then(oauth_email);
        if disk_email.as_deref() != Some(&target.email.to_lowercase()) {
            return Err(AppError::State("~/.claude.json did not accept the new identity".into()));
        }

        // Step 4: ask the CLI (also gives it a chance to refresh the token). A missing email
        // means another credential source shadows the login; that says nothing about our swap.
        let mut shadowed = false;
        if let Ok(exe) = self.exe() {
            match cli::auth_status(&exe).await {
                Ok(status) => {
                    if !status.logged_in {
                        return Err(AppError::State(format!(
                            "Claude CLI does not accept the stored token for {}. Re-authenticate it.",
                            target.email
                        )));
                    }
                    match status.email_normalised() {
                        Some(e) if e != target.email.to_lowercase() => {
                            return Err(AppError::State(format!(
                                "Switch verification failed: expected {} but the CLI reports {e}. Re-authenticate {}.",
                                target.email, target.email
                            )));
                        }
                        Some(_) => {}
                        None => shadowed = true,
                    }
                }
                Err(e) => log::warn!("verification via cli skipped: {e}"),
            }
        }

        {
            let mut s = self.state();
            s.store.data.active_id = Some(id);
            s.unknown_active_email = None;
            if let Some(a) = s.store.account_mut(id) {
                a.last_active_at = Some(now_ms());
            }
            let _ = s.store.save();
        }
        self.sync_active_from_disk();

        let via = match reason {
            SwitchReason::Manual => "switch",
            SwitchReason::Auto => "auto-switch",
        };
        let mut msg = format!("Switched to {}", target.email);
        if shadowed {
            msg.push_str(" (an environment credential shadows the login for the CLI)");
        }
        self.set_event(via, msg);
        Ok(())
    }

    async fn maybe_auto_switch_locked(&self) {
        let now = now_ms();
        let plan = {
            let s = self.state();
            let settings = &s.store.data.settings;
            if !settings.auto_switch {
                return;
            }
            let Some(active) = s.store.active().cloned() else {
                return;
            };
            if let Some(last) = s.last_auto_switch_at {
                if now - last < settings.switch_cooldown_secs as i64 * 1000 {
                    return;
                }
            }
            let active_usage = s.usage.get(&active.id).and_then(|u| u.usage.as_ref());
            let candidates: Vec<engine::Candidate<'_>> = s
                .store
                .data
                .accounts
                .iter()
                .filter(|a| a.id != active.id)
                .map(|a| engine::Candidate {
                    id: a.id,
                    usage: s.usage.get(&a.id).and_then(|u| u.usage.as_ref()),
                    switchable: s.store.backup(a.id).is_some()
                        && !s.usage.get(&a.id).map(|u| u.needs_reauth).unwrap_or(false),
                })
                .collect();
            engine::decide(
                active_usage,
                true,
                &candidates,
                settings.threshold,
                settings.hysteresis,
                now,
            )
            .map(|d| (active, d))
        };

        let Some((active, decision)) = plan else { return };
        if decision.targets.is_empty() {
            log::info!(
                "{} is at {:.0}% but no other account has enough headroom",
                active.email,
                decision.active_utilization
            );
            return;
        }
        for (target_id, util) in decision.targets {
            match self.switch_locked(target_id, SwitchReason::Auto).await {
                Ok(()) => {
                    let email = self
                        .state()
                        .store
                        .account(target_id)
                        .map(|a| a.email.clone())
                        .unwrap_or_default();
                    self.state().last_auto_switch_at = Some(now);
                    let body = format!(
                        "{} reached {:.0}%. Now using {} ({:.0}%).",
                        active.email, decision.active_utilization, email, util
                    );
                    self.set_event("auto-switch", body.clone());
                    self.notify("Account switched", &body);
                    return;
                }
                Err(e) => log::warn!("auto-switch candidate failed: {e}"),
            }
        }
    }

    // ------------------------------------------------------------------
    // Login flows
    // ------------------------------------------------------------------

    /// Runs `claude auth login` and captures whatever account the user logged into.
    async fn login_flow(&self) -> Result<Captured> {
        let exe = self.exe()?;
        let fp_before = credentials::read_live_token()
            .ok()
            .flatten()
            .and_then(|t| credentials::fingerprint(&t));

        // Keep the outgoing account's token; the CLI is about to overwrite it.
        self.backup_live_if_verified();

        let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel::<()>();
        *self.login_cancel.lock().unwrap_or_else(|p| p.into_inner()) = Some(cancel_tx);
        {
            let mut s = self.state();
            s.login = LoginState {
                in_progress: true,
                url: None,
            };
        }
        self.publish();

        let result = async {
            let cli::LoginRun { mut child, mut url_rx } = cli::start_login(&exe)?;
            let deadline = tokio::time::sleep(LOGIN_TIMEOUT);
            tokio::pin!(deadline);
            loop {
                tokio::select! {
                    status = child.wait() => {
                        let status = status?;
                        if !status.success() {
                            return Err(AppError::Cli(format!("`claude auth login` exited with {status}")));
                        }
                        return Ok(());
                    }
                    url = &mut url_rx, if !url_rx.is_terminated() => {
                        if let Ok(url) = url {
                            self.state().login.url = Some(url);
                            self.publish();
                        }
                    }
                    _ = &mut cancel_rx => {
                        let _ = child.kill().await;
                        return Err(AppError::LoginCancelled);
                    }
                    _ = &mut deadline => {
                        let _ = child.kill().await;
                        return Err(AppError::Cli("Login timed out after 10 minutes".into()));
                    }
                }
            }
        }
        .await;

        *self.login_cancel.lock().unwrap_or_else(|p| p.into_inner()) = None;
        self.state().login = LoginState::default();
        self.publish();
        result?;

        let token_json = credentials::read_live_token()?
            .ok_or_else(|| AppError::Credentials("Login finished but no credentials were stored".into()))?;
        if credentials::fingerprint(&token_json) == fp_before {
            log::warn!("token fingerprint unchanged after login");
        }
        let oauth_account = credentials::read_oauth_account()?
            .ok_or_else(|| AppError::Credentials("Login finished but ~/.claude.json has no account block".into()))?;
        let status = cli::auth_status(&exe).await.ok();
        let email = oauth_email(&oauth_account)
            .or_else(|| status.as_ref().and_then(|s| s.email_normalised()))
            .ok_or_else(|| AppError::Credentials("Could not determine which account was logged in".into()))?;
        Ok(Captured {
            email,
            token_json,
            oauth_account,
            status,
        })
    }

    /// Inserts or updates the account described by `captured` and makes it active.
    fn upsert_captured(&self, captured: Captured) -> (Uuid, bool) {
        let Captured {
            email,
            token_json,
            oauth_account,
            status,
        } = captured;
        let token = credentials::parse_token(&token_json);
        let str_field = |key: &str| oauth_account.get(key).and_then(Value::as_str).map(str::to_string);
        let mut s = self.state();
        let existing = s.store.account_by_email(&email).map(|a| a.id);
        let (id, created) = match existing {
            Some(id) => (id, false),
            None => {
                let id = Uuid::new_v4();
                s.store.data.accounts.push(Account {
                    id,
                    email: email.clone(),
                    display_name: str_field("displayName").or_else(|| str_field("fullName")),
                    organization_name: str_field("organizationName")
                        .or_else(|| status.as_ref().and_then(|s| s.org_name.clone())),
                    subscription_type: None,
                    rate_limit_tier: None,
                    added_at: now_ms(),
                    last_active_at: None,
                });
                (id, true)
            }
        };
        if let Some(a) = s.store.account_mut(id) {
            a.display_name = str_field("displayName")
                .or_else(|| str_field("fullName"))
                .or(a.display_name.take());
            a.organization_name = str_field("organizationName").or(a.organization_name.take());
            a.subscription_type = token
                .as_ref()
                .and_then(|t| t.subscription_type.clone())
                .or_else(|| status.as_ref().and_then(|s| s.subscription_type.clone()))
                .or(a.subscription_type.take());
            a.rate_limit_tier = token
                .as_ref()
                .and_then(|t| t.rate_limit_tier.clone())
                .or(a.rate_limit_tier.take());
            a.last_active_at = Some(now_ms());
        }
        s.store.set_backup(
            id,
            Backup {
                token_json,
                oauth_account,
                saved_at: now_ms(),
            },
        );
        s.store.data.active_id = Some(id);
        s.unknown_active_email = None;
        s.usage.entry(id).or_default().needs_reauth = false;
        s.usage.entry(id).or_default().backoff_until = None;
        let _ = s.store.save();
        (id, created)
    }

    pub async fn add_account(&self) -> Result<Uuid> {
        let _guard = self.op.lock().await;
        let captured = self.login_flow().await?;
        let email = captured.email.clone();
        let (id, created) = self.upsert_captured(captured);
        if created {
            self.set_event("login", format!("Added {email}"));
        } else {
            self.set_event("login", format!("Refreshed credentials for {email}"));
        }
        self.refresh_account_usage(id).await;
        self.publish();
        Ok(id)
    }

    pub async fn reauthenticate(&self, id: Uuid) -> Result<()> {
        let _guard = self.op.lock().await;
        let expected = self
            .state()
            .store
            .account(id)
            .map(|a| a.email.clone())
            .ok_or_else(|| AppError::State("Unknown account".into()))?;
        let captured = self.login_flow().await?;
        let got = captured.email.clone();
        let (new_id, _) = self.upsert_captured(captured);
        self.refresh_account_usage(new_id).await;
        if got != expected.to_lowercase() {
            self.set_event("login", format!("Logged in as {got}, not {expected}"));
            self.publish();
            return Err(AppError::State(format!(
                "You logged in as {got}, but {expected} was expected. {got} is now active."
            )));
        }
        self.set_event("login", format!("Re-authenticated {expected}"));
        self.publish();
        Ok(())
    }

    /// Saves whatever the CLI is currently logged in as, without opening a browser.
    pub async fn adopt_current(&self) -> Result<Uuid> {
        let _guard = self.op.lock().await;
        self.sync_active_from_disk();
        let id = {
            let s = self.state();
            s.store.data.active_id.ok_or_else(|| {
                AppError::State("The Claude CLI is not logged in. Use \"Add account\" to sign in.".into())
            })?
        };
        self.refresh_account_usage(id).await;
        self.publish();
        Ok(id)
    }

    pub fn cancel_login(&self) {
        if let Some(tx) = self.login_cancel.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = tx.send(());
        }
    }

    // ------------------------------------------------------------------
    // Account & settings management
    // ------------------------------------------------------------------

    pub async fn remove_account(&self, id: Uuid) -> Result<()> {
        let _guard = self.op.lock().await;
        let email = {
            let mut s = self.state();
            let email = s
                .store
                .account(id)
                .map(|a| a.email.clone())
                .ok_or_else(|| AppError::State("Unknown account".into()))?;
            s.store.remove_account(id);
            s.usage.remove(&id);
            let _ = s.store.save();
            email
        };
        self.sync_active_from_disk();
        self.set_event("remove", format!("Removed {email}"));
        self.publish();
        Ok(())
    }

    pub async fn update_settings(&self, settings: Settings) -> Result<()> {
        let settings = settings.sanitized();
        let (cli_changed, autostart_changed) = {
            let mut s = self.state();
            let old = &s.store.data.settings;
            let changes = (
                old.cli_path != settings.cli_path,
                old.launch_at_login != settings.launch_at_login,
            );
            s.store.data.settings = settings.clone();
            changes
        };
        self.save();
        if autostart_changed {
            use tauri_plugin_autostart::ManagerExt;
            let launcher = self.app.autolaunch();
            let result = if settings.launch_at_login {
                launcher.enable()
            } else {
                launcher.disable()
            };
            if let Err(e) = result {
                log::warn!("autostart change failed: {e}");
            }
        }
        if cli_changed {
            self.detect_cli().await;
        }
        self.publish();
        Ok(())
    }
}
