//! The `lume` command on Windows: a small `lume.cmd` the installer puts in `%LOCALAPPDATA%\Microsoft\WindowsApps`,
//! a folder already on every user's PATH. It points at the installed `lume.exe`, so it goes stale when the
//! folder is moved or Lume is installed somewhere else; the app rewrites it on start when that happens.

use std::path::{Path, PathBuf};

/// First line of every launcher Lume writes; also how an older one (without it) is recognized.
const MARKER: &str = "REM Lume command-line launcher";

/// What `lume.cmd` contains for an executable. A missing executable is reported in the terminal:
/// `start` alone opens a "Windows cannot find ..." dialog instead.
pub fn script(executable: &Path) -> String {
    let path = executable.display();
    format!(
        "@echo off\r\n{MARKER}\r\nif not exist \"{path}\" (\r\n  echo Lume was not found at \"{path}\". Open Lume once from where it is installed, or reinstall it.\r\n  exit /b 1\r\n)\r\nstart \"\" /b /wait \"{path}\" %*\r\n"
    )
}

fn is_lume_launcher(existing: &str) -> bool {
    existing.contains(MARKER) || (existing.contains("start \"\" /b /wait") && existing.to_ascii_lowercase().contains("lume.exe"))
}

/// The launcher's new content when it should change: it exists, it is Lume's, and it points elsewhere.
pub fn refreshed(existing: &str, executable: &Path) -> Option<String> {
    let wanted = script(executable);
    (is_lume_launcher(existing) && existing != wanted).then_some(wanted)
}

fn launcher_path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|base| PathBuf::from(base).join("Microsoft").join("WindowsApps").join("lume.cmd"))
}

/// Points an existing launcher at the Lume that is running. Never creates one: a portable copy or a
/// removed install must not bring the command back.
pub fn refresh(executable: &Path) {
    let Some(path) = launcher_path() else { return };
    let Ok(existing) = std::fs::read_to_string(&path) else { return };
    if let Some(next) = refreshed(&existing, executable) {
        let _ = std::fs::write(path, next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_launcher_for_a_missing_executable_says_so_instead_of_opening_a_dialog() {
        let text = script(Path::new(r"G:\Users\Ana\Downloads\Lume\lume.exe"));
        assert!(text.contains(r#"if not exist "G:\Users\Ana\Downloads\Lume\lume.exe""#));
        assert!(text.contains("exit /b 1"));
        assert!(text.contains(r#"start "" /b /wait "G:\Users\Ana\Downloads\Lume\lume.exe" %*"#));
        assert!(text.starts_with("@echo off\r\n"));
    }

    #[test]
    fn a_stale_launcher_is_pointed_at_the_running_lume_and_only_lumes_own() {
        let old = "@echo off\r\nstart \"\" /b /wait \"G:\\old\\lume.exe\" %*\r\n";
        let running = Path::new(r"C:\Users\Ana\AppData\Local\Lume\lume.exe");
        let next = refreshed(old, running).expect("an old launcher is rewritten");
        assert!(next.contains(r"C:\Users\Ana\AppData\Local\Lume\lume.exe"));
        assert_eq!(refreshed(&next, running), None, "already right: nothing to write");
        assert_eq!(refreshed("@echo off\r\necho my own lume script\r\n", running), None, "someone else's file is left alone");
    }
}
