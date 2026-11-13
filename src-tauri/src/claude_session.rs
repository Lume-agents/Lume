//! One long-lived `claude --print --input-format stream-json` process per
//! conversation Lume controls. Each message goes in as a stream-json line, so
//! background tasks survive the next message, the Task tool and `/goal` /
//! `/loop` are available, and Stop uses the control protocol's `interrupt`
//! instead of killing the process. Hooks still capture the transcript events;
//! this module only tracks turns, answers permission requests and interrupts.

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};

use serde_json::{json, Value};

/// A process with no turn running is closed after this long.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// How long Stop waits for Claude to confirm the interrupt before killing the process.
pub const INTERRUPT_GRACE: Duration = Duration::from_secs(5);

/// Flags that turn a `claude --print` command line into a streaming session.
pub const STREAM_FLAGS: [&str; 7] = [
    "--input-format",
    "stream-json",
    "--output-format",
    "stream-json",
    "--verbose",
    "--permission-prompt-tool",
    "stdio",
];

/// Whether new messages use the persistent process. `LUME_CLAUDE_PRINT_MODE=1`
/// brings back one `claude --print` per message.
pub fn enabled() -> bool {
    std::env::var("LUME_CLAUDE_PRINT_MODE").as_deref() != Ok("1")
}

/// One line Claude wrote to stdout.
#[derive(Debug, PartialEq)]
pub enum Incoming {
    Init,
    /// A turn ended. `queued` counts messages Claude still has to run.
    Result {
        is_error: bool,
        text: Option<String>,
        queued: u64,
    },
    /// Claude asks Lume something (`can_use_tool`, ...).
    ControlRequest {
        id: String,
        request: Value,
    },
    ControlResponse {
        id: String,
        response: Result<Value, String>,
    },
    /// Any other message: the model is working.
    Other,
}

pub fn parse_line(line: &str) -> Option<Incoming> {
    let message = serde_json::from_str::<Value>(line.trim()).ok()?;
    let kind = message.get("type").and_then(Value::as_str)?;
    Some(match kind {
        "system" if message.get("subtype").and_then(Value::as_str) == Some("init") => {
            Incoming::Init
        }
        "result" => Incoming::Result {
            is_error: message
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            text: message
                .get("result")
                .and_then(Value::as_str)
                .map(str::to_string),
            queued: message
                .get("queued_turn_count")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        },
        "control_request" => Incoming::ControlRequest {
            id: message
                .get("request_id")
                .and_then(Value::as_str)?
                .to_string(),
            request: message.get("request").cloned().unwrap_or(Value::Null),
        },
        "control_response" => {
            let response = message.get("response")?;
            let id = response
                .get("request_id")
                .and_then(Value::as_str)?
                .to_string();
            let response = if response.get("subtype").and_then(Value::as_str) == Some("error") {
                Err(response
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("Claude rejected the request")
                    .to_string())
            } else {
                Ok(response.get("response").cloned().unwrap_or(Value::Null))
            };
            Incoming::ControlResponse { id, response }
        }
        _ => Incoming::Other,
    })
}

pub fn user_message_line(prompt: &str) -> String {
    json!({
        "type": "user",
        "message": { "role": "user", "content": prompt },
        "parent_tool_use_id": null,
    })
    .to_string()
}

pub fn control_request_line(id: &str, request: Value) -> String {
    json!({ "type": "control_request", "request_id": id, "request": request }).to_string()
}

pub fn control_response_line(id: &str, response: Value) -> String {
    json!({
        "type": "control_response",
        "response": { "subtype": "success", "request_id": id, "response": response },
    })
    .to_string()
}

/// The arguments that decide whether a running process still fits a message;
/// the conversation ID moves from `--session-id` to `--resume`, so it is left out.
pub fn signature(arguments: &[String]) -> Vec<String> {
    let mut kept = Vec::new();
    let mut skip = false;
    for argument in arguments {
        if skip {
            skip = false;
        } else if matches!(argument.as_str(), "--resume" | "--session-id") {
            skip = true;
        } else {
            kept.push(argument.clone());
        }
    }
    kept
}

