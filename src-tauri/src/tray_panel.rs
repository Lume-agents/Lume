//! Tray mode: the Lume icon in the system tray, and a small panel with the agents that are running.
//!
//! With "Open Lume as → Tray" the Orb stays hidden. Clicking the icon (Windows and macOS report the click and where
//! the icon is) opens the panel beside it. Linux trays report no clicks, so a panel would need a second click on a menu
//! item: there the icon's menu itself lists the agents and their status, and everything else opens the Workspace.
//!
//! The panel is a window like the tour card: placed with the `overlay` functions so it floats above other windows on
//! Wayland too, and it is built where each platform wants windows built (see `tour_card::on_window_thread`).

use std::sync::{
    atomic::{AtomicBool, AtomicI64, Ordering},
    Mutex,
};

use serde::Serialize;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    AppHandle, Emitter, Manager, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};

use crate::{
    domain::{AgentSession, SessionStatus},
    overlay,
    state::{now_millis, AppState},
};

pub const LABEL: &str = "tray-panel";
pub const TRAY_ID: &str = "lume-tray";
pub const MODE: &str = "tray";
/// The panel's size and its distance from the screen edge and the icon (logical px).
const WIDTH: f64 = 344.0;
const HEIGHT: f64 = 468.0;
const GAP: f64 = 10.0;
const MENU_AGENTS: usize = 8;
/// The panel opens from a click on the icon, which Linux trays do not report.
const PANEL_AVAILABLE: bool = !cfg!(target_os = "linux");
/// A click on the icon while the panel is open first takes the focus away from it, which hides it; the click then arrives
/// and must not open it again.
const REOPEN_GUARD_MS: i64 = 350;

static HIDDEN_AT: AtomicI64 = AtomicI64::new(0);
static LAYERED: Mutex<bool> = Mutex::new(false);
static MENU_PENDING: AtomicBool = AtomicBool::new(false);

/// Where the tray icon is, in the monitor's own pixels: x, y, width, height.
pub type IconRect = (f64, f64, f64, f64);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Opened {
    below: bool,
}

pub fn is_tray_mode(app: &AppHandle) -> bool {
    app.try_state::<AppState>()
        .and_then(|state| state.preferences().ok())
        .is_some_and(|preferences| preferences.startup_mode == MODE)
}

fn preferences(app: &AppHandle) -> Option<crate::domain::Preferences> {
    app.try_state::<AppState>().and_then(|state| state.preferences().ok())
}

fn panel_window(app: &AppHandle, monitor: Option<&str>, x: i32, y: i32) -> Result<WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(LABEL) {
        return Ok(window);
    }
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("tray/".into()))
        .title("Lume")
        .inner_size(WIDTH, HEIGHT)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(true)
        .visible(false)
        .build()
        .map_err(|error| error.to_string())?;
    let layered = overlay::configure_workflow_editor(&window, monitor, x, y);
    overlay::apply_spaces_behavior(&window, true);
    if let Ok(mut flag) = LAYERED.lock() {
        *flag = layered;
    }
    // Losing the focus closes the panel, as a tray menu would. A layer surface may never get the focus, so it also has a
    // close button.
    let handle = window.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::Focused(false) = event {
            HIDDEN_AT.store(now_millis(), Ordering::Relaxed);
            let _ = handle.hide();
        }
    });
    Ok(window)
}

/// The work area (the screen without panels and docks) of the monitor the panel goes on, in pixels.
fn work_area(window: &WebviewWindow, monitor_id: Option<&str>, point: Option<(f64, f64)>) -> Result<(f64, f64, f64, f64), String> {
    let monitors = window.available_monitors().map_err(|error| error.to_string())?;
    let monitor = point
        .and_then(|(x, y)| window.monitor_from_point(x, y).ok().flatten())
        .or_else(|| overlay::select_monitor(&monitors, window.primary_monitor().ok().flatten(), monitor_id))
        .ok_or("No monitor for the tray panel")?;
    if let Some((x, y, width, height)) = overlay::monitor_work_area(window, monitor_id) {
        if point.is_none() {
            return Ok((f64::from(x), f64::from(y), f64::from(width), f64::from(height)));
        }
    }
    let area = monitor.work_area();
    Ok((
        f64::from(area.position.x),
        f64::from(area.position.y),
        f64::from(area.size.width),
        f64::from(area.size.height),
    ))
}

