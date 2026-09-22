//! Menu bar / system tray: a live progress-bar icon, a context menu and the popover window.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use uuid::Uuid;

use crate::models::{now_ms, Snapshot, TrayMode, TrayWindow};
use crate::service::{Service, SwitchReason};

pub const TRAY_ID: &str = "main";
pub const POPOVER: &str = "main";
pub const SETTINGS: &str = "settings";

/// When the popover hid because it lost focus, a tray click that arrives right after is
/// the same click that took the focus away: it must not reopen the window.
static LAST_BLUR_HIDE: AtomicI64 = AtomicI64::new(0);

pub fn build(app: &AppHandle, service: Arc<Service>) -> tauri::Result<()> {
    let menu = build_menu(app, &service.snapshot())?;
    let svc_menu = service.clone();
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .icon(render_icon(None, Level::Normal))
        .tooltip("Claude Account Switcher")
        .menu(&menu)
        .show_menu_on_left_click(cfg!(target_os = "linux"))
        .on_menu_event(move |app, event| handle_menu(app, &svc_menu, event.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if !cfg!(target_os = "linux") {
                    toggle_popover(tray.app_handle());
                }
            }
        });
    if cfg!(target_os = "macos") {
        builder = builder.icon_as_template(true);
    }
    builder.build(app)?;
    Ok(())
}

fn handle_menu(app: &AppHandle, service: &Arc<Service>, id: &str) {
    match id {
        "open" => show_popover(app),
        "settings" => open_settings(app),
        "refresh" => {
            let svc = service.clone();
            tauri::async_runtime::spawn(async move { svc.refresh_all().await });
        }
        "add" => {
            let svc = service.clone();
            show_popover(app);
            tauri::async_runtime::spawn(async move {
                if let Err(e) = svc.add_account().await {
                    log::warn!("add account failed: {e}");
                }
            });
        }
        "quit" => app.exit(0),
        other => {
            if let Some(raw) = other.strip_prefix("acct:") {
                if let Ok(uuid) = Uuid::parse_str(raw) {
                    let svc = service.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = svc.switch_to(uuid, SwitchReason::Manual).await {
                            log::warn!("switch failed: {e}");
                        }
                    });
                }
            }
        }
    }
}

fn build_menu(app: &AppHandle, snap: &Snapshot) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::new(app)?;
    menu.append(&MenuItem::with_id(
        app,
        "open",
        "Open Claude Account Switcher",
        true,
        None::<&str>,
    )?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    if snap.accounts.is_empty() {
        menu.append(&MenuItem::with_id(app, "none", "No accounts yet", false, None::<&str>)?)?;
    }
    for a in &snap.accounts {
        let pct = binding_percent(a.usage.as_ref(), TrayWindow::Max);
        let label = match pct {
            Some(p) => format!("{}  ·  {:.0}%", a.account.email, p),
            None => a.account.email.clone(),
        };
        menu.append(&CheckMenuItem::with_id(
            app,
            format!("acct:{}", a.account.id),
            label,
            a.has_backup && !a.needs_reauth,
            a.is_active,
            None::<&str>,
        )?)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(
        app,
        "add",
        "Add account…",
        snap.cli.available && !snap.login.in_progress,
        None::<&str>,
    )?)?;
    menu.append(&MenuItem::with_id(
        app,
        "refresh",
        "Refresh usage",
        !snap.refreshing,
        None::<&str>,
    )?)?;
    menu.append(&MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?)?;
    Ok(menu)
}