/// Adds the streaming flags to a `claude` command line without a prompt.
pub fn stream_arguments(arguments: &[String]) -> Vec<String> {
    let mut result = vec!["--print".to_string()];
    result.extend(
        arguments
            .iter()
            .filter(|argument| *argument != "--print")
            .cloned(),
    );
    result.extend(STREAM_FLAGS.iter().map(|flag| flag.to_string()));
    result
}

/// Requests Lume sent and is waiting on, by request ID.
#[derive(Default)]
pub struct Controls {
    next: AtomicU64,
    waiting: Mutex<HashMap<String, mpsc::Sender<Result<Value, String>>>>,
}

impl Controls {
    pub fn register(&self, prefix: &str) -> (String, mpsc::Receiver<Result<Value, String>>) {
        let id = format!("lume-{prefix}-{}", self.next.fetch_add(1, Ordering::SeqCst));
        let (sender, receiver) = mpsc::channel();
        if let Ok(mut waiting) = self.waiting.lock() {
            waiting.insert(id.clone(), sender);
        }
        (id, receiver)
    }

    /// Hands a response to whoever asked; false when nobody did.
    pub fn resolve(&self, id: &str, response: Result<Value, String>) -> bool {
        let sender = self
            .waiting
            .lock()
            .ok()
            .and_then(|mut waiting| waiting.remove(id));
        sender.is_some_and(|sender| sender.send(response).is_ok())
    }

    pub fn forget(&self, id: &str) {
        if let Ok(mut waiting) = self.waiting.lock() {
            waiting.remove(id);
        }
    }
}

/// Asks for an interrupt and waits for Claude to confirm; kills only when it does not.
/// Returns true when the process had to be killed.
pub fn interrupt_or_kill(
    send: impl FnOnce() -> Result<mpsc::Receiver<Result<Value, String>>, String>,
    grace: Duration,
    kill: impl FnOnce(),
) -> bool {
    let confirmed = send()
        .ok()
        .and_then(|receiver| receiver.recv_timeout(grace).ok())
        .is_some_and(|response| response.is_ok());
    if !confirmed {
        kill();
    }
    !confirmed
}

/// What the process tells its owner.
pub trait SessionEvents: Send + Sync + 'static {
    fn turn_finished(&self, live: &Arc<Live>, is_error: bool, text: Option<String>);
    fn control_request(&self, live: &Arc<Live>, id: String, request: Value);
    /// The process ended (reported unless Lume closed it while idle).
    fn exited(&self, live: &Arc<Live>, exit: Exit);
}

/// How a process ended.
#[derive(Clone, Debug, PartialEq)]
pub struct Exit {
    /// False when it ended before its `init`: streaming never started.
    pub started: bool,
    pub was_running: bool,
    /// Lume closed or killed it (idle timeout, session close, interrupt fallback).
    pub by_lume: bool,
    /// None when it was killed or its status could not be read.
    pub code: Option<i32>,
    pub stderr: String,
}

impl Exit {
    /// Whether the person should be told the session stopped: Lume did not end it, and either a
    /// turn was cut short or the process did not end cleanly.
    pub fn is_unexpected(&self) -> bool {
        !self.by_lume && (self.was_running || self.code != Some(0))
    }
}

pub struct Live {
    pub signature: Vec<String>,
    stdin: Mutex<Option<ChildStdin>>,
    child: Mutex<Child>,
    running: AtomicBool,
    started: AtomicBool,
    alive: AtomicBool,
    closed_by_lume: AtomicBool,
    last_used: Mutex<Instant>,
    pub controls: Controls,
    /// Held while a turn runs so background probes stay out of its way.
    in_flight: Mutex<Option<crate::launcher::InFlight>>,
}

