//! Opt-in, local display metadata. A claimed PID or live lease grants no process authority.
//! Consumers must verify an independently opened, busy lease before and after reading state.
//! CLOEXEC excludes executed children; a fork-only child can retain a crashed parent's lease.

use std::io;
use std::path::Path;

#[cfg(unix)]
mod unix {
    use serde::Serialize;
    use std::ffi::CString;
    use std::fs::File;
    use std::fs::Metadata;
    use std::fs::OpenOptions;
    use std::io::Read;
    use std::io::Write;
    use std::os::fd::AsRawFd;
    use std::os::fd::FromRawFd;
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::OpenOptionsExt;
    use std::path::Path;
    use std::path::PathBuf;
    use uuid::Uuid;

    const ROOT: &str = "client-presence-v1";
    const MARKER: &str = "ENABLED";
    const MARKER_CONTENT: &[u8] = b"version=1\n";
    const MAX_STATE_BYTES: usize = 4096;

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Snapshot {
        version: u8,
        client_instance_id: String,
        process_id: u32,
        revision: u64,
        selected_thread_id: Option<String>,
        client_kind: &'static str,
    }

    pub(crate) struct Publisher {
        home_path: PathBuf,
        home: File,
        root: File,
        directory: File,
        lease: Option<File>,
        snapshot: Snapshot,
    }

