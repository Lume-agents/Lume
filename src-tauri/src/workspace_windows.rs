use std::{
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::terminal_windows::TerminalWindows;

const LOAD_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, PartialEq, Eq)]
enum BootStatus {
    Loading,
    FrontendReady,
    Ready,
    Failed(String),
}

struct BootAttempt {
    id: u64,
    started: Instant,
    status: BootStatus,
}

#[derive(Default)]
struct BootState {
    generation: u64,
    current: Option<BootAttempt>,
}

/// Serializes opens without blocking the UI thread. The frontend acknowledgement
/// uses a separate lock, so it can complete while the native worker awaits it.
#[derive(Clone, Default)]
pub struct WorkspaceWindows {
    opening: Arc<tauri::async_runtime::Mutex<()>>,
    boot: Arc<(Mutex<BootState>, Condvar)>,
}

impl WorkspaceWindows {
    pub async fn open(&self, app: AppHandle) -> Result<(), String> {
        let _opening = self.opening.lock().await;
        let coordinator = self.clone();
        let worker_app = app.clone();
        let result = tauri::async_runtime::spawn_blocking(move || coordinator.reveal(&worker_app))
            .await
            .unwrap_or_else(|error| Err(format!("Não foi possível abrir o Workspace: {error}")));
        if let Err(error) = &result {
            eprintln!("Workspace startup failed: {error}");
            let _ = app.emit_to("main", "lume://workspace-open-failed", error);
        }
        result
    }

    pub fn is_ready(&self) -> bool {
        self.boot.0.lock().ok().is_some_and(|state| {
            state
                .current
                .as_ref()
                .is_some_and(|attempt| attempt.status == BootStatus::Ready)
        })
    }

    fn begin(&self) -> Result<u64, String> {
        let mut state = self
            .boot
            .0
            .lock()
            .map_err(|_| "Não foi possível iniciar o Workspace")?;
        state.generation = state
            .generation
            .checked_add(1)
            .ok_or("Limite de aberturas do Workspace atingido")?;
        let id = state.generation;
        state.current = Some(BootAttempt {
            id,
            started: Instant::now(),
            status: BootStatus::Loading,
        });
        Ok(id)
    }

    pub fn frontend_ready(&self, id: u64) -> Result<(), String> {
        self.update(id, |status| match status {
            BootStatus::Loading => {
                *status = BootStatus::FrontendReady;
                Ok(())
            }
            BootStatus::FrontendReady | BootStatus::Ready => Ok(()),
            BootStatus::Failed(error) => Err(error.clone()),
        })
    }

    pub fn frontend_failed(&self, id: u64, reason: String) -> Result<(), String> {
        self.update(id, |status| {
            if *status == BootStatus::Ready {
                return Err("O carregamento do Workspace já terminou".into());
            }
            *status = BootStatus::Failed(reason.chars().take(4096).collect());
            Ok(())
        })
    }

    fn update(
        &self,
        id: u64,
        update: impl FnOnce(&mut BootStatus) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut state = self
            .boot
            .0
            .lock()
            .map_err(|_| "Não foi possível atualizar o Workspace")?;
        let attempt = state
            .current
            .as_mut()
            .filter(|attempt| attempt.id == id)
            .ok_or("Esta abertura do Workspace não está mais ativa")?;
        let result = update(&mut attempt.status);
        self.boot.1.notify_all();
        result
    }

    fn wait_for_frontend(&self, id: u64, timeout: Duration) -> Result<(), String> {
        let mut state = self
            .boot
            .0
            .lock()
            .map_err(|_| "Não foi possível aguardar o Workspace")?;
        loop {
            let attempt = state
                .current
                .as_mut()
                .filter(|attempt| attempt.id == id)
                .ok_or("A janela do Workspace foi fechada durante o carregamento")?;
            match &attempt.status {
                BootStatus::FrontendReady | BootStatus::Ready => return Ok(()),
                BootStatus::Failed(error) => return Err(error.clone()),
                BootStatus::Loading => {}
            }
            let remaining = timeout.saturating_sub(attempt.started.elapsed());
            if remaining.is_zero() {
                let error =
                    "O Workspace não terminou de carregar. Tente abri-lo novamente.".to_string();
                attempt.status = BootStatus::Failed(error.clone());
                return Err(error);
            }
            state = self
                .boot
                .1
                .wait_timeout(state, remaining)
                .map_err(|_| "Não foi possível aguardar o Workspace")?
                .0;
        }
    }

    fn closed(&self, id: u64) -> bool {
        let Ok(mut state) = self.boot.0.lock() else {
            return false;
        };
        if !state
            .current
            .as_ref()
            .is_some_and(|attempt| attempt.id == id)
        {
            return false;
        }
        state.current = None;
        self.boot.1.notify_all();
        true
    }