/// Puts the panel beside the tray icon (above it when the icon is in the lower half of the screen), or in a corner when
/// the platform did not say where the icon is: the lower right on Windows, where the tray is, the upper right elsewhere.
fn place(app: &AppHandle, window: &WebviewWindow, icon: Option<IconRect>) -> Result<bool, String> {
    let preferences = preferences(app);
    let monitor_id = preferences.as_ref().and_then(|value| value.monitor_id.clone());
    let monitor_ref = monitor_id.as_deref();
    let scale = window.scale_factor().map_err(|error| error.to_string())?;
    let width = (WIDTH * scale).round();
    let height = (HEIGHT * scale).round();
    let gap = GAP * scale;
    let (area_x, area_y, area_width, area_height) =
        work_area(window, monitor_ref, icon.map(|(x, y, ..)| (x, y)))?;
    let (x, y, below) = match icon {
        Some((icon_x, icon_y, icon_width, icon_height)) => {
            let above = icon_y > area_y + area_height / 2.0;
            let y = if above { icon_y - height - gap } else { icon_y + icon_height + gap };
            (icon_x + icon_width / 2.0 - width / 2.0, y, !above)
        }
        None if cfg!(windows) => (area_x + area_width - width - gap, area_y + area_height - height - gap, false),
        None => (area_x + area_width - width - gap, area_y + gap, true),
    };
    let x = x.max(area_x + gap).min(area_x + area_width - width - gap);
    let y = y.max(area_y + gap).min(area_y + area_height - height - gap);
    let (x, y) = (x.round() as i32, y.round() as i32);

    let panel = panel_window(app, monitor_ref, x, y)?;
    let layered = LAYERED.lock().map(|flag| *flag).unwrap_or(false);
    if layered {
        overlay::resize_surface(&panel, width as i32, height as i32)?;
    } else {
        panel
            .set_size(PhysicalSize::new(width as u32, height as u32))
            .map_err(|error| error.to_string())?;
    }
    overlay::move_to(&panel, x, y, monitor_ref)?;
    Ok(below)
}

fn show_or_hide(app: &AppHandle, icon: Option<IconRect>, only_show: bool) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(LABEL) {
        if window.is_visible().unwrap_or(false) {
            if !only_show {
                window.hide().map_err(|error| error.to_string())?;
            }
            return Ok(());
        }
        if !only_show && now_millis() - HIDDEN_AT.load(Ordering::Relaxed) < REOPEN_GUARD_MS {
            return Ok(());
        }
    }
    let below = {
        // Placing builds the window the first time, so it comes before `panel_window` is asked for.
        let preferences = preferences(app);
        let monitor_id = preferences.and_then(|value| value.monitor_id);
        let probe = panel_window(app, monitor_id.as_deref(), 0, 0)?;
        place(app, &probe, icon)?
    };
    let window = app.get_webview_window(LABEL).ok_or("The tray panel is not open")?;
    window.show().map_err(|error| error.to_string())?;
    let _ = window.set_focus();
    let _ = app.emit_to(LABEL, "lume://tray-panel-opened", Opened { below });
    Ok(())
}

/// Opens the panel (or closes it, when `only_show` is false and it is open). Runs off the caller's thread: a tray
/// event handler must not build a window itself.
pub fn toggle(app: &AppHandle, icon: Option<IconRect>, only_show: bool) {
    if !PANEL_AVAILABLE {
        crate::workspace_windows::schedule_open(app);
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = crate::tour_card::on_window_thread(&app, move |app| show_or_hide(app, icon, only_show)) {
            eprintln!("Could not open the tray panel: {error}");
        }
    });
}

pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.hide();
    }
}

fn tr(portuguese: bool, english: &'static str, portuguese_text: &'static str) -> &'static str {
    if portuguese { portuguese_text } else { english }
}

/// A permission or a question is waiting for the person; an agent that only finished is "ready", not urgent.
fn needs_you(session: &AgentSession) -> bool {
    session.status == SessionStatus::PermissionRequired || session.pending_question.is_some()
}

fn attention_rank(session: &AgentSession) -> u8 {
    if needs_you(session) {
        return 0;
    }
    match session.status {
        SessionStatus::WaitingForInput => 1,
        SessionStatus::Running => 2,
        _ => 3,
    }
}

/// The agents the tray menu lists: the ones that need the person first, then the running ones.
fn menu_agents(sessions: &[AgentSession]) -> Vec<&AgentSession> {
    let mut active = sessions
        .iter()
        .filter(|session| {
            matches!(
                session.status,
                SessionStatus::PermissionRequired | SessionStatus::WaitingForInput | SessionStatus::Running
            )
        })
        .collect::<Vec<_>>();
    active.sort_by_key(|session| (attention_rank(session), std::cmp::Reverse(session.updated_at)));
    active.truncate(MENU_AGENTS);
    active
}

