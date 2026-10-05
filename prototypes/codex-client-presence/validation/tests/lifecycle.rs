//! Owned metadata-only fixtures. No real Codex process, daemon, prompt, or
//! installation is started or inspected. Windows is not runtime-validated.
#![cfg(unix)]

use crate::client_presence::ClientPresence;
use crate::presence_reader::{PresenceRecord, read_active_clients};
use pretty_assertions::assert_eq;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tempfile::TempDir;

const THREAD_A: &str = "aaaaaaaa-1234-5678-9abc-123456789abc";
const THREAD_B: &str = "bbbbbbbb-1234-5678-9abc-123456789abc";
const THREAD_C: &str = "cccccccc-1234-5678-9abc-123456789abc";
const CHILD_HOME: &str = "LUME_PRESENCE_VALIDATION_FIXTURE_HOME";
const CHILD_THREAD: &str = "LUME_PRESENCE_VALIDATION_FIXTURE_THREAD";
const CHILD_LEASE_FD: &str = "LUME_PRESENCE_VALIDATION_FIXTURE_LEASE_FD";
const CHILD_LEASE_PATH: &str = "LUME_PRESENCE_VALIDATION_FIXTURE_LEASE_PATH";

fn enabled_home() -> TempDir {
    let home = TempDir::new().unwrap();
    fs::set_permissions(home.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let root = home.path().join("client-presence-v1");
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let marker = root.join("ENABLED");
    fs::write(&marker, b"version=1\n").unwrap();
    fs::set_permissions(&marker, fs::Permissions::from_mode(0o600)).unwrap();
    home
}

fn instance_path(home: &Path, record: &PresenceRecord) -> PathBuf {
    home.join("client-presence-v1")
        .join(&record.client_instance_id)
}

#[test]
fn opt_in_absent_creates_nothing() {
    let home = TempDir::new().unwrap();
    fs::set_permissions(home.path(), fs::Permissions::from_mode(0o700)).unwrap();
    assert!(ClientPresence::start(home.path()).unwrap().is_none());
    assert_eq!(read_active_clients(home.path()).unwrap(), Vec::new());
    assert!(!home.path().join("client-presence-v1").exists());
}

#[test]
fn an_unsafe_fake_home_is_rejected_without_publishing_presence() {
    let home = enabled_home();
    fs::set_permissions(home.path(), fs::Permissions::from_mode(0o777)).unwrap();
    assert!(ClientPresence::start(home.path()).is_err());
    assert!(read_active_clients(home.path()).is_err());
    assert_eq!(
        fs::read_dir(home.path().join("client-presence-v1"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>(),
        vec![std::ffi::OsString::from("ENABLED")]
    );
}

#[test]
fn two_clients_before_prompt_switch_clear_and_close_independently() {
    let home = enabled_home();
    let mut first = ClientPresence::start(home.path()).unwrap().unwrap();
    let mut second = ClientPresence::start(home.path()).unwrap().unwrap();
    let initial = read_active_clients(home.path()).unwrap();
    assert_eq!(initial.len(), 2);
    assert!(
        initial
            .iter()
            .all(|client| client.selected_thread_id.is_none())
    );
    // The identical asserted PID deliberately does not collapse two clients.
    assert!(
        initial
            .iter()
            .all(|client| client.process_id == std::process::id())
    );
    assert_ne!(initial[0].client_instance_id, initial[1].client_instance_id);

    first.select_thread(THREAD_A).unwrap();
    second.select_thread(THREAD_B).unwrap();
    let selected = read_active_clients(home.path()).unwrap();
    let first_snapshot = selected
        .iter()
        .find(|client| client.selected_thread_id.as_deref() == Some(THREAD_A))
        .unwrap()
        .clone();
    let second_snapshot = selected
        .iter()
        .find(|client| client.selected_thread_id.as_deref() == Some(THREAD_B))
        .unwrap()
        .clone();
    first.select_thread(THREAD_C).unwrap();
    let switched = read_active_clients(home.path()).unwrap();
    assert!(switched.contains(&second_snapshot));
    let changed = switched
        .iter()
        .find(|client| client.client_instance_id == first_snapshot.client_instance_id)
        .unwrap();
    assert_eq!(
        changed,
        &PresenceRecord {
            revision: first_snapshot.revision + 1,
            selected_thread_id: Some(THREAD_C.into()),
            ..first_snapshot.clone()
        }
    );
    first.clear_selection().unwrap();
    let cleared = read_active_clients(home.path()).unwrap();
    assert!(cleared.contains(&second_snapshot));
    assert!(cleared.iter().any(|client| {
        client.client_instance_id == first_snapshot.client_instance_id
            && client.selected_thread_id.is_none()
    }));
    drop(first);
    assert_eq!(
        read_active_clients(home.path()).unwrap(),
        vec![second_snapshot]
    );
    assert!(!instance_path(home.path(), &first_snapshot).exists());
    drop(second);
    assert_eq!(read_active_clients(home.path()).unwrap(), Vec::new());
}

#[test]
fn two_clients_can_select_the_same_conversation_without_becoming_one_client() {
    let home = enabled_home();
    let mut first = ClientPresence::start(home.path()).unwrap().unwrap();
    let mut second = ClientPresence::start(home.path()).unwrap().unwrap();
    first.select_thread(THREAD_A).unwrap();
    second.select_thread(THREAD_A).unwrap();
    let records = read_active_clients(home.path()).unwrap();
    assert_eq!(records.len(), 2);
    assert_ne!(records[0].client_instance_id, records[1].client_instance_id);
    assert!(
        records
            .iter()
            .all(|record| { record.selected_thread_id.as_deref() == Some(THREAD_A) })
    );
}

struct OwnedChild(Child);

impl OwnedChild {
    fn spawn(mut command: Command) -> Self {
        let mut child = Self(command.spawn().unwrap());
        let output = child.0.stdout.take().unwrap();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            // Drain the owned fixture's stdout until exit, including the test
            // runner's summary; do not close its pipe immediately after ready.
            for line in BufReader::new(output).lines().take(32) {
                let Ok(line) = line else { break };
                if line.trim().ends_with("presence-fixture-ready") {
                    let _ = ready_tx.send(());
                }
            }
        });
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("owned fixture did not reach its ready state");
        child
    }

    fn finish(&mut self, command: &str) {
        writeln!(self.0.stdin.as_mut().unwrap(), "{command}").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                assert!(status.success());
                return;
            }
            assert!(Instant::now() < deadline, "owned fixture did not exit");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn fixture_command(home: &Path) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "lifecycle::abrupt_presence_child_fixture",
            "--exact",
            "--nocapture",
        ])
        .env(CHILD_HOME, home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    command
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        // This handle belongs only to the disposable test executable we spawn.
        // It is never reconstructed from an asserted presence-record PID.
        if self.0.try_wait().is_ok_and(|status| status.is_none()) {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

#[test]
fn abrupt_exit_leaves_metadata_but_releases_the_lease() {
    let home = enabled_home();
    let mut child = OwnedChild::spawn(fixture_command(home.path()));
    let alive = read_active_clients(home.path()).unwrap();
    assert_eq!(alive.len(), 1);
    assert_eq!(alive[0].selected_thread_id.as_deref(), Some(THREAD_A));
    let directory = instance_path(home.path(), &alive[0]);
    child.finish("exit");
    assert!(directory.join("lease").is_file());
    assert!(directory.join("state.json").is_file());
    assert_eq!(read_active_clients(home.path()).unwrap(), Vec::new());
}

#[test]
fn two_owned_processes_resolve_before_prompt_and_close_independently() {
    let home = enabled_home();
    let mut first_command = fixture_command(home.path());
    first_command.env(CHILD_THREAD, THREAD_A);
    let mut first = OwnedChild::spawn(first_command);
    let mut second_command = fixture_command(home.path());
    second_command.env(CHILD_THREAD, THREAD_B);
    let mut second = OwnedChild::spawn(second_command);
    assert_ne!(first.0.id(), second.0.id());
    let records = read_active_clients(home.path()).unwrap();
    assert_eq!(records.len(), 2);
    let first_record = records
        .iter()
        .find(|record| record.selected_thread_id.as_deref() == Some(THREAD_A))
        .unwrap();
    let second_record = records
        .iter()
        .find(|record| record.selected_thread_id.as_deref() == Some(THREAD_B))
        .unwrap()
        .clone();
    assert_ne!(
        first_record.client_instance_id,
        second_record.client_instance_id
    );
    assert_eq!(first_record.process_id, first.0.id());
    assert_eq!(second_record.process_id, second.0.id());
    first.finish("drop");
    assert_eq!(
        read_active_clients(home.path()).unwrap(),
        vec![second_record]
    );
    assert!(second.0.try_wait().unwrap().is_none());
    assert!(!instance_path(home.path(), first_record).exists());
    second.finish("drop");
    assert_eq!(read_active_clients(home.path()).unwrap(), Vec::new());
}

#[test]
#[cfg(target_os = "linux")]
fn owned_exec_child_does_not_inherit_or_keep_the_parent_lease() {
    let home = enabled_home();
    let mut presence = ClientPresence::start(home.path()).unwrap().unwrap();
    presence.select_thread(THREAD_A).unwrap();
    let record = read_active_clients(home.path()).unwrap().remove(0);
    let lease_path = instance_path(home.path(), &record).join("lease");
    let lease_metadata = fs::metadata(&lease_path).unwrap();
    // Inspect only this test process's descriptors for its own fixture inode.
    let lease_fd = fs::read_dir("/proc/self/fd")
        .unwrap()
        .take(256)
        .filter_map(Result::ok)
        .find_map(|entry| {
            let metadata = fs::metadata(entry.path()).ok()?;
            if metadata.dev() == lease_metadata.dev() && metadata.ino() == lease_metadata.ino() {
                entry.file_name().to_str()?.parse::<i32>().ok()
            } else {
                None
            }
        })
        .expect("parent's own lease descriptor missing");
    // SAFETY: F_GETFD only reads flags on the test-owned descriptor.
    let descriptor_flags = unsafe { libc::fcntl(lease_fd, libc::F_GETFD) };
    assert!(descriptor_flags >= 0, "F_GETFD failed for owned lease");
    assert_ne!(descriptor_flags & libc::FD_CLOEXEC, 0);
    let mut command = fixture_command(home.path());
    command
        .env(CHILD_LEASE_FD, lease_fd.to_string())
        .env(CHILD_LEASE_PATH, &lease_path);
    let mut child = OwnedChild::spawn(command);
    drop(presence);
    assert!(child.0.try_wait().unwrap().is_none());
    assert_eq!(read_active_clients(home.path()).unwrap(), Vec::new());
    assert!(!instance_path(home.path(), &record).exists());
    child.finish("drop");
}

#[test]
fn abrupt_presence_child_fixture() {
    let Some(home) = std::env::var_os(CHILD_HOME) else {
        return;
    };
    let mut presence = if let Some(descriptor) = std::env::var_os(CHILD_LEASE_FD) {
        let descriptor = descriptor.to_str().unwrap().parse::<i32>().unwrap();
        let lease_path = std::env::var_os(CHILD_LEASE_PATH).unwrap();
        let expected = fs::metadata(Path::new(&lease_path)).unwrap();
        let inherited = fs::metadata(format!("/proc/self/fd/{descriptor}")).is_ok_and(|metadata| {
            metadata.dev() == expected.dev() && metadata.ino() == expected.ino()
        });
        assert!(!inherited, "owned exec child inherited its parent's lease");
        None
    } else {
        let mut presence = ClientPresence::start(Path::new(&home)).unwrap().unwrap();
        let thread = std::env::var(CHILD_THREAD).unwrap_or_else(|_| THREAD_A.into());
        presence.select_thread(&thread).unwrap();
        Some(presence)
    };
    println!("presence-fixture-ready");
    std::io::stdout().flush().unwrap();
    let mut command = String::new();
    std::io::stdin().read_line(&mut command).unwrap();
    match command.trim() {
        "exit" => std::process::exit(0), // Skips producer Drop, like a crash.
        "drop" => drop(presence.take()),
        _ => panic!("unexpected owned fixture command"),
    }
}

#[test]
fn marker_permissions_and_symlinks_fail_closed() {
    let home = enabled_home();
    let _presence = ClientPresence::start(home.path()).unwrap().unwrap();
    let root = home.path().join("client-presence-v1");
    let marker = root.join("ENABLED");
    fs::set_permissions(&marker, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(read_active_clients(home.path()).is_err());
    fs::set_permissions(&marker, fs::Permissions::from_mode(0o600)).unwrap();
    let preserved = root.join("preserved-marker");
    fs::rename(&marker, &preserved).unwrap();
    symlink(&preserved, &marker).unwrap();
    assert!(read_active_clients(home.path()).is_err());
}

#[test]
fn stale_replaced_lease_and_symlinked_state_are_rejected() {
    let home = enabled_home();
    let _presence = ClientPresence::start(home.path()).unwrap().unwrap();
    let record = read_active_clients(home.path()).unwrap().remove(0);
    let directory = instance_path(home.path(), &record);
    let state = directory.join("state.json");
    let preserved = directory.join("preserved-state");
    fs::rename(&state, &preserved).unwrap();
    symlink(&preserved, &state).unwrap();
    assert_eq!(read_active_clients(home.path()).unwrap(), Vec::new());
    fs::remove_file(&state).unwrap();
    fs::rename(&preserved, &state).unwrap();
    let lease = directory.join("lease");
    let previous_inode = fs::metadata(&lease).unwrap().ino();
    fs::rename(&lease, directory.join("preserved-lease")).unwrap();
    fs::write(&lease, []).unwrap();
    fs::set_permissions(&lease, fs::Permissions::from_mode(0o600)).unwrap();
    assert_ne!(fs::metadata(&lease).unwrap().ino(), previous_inode);
    assert_eq!(read_active_clients(home.path()).unwrap(), Vec::new());
}

#[test]
fn fifo_state_and_lease_are_rejected_without_waiting_for_a_writer() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    for filename in ["state.json", "lease"] {
        let home = enabled_home();
        let _presence = ClientPresence::start(home.path()).unwrap().unwrap();
        let record = read_active_clients(home.path()).unwrap().remove(0);
        let directory = instance_path(home.path(), &record);
        let path = directory.join(filename);
        fs::rename(&path, directory.join("preserved-file")).unwrap();
        let path = CString::new(path.as_os_str().as_bytes()).unwrap();
        // SAFETY: a new FIFO is created only at this owned fixture's exact path.
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        assert_eq!(read_active_clients(home.path()).unwrap(), Vec::new());
    }
}

#[test]
fn malformed_oversized_or_unknown_metadata_does_not_create_presence() {
    let home = enabled_home();
    let _presence = ClientPresence::start(home.path()).unwrap().unwrap();
    let record = read_active_clients(home.path()).unwrap().remove(0);
    let state = instance_path(home.path(), &record).join("state.json");
    let original = fs::read(&state).unwrap();
    let mut unknown: serde_json::Value = serde_json::from_slice(&original).unwrap();
    unknown["unexpected"] = serde_json::json!(true);
    for invalid in [
        b"not-json".to_vec(),
        vec![b' '; 4097],
        serde_json::to_vec(&unknown).unwrap(),
    ] {
        fs::write(&state, invalid).unwrap();
        assert_eq!(read_active_clients(home.path()).unwrap(), Vec::new());
    }
    let mut invalid: serde_json::Value = serde_json::from_slice(&original).unwrap();
    invalid["clientInstanceId"] = serde_json::json!(uuid::Uuid::new_v4().to_string());
    fs::write(&state, serde_json::to_vec(&invalid).unwrap()).unwrap();
    assert_eq!(read_active_clients(home.path()).unwrap(), Vec::new());
    let mut missing: serde_json::Value = serde_json::from_slice(&original).unwrap();
    missing.as_object_mut().unwrap().remove("selectedThreadId");
    fs::write(&state, serde_json::to_vec(&missing).unwrap()).unwrap();
    assert_eq!(read_active_clients(home.path()).unwrap(), Vec::new());
}
