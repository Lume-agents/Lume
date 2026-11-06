use std::{
    env,
    io::Read,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use semver::Version;
use serde::Deserialize;

pub fn command(name: &str) -> Result<Command, String> {
    let path = resolve(name).ok_or_else(|| {
        format!(
            "Could not find `{name}`. Open a terminal, confirm `{name} --version`, and restart Lume."
        )
    })?;
    Ok(command_from_path(&path))
}

/// Lume runs its own app-server. A separately updated Codex daemon can have a
/// newer binary and model catalog than the CLI that the GUI finds on PATH.
pub fn codex_app_server_command() -> Result<Command, String> {
    if let Some(path) = explicit_path("codex") {
        return Ok(command_from_path(&path));
    }

    let cli = resolve("codex");
    let daemon = codex_home().and_then(|home| codex_daemon_package(&home));
    let path = match (cli, daemon) {
        (Some(cli), Some(daemon)) => {
            let version = executable_version(&cli);
            if prefer_daemon(version.as_ref(), &daemon.version) {
                daemon.entrypoint
            } else {
                cli
            }
        }
        (Some(cli), None) => cli,
        (None, Some(daemon)) => daemon.entrypoint,
        (None, None) => {
            return Err("Could not find `codex`. Open a terminal, confirm `codex --version`, and restart Lume.".into());
        }
    };
    Ok(command_from_path(&path))
}

fn command_from_path(path: &Path) -> Command {
    let mut command = platform_command(path);
    prepend_parent_to_path(&mut command, path);
    command
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexPackageManifest {
    layout_version: u32,
    version: String,
    target: String,
    variant: String,
    entrypoint: PathBuf,
}

struct CodexDaemonPackage {
    version: Version,
    entrypoint: PathBuf,
}

fn codex_home() -> Option<PathBuf> {
    env::var_os("CODEX_HOME").map(PathBuf::from).or_else(|| {
        env::var_os("HOME")
            .or_else(|| env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .map(|home| home.join(".codex"))
    })
}

fn codex_daemon_package(codex_home: &Path) -> Option<CodexDaemonPackage> {
    let root = codex_home.join("packages/app-server-daemon/current");
    let manifest: CodexPackageManifest =
        serde_json::from_slice(&std::fs::read(root.join("codex-package.json")).ok()?).ok()?;
    if manifest.layout_version != 1
        || manifest.variant != "codex"
        || !package_target_matches(&manifest.target)
        || !manifest
            .entrypoint
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return None;
    }
    let entrypoint = root.join(manifest.entrypoint);
    entrypoint.is_file().then_some(CodexDaemonPackage {
        version: Version::parse(&manifest.version).ok()?,
        entrypoint,
    })
}

fn package_target_matches(target: &str) -> bool {
    let os = match env::consts::OS {
        "macos" => "darwin",
        "windows" => "windows",
        "linux" => "linux",
        _ => return false,
    };
    let mut parts = target.split('-');
    parts.next() == Some(env::consts::ARCH) && parts.any(|part| part == os)
}

fn prefer_daemon(cli_version: Option<&Version>, daemon_version: &Version) -> bool {
    cli_version.is_some_and(|cli_version| daemon_version > cli_version)
}

fn executable_version(path: &Path) -> Option<Version> {
    let mut child = command_from_path(path)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                let mut output = String::new();
                child
                    .stdout
                    .take()?
                    .take(4096)
                    .read_to_string(&mut output)
                    .ok()?;
                return output
                    .split_whitespace()
                    .find_map(|part| Version::parse(part.trim_start_matches('v')).ok());
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

pub fn available(name: &str) -> bool {
    resolve(name).is_some()
}

pub fn path(name: &str) -> Option<PathBuf> {
    resolve(name)
}

fn resolve(name: &str) -> Option<PathBuf> {
    explicit_path(name)
        .or_else(|| path_lookup(name))
        .or_else(|| shell_lookup(name))
        .or_else(|| common_user_path(name))
}

fn explicit_path(name: &str) -> Option<PathBuf> {
    let variable = format!("LUME_{}_PATH", name.to_ascii_uppercase());
    env::var_os(variable)
        .map(PathBuf::from)
        .filter(|path| path.is_file())
}

fn path_lookup(name: &str) -> Option<PathBuf> {
    env::var_os("PATH").and_then(|value| {
        env::split_paths(&value)
            .flat_map(|directory| executable_candidates(&directory, name))
            .find(|path| path.is_file())
    })
}

#[cfg(not(target_os = "windows"))]
fn shell_lookup(name: &str) -> Option<PathBuf> {
    let shell = env::var_os("SHELL").unwrap_or_else(|| "/bin/bash".into());
    let output = Command::new(shell)
        .args(["-ic", &format!("command -v -- {name}")])
        .output()
        .ok()?;
    output.status.success().then(|| {
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .find(|line| line.starts_with('/'))
            .map(PathBuf::from)
    })?
}

#[cfg(target_os = "windows")]
fn shell_lookup(name: &str) -> Option<PathBuf> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = Command::new("where.exe")
        .arg(name)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    output.status.success().then(|| {
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(PathBuf::from)
    })?
}

fn common_user_path(name: &str) -> Option<PathBuf> {
    let home = env::var_os(if cfg!(target_os = "windows") {
        "USERPROFILE"
    } else {
        "HOME"
    })
    .map(PathBuf::from)?;
    let mut directories = vec![home.join(".local/bin"), home.join(".cargo/bin")];
    #[cfg(not(target_os = "windows"))]
    directories.extend(unix_install_directories(&home));
    #[cfg(target_os = "windows")]
    if let Some(app_data) = env::var_os("APPDATA") {
        directories.push(PathBuf::from(app_data).join("npm"));
    }
    #[cfg(not(target_os = "windows"))]
    if let Ok(versions) = std::fs::read_dir(home.join(".nvm/versions/node")) {
        directories.extend(versions.flatten().map(|entry| entry.path().join("bin")));
    }
    directories
        .into_iter()
        .flat_map(|directory| executable_candidates(&directory, name))
        .find(|path| path.is_file())
}

/// Install locations a GUI app's PATH leaves out: Claude's local installer (reached through a
/// shell alias, which `command -v` does not print as a path) and Homebrew.
#[cfg(not(target_os = "windows"))]
fn unix_install_directories(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".claude/local"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ]
}

fn executable_candidates(directory: &Path, name: &str) -> Vec<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        return ["exe", "cmd", "bat", ""]
            .into_iter()
            .map(|extension| {
                if extension.is_empty() {
                    directory.join(name)
                } else {
                    directory.join(format!("{name}.{extension}"))
                }
            })
            .collect();
    }
    #[cfg(not(target_os = "windows"))]
    vec![directory.join(name)]
}

