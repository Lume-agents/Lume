//! A separate process keeps ownership of the Codex app-server while monitoring
//! a pipe held by Lume. EOF still arrives if Lume crashes or receives SIGKILL.

use std::{
    env,
    io::{self, Read},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

pub const CLI_COMMAND: &str = "codex-process-supervisor";
const POLL_INTERVAL: Duration = Duration::from_millis(40);

pub fn spawn(command: Command) -> Result<Child, String> {
    let executable = env::current_exe()
        .map_err(|error| format!("Could not locate the Lume process supervisor: {error}"))?;
    let mut supervisor = Command::new(executable);
    supervisor
        .arg(CLI_COMMAND)
        .arg(std::process::id().to_string())
        .arg(command.get_program())
        .args(command.get_args())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Preserve executable discovery's PATH and the managed-session marker.
    for (key, value) in command.get_envs() {
        match value {
            Some(value) => {
                supervisor.env(key, value);
            }
            None => {
                supervisor.env_remove(key);
            }
        }
    }
    if let Some(directory) = command.get_current_dir() {
        supervisor.current_dir(directory);
    }
    // ChildStdin is retained by ManagedChild. Rust's piped stdio keeps the
    // parent's write end out of subsequent execs. The PPID guard below also
    // covers a concurrent exec racing macOS's non-atomic pipe/CLOEXEC setup.
    supervisor
        .spawn()
        .map_err(|error| format!("Could not start the Lume process supervisor: {error}"))
}

pub fn run_cli(args: &[String]) -> i32 {
    match supervise(args) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("Lume Codex process supervisor: {error}");
            1
        }
    }
}

fn supervise(args: &[String]) -> Result<i32, String> {
    let parent_id = args
        .first()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value > 1)
        .ok_or("Missing Lume parent process ID")?;
    let program = args.get(1).ok_or("Missing Codex executable")?;
    if args.get(2).map(String::as_str) != Some("app-server") {
        return Err("The process supervisor only accepts a Codex app-server command".into());
    }

    let disconnected = Arc::new(AtomicBool::new(false));
    let pipe_disconnected = disconnected.clone();
    thread::Builder::new()
        .name("lume-parent-lifetime".into())
        .spawn(move || {
            let mut input = io::stdin().lock();
            let mut buffer = [0u8; 32];
            loop {
                match input.read(&mut buffer) {
                    Ok(_) => break,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
            pipe_disconnected.store(true, Ordering::Release);
        })
        .map_err(|error| format!("Could not monitor the Lume lifetime pipe: {error}"))?;

    if !parent_is_alive(parent_id, &disconnected) {
        return Ok(0);
    }
    let mut child = SupervisedChild(
        Command::new(program)
            .args(&args[2..])
            .env("LUME_MANAGED_SESSION", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("Could not start `codex app-server`: {error}"))?,
    );

    loop {
        if let Some(status) = child.0.try_wait().map_err(|error| error.to_string())? {
            return Ok(status.code().unwrap_or(1));
        }
        if !parent_is_alive(parent_id, &disconnected) {
            child.0.kill().map_err(|error| error.to_string())?;
            child.0.wait().map_err(|error| error.to_string())?;
            return Ok(0);
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn parent_is_alive(expected_parent: u32, disconnected: &AtomicBool) -> bool {
    if disconnected.load(Ordering::Acquire) {
        return false;
    }
    // SAFETY: getppid has no arguments and is always successful on macOS.
    // It reports this helper's actual parent relationship. Once Lume exits,
    // reparenting changes it even if another process reuses Lume's numeric PID.
    let actual_parent = unsafe { libc::getppid() };
    u32::try_from(actual_parent).ok() == Some(expected_parent)
}

struct SupervisedChild(Child);

impl Drop for SupervisedChild {
    fn drop(&mut self) {
        // Only this helper waits for this child. Until it is reaped, its PID
        // cannot be reused. Never look up and signal a persisted numeric PID.
        if matches!(self.0.try_wait(), Ok(Some(_))) {
            return;
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
