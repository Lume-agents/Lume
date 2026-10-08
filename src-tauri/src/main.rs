// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "linux")]
fn should_use_xwayland_fallback(
    session_type: &str,
    desktop: &str,
    display: Option<&str>,
    force_native_wayland: bool,
) -> bool {
    let desktop_parts = desktop.split([':', ';']).map(str::trim).collect::<Vec<_>>();
    let is_gnome = desktop_parts.iter().any(|part| {
        part.eq_ignore_ascii_case("gnome") || part.to_lowercase().starts_with("gnome-")
    });
    let uses_native_gnome_backend = desktop_parts
        .iter()
        .any(|part| part.eq_ignore_ascii_case("pop"));
    session_type.eq_ignore_ascii_case("wayland")
        && is_gnome
        && !uses_native_gnome_backend
        && display.is_some_and(|value| !value.trim().is_empty())
        && !force_native_wayland
}

#[cfg(target_os = "linux")]
fn should_use_native_gnome_drag(session_type: &str, desktop: &str) -> bool {
    let desktop_parts = desktop.split([':', ';']).map(str::trim).collect::<Vec<_>>();
    let is_gnome = desktop_parts.iter().any(|part| {
        part.eq_ignore_ascii_case("gnome") || part.to_lowercase().starts_with("gnome-")
    });
    let is_pop = desktop_parts
        .iter()
        .any(|part| part.eq_ignore_ascii_case("pop"));
    session_type.eq_ignore_ascii_case("wayland") && is_gnome && is_pop
}

#[cfg(target_os = "linux")]
fn is_limited_gnome_wayland(
    session_type: &str,
    desktop: &str,
    display: Option<&str>,
    force_native_wayland: bool,
) -> bool {
    let desktop_parts = desktop.split([':', ';']).map(str::trim).collect::<Vec<_>>();
    let is_gnome = desktop_parts.iter().any(|part| {
        part.eq_ignore_ascii_case("gnome") || part.to_lowercase().starts_with("gnome-")
    });
    let is_pop = desktop_parts
        .iter()
        .any(|part| part.eq_ignore_ascii_case("pop"));
    session_type.eq_ignore_ascii_case("wayland")
        && is_gnome
        && !is_pop
        && (force_native_wayland || display.is_none_or(|value| value.trim().is_empty()))
}

#[cfg(target_os = "linux")]
fn webkit_dmabuf_renderer_override(
    session_type: &str,
    desktop: &str,
    current: Option<&std::ffi::OsStr>,
) -> Option<&'static str> {
    if !session_type.eq_ignore_ascii_case("wayland") {
        return None;
    }
    if desktop
        .split([':', ';'])
        .any(|part| part.trim().eq_ignore_ascii_case("cosmic"))
    {
        // Desktop launchers can inherit "1" even when the user's shell does not.
        // COSMIC needs the DMA-BUF renderer to clear transparent Orb contours.
        return (current != Some(std::ffi::OsStr::new("0"))).then_some("0");
    }
    current.is_none().then_some("1")
}

#[cfg(target_os = "linux")]
fn configure_linux_display_backend() {
    let force_native_wayland = std::env::var("LUME_FORCE_NATIVE_WAYLAND")
        .ok()
        .is_some_and(|value| matches!(value.to_lowercase().as_str(), "1" | "true" | "yes"));
    let session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let display = std::env::var("DISPLAY").ok();

    if should_use_xwayland_fallback(
        &session_type,
        &desktop,
        display.as_deref(),
        force_native_wayland,
    ) {
        // GNOME Wayland does not expose the Layer Shell protocol and regular
        // windows cannot be positioned globally. XWayland keeps dragging and
        // persisted overlay coordinates functional on Fedora Workstation.
        std::env::set_var("GDK_BACKEND", "x11");
        std::env::set_var("LUME_LINUX_BACKEND", "xwayland-fallback");
        eprintln!("Lume: usando XWayland para posicionamento compatível com GNOME");
    } else if should_use_native_gnome_drag(&session_type, &desktop) {
        std::env::set_var("LUME_LINUX_BACKEND", "native-gnome");
        eprintln!("Lume: usando Wayland nativo com arraste do GNOME");
    } else if is_limited_gnome_wayland(
        &session_type,
        &desktop,
        display.as_deref(),
        force_native_wayland,
    ) {
        std::env::set_var("LUME_LINUX_BACKEND", "gnome-wayland-limited");
        eprintln!(
            "Lume: GNOME Wayland sem XWayland; posicionamento e docking de janelas estão limitados"
        );
    }
}

