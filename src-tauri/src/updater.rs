//! Background auto-updates from GitHub releases.
//!
//! Every tagged release ships signed updater bundles plus a `latest.json` manifest (see
//! `.github/workflows/release.yml`). Shortly after launch, and every few hours after that,
//! the app fetches the manifest. When it announces a newer version, the bundle is downloaded
//! and its signature verified in the background. The verified bundle is then installed the
//! next time the app is idle: popover and settings closed, no login, switch or refresh in
//! flight. On macOS and Linux the app restarts itself afterwards; on Windows the installer
//! takes over and relaunches the app.
//!
//! Both steps can also be triggered by hand from the tray menu or the settings window,
//! and the automatic path can be switched off in Settings.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::error::{AppError, Result};
use crate::models::{now_ms, UpdateStage};
use crate::service::Service;
use crate::tray;

/// Let the usage poller settle before the first check.
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(20);
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// How often a downloaded update re-checks whether the app is idle enough to install.
const IDLE_POLL: Duration = Duration::from_secs(15);

/// What a check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    UpToDate,
    /// A newer build was downloaded, verified and is waiting to be installed.
    Downloaded(String),
}

/// A verified bundle waiting for a quiet moment.
struct Pending {
    update: Update,
    bytes: Vec<u8>,
}

pub struct Updater {
    app: AppHandle,
    service: Arc<Service>,
    pending: Mutex<Option<Pending>>,
    /// A check or download is in flight; a second one would only race it.
    running: AtomicBool,
}

impl Updater {
    pub fn new(app: AppHandle, service: Arc<Service>) -> Arc<Self> {
        Arc::new(Self {
            app,
            service,
            pending: Mutex::new(None),
            running: AtomicBool::new(false),
        })
    }

    /// Periodic check-download-install loop. Runs for the lifetime of the app and honours
    /// the `auto_update` setting on every iteration, so toggling it needs no restart.
    pub fn spawn_loop(self: &Arc<Self>) {
        // A dev binary lives outside any bundle; installing over it makes no sense.
        if cfg!(debug_assertions) {
            log::info!("auto-update loop disabled in debug builds");
            return;
        }
        if !auto_install_supported() {
            log::info!("auto-update loop disabled: install needs a privilege prompt on this packaging");
            return;
        }
        let this = self.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(FIRST_CHECK_DELAY).await;
            loop {
                if this.service.settings().auto_update {
                    match this.check().await {
                        Ok(Outcome::Downloaded(version)) => {
                            log::info!("update {version} downloaded, installing when idle");
                            this.install_when_idle().await;
                        }
                        Ok(Outcome::UpToDate) => {}
                        Err(e) => log::warn!("update check failed: {e}"),
                    }
                }
                tokio::time::sleep(CHECK_INTERVAL).await;
            }
        });
    }

    /// Fetches the manifest and, if a newer build exists, downloads and verifies it.
    /// A build downloaded earlier is reported again without a new download.
    pub async fn check(&self) -> Result<Outcome> {
        if self.running.swap(true, Ordering::SeqCst) {
            return Err(AppError::State("An update check is already running".into()));
        }
        let result = self.check_inner().await;
        self.running.store(false, Ordering::SeqCst);
        result
    }

    async fn check_inner(&self) -> Result<Outcome> {
        if let Some(version) = self.pending_version() {
            return Ok(Outcome::Downloaded(version));
        }
        self.service.set_update_info(|u| {
            u.stage = UpdateStage::Checking;
            u.error = None;
        });

        let checked = match self.app.updater() {
            Ok(updater) => updater.check().await,
            Err(e) => Err(e),
        };
        let update = match checked {
            Ok(update) => update,
            Err(e) => {
                let message = format!("Update check failed: {e}");
                self.service.set_update_info(|u| {
                    u.stage = UpdateStage::Idle;
                    u.last_checked_at = Some(now_ms());
                    u.error = Some(message.clone());
                });
                return Err(AppError::State(message));
            }
        };
        let Some(update) = update else {
            self.service.set_update_info(|u| {
                u.stage = UpdateStage::Idle;
                u.version = None;
                u.last_checked_at = Some(now_ms());
            });
            return Ok(Outcome::UpToDate);
        };

        let version = update.version.clone();
        log::info!("update available: {} -> {version}", update.current_version);
        self.service.set_update_info(|u| {
            u.stage = UpdateStage::Downloading;
            u.version = Some(version.clone());
            u.last_checked_at = Some(now_ms());
        });
        // Signature verification happens inside `download`; unsigned or tampered bundles
        // never reach `pending`.
        let bytes = match update.download(|_, _| {}, || {}).await {
            Ok(bytes) => bytes,
            Err(e) => {
                let message = format!("Update download failed: {e}");
                self.service.set_update_info(|u| {
                    u.stage = UpdateStage::Idle;
                    u.error = Some(message.clone());
                });
                return Err(AppError::State(message));
            }
        };
        *self.lock_pending() = Some(Pending { update, bytes });
        self.service.set_update_info(|u| u.stage = UpdateStage::Ready);
        self.service
            .set_event("update", format!("Update {version} ready to install"));
        self.service.publish();
        Ok(Outcome::Downloaded(version))
    }

    /// Installs the downloaded build and restarts. Only returns on failure.
    pub async fn install_pending(&self) -> Result<()> {
        let Some(version) = self.pending_version() else {
            return Err(AppError::State("No update has been downloaded yet".into()));
        };
        self.service.set_update_info(|u| u.stage = UpdateStage::Installing);
        self.service.notify_always(
            "Updating Claude Account Switcher",
            &format!("Installing {version}. Back in a moment."),
        );

        // `install` blocks on file I/O (and may prompt for privileges on Linux packages),
        // so keep it off the async runtime.
        let pending = self.lock_pending().take();
        let install = tauri::async_runtime::spawn_blocking(move || match pending {
            Some(Pending { update, bytes }) => update.install(bytes).map_err(|e| e.to_string()),
            None => Err("No update has been downloaded yet".into()),
        })
        .await
        .map_err(|e| AppError::State(format!("Update install task failed: {e}")))?;

        if let Err(e) = install {
            let message = format!("Update install failed: {e}");
            log::error!("{message}");
            self.service.set_update_info(|u| {
                u.stage = UpdateStage::Idle;
                u.version = None;
                u.error = Some(message.clone());
            });
            return Err(AppError::State(message));
        }
        log::info!("update {version} installed, restarting");
        // On Windows the installer has already exited this process by now.
        self.app.restart();
    }

    /// Waits for a quiet moment, then installs. Gives up when auto-update is turned off
    /// in the meantime (the build stays downloaded for a manual install) or when the
    /// install fails.
    async fn install_when_idle(&self) {
        loop {
            if !self.service.settings().auto_update {
                return;
            }
            if self.pending_version().is_none() {
                return;
            }
            if self.is_idle() {
                if let Err(e) = self.install_pending().await {
                    log::warn!("deferred install failed: {e}");
                }
                return;
            }
            tokio::time::sleep(IDLE_POLL).await;
        }
    }

    /// Nobody is looking at the app and nothing is touching the credential store.
    fn is_idle(&self) -> bool {
        let popover_visible = self
            .app
            .get_webview_window(tray::POPOVER)
            .and_then(|w| w.is_visible().ok())
            .unwrap_or(false);
        let settings_open = self.app.get_webview_window(tray::SETTINGS).is_some();
        !popover_visible && !settings_open && !self.service.is_busy()
    }

    pub fn pending_version(&self) -> Option<String> {
        self.lock_pending().as_ref().map(|p| p.update.version.clone())
    }

    fn lock_pending(&self) -> std::sync::MutexGuard<'_, Option<Pending>> {
        self.pending.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// Whether an install can run without user interaction. Debian and RPM packages are