/// Re-renders icon, title, tooltip and menu from a snapshot.
pub fn update(app: &AppHandle, snap: &Snapshot) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    let active = snap.accounts.iter().find(|a| a.is_active);
    let pct = active.and_then(|a| binding_percent(a.usage.as_ref(), snap.settings.tray_window));
    let level = Level::from_percent(pct, snap.settings.threshold);

    let show_bar = snap.settings.tray_mode != TrayMode::Percent || !cfg!(target_os = "macos");
    let show_text = cfg!(target_os = "macos") && snap.settings.tray_mode != TrayMode::Bar;

    let _ = tray.set_icon(Some(if show_bar {
        render_icon(pct, level)
    } else {
        render_dot_icon()
    }));
    if cfg!(target_os = "macos") {
        let title = match (show_text, pct) {
            (true, Some(p)) => Some(format!("{p:.0}%")),
            _ => None,
        };
        let _ = tray.set_title(title);
    }

    let tooltip = match active {
        Some(a) => {
            let five = a
                .usage
                .as_ref()
                .and_then(|u| u.five_hour.as_ref())
                .and_then(|w| w.utilization);
            let week = a
                .usage
                .as_ref()
                .and_then(|u| u.seven_day.as_ref())
                .and_then(|w| w.utilization);
            format!(
                "{}\n5h: {}  ·  7d: {}",
                a.account.email,
                five.map(|v| format!("{v:.0}%")).unwrap_or_else(|| "–".into()),
                week.map(|v| format!("{v:.0}%")).unwrap_or_else(|| "–".into())
            )
        }
        None => "Claude Account Switcher".to_string(),
    };
    let _ = tray.set_tooltip(Some(tooltip));

    if let Ok(menu) = build_menu(app, snap) {
        let _ = tray.set_menu(Some(menu));
    }
}

pub fn binding_percent(usage: Option<&crate::models::Usage>, window: TrayWindow) -> Option<f64> {
    let u = usage?;
    let five = u.five_hour.as_ref().and_then(|w| w.utilization);
    let week = u.seven_day.as_ref().and_then(|w| w.utilization);
    match window {
        TrayWindow::FiveHour => five.or(week),
        TrayWindow::SevenDay => week.or(five),
        TrayWindow::Max => match (five, week) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        },
    }
}

// ---------------------------------------------------------------------------
// Popover / settings windows
// ---------------------------------------------------------------------------

pub fn toggle_popover(app: &AppHandle) {
    let Some(window) = app.get_webview_window(POPOVER) else {
        return;
    };
    let visible = window.is_visible().unwrap_or(false);
    let just_blurred = now_ms() - LAST_BLUR_HIDE.load(Ordering::Relaxed) < 350;
    if visible {
        hide_popover(app);
    } else if !just_blurred {
        show_popover(app);
    }
}

pub const POPOVER_WIDTH: f64 = 380.0;

fn position_popover(window: &tauri::WebviewWindow) {
    use tauri_plugin_positioner::{Position, WindowExt};
    let position = if cfg!(target_os = "macos") {
        Position::TrayCenter
    } else if cfg!(target_os = "windows") {
        Position::TrayBottomCenter
    } else {
        Position::Center
    };
    if window.as_ref().window().move_window_constrained(position).is_err() {
        let _ = window.as_ref().window().move_window(Position::Center);
    }
}

pub fn show_popover(app: &AppHandle) {
    let Some(window) = app.get_webview_window(POPOVER) else {
        return;
    };
    position_popover(&window);
    let _ = window.show();
    let _ = window.set_focus();
}

/// Fits the popover window to its content, keeping it anchored to the tray icon.
pub fn resize_popover(app: &AppHandle, height: f64) {
    let Some(window) = app.get_webview_window(POPOVER) else {
        return;
    };
    let height = height.clamp(160.0, 720.0);
    let current = window
        .inner_size()
        .ok()
        .zip(window.scale_factor().ok())
        .map(|(size, scale)| size.to_logical::<f64>(scale).height)
        .unwrap_or(0.0);
    if (current - height).abs() < 1.0 {
        return;
    }
    let _ = window.set_size(tauri::LogicalSize::new(POPOVER_WIDTH, height));
    if window.is_visible().unwrap_or(false) {
        position_popover(&window);
    }
}

pub fn hide_popover(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(POPOVER) {
        let _ = window.hide();
    }
}

