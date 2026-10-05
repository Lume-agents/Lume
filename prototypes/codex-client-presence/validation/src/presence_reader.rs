//! Passive validation fixture: locks, not PIDs or timestamps, establish a live
//! metadata publisher. This reader never writes a record or controls a process.

use std::io;
use std::path::Path;

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PresenceRecord {
    pub version: u32,
    pub client_instance_id: String,
    /// Asserted by the publisher; never used as liveness or control authority.
    pub process_id: u32,
    pub revision: u64,
    #[serde(deserialize_with = "deserialize_selection")]
    pub selected_thread_id: Option<String>,
    pub client_kind: String,
}

fn deserialize_selection<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)
}

/// Returns bounded, lease-live clients, including clients with no selection.
/// Malformed instance records are omitted; unsafe root/opt-in metadata fails
/// closed. Windows is intentionally unsupported by this Unix-only prototype.
#[cfg(unix)]
pub fn read_active_clients(codex_home: &Path) -> io::Result<Vec<PresenceRecord>> {
    unix::read_active_clients(codex_home)
}

#[cfg(not(unix))]
pub fn read_active_clients(_codex_home: &Path) -> io::Result<Vec<PresenceRecord>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "passive client presence has not been implemented or validated on Windows",
    ))
}

#[cfg(unix)]
mod unix {
    use super::PresenceRecord;
    use std::ffi::CString;
    use std::fs::{self, File, Metadata, TryLockError};
    use std::io::{self, Read};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;
    use uuid::Uuid;

    const ROOT_NAME: &str = "client-presence-v1";
    const MAX_ENTRIES: usize = 256;
    const MAX_CLIENTS: usize = 128;
    const MAX_STATE_BYTES: u64 = 4096;
    const MARKER: &[u8] = b"version=1\n";

    fn invalid(reason: &'static str) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, reason)
    }

    fn open_directory(path: &Path) -> io::Result<File> {
        let name = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| invalid("directory path contains a null byte"))?;
        let descriptor = unsafe {
            libc::open(
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if descriptor < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { File::from_raw_fd(descriptor) })
    }