impl Live {
    /// Starts the process and the thread that reads its output.
    pub fn start(
        mut command: Command,
        signature: Vec<String>,
        events: Arc<dyn SessionEvents>,
    ) -> Result<Arc<Self>, String> {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|error| format!("Could not start Claude: {error}"))?;
        let stdout = child.stdout.take().ok_or("Claude closed its output")?;
        let stderr = child.stderr.take();
        let stdin = child.stdin.take();
        let live = Arc::new(Self {
            signature,
            stdin: Mutex::new(stdin),
            child: Mutex::new(child),
            running: AtomicBool::new(false),
            started: AtomicBool::new(false),
            alive: AtomicBool::new(true),
            closed_by_lume: AtomicBool::new(false),
            last_used: Mutex::new(Instant::now()),
            controls: Controls::default(),
            in_flight: Mutex::new(None),
        });
        let stderr_tail = Arc::new(Mutex::new(String::new()));
        if let Some(mut stderr) = stderr {
            let tail = stderr_tail.clone();
            thread::spawn(move || {
                let mut buffer = [0u8; 4096];
                while let Ok(read) = stderr.read(&mut buffer) {
                    if read == 0 {
                        break;
                    }
                    if let Ok(mut tail) = tail.lock() {
                        tail.push_str(&String::from_utf8_lossy(&buffer[..read]));
                        if tail.len() > 16 * 1024 {
                            let cut = tail.len() - 8 * 1024;
                            let cut = (cut..tail.len())
                                .find(|i| tail.is_char_boundary(*i))
                                .unwrap_or(0);
                            tail.drain(..cut);
                        }
                    }
                }
            });
        }
        let reader = live.clone();
        thread::Builder::new()
            .name("lume-claude-session".into())
            .spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    match parse_line(&line) {
                        Some(Incoming::Init) => reader.started.store(true, Ordering::SeqCst),
                        Some(Incoming::Result {
                            is_error,
                            text,
                            queued,
                        }) => {
                            reader.started.store(true, Ordering::SeqCst);
                            if queued > 0 {
                                continue;
                            }
                            reader.set_running(false);
                            events.turn_finished(&reader, is_error, text);
                        }
                        Some(Incoming::ControlRequest { id, request }) => {
                            events.control_request(&reader, id, request)
                        }
                        Some(Incoming::ControlResponse { id, response }) => {
                            reader.controls.resolve(&id, response);
                        }
                        Some(Incoming::Other) => reader.started.store(true, Ordering::SeqCst),
                        None => {}
                    }
                }
                reader.alive.store(false, Ordering::SeqCst);
                let was_running = reader.running.swap(false, Ordering::SeqCst);
                if let Ok(mut in_flight) = reader.in_flight.lock() {
                    in_flight.take();
                }
                let code = reader
                    .child
                    .lock()
                    .ok()
                    .and_then(|mut child| child.wait().ok())
                    .and_then(|status| status.code());
                // Give the stderr reader a moment to collect the last lines.
                thread::sleep(Duration::from_millis(100));
                let stderr = stderr_tail
                    .lock()
                    .map(|tail| tail.trim().to_string())
                    .unwrap_or_default();
                let exit = Exit {
                    started: reader.started.load(Ordering::SeqCst),
                    was_running,
                    by_lume: reader.closed_by_lume.load(Ordering::SeqCst),
                    code,
                    stderr,
                };
                if exit.is_unexpected() || !exit.started {
                    events.exited(&reader, exit);
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(live)
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    fn set_running(&self, running: bool) {
        self.running.store(running, Ordering::SeqCst);
        if let Ok(mut in_flight) = self.in_flight.lock() {
            *in_flight = running.then(crate::launcher::InFlight::begin);
        }
        if let Ok(mut last_used) = self.last_used.lock() {
            *last_used = Instant::now();
        }
    }

    pub fn idle_for(&self) -> Duration {
        self.last_used
            .lock()
            .map(|at| at.elapsed())
            .unwrap_or_default()
    }

    pub fn write_line(&self, line: &str) -> Result<(), String> {
        let mut stdin = self
            .stdin
            .lock()
            .map_err(|_| "Claude's input is unavailable".to_string())?;
        let stdin = stdin.as_mut().ok_or("Claude's input is closed")?;
        writeln!(stdin, "{line}")
            .and_then(|_| stdin.flush())
            .map_err(|error| format!("Could not write to Claude: {error}"))
    }

    pub fn send_prompt(&self, prompt: &str) -> Result<(), String> {
        self.write_line(&user_message_line(prompt))?;
        self.set_running(true);
        Ok(())
    }

    pub fn respond(&self, id: &str, response: Value) -> Result<(), String> {
        self.write_line(&control_response_line(id, response))
    }

    /// Asks Claude to stop the turn; kills the process in the background if it does not confirm.
    pub fn interrupt(self: &Arc<Self>) -> Result<(), String> {
        let (id, receiver) = self.controls.register("interrupt");
        self.write_line(&control_request_line(
            &id,
            json!({ "subtype": "interrupt" }),
        ))?;
        self.set_running(false);
        let live = self.clone();
        thread::spawn(move || {
            interrupt_or_kill(|| Ok(receiver), INTERRUPT_GRACE, || live.kill());
            live.controls.forget(&id);
        });
        Ok(())
    }

    /// Closes stdin so Claude exits on its own; kills it if it lingers.
    pub fn close(self: &Arc<Self>) {
        self.closed_by_lume.store(true, Ordering::SeqCst);
        self.running.store(false, Ordering::SeqCst);
        if let Ok(mut stdin) = self.stdin.lock() {
            stdin.take();
        }
        let live = self.clone();
        thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            while live.is_alive() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(100));
            }
            if live.is_alive() {
                live.kill();
            }
        });
    }

    pub fn kill(&self) {
        self.closed_by_lume.store(true, Ordering::SeqCst);
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }
    }
}