    fn reveal(&self, app: &AppHandle) -> Result<(), String> {
        if let Some(window) = app.get_webview_window("workspace") {
            if self.is_ready() {
                let result = self.present(app, &window);
                if result.is_err() {
                    let _ = window.hide();
                    let _ = window.destroy();
                    restore_orb(app);
                }
                return result;
            }
            // A failed webview must not be reused by a retry.
            window.destroy().map_err(|error| error.to_string())?;
            let deadline = Instant::now() + Duration::from_secs(1);
            while app.get_webview_window("workspace").is_some() {
                if Instant::now() >= deadline {
                    return Err(
                        "A janela anterior do Workspace ainda está fechando. Tente novamente."
                            .into(),
                    );
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        let id = self.begin()?;
        let result = (|| {
            let window =
                WebviewWindowBuilder::new(app, "workspace", WebviewUrl::App("workspace/".into()))
                    .title("Lume · Workspace")
                    .inner_size(1280.0, 800.0)
                    .min_inner_size(720.0, 520.0)
                    .resizable(true)
                    .decorations(false)
                    .transparent(true)
                    .visible(false)
                    .initialization_script(format!("window.__LUME_WORKSPACE_BOOT_ID__ = {id};"))
                    .center()
                    .build()
                    .map_err(|error| error.to_string())?;
            let coordinator = self.clone();
            let app_for_close = app.clone();
            window.on_window_event(move |event| {
                if matches!(event, tauri::WindowEvent::Destroyed) && coordinator.closed(id) {
                    let coordinator = coordinator.clone();
                    let app = app_for_close.clone();
                    tauri::async_runtime::spawn(async move {
                        let _opening = coordinator.opening.lock().await;
                        if !coordinator.is_ready() {
                            let _ = tauri::async_runtime::spawn_blocking(move || restore_orb(&app))
                                .await;
                        }
                    });
                }
            });
            self.wait_for_frontend(id, LOAD_TIMEOUT)?;
            self.present(app, &window)?;
            self.update(id, |status| {
                if *status != BootStatus::FrontendReady {
                    return Err("O Workspace foi interrompido antes de abrir".into());
                }
                *status = BootStatus::Ready;
                Ok(())
            })
        })();
        if let Err(error) = &result {
            let _ = self.frontend_failed(id, error.clone());
            if let Some(window) = app.get_webview_window("workspace") {
                let _ = window.hide();
                let _ = window.destroy();
            }
            restore_orb(app);
        }
        result
    }

    fn present(&self, app: &AppHandle, window: &WebviewWindow) -> Result<(), String> {
        let terminals = app.state::<TerminalWindows>();
        terminals.suspend_for_workspace(app)?;
        if let Err(error) = window
            .unminimize()
            .and_then(|_| window.show())
            .and_then(|_| window.set_focus())
        {
            let _ = terminals.restore_after_workspace(app);
            return Err(error.to_string());
        }
        if let Some(main) = app.get_webview_window("main") {
            let _ = main.hide();
        }
        Ok(())
    }
}

fn restore_orb(app: &AppHandle) {
    if let Some(terminals) = app.try_state::<TerminalWindows>() {
        let _ = terminals.restore_after_workspace(app);
    }
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.unminimize();
        let _ = main.show();
        let _ = main.set_focus();
    }
}

pub fn schedule_open(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let coordinator = app.state::<WorkspaceWindows>().inner().clone();
        let _ = coordinator.open(app).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_is_not_ready_until_frontend_and_presentation_complete() {
        let windows = WorkspaceWindows::default();
        let id = windows.begin().unwrap();
        assert!(!windows.is_ready());
        windows.frontend_ready(id).unwrap();
        assert!(!windows.is_ready());
        windows
            .wait_for_frontend(id, Duration::from_millis(10))
            .unwrap();
        windows
            .update(id, |status| {
                *status = BootStatus::Ready;
                Ok(())
            })
            .unwrap();
        assert!(windows.is_ready());
    }

    #[test]
    fn failed_boot_wakes_the_worker_with_the_original_error() {
        let windows = WorkspaceWindows::default();
        let id = windows.begin().unwrap();
        let waiting = windows.clone();
        let worker =
            std::thread::spawn(move || waiting.wait_for_frontend(id, Duration::from_secs(1)));
        windows
            .frontend_failed(id, "Preferences unavailable".into())
            .unwrap();
        assert_eq!(
            worker.join().unwrap().unwrap_err(),
            "Preferences unavailable"
        );
        assert!(!windows.is_ready());
    }

    #[test]
    fn timeout_rejects_a_late_acknowledgement() {
        let windows = WorkspaceWindows::default();
        let id = windows.begin().unwrap();
        assert!(windows.wait_for_frontend(id, Duration::ZERO).is_err());
        assert!(windows.frontend_ready(id).is_err());
    }

    #[test]
    fn retry_rejects_signals_and_destroy_events_from_the_old_window() {
        let windows = WorkspaceWindows::default();
        let old_id = windows.begin().unwrap();
        windows.frontend_failed(old_id, "Failed".into()).unwrap();
        let new_id = windows.begin().unwrap();
        assert_ne!(old_id, new_id);
        assert!(windows.frontend_ready(old_id).is_err());
        assert!(!windows.closed(old_id));
        windows.frontend_ready(new_id).unwrap();
        windows
            .wait_for_frontend(new_id, Duration::from_millis(10))
            .unwrap();
    }

    #[test]
    fn closing_a_loading_window_unblocks_the_waiter() {
        let windows = WorkspaceWindows::default();
        let id = windows.begin().unwrap();
        assert!(windows.closed(id));
        assert!(windows
            .wait_for_frontend(id, Duration::from_secs(1))
            .is_err());
    }

    #[test]
    fn simultaneous_opens_share_the_same_async_gate() {
        let windows = WorkspaceWindows::default();
        let other = windows.clone();
        tauri::async_runtime::block_on(async {
            let first = windows.opening.lock().await;
            assert!(other.opening.try_lock().is_err());
            drop(first);
            assert!(other.opening.try_lock().is_ok());
        });
    }

    #[test]
    fn completed_window_does_not_accept_a_stale_startup_failure() {
        let windows = WorkspaceWindows::default();
        let id = windows.begin().unwrap();
        windows.frontend_ready(id).unwrap();
        windows
            .update(id, |status| {
                *status = BootStatus::Ready;
                Ok(())
            })
            .unwrap();
        assert!(windows.frontend_failed(id, "Late error".into()).is_err());
        assert!(windows.is_ready());
    }
}