    fn open_at(directory: &File, name: &str, flags: libc::c_int) -> io::Result<File> {
        let name = CString::new(name).map_err(|_| invalid("filename contains a null byte"))?;
        let descriptor = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            )
        };
        if descriptor < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { File::from_raw_fd(descriptor) })
    }

    fn private_metadata(file: &File, directory: bool) -> io::Result<Metadata> {
        let metadata = file.metadata()?;
        let owned = metadata.uid() == unsafe { libc::geteuid() };
        let correct_type = if directory {
            metadata.is_dir()
        } else {
            metadata.is_file() && metadata.nlink() == 1
        };
        let expected_mode = if directory { 0o700 } else { 0o600 };
        if !owned || !correct_type || metadata.mode() & 0o7777 != expected_mode {
            return Err(invalid(
                "metadata must be private, owned, and of the expected type",
            ));
        }
        Ok(metadata)
    }

    fn trusted_home(file: &File) -> io::Result<Metadata> {
        let metadata = file.metadata()?;
        if !metadata.is_dir()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o022 != 0
        {
            return Err(invalid(
                "Codex home must be owned and not writable by other users",
            ));
        }
        Ok(metadata)
    }

    fn same_inode(left: &Metadata, right: &Metadata) -> bool {
        left.dev() == right.dev() && left.ino() == right.ino()
    }

    fn unchanged_file(left: &Metadata, right: &Metadata) -> bool {
        same_inode(left, right)
            && left.len() == right.len()
            && left.mtime() == right.mtime()
            && left.mtime_nsec() == right.mtime_nsec()
            && left.ctime() == right.ctime()
            && left.ctime_nsec() == right.ctime_nsec()
    }

    fn bounded_read(file: &mut File, maximum: u64) -> io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        file.take(maximum + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > maximum {
            return Err(invalid("metadata exceeds its byte limit"));
        }
        Ok(bytes)
    }

    fn canonical_uuid(value: &str) -> bool {
        value.len() == 36
            && Uuid::parse_str(value)
                .is_ok_and(|uuid| !uuid.is_nil() && uuid.hyphenated().to_string() == value)
    }

    fn lease_busy(lease: &File) -> io::Result<bool> {
        match lease.try_lock() {
            Err(TryLockError::WouldBlock) => Ok(true),
            Ok(()) => {
                lease.unlock()?;
                Ok(false)
            }
            Err(TryLockError::Error(error)) => Err(error),
        }
    }

    fn unchanged_path(
        directory: &File,
        name: &str,
        expected: &Metadata,
        is_directory: bool,
    ) -> io::Result<bool> {
        let flags = if is_directory {
            libc::O_RDONLY | libc::O_DIRECTORY
        } else {
            libc::O_RDONLY
        };
        let current = open_at(directory, name, flags)?;
        let current = private_metadata(&current, is_directory)?;
        Ok(if is_directory {
            same_inode(expected, &current)
        } else {
            unchanged_file(expected, &current)
        })
    }

    fn read_instance(root: &File, instance: &str) -> io::Result<Option<PresenceRecord>> {
        let directory = open_at(root, instance, libc::O_RDONLY | libc::O_DIRECTORY)?;
        let directory_metadata = private_metadata(&directory, true)?;
        let lease = open_at(&directory, "lease", libc::O_RDWR)?;
        let lease_metadata = private_metadata(&lease, false)?;
        if !lease_busy(&lease)? {
            return Ok(None);
        }
        let mut state = open_at(&directory, "state.json", libc::O_RDONLY)?;
        let state_metadata = private_metadata(&state, false)?;
        if state_metadata.len() > MAX_STATE_BYTES {
            return Err(invalid("metadata exceeds its byte limit"));
        }
        let bytes = bounded_read(&mut state, MAX_STATE_BYTES)?;
        let record: PresenceRecord = serde_json::from_slice(&bytes)
            .map_err(|_| invalid("invalid presence metadata schema"))?;
        if record.version != 1
            || record.client_kind != "tui"
            || record.client_instance_id != instance
            || !canonical_uuid(&record.client_instance_id)
            || record.process_id == 0
            || record.revision == 0
            || record
                .selected_thread_id
                .as_deref()
                .is_some_and(|thread| !canonical_uuid(thread))
        {
            return Err(invalid("invalid presence metadata values"));
        }
        // Recheck the same lease handle after the bounded read. Each path is
        // reopened relative to its retained parent handle, never through a
        // replacement/symlinked directory. Closing midway discards the sample.
        if !unchanged_file(&state_metadata, &private_metadata(&state, false)?)
            || !unchanged_path(&directory, "state.json", &state_metadata, false)?
            || !unchanged_file(&lease_metadata, &private_metadata(&lease, false)?)
            || !unchanged_path(&directory, "lease", &lease_metadata, false)?
            || !unchanged_path(root, instance, &directory_metadata, true)?
            || !lease_busy(&lease)?
        {
            return Ok(None);
        }
        Ok(Some(record))
    }

    pub(super) fn read_active_clients(codex_home: &Path) -> io::Result<Vec<PresenceRecord>> {
        let root_path = codex_home.join(ROOT_NAME);
        let home = match open_directory(codex_home) {
            Ok(home) => home,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let home_metadata = trusted_home(&home)?;
        let root = match open_at(&home, ROOT_NAME, libc::O_RDONLY | libc::O_DIRECTORY) {
            Ok(root) => root,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let root_metadata = private_metadata(&root, true)?;
        let mut marker = match open_at(&root, "ENABLED", libc::O_RDONLY) {
            Ok(marker) => marker,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let marker_metadata = private_metadata(&marker, false)?;
        if bounded_read(&mut marker, MARKER.len() as u64)? != MARKER {
            return Err(invalid("unsupported presence opt-in marker"));
        }
        let mut entries = fs::read_dir(&root_path)?
            .take(MAX_ENTRIES + 1)
            .collect::<io::Result<Vec<_>>>()?;
        if entries.len() > MAX_ENTRIES {
            return Err(invalid("presence directory exceeds its entry limit"));
        }
        entries.sort_by_key(std::fs::DirEntry::file_name);
        let mut clients = Vec::new();
        for entry in entries {
            let Ok(instance) = entry.file_name().into_string() else {
                continue;
            };
            if !canonical_uuid(&instance) {
                continue;
            }
            if let Ok(Some(record)) = read_instance(&root, &instance) {
                clients.push(record);
            }
            if clients.len() > MAX_CLIENTS {
                return Err(invalid("presence directory exceeds its client limit"));
            }
        }
        let home_now = open_directory(codex_home)?;
        let root_now = open_at(&home, ROOT_NAME, libc::O_RDONLY | libc::O_DIRECTORY)?;
        if !same_inode(&home_metadata, &trusted_home(&home_now)?)
            || !same_inode(&root_metadata, &private_metadata(&root_now, true)?)
            || !unchanged_file(&marker_metadata, &private_metadata(&marker, false)?)
            || !unchanged_path(&root, "ENABLED", &marker_metadata, false)?
        {
            return Err(invalid("presence root or opt-in changed during sampling"));
        }
        Ok(clients)
    }
}
