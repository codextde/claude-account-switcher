use std::sync::Arc;

use tauri::{AppHandle, State};
use uuid::Uuid;

use crate::error::Result;
use crate::models::{Settings, Snapshot};
use crate::service::{Service, SwitchReason};
use crate::tray;

type Svc<'a> = State<'a, Arc<Service>>;

#[tauri::command]
pub fn get_snapshot(service: Svc<'_>) -> Snapshot {
    service.snapshot()
}

#[tauri::command]
pub async fn refresh_usage(service: Svc<'_>) -> Result<()> {
    service.refresh_all().await;
    Ok(())
}

#[tauri::command]
pub async fn add_account(service: Svc<'_>) -> Result<Uuid> {
    service.add_account().await
}

#[tauri::command]
pub async fn adopt_current(service: Svc<'_>) -> Result<Uuid> {
    service.adopt_current().await
}

#[tauri::command]
pub fn cancel_login(service: Svc<'_>) {
    service.cancel_login();
}

#[tauri::command]
pub async fn switch_account(service: Svc<'_>, id: Uuid) -> Result<()> {
    service.switch_to(id, SwitchReason::Manual).await
}

#[tauri::command]
pub async fn remove_account(service: Svc<'_>, id: Uuid) -> Result<()> {
    service.remove_account(id).await
}

#[tauri::command]
pub async fn reauthenticate(service: Svc<'_>, id: Uuid) -> Result<()> {
    service.reauthenticate(id).await
}

#[tauri::command]
pub async fn update_settings(service: Svc<'_>, settings: Settings) -> Result<()> {
    service.update_settings(settings).await
}

#[tauri::command]
pub async fn detect_cli(service: Svc<'_>) -> Result<()> {
    service.detect_cli().await;
    Ok(())
}

#[tauri::command]
pub fn open_settings(app: AppHandle) {
    tray::open_settings(&app);
}

#[tauri::command]
pub fn resize_popover(app: AppHandle, height: f64) {
    tray::resize_popover(&app, height);
}

#[tauri::command]
pub fn hide_popover(app: AppHandle) {
    tray::hide_popover(&app);
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}