pub fn note_blur_hide() {
    LAST_BLUR_HIDE.store(now_ms(), Ordering::Relaxed);
}

pub fn open_settings(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(SETTINGS) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        return;
    }
    let result = WebviewWindowBuilder::new(app, SETTINGS, WebviewUrl::App("index.html".into()))
        .title("Claude Account Switcher – Settings")
        .inner_size(560.0, 720.0)
        .min_inner_size(480.0, 560.0)
        .resizable(true)
        .center()
        .build();
    match result {
        Ok(window) => {
            let _ = window.set_focus();
        }
        Err(e) => log::error!("could not open settings window: {e}"),
    }
}

// ---------------------------------------------------------------------------
// Icon rendering
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Normal,
    Warn,
    Critical,
}

impl Level {
    fn from_percent(pct: Option<f64>, threshold: f64) -> Self {
        match pct {
            Some(p) if p >= threshold => Level::Critical,
            Some(p) if p >= threshold - 20.0 => Level::Warn,
            _ => Level::Normal,
        }
    }
}

struct Canvas {
    w: u32,
    h: u32,
    px: Vec<u8>,
}

impl Canvas {
    fn new(w: u32, h: u32) -> Self {
        Self {
            w,
            h,
            px: vec![0; (w * h * 4) as usize],
        }
    }

    /// Paints a rounded rectangle with 4x4 supersampling for smooth edges.
    fn rounded_rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, radius: f32, rgba: [u8; 4]) {
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let r = radius.min((x1 - x0) / 2.0).min((y1 - y0) / 2.0);
        let inside = |px: f32, py: f32| -> bool {
            if px < x0 || px > x1 || py < y0 || py > y1 {
                return false;
            }
            let cx = px.clamp(x0 + r, x1 - r);
            let cy = py.clamp(y0 + r, y1 - r);
            (px - cx).powi(2) + (py - cy).powi(2) <= r * r
        };
        for y in 0..self.h {
            for x in 0..self.w {
                let mut hits = 0;
                for sy in 0..4 {
                    for sx in 0..4 {
                        let px = x as f32 + (sx as f32 + 0.5) / 4.0;
                        let py = y as f32 + (sy as f32 + 0.5) / 4.0;
                        if inside(px, py) {
                            hits += 1;
                        }
                    }
                }
                if hits == 0 {
                    continue;
                }
                let cov = hits as f32 / 16.0;
                let a = (rgba[3] as f32 * cov) as u32;
                let i = ((y * self.w + x) * 4) as usize;
                // "over" compositing on straight alpha
                let da = self.px[i + 3] as u32;
                let out_a = a + da * (255 - a) / 255;
                if out_a == 0 {
                    continue;
                }
                for (c, &src) in rgba.iter().take(3).enumerate() {
                    let s = src as u32;
                    let d = self.px[i + c] as u32;
                    self.px[i + c] = ((s * a + d * da * (255 - a) / 255) / out_a) as u8;
                }
                self.px[i + 3] = out_a as u8;
            }
        }
    }

    fn into_image(self) -> Image<'static> {
        Image::new_owned(self.px, self.w, self.h)
    }
}

fn ink(level: Level) -> ([u8; 4], [u8; 4]) {
    if cfg!(target_os = "macos") {
        // Template image: only alpha matters, macOS tints it for light/dark menu bars.
        return ([0, 0, 0, 255], [0, 0, 0, 255]);
    }
    let outline = [225, 225, 225, 255];
    let fill = match level {
        Level::Normal => [235, 235, 235, 255],
        Level::Warn => [245, 158, 11, 255],
        Level::Critical => [239, 68, 68, 255],
    };
    (outline, fill)
}

