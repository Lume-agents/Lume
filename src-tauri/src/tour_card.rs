//! The Orb is too small to hold a tour card over its own panel, so the card lives in a window of its own
//! beside it: on the right when the screen has room, otherwise on the left, level with what the Orb highlights.
//!
//! The card is an overlay window like the terminals (a layer-shell surface on Wayland), sitting above the Orb,
//! and it is placed and sized through the same `overlay` functions, in the monitor's own pixels.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{
    AppHandle, Manager, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

use crate::{overlay, state::AppState};

pub const LABEL: &str = "tour-card";
/// Space between the Orb and the card, and between the card and the screen edge (logical px).
const GAP: f64 = 14.0;

#[derive(Clone, Copy)]
struct Placement {
    /// Height in the Orb window (logical px) the card lines up with; none centers it on the Orb.
    anchor_y: Option<f64>,
    /// The card window's size in logical px: the card plus the margin its shadow needs.
    width: f64,
    height: f64,
}

static PLACEMENT: Mutex<Placement> = Mutex::new(Placement {
    anchor_y: None,
    width: 376.0,
    height: 260.0,
});
/// Whether the card window is a layer-shell surface (it then takes its size from `resize_surface`).
static LAYERED: Mutex<bool> = Mutex::new(false);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TourCardSide {
    /// `right` or `left` of the Orb: the side the card points back from.
    pub side: &'static str,
}

fn monitor_id(app: &AppHandle) -> Option<String> {
    app.try_state::<AppState>()
        .and_then(|state| state.preferences().ok())
        .and_then(|preferences| preferences.monitor_id)
}

fn card_window(app: &AppHandle, monitor: Option<&str>, x: i32, y: i32) -> Result<WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(LABEL) {
        return Ok(window);
    }
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("tour-card/".into()))
        .title("Lume")
        .inner_size(376.0, 260.0)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .resizable(false)
        .visible(false)
        .build()
        .map_err(|error| error.to_string())?;
    // Above the Orb on every platform: as an overlay surface where the desktop has them.
    let layered = overlay::configure_workflow_editor(&window, monitor, x, y);
    overlay::apply_spaces_behavior(&window, true);
    if let Ok(mut flag) = LAYERED.lock() {
        *flag = layered;
    }
    Ok(window)
}

/// Puts the card next to the Orb for the current placement and returns the side it took.
fn arrange(app: &AppHandle) -> Result<TourCardSide, String> {
    let placement = *PLACEMENT.lock().map_err(|_| "tour card lock")?;
    let orb = app.get_webview_window("main").ok_or("The Orb window is not open")?;
    let monitor_name = monitor_id(app);
    let monitor_ref = monitor_name.as_deref();
    let scale = orb.scale_factor().map_err(|error| error.to_string())?;
    let size = orb.outer_size().map_err(|error| error.to_string())?;
    // Where the Orb is, in the monitor's own pixels (the coordinates `overlay::move_to` takes).
    let (orb_x, orb_y) = overlay::position(&orb).ok_or("The Orb's position is unknown")?;
    let monitors = orb.available_monitors().map_err(|error| error.to_string())?;
    let monitor = overlay::select_monitor(&monitors, orb.primary_monitor().ok().flatten(), monitor_ref)
        .ok_or("No monitor for the Orb")?;
    let screen_width = f64::from(monitor.size().width);
    let screen_height = f64::from(monitor.size().height);

    let width = (placement.width * scale).round();
    let height = (placement.height * scale).round();
    let gap = GAP * scale;
    let (orb_left, orb_top) = (f64::from(orb_x), f64::from(orb_y));
    let orb_right = orb_left + f64::from(size.width);
    let room_right = screen_width - orb_right;
    let room_left = orb_left;
    let side = if room_right >= width + gap * 2.0 || room_right >= room_left {
        "right"
    } else {
        "left"
    };
    let x = if side == "right" {
        (orb_right + gap).min(screen_width - width - gap)
    } else {
        (orb_left - gap - width).max(gap)
    };
    let center = match placement.anchor_y {
        Some(anchor) => orb_top + anchor * scale,
        None => orb_top + f64::from(size.height) / 2.0,
    };
    let y = (center - height / 2.0).max(gap).min(screen_height - height - gap);
    let (x, y) = (x.round() as i32, y.round() as i32);

    let card = card_window(app, monitor_ref, x, y)?;
    let layered = LAYERED.lock().map(|flag| *flag).unwrap_or(false);
    if layered {
        // A layer surface takes its size from the compositor protocol, not from the window.
        overlay::resize_surface(&card, width as i32, height as i32)?;
    } else {
        card.set_size(PhysicalSize::new(width as u32, height as u32))
            .map_err(|error| error.to_string())?;
    }
    overlay::move_to(&card, x, y, monitor_ref)?;
    card.show().map_err(|error| error.to_string())?;
    Ok(TourCardSide { side })
}

/// Called by the Orb on every step: where the highlighted control sits, and how wide the card is.
#[tauri::command]
pub fn place_tour_card(
    app: AppHandle,
    anchor_y: Option<f64>,
    width: f64,
) -> Result<TourCardSide, String> {
    {
        let mut placement = PLACEMENT.lock().map_err(|_| "tour card lock")?;
        placement.anchor_y = anchor_y.filter(|value| value.is_finite());
        placement.width = width.clamp(260.0, 520.0);
    }
    arrange(&app)
}

/// Called by the card once it has laid out a step, with the height it needs.
#[tauri::command]
pub fn fit_tour_card(app: AppHandle, height: f64) -> Result<TourCardSide, String> {
    PLACEMENT.lock().map_err(|_| "tour card lock")?.height = height.clamp(140.0, 720.0);
    arrange(&app)
}

#[tauri::command]
pub fn close_tour_card(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(LABEL) {
        overlay::forget_window(LABEL);
        window.close().map_err(|error| error.to_string())?;
    }
    if let Ok(mut flag) = LAYERED.lock() {
        *flag = false;
    }
    Ok(())
}