fn registry() -> &'static Mutex<HashMap<String, Arc<Live>>> {
    static LIVE: OnceLock<Mutex<HashMap<String, Arc<Live>>>> = OnceLock::new();
    LIVE.get_or_init(|| {
        thread::Builder::new()
            .name("lume-claude-session-reaper".into())
            .spawn(|| loop {
                thread::sleep(Duration::from_secs(60));
                close_idle(IDLE_TIMEOUT);
            })
            .ok();
        Mutex::new(HashMap::new())
    })
}

pub fn get(session_id: &str) -> Option<Arc<Live>> {
    registry()
        .lock()
        .ok()
        .and_then(|live| live.get(session_id).cloned())
        .filter(|live| live.is_alive())
}

pub fn insert(session_id: &str, live: Arc<Live>) {
    if let Ok(mut registry) = registry().lock() {
        if let Some(previous) = registry.insert(session_id.to_string(), live) {
            previous.close();
        }
    }
}

/// Forgets this exact process (a newer one for the session stays).
pub fn remove(session_id: &str, live: &Arc<Live>) {
    if let Ok(mut registry) = registry().lock() {
        if registry
            .get(session_id)
            .is_some_and(|current| Arc::ptr_eq(current, live))
        {
            registry.remove(session_id);
        }
    }
}

/// Interrupts the running turn of a session's process; None when it has none.
pub fn interrupt(session_id: &str) -> Option<Result<(), String>> {
    get(session_id).map(|live| live.interrupt())
}

pub fn close(session_id: &str) {
    let live = registry()
        .lock()
        .ok()
        .and_then(|mut registry| registry.remove(session_id));
    if let Some(live) = live {
        live.close();
    }
}

pub fn close_all() {
    let all = registry()
        .lock()
        .map(|mut registry| registry.drain().map(|(_, live)| live).collect::<Vec<_>>())
        .unwrap_or_default();
    for live in all {
        live.close();
        live.kill();
    }
}