/// The Windows release is a GUI-subsystem program, so a terminal gives it no stdout. The command-line
/// subcommands attach to the parent terminal and fill in only the standard handles that are missing,
/// leaving redirected ones (pipes, files) untouched.
#[cfg(windows)]
fn attach_parent_console() {
    use std::os::windows::io::AsRawHandle;
    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
        fn GetStdHandle(handle_id: u32) -> *mut core::ffi::c_void;
        fn SetStdHandle(handle_id: u32, handle: *mut core::ffi::c_void) -> i32;
    }
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const STD_ERROR_HANDLE: u32 = -12i32 as u32;
    // SAFETY: plain Win32 calls with valid constants; the opened handle is intentionally kept for the process lifetime.
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            return;
        }
        for id in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            let current = GetStdHandle(id);
            if current.is_null() || current as isize == -1 {
                if let Ok(console) = std::fs::OpenOptions::new().write(true).open("CONOUT$") {
                    SetStdHandle(id, console.as_raw_handle() as *mut core::ffi::c_void);
                    std::mem::forget(console);
                }
            }
        }
    }
}

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    #[cfg(windows)]
    if matches!(args.get(1).map(String::as_str), Some("node" | "identity-probe")) {
        attach_parent_console();
    }
    if args.get(1).map(String::as_str) == Some("node") {
        std::process::exit(lume_lib::run_node_cli(&args[2..]));
    }
    if args.get(1).map(String::as_str) == Some("identity-probe") {
        std::process::exit(lume_lib::run_identity_probe_cli(&args[2..]));
    }
    #[cfg(target_os = "macos")]
    if args.get(1).map(String::as_str) == Some("codex-process-supervisor") {
        std::process::exit(lume_lib::run_codex_process_supervisor_cli(&args[2..]));
    }
    if args.get(1).map(String::as_str) == Some("hook") {
        let provider = args.get(2).map(String::as_str).unwrap_or("");
        std::process::exit(lume_lib::run_hook_client(provider));
    }
    if args.get(1).map(String::as_str) == Some("terminal-run") {
        let payload = args.get(2).map(String::as_str).unwrap_or("");
        std::process::exit(lume_lib::run_terminal_payload(payload));
    }
    if args.get(1).map(String::as_str) == Some("ingest") {
        std::process::exit(lume_lib::run_ingest_client());
    }
    #[cfg(target_os = "linux")]
    configure_linux_display_backend();
    #[cfg(target_os = "linux")]
    {
        let current_renderer = std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER");
        let session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        if let Some(value) =
            webkit_dmabuf_renderer_override(&session_type, &desktop, current_renderer.as_deref())
        {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", value);
        }
    }
    lume_lib::run()
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::{
        is_limited_gnome_wayland, should_use_native_gnome_drag, should_use_xwayland_fallback,
        webkit_dmabuf_renderer_override,
    };
    use std::ffi::OsStr;

    #[test]
    fn cosmic_wayland_enables_dmabuf_even_when_the_desktop_inherits_the_old_fallback() {
        assert_eq!(
            webkit_dmabuf_renderer_override("wayland", "COSMIC", None),
            Some("0")
        );
        assert_eq!(
            webkit_dmabuf_renderer_override("wayland", "pop:COSMIC", Some(OsStr::new("1"))),
            Some("0")
        );
        assert_eq!(
            webkit_dmabuf_renderer_override("wayland", "COSMIC", Some(OsStr::new("0"))),
            None
        );
    }

    #[test]
    fn other_linux_backends_keep_their_existing_renderer_choice() {
        assert_eq!(
            webkit_dmabuf_renderer_override("wayland", "ubuntu:GNOME", None),
            Some("1")
        );
        assert_eq!(
            webkit_dmabuf_renderer_override("wayland", "ubuntu:GNOME", Some(OsStr::new("0"))),
            None
        );
        assert_eq!(webkit_dmabuf_renderer_override("x11", "COSMIC", None), None);
        assert_eq!(
            webkit_dmabuf_renderer_override("x11", "COSMIC", Some(OsStr::new("1"))),
            None
        );
    }

    #[test]
    fn uses_xwayland_on_fedora_gnome_wayland() {
        assert!(should_use_xwayland_fallback(
            "wayland",
            "GNOME",
            Some(":0"),
            false
        ));
    }

    #[test]
    fn uses_position_compatible_backend_on_ubuntu_and_keeps_pop_native() {
        assert!(should_use_xwayland_fallback(
            "wayland",
            "ubuntu:GNOME",
            Some(":1"),
            false
        ));
        assert!(!should_use_xwayland_fallback(
            "wayland",
            "pop:GNOME",
            Some(":1"),
            false
        ));
        assert!(!should_use_native_gnome_drag("wayland", "ubuntu:GNOME"));
        assert!(should_use_native_gnome_drag("wayland", "pop:GNOME"));
        assert!(!should_use_native_gnome_drag("wayland", "COSMIC"));
        assert!(!should_use_native_gnome_drag("x11", "ubuntu:GNOME"));
    }

    #[test]
    fn keeps_native_backend_when_layer_shell_can_be_available() {
        assert!(!should_use_xwayland_fallback(
            "wayland",
            "KDE",
            Some(":0"),
            false
        ));
        assert!(!should_use_xwayland_fallback(
            "wayland",
            "COSMIC",
            Some(":0"),
            false
        ));
    }

    #[test]
    fn respects_session_capabilities_and_native_override() {
        assert!(!should_use_xwayland_fallback(
            "x11",
            "GNOME",
            Some(":0"),
            false
        ));
        assert!(!should_use_xwayland_fallback(
            "wayland", "GNOME", None, false
        ));
        assert!(!should_use_xwayland_fallback(
            "wayland",
            "GNOME",
            Some(":0"),
            true
        ));
    }

    #[test]
    fn reports_gnome_wayland_when_xwayland_is_unavailable_or_disabled() {
        assert!(is_limited_gnome_wayland(
            "wayland",
            "ubuntu:GNOME",
            None,
            false
        ));
        assert!(is_limited_gnome_wayland(
            "wayland",
            "GNOME",
            Some(":1"),
            true
        ));
        assert!(!is_limited_gnome_wayland(
            "wayland",
            "pop:GNOME",
            None,
            false
        ));
        assert!(!is_limited_gnome_wayland(
            "x11",
            "ubuntu:GNOME",
            None,
            false
        ));
    }
}
