use super::ClientPresence;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::fs;
use std::fs::File;
use std::fs::Permissions;
use std::fs::TryLockError;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::path::PathBuf;
use tempfile::TempDir;
use uuid::Uuid;

fn opted_in() -> TempDir {
    let home = tempfile::tempdir().unwrap();
    fs::set_permissions(home.path(), Permissions::from_mode(0o700)).unwrap();
    let root = home.path().join("client-presence-v1");
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, Permissions::from_mode(0o700)).unwrap();
    fs::write(root.join("ENABLED"), b"version=1\n").unwrap();
    fs::set_permissions(root.join("ENABLED"), Permissions::from_mode(0o600)).unwrap();
    home
}

fn records(home: &Path) -> Vec<PathBuf> {
    let mut entries = fs::read_dir(home.join("client-presence-v1"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

fn state(directory: &Path) -> Value {
    serde_json::from_slice(&fs::read(directory.join("state.json")).unwrap()).unwrap()
}

#[test]
fn absent_marker_creates_nothing() {
    let home = tempfile::tempdir().unwrap();
    fs::set_permissions(home.path(), Permissions::from_mode(0o700)).unwrap();
    assert!(ClientPresence::start(home.path()).unwrap().is_none());
    assert_eq!(fs::read_dir(home.path()).unwrap().count(), 0);
    fs::create_dir(home.path().join("client-presence-v1")).unwrap();
    fs::set_permissions(
        home.path().join("client-presence-v1"),
        Permissions::from_mode(0o700),
    )
    .unwrap();
    fs::write(home.path().join("client-presence-v1/sentinel"), b"keep").unwrap();
    assert!(ClientPresence::start(home.path()).unwrap().is_none());
    assert_eq!(
        fs::read(home.path().join("client-presence-v1/sentinel")).unwrap(),
        b"keep"
    );
}

#[test]
fn same_process_and_thread_have_distinct_leased_instances() {
    let home = opted_in();
    let mut first = ClientPresence::start(home.path()).unwrap().unwrap();
    let mut second = ClientPresence::start(home.path()).unwrap().unwrap();
    let thread = Uuid::new_v4().to_string();
    first.select_thread(&thread).unwrap();
    second.select_thread(&thread).unwrap();
    let records = records(home.path());
    assert_eq!(records.len(), 2);
    let first_state = state(&records[0]);
    let second_state = state(&records[1]);
    assert_ne!(
        first_state["clientInstanceId"],
        second_state["clientInstanceId"]
    );
    assert_eq!(first_state["processId"], second_state["processId"]);
    assert_eq!(
        first_state["selectedThreadId"],
        second_state["selectedThreadId"]
    );
    for directory in records {
        assert!(matches!(
            File::open(directory.join("lease")).unwrap().try_lock(),
            Err(TryLockError::WouldBlock)
        ));
        assert_eq!(fs::metadata(&directory).unwrap().mode() & 0o7777, 0o700);
        for name in ["lease", "state.json"] {
            assert_eq!(
                fs::metadata(directory.join(name)).unwrap().mode() & 0o7777,
                0o600
            );
        }
    }
}

#[test]
fn snapshot_is_metadata_only_and_selection_changes_are_monotonic() {
    let home = opted_in();
    let mut presence = ClientPresence::start(home.path()).unwrap().unwrap();
    let directory = records(home.path()).pop().unwrap();
    let instance = directory.file_name().unwrap().to_str().unwrap();
    let expected = json!({"version":1,"clientInstanceId":instance,"processId":std::process::id(),"revision":1,"selectedThreadId":null,"clientKind":"tui"});
    assert_eq!(state(&directory), expected);
    let first = Uuid::new_v4().to_string();
    let second = Uuid::new_v4().to_string();
    presence.select_thread(&first).unwrap();
    let inode = fs::metadata(directory.join("state.json")).unwrap().ino();
    presence.select_thread(&first).unwrap();
    assert_eq!(
        fs::metadata(directory.join("state.json")).unwrap().ino(),
        inode
    );
    presence.select_thread(&second).unwrap();
    let mut expected = expected;
    expected["revision"] = json!(3);
    expected["selectedThreadId"] = json!(second);
    assert_eq!(state(&directory), expected);
    presence.clear_selection().unwrap();
    expected["revision"] = json!(4);
    expected["selectedThreadId"] = Value::Null;
    assert_eq!(state(&directory), expected);
    let inode = fs::metadata(directory.join("state.json")).unwrap().ino();
    presence.clear_selection().unwrap();
    assert_eq!(
        fs::metadata(directory.join("state.json")).unwrap().ino(),
        inode
    );
}

#[test]
fn drop_removes_only_its_record_and_releases_lease() {
    let home = opted_in();
    let first = ClientPresence::start(home.path()).unwrap().unwrap();
    let directory = records(home.path()).pop().unwrap();
    let lease = File::open(directory.join("lease")).unwrap();
    let second = ClientPresence::start(home.path()).unwrap().unwrap();
    fs::write(home.path().join("client-presence-v1/sentinel"), b"keep").unwrap();
    drop(first);
    assert!(!directory.exists());
    assert!(lease.try_lock().is_ok());
    assert_eq!(records(home.path()).len(), 1);
    assert_eq!(
        fs::read(home.path().join("client-presence-v1/sentinel")).unwrap(),
        b"keep"
    );
    drop(second);
}

#[test]
fn invalid_uuid_revokes_the_lease_without_publishing_input() {
    let home = opted_in();
    let mut presence = ClientPresence::start(home.path()).unwrap().unwrap();
    let directory = records(home.path()).pop().unwrap();
    let previous = state(&directory);
    assert_eq!(
        presence
            .select_thread("private prompt, not an ID")
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
    assert_eq!(state(&directory), previous);
    assert!(
        File::open(directory.join("lease"))
            .unwrap()
            .try_lock()
            .is_ok()
    );
    assert!(presence.select_thread(&Uuid::new_v4().to_string()).is_err());
}

#[test]
fn bad_permissions_symlinks_and_malformed_markers_fail_closed() {
    for mode in [0o644, 0o660, 0o1600] {
        let home = opted_in();
        fs::set_permissions(
            home.path().join("client-presence-v1/ENABLED"),
            Permissions::from_mode(mode),
        )
        .unwrap();
        assert!(ClientPresence::start(home.path()).is_err());
        assert!(records(home.path()).is_empty());
    }
    let home = opted_in();
    fs::set_permissions(
        home.path().join("client-presence-v1"),
        Permissions::from_mode(0o755),
    )
    .unwrap();
    assert!(ClientPresence::start(home.path()).is_err());
    let home = opted_in();
    let marker = home.path().join("client-presence-v1/ENABLED");
    fs::remove_file(&marker).unwrap();
    symlink(home.path().join("secret"), &marker).unwrap();
    assert!(ClientPresence::start(home.path()).is_err());
    let home = opted_in();
    fs::write(
        home.path().join("client-presence-v1/ENABLED"),
        vec![b'x'; 8192],
    )
    .unwrap();
    assert!(ClientPresence::start(home.path()).is_err());
}

#[test]
fn enabled_unsafe_home_is_rejected_but_opted_out_home_stays_disabled() {
    let home = opted_in();
    fs::set_permissions(home.path(), Permissions::from_mode(0o775)).unwrap();
    assert_eq!(
        ClientPresence::start(home.path()).err().unwrap().kind(),
        io::ErrorKind::PermissionDenied,
    );
    assert!(records(home.path()).is_empty());
    fs::remove_file(home.path().join("client-presence-v1/ENABLED")).unwrap();
    assert!(ClientPresence::start(home.path()).unwrap().is_none());
    assert!(records(home.path()).is_empty());
}

#[test]
fn opt_out_and_write_failure_leave_no_live_stale_selection() {
    let home = opted_in();
    let mut presence = ClientPresence::start(home.path()).unwrap().unwrap();
    let directory = records(home.path()).pop().unwrap();
    presence.select_thread(&Uuid::new_v4().to_string()).unwrap();
    fs::remove_file(home.path().join("client-presence-v1/ENABLED")).unwrap();
    assert!(presence.clear_selection().is_err());
    assert!(
        File::open(directory.join("lease"))
            .unwrap()
            .try_lock()
            .is_ok()
    );
    let home = opted_in();
    let mut presence = ClientPresence::start(home.path()).unwrap().unwrap();
    let directory = records(home.path()).pop().unwrap();
    fs::remove_file(directory.join("state.json")).unwrap();
    fs::create_dir(directory.join("state.json")).unwrap();
    assert!(presence.select_thread(&Uuid::new_v4().to_string()).is_err());
    assert!(
        File::open(directory.join("lease"))
            .unwrap()
            .try_lock()
            .is_ok()
    );
}

#[test]
fn swapped_instance_directory_is_not_written_or_removed() {
    let home = opted_in();
    let mut presence = ClientPresence::start(home.path()).unwrap().unwrap();
    let original = records(home.path()).pop().unwrap();
    let moved = home.path().join("client-presence-v1/moved");
    fs::rename(&original, &moved).unwrap();
    fs::create_dir(&original).unwrap();
    fs::set_permissions(&original, Permissions::from_mode(0o700)).unwrap();
    fs::write(original.join("sentinel"), b"keep").unwrap();
    assert!(presence.select_thread(&Uuid::new_v4().to_string()).is_err());
    drop(presence);
    assert_eq!(fs::read(original.join("sentinel")).unwrap(), b"keep");
    assert!(File::open(moved.join("lease")).unwrap().try_lock().is_ok());
}

#[test]
fn root_symlink_and_fifo_marker_are_rejected_without_reading_content() {
    let home = opted_in();
    let root = home.path().join("client-presence-v1");
    let moved = home.path().join("moved");
    fs::rename(&root, &moved).unwrap();
    symlink(&moved, &root).unwrap();
    assert!(ClientPresence::start(home.path()).is_err());
    let home = opted_in();
    let marker = home.path().join("client-presence-v1/ENABLED");
    fs::remove_file(&marker).unwrap();
    let marker = std::ffi::CString::new(marker.as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: this exact path belongs to the disposable test directory.
    assert_eq!(unsafe { libc::mkfifo(marker.as_ptr(), 0o600) }, 0);
    assert!(ClientPresence::start(home.path()).is_err());
}