#[cfg(not(target_os = "windows"))]
fn platform_command(path: &Path) -> Command {
    Command::new(path)
}

#[cfg(target_os = "windows")]
fn platform_command(path: &Path) -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // A `.cmd` or `.bat` shim (npm installs Codex and Claude as one) is handed to `Command` as it is: the
    // standard library runs it through `cmd.exe` and quotes the path and every argument for it. Building
    // `cmd.exe /S /C <path> <args>` by hand breaks on a folder with a space (`C:\Users\João Pedro`):
    // `/S` strips the quotes around the path, and `cmd` then looks for `C:\Users\João`.
    let mut command = Command::new(path);
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

fn prepend_parent_to_path(command: &mut Command, executable: &Path) {
    let Some(parent) = executable.parent() else {
        return;
    };
    let mut paths = vec![parent.to_path_buf()];
    if let Some(current) = env::var_os("PATH") {
        paths.extend(env::split_paths(&current));
    }
    if let Ok(path) = env::join_paths(paths) {
        command.env("PATH", path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executable_candidates_include_the_plain_unix_name() {
        let candidates = executable_candidates(Path::new("/tmp/bin"), "codex");
        assert!(candidates.iter().any(|path| path.ends_with("codex")));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn gui_lookup_covers_claude_local_installs_and_homebrew() {
        let directories = unix_install_directories(Path::new("/Users/me"));
        assert!(directories.contains(&PathBuf::from("/Users/me/.claude/local")));
        assert!(directories.contains(&PathBuf::from("/opt/homebrew/bin")));
    }

    #[test]
    fn codex_app_server_prefers_only_a_newer_daemon_package() {
        let cli = Version::parse("0.163.0").unwrap();
        assert!(prefer_daemon(
            Some(&cli),
            &Version::parse("0.165.0").unwrap()
        ));
        assert!(!prefer_daemon(
            Some(&cli),
            &Version::parse("0.162.0").unwrap()
        ));
        assert!(!prefer_daemon(
            Some(&cli),
            &Version::parse("0.163.0-rc.1").unwrap()
        ));
        assert!(!prefer_daemon(None, &Version::parse("0.165.0").unwrap()));
    }

    #[test]
    fn codex_daemon_package_uses_the_installed_manifest_safely() {
        let mut random = [0u8; 8];
        getrandom::getrandom(&mut random).unwrap();
        let home = env::temp_dir().join(format!(
            "lume-codex-package-{:x}",
            u64::from_ne_bytes(random)
        ));
        let root = home.join("packages/app-server-daemon/current");
        std::fs::create_dir_all(root.join("bin")).unwrap();
        let executable = if cfg!(target_os = "windows") {
            "bin/codex.exe"
        } else {
            "bin/codex"
        };
        std::fs::write(root.join(executable), "fixture").unwrap();
        let os = if cfg!(target_os = "macos") {
            "darwin"
        } else {
            env::consts::OS
        };
        let mut manifest = serde_json::json!({
            "layoutVersion": 1,
            "version": "0.165.0",
            "target": format!("{}-unknown-{}", env::consts::ARCH, os),
            "variant": "codex",
            "entrypoint": executable,
        });
        let manifest_path = root.join("codex-package.json");
        std::fs::write(&manifest_path, manifest.to_string()).unwrap();
        let package = codex_daemon_package(&home).expect("valid daemon package");
        assert_eq!(package.version, Version::parse("0.165.0").unwrap());
        assert_eq!(package.entrypoint, root.join(executable));

        manifest["entrypoint"] = serde_json::json!("../outside");
        std::fs::write(&manifest_path, manifest.to_string()).unwrap();
        assert!(codex_daemon_package(&home).is_none());
        manifest["entrypoint"] = serde_json::json!(executable);
        manifest["target"] = serde_json::json!("other-unknown-platform");
        std::fs::write(&manifest_path, manifest.to_string()).unwrap();
        assert!(codex_daemon_package(&home).is_none());
        std::fs::remove_dir_all(home).unwrap();
    }

    /// The folder of the person who reported this had a space and an accent in its name.
    #[cfg(target_os = "windows")]
    #[test]
    fn a_cmd_shim_in_a_folder_with_a_space_and_an_accent_runs_with_its_arguments() {
        let directory = env::temp_dir().join(format!("Lume teste Jo\u{e3}o Pedro {}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let shim = directory.join("echoer.cmd");
        std::fs::write(&shim, "@echo off\r\necho got %*\r\n").unwrap();
        let output = platform_command(&shim).args(["app-server", "--listen", "ws://127.0.0.1:43999"]).output().expect("the shim starts");
        let _ = std::fs::remove_dir_all(&directory);
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.contains("got app-server --listen ws://127.0.0.1:43999"), "{text}");
    }
}