/// installed through `pkexec`, which pops a password prompt; that is fine when the user
/// clicks "Check for updates" but not out of the blue.
fn auto_install_supported() -> bool {
    if cfg!(target_os = "linux") {
        return std::env::var_os("APPIMAGE").is_some();
    }
    true
}

/// Tray/menu entry point: check, and if something is there, install right away.
pub async fn check_and_install_now(updater: &Updater, service: &Service) {
    match updater.check().await {
        Ok(Outcome::UpToDate) => {
            service.notify_always(
                "Claude Account Switcher is up to date",
                &format!("Version {} is the latest release.", env!("CARGO_PKG_VERSION")),
            );
        }
        Ok(Outcome::Downloaded(_)) => {
            if let Err(e) = updater.install_pending().await {
                service.notify_always("Update failed", &e.to_string());
            }
        }
        Err(e) => service.notify_always("Update check failed", &e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    /// The plugin reads its section of `tauri.conf.json` at startup; a malformed public
    /// key or endpoint would only surface as a failed launch. Parse it the way the
    /// plugin does so CI catches that instead.
    #[test]
    fn updater_config_is_valid() {
        let raw = include_str!("../tauri.conf.json");
        let conf: serde_json::Value = serde_json::from_str(raw).unwrap();
        let section = conf["plugins"]["updater"].clone();
        let config: tauri_plugin_updater::Config = serde_json::from_value(section).unwrap();
        assert!(
            config.endpoints.iter().any(|e| e.to_string().ends_with("/latest.json")),
            "endpoint must point at the release manifest"
        );
        assert!(!config.pubkey.is_empty());
        assert!(
            conf["bundle"]["createUpdaterArtifacts"].as_bool() == Some(true),
            "release builds must produce updater bundles"
        );
    }
}
