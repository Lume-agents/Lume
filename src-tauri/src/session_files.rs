//! Opening a file an agent mentioned, without letting the agent's text run anything.
//!
//! The frontend cannot be trusted with `opener:allow-open-path`: the path comes from the agent's
//! own words, and opening a `.command`, `.app` or `.bat` from there would execute code. The path is
//! checked here instead: it must resolve, symlinks included, to a regular file inside the
//! session's working directory, and it must not be something the system would run when opened.

use std::path::{Path, PathBuf};

/// Extensions that run code, install something or open a bundle when the system "opens" them.
const NEVER_OPEN: &[&str] = &[
    "app", "command", "tool", "terminal", "workflow", "action", "scpt", "scptd", "applescript", "sh", "bash", "zsh", "fish", "csh", "ksh",
    "exe", "com", "bat", "cmd", "msi", "msix", "msc", "scr", "pif", "cpl", "dll", "ps1", "psm1", "psd1", "vbs", "vbe", "wsf", "wsh", "hta", "reg", "inf",
    "lnk", "url", "desktop", "jar", "appimage", "run", "bin", "pkg", "dmg", "deb", "rpm", "application", "gadget", "settingcontent-ms",
];

/// Scripts that only run on Windows, where a double click hands them to the script host.
#[cfg(windows)]
const NEVER_OPEN_ON_WINDOWS: &[&str] = &["js", "jse", "py", "pyw", "pl", "rb"];
#[cfg(not(windows))]
const NEVER_OPEN_ON_WINDOWS: &[&str] = &[];

/// `file.ts:12:3` is how agents cite a line; the position is not part of the path.
fn without_position(path: &str) -> &str {
    let mut end = path.len();
    for _ in 0..2 {
        match path[..end].rsplit_once(':') {
            Some((head, tail)) if !tail.is_empty() && tail.bytes().all(|byte| byte.is_ascii_digit()) && !head.is_empty() => end = head.len(),
            _ => break,
        }
    }
    &path[..end]
}

fn is_executable_file(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).map(|metadata| metadata.permissions().mode() & 0o111 != 0).unwrap_or(true)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        false
    }
}

/// The file to open, or the reason it must not be opened.
pub fn openable_file(working_directory: &str, requested: &str) -> Result<PathBuf, String> {
    if working_directory.trim().is_empty() {
        return Err("This session has no project folder, so its files cannot be opened from here".into());
    }
    let root = std::fs::canonicalize(working_directory).map_err(|_| "The session's project folder no longer exists".to_string())?;
    let requested = without_position(requested.trim().trim_start_matches("file://"));
    if requested.is_empty() {
        return Err("No file was given".into());
    }
    let candidate = Path::new(requested);
    let candidate = if candidate.is_absolute() { candidate.to_path_buf() } else { root.join(candidate) };
    let resolved = std::fs::canonicalize(&candidate).map_err(|_| "That file does not exist".to_string())?;
    if !resolved.starts_with(&root) {
        return Err("That file is outside the session's project folder".into());
    }
    if !resolved.is_file() {
        return Err("Only files can be opened".into());
    }
    let extension = resolved.extension().map(|extension| extension.to_string_lossy().to_lowercase()).unwrap_or_default();
    if NEVER_OPEN.contains(&extension.as_str()) || NEVER_OPEN_ON_WINDOWS.contains(&extension.as_str()) {
        return Err(format!("Lume does not open .{extension} files from an agent's text because opening them could run code"));
    }
    if is_executable_file(&resolved) {
        return Err("Lume does not open executable files from an agent's text".into());
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!("lume-session-files-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join("project/src")).unwrap();
            fs::create_dir_all(root.join("outside")).unwrap();
            Self(root)
        }
        fn project(&self) -> String {
            self.0.join("project").to_string_lossy().into_owned()
        }
        fn write(&self, relative: &str, contents: &str) -> PathBuf {
            let path = self.0.join(relative);
            fs::write(&path, contents).unwrap();
            path
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn opens_project_files_by_absolute_or_relative_path_and_ignores_the_cited_position() {
        let fixture = Fixture::new("open");
        let file = fixture.write("project/src/main.rs", "fn main() {}");
        let expected = fs::canonicalize(&file).unwrap();
        assert_eq!(openable_file(&fixture.project(), "src/main.rs").unwrap(), expected);
        assert_eq!(openable_file(&fixture.project(), "./src/main.rs:12:3").unwrap(), expected);
        assert_eq!(openable_file(&fixture.project(), file.to_str().unwrap()).unwrap(), expected);
        assert_eq!(openable_file(&fixture.project(), &format!("file://{}", file.display())).unwrap(), expected);
    }

    #[test]
    fn refuses_files_outside_the_project_including_through_dots_and_symlinks() {
        let fixture = Fixture::new("outside");
        fixture.write("outside/secret.txt", "x");
        assert!(openable_file(&fixture.project(), "../outside/secret.txt").unwrap_err().contains("outside"));
        assert!(openable_file(&fixture.project(), fixture.0.join("outside/secret.txt").to_str().unwrap()).unwrap_err().contains("outside"));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(fixture.0.join("outside/secret.txt"), fixture.0.join("project/link.txt")).unwrap();
            assert!(openable_file(&fixture.project(), "link.txt").unwrap_err().contains("outside"), "a symlink must not lead out of the project");
        }
    }

    #[test]
    fn refuses_what_the_system_would_run() {
        let fixture = Fixture::new("exec");
        for name in ["run.command", "setup.sh", "Tool.APP", "go.bat", "x.ps1", "evil.desktop", "a.lnk", "t.terminal", "w.workflow"] {
            fixture.write(&format!("project/{name}"), "echo hi");
            let error = openable_file(&fixture.project(), name).unwrap_err();
            assert!(error.contains("could run code"), "{name}: {error}");
        }
        fixture.write("project/notes.md", "# notes");
        assert!(openable_file(&fixture.project(), "notes.md").is_ok());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let path = fixture.write("project/build", "#!/bin/sh");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            assert!(openable_file(&fixture.project(), "build").unwrap_err().contains("executable"));
        }
    }

    #[test]
    fn refuses_missing_files_directories_and_sessions_without_a_folder() {
        let fixture = Fixture::new("missing");
        assert!(openable_file(&fixture.project(), "nope.txt").unwrap_err().contains("does not exist"));
        assert!(openable_file(&fixture.project(), "src").unwrap_err().contains("Only files"));
        assert!(openable_file("", "a.txt").is_err());
        assert!(openable_file(&fixture.project(), "").is_err());
    }

    #[test]
    fn positions_are_stripped_only_when_they_are_numbers() {
        assert_eq!(without_position("a.ts:12:3"), "a.ts");
        assert_eq!(without_position("a.ts:12"), "a.ts");
        assert_eq!(without_position("a.ts"), "a.ts");
        assert_eq!(without_position("C:\\work\\a.ts:7"), "C:\\work\\a.ts");
        assert_eq!(without_position("C:\\work"), "C:\\work");
    }
}