    fn open_at(parent: &File, name: &str, flags: i32) -> std::io::Result<File> {
        let name = CString::new(name)?;
        // SAFETY: the directory handle and NUL-terminated name live through openat.
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
                0o600,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: openat returned a new, uniquely owned descriptor.
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    fn directory(path: &Path) -> std::io::Result<File> {
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
    }

    fn private_metadata(file: &File) -> std::io::Result<Metadata> {
        let metadata = file.metadata()?;
        // SAFETY: geteuid has no arguments or memory preconditions.
        let uid = unsafe { libc::geteuid() };
        let expected_mode = if metadata.is_dir() { 0o700 } else { 0o600 };
        if metadata.uid() != uid
            || metadata.mode() & 0o7777 != expected_mode
            || (!metadata.is_dir() && (!metadata.is_file() || metadata.nlink() != 1))
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "presence metadata must be owner-only and non-linked",
            ));
        }
        Ok(metadata)
    }

    fn same_file(expected: &File, actual: &File) -> std::io::Result<()> {
        let expected = private_metadata(expected)?;
        let actual = private_metadata(actual)?;
        if (expected.dev(), expected.ino()) != (actual.dev(), actual.ino()) {
            return Err(std::io::Error::other(
                "presence directory or lease replaced",
            ));
        }
        Ok(())
    }

    fn trusted_home(file: &File) -> std::io::Result<Metadata> {
        let metadata = file.metadata()?;
        // SAFETY: geteuid has no arguments or memory preconditions.
        if !metadata.is_dir()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o022 != 0
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "unsafe Codex home",
            ));
        }
        Ok(metadata)
    }

    fn enabled(root: &File) -> std::io::Result<bool> {
        let marker = match open_at(root, MARKER, libc::O_RDONLY) {
            Ok(marker) => marker,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        private_metadata(&marker)?;
        let mut bytes = Vec::new();
        marker.take(64).read_to_end(&mut bytes)?;
        if bytes != MARKER_CONTENT {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "unsupported presence opt-in marker",
            ));
        }
        Ok(true)
    }

    fn unlink(parent: &File, name: &str, flags: i32) {
        if let Ok(name) = CString::new(name) {
            // SAFETY: exact relative filename, with a live directory descriptor.
            unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), flags) };
        }
    }

    impl Publisher {
        pub(crate) fn start(home_path: &Path) -> std::io::Result<Option<Self>> {
            let home = match directory(home_path) {
                Ok(home) => home,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error),
            };
            let root = match open_at(&home, ROOT, libc::O_RDONLY | libc::O_DIRECTORY) {
                Ok(root) => root,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error),
            };
            private_metadata(&root)?;
            if !enabled(&root)? {
                return Ok(None);
            }
            trusted_home(&home)?;
            let client_instance_id = Uuid::new_v4().to_string();
            let name = CString::new(client_instance_id.as_str())?;
            // SAFETY: mkdirat creates only this UUID entry in the validated root.
            if unsafe { libc::mkdirat(root.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
                return Err(std::io::Error::last_os_error());
            }
            let directory = match open_at(
                &root,
                &client_instance_id,
                libc::O_RDONLY | libc::O_DIRECTORY,
            ) {
                Ok(directory) => directory,
                Err(error) => {
                    unlink(&root, &client_instance_id, libc::AT_REMOVEDIR);
                    return Err(error);
                }
            };
            let mut publisher = Self {
                home_path: home_path.to_path_buf(),
                home,
                root,
                directory,
                lease: None,
                snapshot: Snapshot {
                    version: 1,
                    client_instance_id,
                    process_id: std::process::id(),
                    revision: 1,
                    selected_thread_id: None,
                    client_kind: "tui",
                },
            };
            private_metadata(&publisher.directory)?;
            let lease = open_at(
                &publisher.directory,
                "lease",
                libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
            )?;
            private_metadata(&lease)?;
            lease.try_lock().map_err(std::io::Error::from)?;
            publisher.lease = Some(lease);
            publisher.publish()?;
            Ok(Some(publisher))
        }

        fn validate(&self) -> std::io::Result<()> {
            let expected_home = trusted_home(&self.home)?;
            let actual_home = trusted_home(&directory(&self.home_path)?)?;
            if (expected_home.dev(), expected_home.ino()) != (actual_home.dev(), actual_home.ino())
            {
                return Err(std::io::Error::other("Codex home replaced"));
            }
            same_file(
                &self.root,
                &open_at(&self.home, ROOT, libc::O_RDONLY | libc::O_DIRECTORY)?,
            )?;
            same_file(
                &self.directory,
                &open_at(
                    &self.root,
                    &self.snapshot.client_instance_id,
                    libc::O_RDONLY | libc::O_DIRECTORY,
                )?,
            )?;
            let lease = self
                .lease
                .as_ref()
                .ok_or_else(|| std::io::Error::other("presence stopped"))?;
            same_file(lease, &open_at(&self.directory, "lease", libc::O_RDONLY)?)?;
            if !enabled(&self.root)? {
                return Err(std::io::Error::other("presence opt-in removed"));
            }
            Ok(())
        }

        fn publish(&self) -> std::io::Result<()> {
            self.validate()?;
            let bytes = serde_json::to_vec(&self.snapshot)?;
            if bytes.len() > MAX_STATE_BYTES {
                return Err(std::io::Error::other("presence snapshot too large"));
            }
            let temporary = format!(".state-{}.tmp", Uuid::new_v4());
            let result = (|| {
                let mut file = open_at(
                    &self.directory,
                    &temporary,
                    libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
                )?;
                private_metadata(&file)?;
                file.write_all(&bytes)?;
                self.validate()?;
                let source = CString::new(temporary.as_str())?;
                let destination = CString::new("state.json")?;
                // SAFETY: rename uses two exact filenames within the same pinned directory.
                if unsafe {
                    libc::renameat(
                        self.directory.as_raw_fd(),
                        source.as_ptr(),
                        self.directory.as_raw_fd(),
                        destination.as_ptr(),
                    )
                } != 0
                {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            })();
            if result.is_err() {
                unlink(&self.directory, &temporary, /*flags*/ 0);
            }
            result
        }

        pub(crate) fn select(&mut self, thread: Option<&str>) -> std::io::Result<()> {
            let result = (|| {
                self.validate()?;
                if let Some(thread) = thread
                    && Uuid::parse_str(thread)
                        .ok()
                        .map(|id| id.to_string())
                        .as_deref()
                        != Some(thread)
                {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "invalid selected thread UUID",
                    ));
                }
                if self.snapshot.selected_thread_id.as_deref() == thread {
                    return Ok(());
                }
                self.snapshot.revision = self
                    .snapshot
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| std::io::Error::other("presence revision exhausted"))?;
                self.snapshot.selected_thread_id = thread.map(str::to_owned);
                self.publish()
            })();
            if result.is_err()
                && let Some(lease) = self.lease.take()
            {
                let _ = File::unlock(&lease);
            }
            result
        }
    }

    impl Drop for Publisher {
        fn drop(&mut self) {
            if let Some(lease) = self.lease.take() {
                let _ = File::unlock(&lease);
            }
            if open_at(
                &self.root,
                &self.snapshot.client_instance_id,
                libc::O_RDONLY | libc::O_DIRECTORY,
            )
            .and_then(|actual| same_file(&self.directory, &actual))
            .is_ok()
            {
                unlink(&self.directory, "state.json", /*flags*/ 0);
                unlink(&self.directory, "lease", /*flags*/ 0);
                unlink(
                    &self.root,
                    &self.snapshot.client_instance_id,
                    libc::AT_REMOVEDIR,
                );
            }
        }
    }
}

pub(crate) struct ClientPresence {
    #[cfg(unix)]
    publisher: unix::Publisher,
}

impl ClientPresence {
    pub(crate) fn start(home: &Path) -> io::Result<Option<Self>> {
        #[cfg(unix)]
        {
            unix::Publisher::start(home)
                .map(|publisher| publisher.map(|publisher| Self { publisher }))
        }
        #[cfg(not(unix))]
        {
            let _ = home;
            Ok(None)
        }
    }

    pub(crate) fn select_thread(&mut self, thread: &str) -> io::Result<()> {
        #[cfg(unix)]
        {
            self.publisher.select(Some(thread))
        }
        #[cfg(not(unix))]
        {
            let _ = thread;
            Ok(())
        }
    }

    pub(crate) fn clear_selection(&mut self) -> io::Result<()> {
        #[cfg(unix)]
        {
            self.publisher.select(/*thread*/ None)
        }
        #[cfg(not(unix))]
        {
            Ok(())
        }
    }
}

#[cfg(all(test, unix))]
#[path = "client_presence_tests.rs"]
mod tests;
