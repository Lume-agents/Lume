//! The Orb is too small to hold a tour card over its own panel, so the card lives in a window of its own
//! beside it: on the right when the screen has room, otherwise on the left, level with what the Orb highlights.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

pub const LABEL: &str = "tour-card";
/// Space between the Orb and the card, and between the card and the screen edge (logical px).
const GAP: f64 = 14.0;

#[derive(Clone, Copy, Default)]
struct Placement {
    /// Height in the Orb window (logical px) the card lines up with; none centers it on the Orb.
    anchor_y: Option<f64>,
    width: f64,
    height: f64,
}

static PLACEMENT: Mutex<Placement> = Mutex::new(Placement {
    anchor_y: None,
    width: 340.0,
    height: 220.0,
});

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TourCardSide {
    /// `right` or `left` of the Orb: the side the card points back from.
    pub side: &'static str,
}

fn card_window(app: &AppHandle) -> Result<WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(LABEL) {
        return Ok(window);
    }
    WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("tour-card/".into()))
        .title("Lume")
        .inner_size(340.0, 220.0)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .resizable(false)
        .visible(false)
        .build()
        .map_err(|error| error.to_string())
}

/// Puts the card next to the Orb for the current placement and returns the side it took.
fn arrange(app: &AppHandle) -> Result<TourCardSide, String> {
    let placement = *PLACEMENT.lock().map_err(|_| "tour card lock")?;
    let orb = app
        .get_webview_window("main")
        .ok_or("The Orb window is not open")?;
    let card = card_window(app)?;
    let scale = orb.scale_factor().map_err(|error| error.to_string())?;
    let position = orb.outer_position().map_err(|error| error.to_string())?;
    let size = orb.outer_size().map_err(|error| error.to_string())?;
    let monitor = orb
        .current_monitor()
        .map_err(|error| error.to_string())?
        .ok_or("No monitor under the Orb")?;
    let area = monitor.work_area();
    let (left_edge, top_edge) = (f64::from(area.position.x), f64::from(area.position.y));
    let (right_edge, bottom_edge) = (
        left_edge + f64::from(area.size.width),
        top_edge + f64::from(area.size.height),
    );

    let width = placement.width * scale;
    let height = placement.height * scale;
    let gap = GAP * scale;
    let orb_left = f64::from(position.x);
    let orb_right = orb_left + f64::from(size.width);
    let room_right = right_edge - orb_right;
    let room_left = orb_left - left_edge;
    let side = if room_right >= width + gap * 2.0 || room_right >= room_left {
        "right"
    } else {
        "left"
    };
    let x = if side == "right" {
        (orb_right + gap).min(right_edge - width - gap)
    } else {
        (orb_left - gap - width).max(left_edge + gap)
    };
    let orb_top = f64::from(position.y);
    let center = match placement.anchor_y {
        Some(anchor) => orb_top + anchor * scale,
        None => orb_top + f64::from(size.height) / 2.0,
    };
    let y = (center - height / 2.0)
        .max(top_edge + gap)
        .min(bottom_edge - height - gap);

    card.set_size(PhysicalSize::new(
        width.round() as u32,
        height.round() as u32,
    ))
    .map_err(|error| error.to_string())?;
    card.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32))
        .map_err(|error| error.to_string())?;
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
        placement.width = width.clamp(240.0, 480.0);
    }
    arrange(&app)
}

/// Called by the card once it has laid out a step, with the height it needs.
#[tauri::command]
pub fn fit_tour_card(app: AppHandle, height: f64) -> Result<TourCardSide, String> {
    PLACEMENT.lock().map_err(|_| "tour card lock")?.height = height.clamp(120.0, 640.0);
    arrange(&app)
}

#[tauri::command]
pub fn close_tour_card(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(LABEL) {
        window.close().map_err(|error| error.to_string())?;
    }
    Ok(())
}
