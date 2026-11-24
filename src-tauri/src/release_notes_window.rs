//! The Orb panel is too narrow to hold the release notes dialog comfortably, so
//! when release notes are shown while using the Orb they open in their own floating
//! window centered on the Orb's monitor.

use std::sync::Mutex;

use tauri::{AppHandle, Manager, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::{overlay, state::AppState};

pub const LABEL: &str = "release-notes";
const WINDOW_WIDTH: f64 = 580.0;
const WINDOW_HEIGHT: f64 = 540.0;

static LAYERED: Mutex<bool> = Mutex::new(false);

fn monitor_id(app: &AppHandle) -> Option<String> {
    app.try_state::<AppState>()
        .and_then(|state| state.preferences().ok())
        .and_then(|preferences| preferences.monitor_id)
}

fn notes_window(
    app: &AppHandle,
    monitor: Option<&str>,
    x: i32,
    y: i32,
) -> Result<WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(LABEL) {
        return Ok(window);
    }
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("release-notes/".into()))
        .title("Lume · Novidades")
        .inner_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .resizable(false)
        .visible(false)
        .build()
        .map_err(|error| error.to_string())?;
    overlay::neutralize_text_scaling(&window);
    let layered = overlay::configure_workflow_editor(&window, monitor, x, y);
    overlay::apply_spaces_behavior(&window, true);
    if let Ok(mut flag) = LAYERED.lock() {
        *flag = layered;
    }
    Ok(window)
}

fn present(app: &AppHandle) -> Result<(), String> {
    let orb = app
        .get_webview_window("main")
        .ok_or("The Orb window is not open")?;
    let monitor_name = monitor_id(app);
    let monitor_ref = monitor_name.as_deref();
    let monitors = orb
        .available_monitors()
        .map_err(|error| error.to_string())?;
    let monitor =
        overlay::select_monitor(&monitors, orb.primary_monitor().ok().flatten(), monitor_ref)
            .ok_or("No monitor for the release notes")?;
    let scale = monitor.scale_factor().max(1.0);
    let screen_width = f64::from(monitor.size().width);
    let screen_height = f64::from(monitor.size().height);
    let physical_width = (WINDOW_WIDTH * scale).round();
    let physical_height = (WINDOW_HEIGHT * scale).round();
    let x = ((screen_width - physical_width) / 2.0).max(16.0 * scale).round() as i32;
    let y = ((screen_height - physical_height) / 2.0).max(24.0 * scale).round() as i32;

    let window = notes_window(app, monitor_ref, x, y)?;
    let layered = LAYERED.lock().map(|flag| *flag).unwrap_or(false);
    if cfg!(target_os = "linux") {
        overlay::resize_surface(&window, WINDOW_WIDTH as i32, WINDOW_HEIGHT as i32)?;
    } else {
        window
            .set_size(PhysicalSize::new(
                physical_width as u32,
                physical_height as u32,
            ))
            .map_err(|error| error.to_string())?;
    }
    overlay::move_to(&window, x, y, monitor_ref)?;
    window.show().map_err(|error| error.to_string())?;
    if !layered {
        let _ = overlay::move_to(&window, x, y, monitor_ref);
    }
    if !overlay::activate_window(&window) {
        let _ = window.set_focus();
    }
    Ok(())
}

fn on_window_thread<T, F>(app: &AppHandle, work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&AppHandle) -> Result<T, String> + Send + 'static,
{
    if cfg!(windows) {
        return work(app);
    }
    let (sender, receiver) = std::sync::mpsc::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let _ = sender.send(work(&handle));
    })
    .map_err(|error| error.to_string())?;
    receiver
        .recv()
        .map_err(|_| "The release notes window work did not finish".to_string())?
}

#[tauri::command]
pub async fn open_release_notes_window(app: AppHandle) -> Result<(), String> {
    on_window_thread(&app, present)
}

#[tauri::command]
pub async fn close_release_notes_window(app: AppHandle) -> Result<(), String> {
    on_window_thread(&app, |app| {
        if let Some(window) = app.get_webview_window(LABEL) {
            overlay::forget_window(LABEL);
            window.close().map_err(|error| error.to_string())?;
        }
        Ok(())
    })?;
    if let Ok(mut flag) = LAYERED.lock() {
        *flag = false;
    }
    Ok(())
}
