//! The `lume` command on Windows: a small `lume.cmd` the installer puts in `%LOCALAPPDATA%\Microsoft\WindowsApps`,
//! a folder already on every user's PATH. It points at the installed `lume.exe`, so it goes stale when the
//! folder is moved or Lume is installed somewhere else; the app rewrites it on start when that happens.

use std::path::{Path, PathBuf};

/// First line of every launcher Lume writes; also how an older one (without it) is recognized.
const MARKER: &str = "REM Lume command-line launcher";

/// What `lume.cmd` contains for an executable.
///
/// The file is UTF-8 and switches the console to code page 65001 before the line with the path: `cmd`
/// reads a batch file with the console's code page, so a folder such as `João Pedro` written in any other
/// encoding reads back as other characters (`Joπo` on code page 437) and the path no longer exists.
/// The original code page is restored on the way out. A missing executable is reported in the terminal:
/// `start` alone opens a "Windows cannot find ..." dialog instead.
pub fn script(executable: &Path) -> String {
    let path = executable.display();
    [
        "@echo off".to_string(),
        MARKER.to_string(),
        r#"for /f "tokens=2 delims=:." %%c in ('chcp') do set "LUME_CP=%%c""#.to_string(),
        "chcp 65001 >nul".to_string(),
        format!(r#"set "LUME_EXE={path}""#),
        r#"if exist "%LUME_EXE%" goto run"#.to_string(),
        r#"echo Lume was not found at "%LUME_EXE%". Open Lume once from where it is installed, or reinstall it."#.to_string(),
        "set LUME_EXIT=1".to_string(),
        "goto done".to_string(),
        ":run".to_string(),
        r#"start "" /b /wait "%LUME_EXE%" %*"#.to_string(),
        "set LUME_EXIT=%errorlevel%".to_string(),
        ":done".to_string(),
        "chcp %LUME_CP% >nul".to_string(),
        "exit /b %LUME_EXIT%".to_string(),
        String::new(),
    ]
    .join("\r\n")
}

fn is_lume_launcher(existing: &str) -> bool {
    existing.contains(MARKER) || (existing.contains("start \"\" /b /wait") && existing.to_ascii_lowercase().contains("lume.exe"))
}

/// The launcher's new bytes when it should change: it exists, it is Lume's, and it is not what Lume writes now
/// (a different folder, or an older layout, or the installer's ANSI text that `cmd` misreads).
pub fn refreshed(existing: &[u8], executable: &Path) -> Option<String> {
    let wanted = script(executable);
    let text = String::from_utf8_lossy(existing);
    (is_lume_launcher(&text) && existing != wanted.as_bytes()).then_some(wanted)
}

fn launcher_path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|base| PathBuf::from(base).join("Microsoft").join("WindowsApps").join("lume.cmd"))
}

/// Points an existing launcher at the Lume that is running. Never creates one: a portable copy or a
/// removed install must not bring the command back.
pub fn refresh(executable: &Path) {
    let Some(path) = launcher_path() else { return };
    // Read bytes, not text: the installer writes this file in the ANSI code page, which is not UTF-8.
    let Ok(existing) = std::fs::read(&path) else { return };
    if let Some(next) = refreshed(&existing, executable) {
        let _ = std::fs::write(path, next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_launcher_switches_to_utf8_before_the_path_and_restores_the_code_page() {
        let text = script(Path::new(r"G:\Users\João Pedro\Downloads\Lume\lume.exe"));
        let line = |needle: &str| text.lines().position(|line| line.contains(needle)).unwrap_or_else(|| panic!("{needle}"));
        assert!(line("chcp 65001") < line("João Pedro"), "the path is read after cmd switched to UTF-8");
        assert!(line("tokens=2 delims=:.") < line("chcp 65001"), "the old code page is saved first");
        assert!(text.contains(r#"set "LUME_EXE=G:\Users\João Pedro\Downloads\Lume\lume.exe""#));
        assert!(text.contains(r#"start "" /b /wait "%LUME_EXE%" %*"#));
        assert!(line("chcp %LUME_CP%") > line("start \"\""), "restored after Lume exits");
        assert!(text.ends_with("exit /b %LUME_EXIT%\r\n"));
        assert!(!text.contains('\u{feff}'), "a byte-order mark would break the first line");
    }

    #[test]
    fn a_missing_executable_is_reported_in_the_terminal_not_in_a_dialog() {
        let text = script(Path::new(r"C:\Lume\lume.exe"));
        assert!(text.contains(r#"if exist "%LUME_EXE%" goto run"#));
        assert!(text.contains("Lume was not found at"));
        assert!(text.contains("set LUME_EXIT=1"));
    }

    #[test]
    fn a_stale_or_ansi_launcher_is_rewritten_and_someone_elses_file_is_not() {
        let running = Path::new(r"C:\Users\Ana\AppData\Local\Lume\lume.exe");
        // What the installer wrote: ANSI bytes, `ã` as 0xE3, which is not valid UTF-8.
        let mut ansi = b"@echo off\r\nstart \"\" /b /wait \"G:\\Users\\Jo".to_vec();
        ansi.extend_from_slice(&[0xE3]);
        ansi.extend_from_slice(b"o Pedro\\Downloads\\Lume\\lume.exe\" %*\r\n");
        let next = refreshed(&ansi, running).expect("the installer's launcher is replaced");
        assert!(next.contains(r"C:\Users\Ana\AppData\Local\Lume\lume.exe"));
        assert_eq!(refreshed(next.as_bytes(), running), None, "already right: nothing to write");
        assert_eq!(refreshed(b"@echo off\r\necho my own lume script\r\n", running), None);
    }
}