/// A horizontal progress bar. Wide on macOS (menu bar), square elsewhere.
pub fn render_icon(pct: Option<f64>, level: Level) -> Image<'static> {
    let (w, h, bar_h) = if cfg!(target_os = "macos") {
        (64u32, 32u32, 20.0f32)
    } else {
        (32u32, 32u32, 14.0f32)
    };
    let mut c = Canvas::new(w, h);
    let (outline, fill) = ink(level);
    let stroke = if w > 32 { 3.0 } else { 2.0 };
    let margin = if w > 32 { 2.0 } else { 1.0 };
    let y0 = (h as f32 - bar_h) / 2.0;
    let y1 = y0 + bar_h;
    let x0 = margin;
    let x1 = w as f32 - margin;
    let radius = bar_h / 2.0;

    // Outline: outer shape minus inner shape (drawn by punching alpha 0 is not possible
    // with "over", so draw outer in ink and inner as fully transparent by rebuilding).
    c.rounded_rect(x0, y0, x1, y1, radius, outline);
    let mut hole = Canvas::new(w, h);
    hole.rounded_rect(
        x0 + stroke,
        y0 + stroke,
        x1 - stroke,
        y1 - stroke,
        radius - stroke,
        [0, 0, 0, 255],
    );
    for i in 0..(w * h) as usize {
        let cut = hole.px[i * 4 + 3] as u32;
        let a = c.px[i * 4 + 3] as u32;
        c.px[i * 4 + 3] = (a * (255 - cut) / 255) as u8;
    }

    if let Some(p) = pct {
        let p = (p / 100.0).clamp(0.0, 1.0) as f32;
        let gap = stroke + if w > 32 { 2.5 } else { 1.5 };
        let inner_w = (x1 - x0) - 2.0 * gap;
        let fill_w = if p > 0.0 { (inner_w * p).max(gap * 1.2) } else { 0.0 };
        let fy0 = y0 + gap;
        let fy1 = y1 - gap;
        c.rounded_rect(x0 + gap, fy0, x0 + gap + fill_w, fy1, (fy1 - fy0) / 2.0, fill);
    } else {
        // Unknown: a small centred dash so the icon does not look broken.
        let cx = (x0 + x1) / 2.0;
        let cy = (y0 + y1) / 2.0;
        c.rounded_rect(cx - 5.0, cy - 1.5, cx + 5.0, cy + 1.5, 1.5, fill);
    }
    c.into_image()
}

fn render_dot_icon() -> Image<'static> {
    let size = 32u32;
    let mut c = Canvas::new(size, size);
    let (outline, _) = ink(Level::Normal);
    let m = size as f32 * 0.22;
    c.rounded_rect(
        m,
        m,
        size as f32 - m,
        size as f32 - m,
        (size as f32 - 2.0 * m) / 2.0,
        outline,
    );
    c.into_image()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_sane_dimensions() {
        let img = render_icon(Some(42.0), Level::Normal);
        assert_eq!(img.rgba().len(), (img.width() * img.height() * 4) as usize);
        assert!(img.rgba().iter().skip(3).step_by(4).any(|&a| a > 0));
    }

    /// `CAS_DUMP_ICON=/path/prefix cargo test dump_icon` writes raw RGBA renders for eyeballing.
    #[test]
    fn dump_icon() {
        let Ok(prefix) = std::env::var("CAS_DUMP_ICON") else {
            return;
        };
        for (name, pct, level) in [
            ("none", None, Level::Normal),
            ("p20", Some(20.0), Level::Normal),
            ("p63", Some(63.0), Level::Warn),
            ("p95", Some(95.0), Level::Critical),
        ] {
            let img = render_icon(pct, level);
            let path = format!("{prefix}-{name}-{}x{}.rgba", img.width(), img.height());
            std::fs::write(path, img.rgba()).unwrap();
        }
    }

    #[test]
    fn level_thresholds() {
        assert_eq!(Level::from_percent(Some(95.0), 90.0), Level::Critical);
        assert_eq!(Level::from_percent(Some(75.0), 90.0), Level::Warn);
        assert_eq!(Level::from_percent(Some(10.0), 90.0), Level::Normal);
        assert_eq!(Level::from_percent(None, 90.0), Level::Normal);
    }
}