fn close_idle(timeout: Duration) {
    let idle = registry()
        .lock()
        .map(|mut registry| {
            let ids = registry
                .iter()
                .filter(|(_, live)| {
                    !live.is_alive() || (!live.is_running() && live.idle_for() >= timeout)
                })
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            ids.into_iter()
                .filter_map(|id| registry.remove(&id))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for live in idle {
        live.close();
    }
}

/// The answer to a `can_use_tool` request that nobody can answer.
pub fn deny(message: &str) -> Value {
    json!({ "behavior": "deny", "message": message })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_result_control_and_other_lines() {
        assert_eq!(
            parse_line(r#"{"type":"system","subtype":"init","tools":["Task"]}"#),
            Some(Incoming::Init)
        );
        assert_eq!(
            parse_line(r#"{"is_error":false,"result":"OK","type":"result","queued_turn_count":1}"#),
            Some(Incoming::Result {
                is_error: false,
                text: Some("OK".into()),
                queued: 1
            })
        );
        assert_eq!(
            parse_line(
                r#"{"type":"control_request","request_id":"r1","request":{"subtype":"can_use_tool","tool_name":"Bash"}}"#
            ),
            Some(Incoming::ControlRequest {
                id: "r1".into(),
                request: json!({ "subtype": "can_use_tool", "tool_name": "Bash" }),
            })
        );
        assert_eq!(
            parse_line(
                r#"{"type":"control_response","response":{"subtype":"error","request_id":"x","error":"no"}}"#
            ),
            Some(Incoming::ControlResponse {
                id: "x".into(),
                response: Err("no".into())
            })
        );
        assert_eq!(
            parse_line(r#"{"type":"assistant","message":{}}"#),
            Some(Incoming::Other)
        );
        assert_eq!(parse_line("not json"), None);
    }

    #[test]
    fn builds_protocol_lines() {
        let user: Value = serde_json::from_str(&user_message_line("hi")).unwrap();
        assert_eq!(user["type"], "user");
        assert_eq!(user["message"]["content"], "hi");
        let response: Value =
            serde_json::from_str(&control_response_line("r1", json!({ "behavior": "allow" })))
                .unwrap();
        assert_eq!(response["response"]["request_id"], "r1");
        assert_eq!(response["response"]["response"]["behavior"], "allow");
    }

    #[test]
    fn control_responses_reach_only_their_request() {
        let controls = Controls::default();
        let (first, first_rx) = controls.register("interrupt");
        let (second, second_rx) = controls.register("interrupt");
        assert_ne!(first, second);
        assert!(controls.resolve(&second, Ok(json!(2))));
        assert!(!controls.resolve(&second, Ok(json!(3))));
        assert!(!controls.resolve("unknown", Ok(json!(0))));
        assert_eq!(second_rx.try_recv().unwrap(), Ok(json!(2)));
        assert!(first_rx.try_recv().is_err());
    }

    #[test]
    fn interrupt_kills_only_without_confirmation() {
        let killed = std::cell::Cell::new(false);
        let (sender, receiver) = mpsc::channel();
        sender.send(Ok(json!({ "still_queued": [] }))).unwrap();
        assert!(!interrupt_or_kill(
            || Ok(receiver),
            Duration::from_millis(50),
            || killed.set(true)
        ));
        assert!(!killed.get());

        let (_sender, silent) = mpsc::channel();
        assert!(interrupt_or_kill(
            || Ok(silent),
            Duration::from_millis(50),
            || killed.set(true)
        ));
        assert!(killed.get());

        let killed = std::cell::Cell::new(false);
        assert!(interrupt_or_kill(
            || Err("closed".into()),
            Duration::from_millis(50),
            || killed.set(true)
        ));
        assert!(killed.get());
    }

    #[test]
    fn signature_ignores_the_conversation_id_and_stream_flags_are_added_once() {
        let first = vec![
            "--session-id".into(),
            "abc".into(),
            "--model".into(),
            "haiku".into(),
        ];
        let later = vec![
            "--model".into(),
            "haiku".into(),
            "--resume".into(),
            "abc".into(),
        ];
        assert_eq!(signature(&first), signature(&later));
        let arguments = stream_arguments(&["--print".into(), "--model".into(), "haiku".into()]);
        assert_eq!(arguments.iter().filter(|a| *a == "--print").count(), 1);
        assert!(arguments
            .windows(2)
            .any(|w| w[0] == "--input-format" && w[1] == "stream-json"));
        assert!(arguments
            .windows(2)
            .any(|w| w[0] == "--permission-prompt-tool" && w[1] == "stdio"));
    }

    struct Recorder {
        exits: Mutex<Vec<bool>>,
        results: Mutex<Vec<(bool, Option<String>)>>,
    }

    impl SessionEvents for Recorder {
        fn turn_finished(&self, _: &Arc<Live>, is_error: bool, text: Option<String>) {
            self.results.lock().unwrap().push((is_error, text));
        }
        fn control_request(&self, _: &Arc<Live>, _: String, _: Value) {}
        fn exited(&self, _: &Arc<Live>, exit: Exit) {
            self.exits.lock().unwrap().push(exit.started);
        }
    }

    /// A stub "Claude" that prints an init and a result, then exits.
    #[cfg(windows)]
    fn stub_command(lines: &[&str]) -> Command {
        use std::os::windows::process::CommandExt;
        let mut script = lines
            .iter()
            .map(|line| format!("echo {line}"))
            .collect::<Vec<_>>();
        script.push("exit 0".into());
        let mut command = Command::new("cmd");
        // Raw, so cmd sees the JSON quotes as they are.
        command.arg("/C").raw_arg(script.join("&"));
        command
    }

    #[cfg(not(windows))]
    fn stub_command(lines: &[&str]) -> Command {
        let mut script = lines
            .iter()
            .map(|line| format!("echo '{line}'"))
            .collect::<Vec<_>>();
        script.push("exit 0".into());
        let mut command = Command::new("sh");
        command.arg("-c").arg(script.join(";"));
        command
    }

    /// Talks to the installed CLI: `cargo test --lib claude_session -- --ignored`.
    #[test]
    #[ignore]
    fn live_cli_answers_interrupts_without_dying_and_takes_the_next_message() {
        let recorder = Arc::new(Recorder {
            exits: Mutex::new(Vec::new()),
            results: Mutex::new(Vec::new()),
        });
        let mut command = crate::executables::command("claude").unwrap();
        let arguments = stream_arguments(&[
            "--model".into(),
            "haiku".into(),
            "--permission-mode".into(),
            "bypassPermissions".into(),
            "--allow-dangerously-skip-permissions".into(),
            "--settings".into(),
            r#"{"disableAllHooks":true}"#.into(),
        ]);
        command.args(&arguments).current_dir(std::env::temp_dir());
        let live = Live::start(command, signature(&arguments), recorder.clone()).unwrap();
        let results = || recorder.results.lock().unwrap().len();
        let wait_for = |count: usize| {
            let deadline = Instant::now() + Duration::from_secs(90);
            while results() < count && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(100));
            }
        };
        live.send_prompt("Reply with just OK").unwrap();
        wait_for(1);
        assert_eq!(results(), 1);
        live.send_prompt("Run the shell command `sleep 30`, then say DONE.").unwrap();
        thread::sleep(Duration::from_secs(8));
        live.interrupt().unwrap();
        wait_for(2);
        thread::sleep(Duration::from_secs(INTERRUPT_GRACE.as_secs() + 1));
        assert!(live.is_alive(), "an interrupt must not kill the process");
        live.send_prompt("Reply with just AFTER").unwrap();
        wait_for(3);
        let all = recorder.results.lock().unwrap().clone();
        assert!(all[1].0, "the interrupted turn ends as an error result");
        assert!(all[2].1.as_deref().unwrap_or_default().contains("AFTER"));
        live.close();
    }

    fn wait_until(condition: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !condition() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn only_exits_lume_did_not_cause_are_unexpected() {
        let exit = |was_running, by_lume, code| Exit {
            started: true,
            was_running,
            by_lume,
            code,
            stderr: String::new(),
        };
        assert!(exit(true, false, Some(0)).is_unexpected());
        assert!(exit(false, false, Some(1)).is_unexpected());
        assert!(exit(false, false, None).is_unexpected());
        assert!(!exit(false, false, Some(0)).is_unexpected());
        assert!(!exit(true, true, None).is_unexpected());
    }

    #[test]
    fn exit_is_noticed_and_a_new_process_replaces_it() {
        let recorder = Arc::new(Recorder {
            exits: Mutex::new(Vec::new()),
            results: Mutex::new(Vec::new()),
        });
        // Exits before its init: reported as a failed start so the caller can fall back.
        let first = Live::start(stub_command(&[]), vec![], recorder.clone()).unwrap();
        wait_until(|| !first.is_alive());
        assert!(!first.is_alive());
        wait_until(|| !recorder.exits.lock().unwrap().is_empty());
        assert_eq!(*recorder.exits.lock().unwrap(), vec![false]);

        insert("test-session", first.clone());
        assert!(
            get("test-session").is_none(),
            "a dead process is not reused"
        );
        let result = r#"{"type":"result","is_error":false,"result":"OK"}"#;
        let second = Live::start(
            stub_command(&[r#"{"type":"system","subtype":"init"}"#, result]),
            vec![],
            recorder.clone(),
        )
        .unwrap();
        wait_until(|| !second.is_alive());
        assert_eq!(
            *recorder.results.lock().unwrap(),
            vec![(false, Some("OK".into()))]
        );
        // It started and was idle when it ended: nothing to report.
        assert_eq!(recorder.exits.lock().unwrap().len(), 1);
        remove("test-session", &first);
    }
}