fn build_menu(app: &AppHandle, tray_mode: bool, portuguese: bool, sessions: &[AgentSession]) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::new(app)?;
    if tray_mode {
        let agents = menu_agents(sessions);
        let attention = agents.iter().filter(|session| attention_rank(session) == 0).count();
        let title = if agents.is_empty() {
            tr(portuguese, "No agents running", "Nenhum agente rodando").to_string()
        } else if attention > 0 {
            format!("{} {}", attention, tr(portuguese, "need you", "precisam de você"))
        } else {
            format!("{} {}", agents.len(), tr(portuguese, "running", "rodando"))
        };
        menu.append(&MenuItem::with_id(app, "title", format!("Lume — {title}"), false, None::<&str>)?)?;
        for session in &agents {
            let name = if session.session_name.is_empty() { session.project.clone() } else { session.session_name.clone() };
            let marker = if attention_rank(session) == 0 { "●" } else { "○" };
            menu.append(&MenuItem::with_id(
                app,
                format!("session:{}", session.id),
                format!("{marker} {name} — {}", session.status_label),
                true,
                None::<&str>,
            )?)?;
        }
        menu.append(&PredefinedMenuItem::separator(app)?)?;
        if PANEL_AVAILABLE {
            menu.append(&MenuItem::with_id(app, "panel", tr(portuguese, "Open panel", "Abrir painel"), true, None::<&str>)?)?;
        }
        menu.append(&MenuItem::with_id(app, "workspace", tr(portuguese, "Open Workspace", "Abrir Workspace"), true, None::<&str>)?)?;
        menu.append(&MenuItem::with_id(app, "orb", tr(portuguese, "Switch to the Orb", "Trocar para o Orb"), true, None::<&str>)?)?;
    } else {
        menu.append(&MenuItem::with_id(app, "show", tr(portuguese, "Show Lume", "Mostrar Lume"), true, None::<&str>)?)?;
        menu.append(&MenuItem::with_id(app, "workspace", tr(portuguese, "Open Workspace", "Abrir Workspace"), true, None::<&str>)?)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, "quit", tr(portuguese, "Quit", "Sair"), true, None::<&str>)?)?;
    Ok(menu)
}

/// Rebuilds the tray menu for the current mode and agents.
pub fn refresh_menu(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    let Some(state) = app.try_state::<AppState>() else { return };
    let Ok(preferences) = state.preferences() else { return };
    let tray_mode = preferences.startup_mode == MODE;
    let sessions = if tray_mode { state.sessions().unwrap_or_default() } else { Vec::new() };
    let portuguese = preferences.language == "pt-BR";
    if let Ok(menu) = build_menu(app, tray_mode, portuguese, &sessions) {
        let _ = tray.set_menu(Some(menu));
    }
    let needing = sessions.iter().filter(|session| needs_you(session)).count();
    let tooltip = if needing > 0 {
        format!("Lume — {needing} {}", tr(portuguese, "need you", "precisam de você"))
    } else {
        "Lume".to_string()
    };
    let _ = tray.set_tooltip(Some(tooltip));
}

/// Called whenever the sessions change: the menu follows them, at most once every couple of seconds.
pub fn sessions_changed(app: &AppHandle) {
    if !is_tray_mode(app) || MENU_PENDING.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(2));
        MENU_PENDING.store(false, Ordering::Release);
        refresh_menu(&app);
    });
}

/// The Orb goes away in tray mode and comes back otherwise.
pub fn apply_mode(app: &AppHandle) {
    let tray_mode = is_tray_mode(app);
    if let Some(window) = app.get_webview_window("main") {
        let handle = window.clone();
        let _ = window.run_on_main_thread(move || {
            if tray_mode {
                let _ = handle.hide();
            } else {
                let _ = handle.unminimize();
                let _ = handle.show();
            }
        });
    }
    if !tray_mode {
        hide(app);
    }
    refresh_menu(app);
}

/// Leaves tray mode for good: the Orb is the way Lume opens from now on.
pub fn go_orb(app: &AppHandle, state: &AppState) -> Result<(), String> {
    let mut preferences = state.preferences()?;
    preferences.startup_mode = "orb".into();
    state.save_preferences(&preferences)?;
    hide(app);
    apply_mode(app);
    Ok(())
}

/// The panel's buttons: go to the Orb (and stay there) or to the Workspace (and keep the tray).
#[tauri::command]
pub async fn switch_from_tray(app: AppHandle, state: tauri::State<'_, AppState>, target: String) -> Result<(), String> {
    match target.as_str() {
        "orb" => go_orb(&app, &state)?,
        "workspace" => {
            hide(&app);
            crate::workspace_windows::schedule_open(&app);
        }
        _ => return Err("Unknown target".into()),
    }
    Ok(())
}

#[tauri::command]
pub async fn hide_tray_panel(app: AppHandle) -> Result<(), String> {
    hide(&app);
    Ok(())
}
