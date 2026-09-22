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
        .icon(render_icon(None, Level::Normal).0)
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

    let (icon, template) = if show_bar {
        render_icon(pct, level)
    } else {
        (render_dot_icon(), true)
    };
    // Warn/critical use real colours, which a template image would flatten to monochrome.
    // The flag must be set before the icon so tray-icon applies it to the new image.
    if cfg!(target_os = "macos") {
        let _ = tray.set_icon_as_template(template);
    }
    let _ = tray.set_icon(Some(icon));
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

/// Ink for the bar: `(outline, fill, is_template)`.
///
/// On macOS the normal state is a template image (only alpha matters, macOS tints it for
/// light/dark menu bars) with a half-transparent outline, mirroring CCSwitcher's strip.
/// Warn/critical need real colour, so they are rendered as plain images with a mid-grey
/// outline that reads on both light and dark menu bars.
fn ink(pct: Option<f64>, level: Level) -> ([u8; 4], [u8; 4], bool) {
    if cfg!(target_os = "macos") {
        return match level {
            Level::Normal => {
                let outline_a = if pct.is_some() { 140 } else { 64 };
                ([0, 0, 0, outline_a], [0, 0, 0, 255], true)
            }
            Level::Warn => ([135, 135, 135, 255], [245, 158, 11, 255], false),
            Level::Critical => ([135, 135, 135, 255], [239, 68, 68, 255], false),
        };
    }
    let outline = [225, 225, 225, 255];
    let fill = match level {
        Level::Normal => [235, 235, 235, 255],
        Level::Warn => [245, 158, 11, 255],
        Level::Critical => [239, 68, 68, 255],
    };
    (outline, fill, false)
}

/// Bar geometry in pixels: canvas size, bar rectangle, stroke and inner inset.
struct BarGeometry {
    w: u32,
    h: u32,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    stroke: f32,
    inset: f32,
}

fn bar_geometry() -> BarGeometry {
    if cfg!(target_os = "macos") {
        // tray-icon scales every image to 18pt high, so a 64x36 canvas is a 32x18pt @2x
        // image holding a 26x8pt capsule with a 1pt stroke and a 1.5pt inset (CCSwitcher).
        BarGeometry {
            w: 64,
            h: 36,
            x0: 6.0,
            y0: 10.0,
            x1: 58.0,
            y1: 26.0,
            stroke: 2.0,
            inset: 3.0,
        }
    } else {
        BarGeometry {
            w: 32,
            h: 32,
            x0: 1.0,
            y0: 9.0,
            x1: 31.0,
            y1: 23.0,
            stroke: 2.0,
            inset: 3.5,
        }
    }
}

/// A horizontal progress bar. Wide on macOS (menu bar), square elsewhere.
/// Returns the image and whether it should be shown as a macOS template image.
pub fn render_icon(pct: Option<f64>, level: Level) -> (Image<'static>, bool) {
    let g = bar_geometry();
    let mut c = Canvas::new(g.w, g.h);
    let (outline, fill, template) = ink(pct, level);
    let radius = (g.y1 - g.y0) / 2.0;

    // Outline: outer capsule with the inner capsule punched out of its alpha.
    c.rounded_rect(g.x0, g.y0, g.x1, g.y1, radius, outline);
    let mut hole = Canvas::new(g.w, g.h);
    hole.rounded_rect(
        g.x0 + g.stroke,
        g.y0 + g.stroke,
        g.x1 - g.stroke,
        g.y1 - g.stroke,
        radius - g.stroke,
        [0, 0, 0, 255],
    );
    for i in 0..(g.w * g.h) as usize {
        let cut = hole.px[i * 4 + 3] as u32;
        let a = c.px[i * 4 + 3] as u32;
        c.px[i * 4 + 3] = (a * (255 - cut) / 255) as u8;
    }

    // Fill: an inset capsule scaled to the percentage. Omitted when there is no data so
    // "unknown" (faint outline) is visually distinct from 0%.
    if let Some(p) = pct {
        let p = (p / 100.0).clamp(0.0, 1.0) as f32;
        let fy0 = g.y0 + g.inset;
        let fy1 = g.y1 - g.inset;
        let inner_w = (g.x1 - g.x0) - 2.0 * g.inset;
        let min_w = fy1 - fy0;
        let fill_w = if p > 0.0 { (inner_w * p).max(min_w) } else { 0.0 };
        c.rounded_rect(g.x0 + g.inset, fy0, g.x0 + g.inset + fill_w, fy1, min_w / 2.0, fill);
    }
    (c.into_image(), template)
}

fn render_dot_icon() -> Image<'static> {
    let size = if cfg!(target_os = "macos") { 36u32 } else { 32u32 };
    let mut c = Canvas::new(size, size);
    let (outline, _, _) = ink(Some(0.0), Level::Normal);
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
        let (img, _) = render_icon(Some(42.0), Level::Normal);
        assert_eq!(img.rgba().len(), (img.width() * img.height() * 4) as usize);
        assert!(img.rgba().iter().skip(3).step_by(4).any(|&a| a > 0));
    }

    /// Fill coverage must grow with the percentage and vanish when unknown.
    #[test]
    fn fill_scales_with_percent() {
        let opaque = |img: &Image<'_>| img.rgba().iter().skip(3).step_by(4).filter(|&&a| a == 255).count();
        let none = opaque(&render_icon(None, Level::Normal).0);
        let low = opaque(&render_icon(Some(20.0), Level::Normal).0);
        let high = opaque(&render_icon(Some(80.0), Level::Normal).0);
        assert!(none < low, "unknown must not draw a fill ({none} >= {low})");
        assert!(low < high, "fill must grow with percent ({low} >= {high})");
    }

    /// `CAS_DUMP_ICON=/path/prefix cargo test dump_icon` writes raw RGBA renders for eyeballing.
    #[test]
    fn dump_icon() {
        let Ok(prefix) = std::env::var("CAS_DUMP_ICON") else {
            return;
        };
        for (name, pct, level) in [
            ("none", None, Level::Normal),
            ("p0", Some(0.0), Level::Normal),
            ("p20", Some(20.0), Level::Normal),
            ("p63", Some(63.0), Level::Warn),
            ("p95", Some(95.0), Level::Critical),
        ] {
            let (img, _) = render_icon(pct, level);
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
