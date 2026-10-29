//! Repository context, plus committing the files the user ticked. Commands are fixed, bounded and never invoke a shell.
use std::{
    collections::{BTreeMap, HashMap},
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{mpsc, Arc, Mutex, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};

const OUTPUT_LIMIT: usize = 1_048_576;
const LOCAL_TTL: Duration = Duration::from_secs(5);
const REMOTE_TTL: Duration = Duration::from_secs(60);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityDay {
    pub date: String,
    pub count: u32,
    pub level: u8,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryFile {
    pub path: String,
    pub index: String,
    pub worktree: String,
    pub conflict: bool,
    pub untracked: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryCommit {
    pub oid: String,
    pub subject: String,
    pub author: String,
    pub date: String,
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositorySnapshot {
    pub root: String,
    pub name: String,
    pub branch: String,
    pub head: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub staged: usize,
    pub modified: usize,
    pub untracked: usize,
    pub conflicts: usize,
    pub files: Vec<RepositoryFile>,
    pub commits: Vec<RepositoryCommit>,
    pub days: Vec<ActivityDay>,
    pub activity_limited: bool,
    pub shallow: bool,
    pub github: Option<String>,
    pub fetched_at: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryDiff {
    pub path: String,
    pub diff: String,
    pub binary: bool,
    pub untracked: bool,
}

struct Cached<T> {
    value: T,
    at: Instant,
}
type LocalEntry = Arc<Mutex<Option<Cached<RepositorySnapshot>>>>;
static LOCAL: OnceLock<Mutex<HashMap<PathBuf, LocalEntry>>> = OnceLock::new();
type RemoteEntry = Arc<Mutex<Option<Cached<Value>>>>;
static REMOTE: OnceLock<Mutex<HashMap<String, RemoteEntry>>> = OnceLock::new();

fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn read_pipe(
    mut pipe: impl Read + Send + 'static,
    limit: usize,
) -> mpsc::Receiver<Result<Vec<u8>, String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = pipe
            .by_ref()
            .take((limit + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| "command_read_failed".to_string())
            .and_then(|_| {
                if bytes.len() > limit {
                    Err("command_output_limit".into())
                } else {
                    Ok(bytes)
                }
            });
        let _ = tx.send(result);
    });
    rx
}

fn run(mut command: Command, timeout: Duration) -> Result<Vec<u8>, String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "command_unavailable".to_string())?;
    let stdout = read_pipe(
        child.stdout.take().ok_or("command_read_failed")?,
        OUTPUT_LIMIT,
    );
    let stderr = read_pipe(child.stderr.take().ok_or("command_read_failed")?, 16_384);
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(20))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("command_timeout".into());
            }
        }
    };
    let output = stdout
        .recv_timeout(Duration::from_millis(200))
        .map_err(|_| "command_read_failed")??;
    if !status.success() {
        // Map known failures without sending stderr (which can contain credentials) to the webview.
        let error = stderr
            .recv_timeout(Duration::from_millis(200))
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default();
        let message = String::from_utf8_lossy(&error).to_ascii_lowercase();
        return Err(if message.contains("401")
            || message.contains("gh auth login")
            || message.contains("not logged")
        {
            "auth_required"
        } else if message.contains("403") || message.contains("rate limit") {
            "access_limited"
        } else if message.contains("tell me who you are")
            || message.contains("author identity unknown")
            || message.contains("empty ident name")
        {
            "identity_missing"
        } else if message.contains("nothing to commit") || message.contains("no changes added") {
            "nothing_to_commit"
        } else if message.contains("not possible to fast-forward")
            || message.contains("diverging branches")
        {
            "diverged"
        } else if message.contains("would be overwritten") || message.contains("please commit your changes or stash") {
            "local_changes_block"
        } else if message.contains("authentication failed")
            || message.contains("could not read username")
            || message.contains("permission denied")
        {
            "git_auth_failed"
        } else if message.contains("could not resolve host")
            || message.contains("unable to access")
            || message.contains("network is unreachable")
        {
            "network_failed"
        } else if message.contains("conflict") {
            "stash_conflict"
        } else if message.contains("hook") {
            "hook_failed"
        } else {
            "command_failed"
        }
        .into());
    }
    Ok(output)
}

pub(crate) fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    git_within(root, args, Duration::from_secs(5))
}

fn git_within(root: &Path, args: &[&str], timeout: Duration) -> Result<Vec<u8>, String> {
    let mut command = crate::executables::command("git").map_err(|_| "git_missing")?;
    command
        .current_dir(root)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_PAGER", "cat")
        .env("GIT_CONFIG_COUNT", "0")
        .args([
            "--no-pager",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.quotePath=false",
            "-c",
            "color.ui=false",
        ])
        .args(args);
    run(command, timeout)
}

fn git_text(root: &Path, args: &[&str]) -> Result<String, String> {
    git(root, args).map(|bytes| {
        String::from_utf8_lossy(&bytes)
            .trim_end_matches(['\n', '\r'])
            .to_string()
    })
}

fn root_for(directory: &str) -> Result<PathBuf, String> {
    let directory = Path::new(directory)
        .canonicalize()
        .map_err(|_| "directory_unavailable")?;
    let root = git_text(&directory, &["rev-parse", "--show-toplevel"]).map_err(|error| {
        if error == "command_failed" {
            "not_a_repository".into()
        } else {
            error
        }
    })?;
    PathBuf::from(root)
        .canonicalize()
        .map_err(|_| "directory_unavailable".into())
}

fn github_name(remote: &str) -> Option<String> {
    let remote = remote.trim();
    let path = remote
        .strip_prefix("https://github.com/")
        .or_else(|| remote.strip_prefix("http://github.com/"))
        .or_else(|| remote.strip_prefix("git@github.com:"))
        .or_else(|| remote.strip_prefix("ssh://git@github.com/"))?;
    let path = path
        .trim_end_matches('/')
        .strip_suffix(".git")
        .unwrap_or(path.trim_end_matches('/'));
    let parts: Vec<_> = path.split('/').collect();
    (parts.len() == 2
        && parts.iter().all(|part| {
            !part.is_empty()
                && *part != "."
                && *part != ".."
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        }))
    .then(|| path.to_string())
}

pub fn snapshot(directory: &str, refresh: bool) -> Result<RepositorySnapshot, String> {
    let root = root_for(directory)?;
    let entry = {
        let mut cache = LOCAL
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| "repository_busy")?;
        if cache.len() >= 64 && !cache.contains_key(&root) {
            cache.clear();
        }
        cache.entry(root.clone()).or_default().clone()
    };
    let mut cached = entry.lock().map_err(|_| "repository_busy")?;
    if let Some(value) = cached
        .as_ref()
        .filter(|value| value.at.elapsed() < LOCAL_TTL && !refresh)
    {
        return Ok(value.value.clone());
    }
    let previous = cached.as_ref().map(|value| &value.value);
    let mut value = RepositorySnapshot {
        root: root.to_string_lossy().into_owned(),
        name: root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        fetched_at: timestamp(),
        shallow: git_text(&root, &["rev-parse", "--is-shallow-repository"])? == "true",
        ..Default::default()
    };
    let status = git(
        &root,
        &[
            "status",
            "--porcelain=v2",
            "--branch",
            "-z",
            "--untracked-files=all",
        ],
    )?;
    let mut records = status.split(|byte| *byte == 0);
    while let Some(record) = records.next() {
        let record = String::from_utf8_lossy(record);
        if let Some(head) = record.strip_prefix("# branch.oid ") {
            value.head = (head != "(initial)").then(|| head.to_string());
        } else if let Some(branch) = record.strip_prefix("# branch.head ") {
            value.branch = branch.to_string();
        } else if let Some(upstream) = record.strip_prefix("# branch.upstream ") {
            value.upstream = Some(upstream.to_string());
        } else if let Some(ab) = record.strip_prefix("# branch.ab ") {
            let mut parts = ab.split_whitespace();
            value.ahead = parts
                .next()
                .unwrap_or_default()
                .trim_start_matches('+')
                .parse()
                .unwrap_or(0);
            value.behind = parts
                .next()
                .unwrap_or_default()
                .trim_start_matches('-')
                .parse()
                .unwrap_or(0);
        } else {
            let (path, xy, conflict, untracked) = if let Some(path) = record.strip_prefix("? ") {
                (path, "??", false, true)
            } else if record.starts_with("1 ")
                || record.starts_with("2 ")
                || record.starts_with("u ")
            {
                let kind = record.as_bytes()[0];
                let fields: Vec<_> = record
                    .splitn(
                        if kind == b'1' {
                            9
                        } else if kind == b'2' {
                            10
                        } else {
                            11
                        },
                        ' ',
                    )
                    .collect();
                if kind == b'2' {
                    let _ = records.next();
                }
                (
                    fields.last().copied().unwrap_or_default(),
                    fields.get(1).copied().unwrap_or(".."),
                    kind == b'u',
                    false,
                )
            } else {
                continue;
            };
            let index = xy.chars().next().unwrap_or('.').to_string();
            let worktree = xy.chars().nth(1).unwrap_or('.').to_string();
            value.staged += usize::from(!untracked && index != ".");
            value.modified += usize::from(!untracked && worktree != ".");
            value.untracked += usize::from(untracked);
            value.conflicts += usize::from(conflict);
            value.files.push(RepositoryFile {
                path: path.to_string(),
                index,
                worktree,
                conflict,
                untracked,
            });
        }
    }
    if let Ok(remotes) = git_text(&root, &["config", "--get-regexp", "^remote\\..*\\.url$"]) {
        let mut choices: Vec<_> = remotes
            .lines()
            .filter_map(|line| line.split_once(' '))
            .collect();
        choices.sort_by_key(|(key, _)| *key != "remote.origin.url");
        value.github = choices.iter().find_map(|(_, url)| github_name(url));
    }
    if previous.is_some_and(|previous| {
        previous.head == value.head && previous.fetched_at / 86_400 == value.fetched_at / 86_400
    }) {
        if let Some(previous) = previous {
            value.commits = previous.commits.clone();
            value.days = previous.days.clone();
            value.activity_limited = previous.activity_limited;
        }
    } else if value.head.is_some() {
        let log = git_text(
            &root,
            &[
                "log",
                "-20",
                "--format=%H%x1f%s%x1f%an%x1f%cs",
                "HEAD",
                "--",
            ],
        )?;
        value.commits = log
            .lines()
            .filter_map(|line| {
                let parts: Vec<_> = line.splitn(4, '\u{1f}').collect();
                (parts.len() == 4).then(|| RepositoryCommit {
                    oid: parts[0].into(),
                    subject: parts[1].into(),
                    author: parts[2].into(),
                    date: parts[3].into(),
                })
            })
            .collect();
        let since = DateTime::<Utc>::from_timestamp(timestamp() as i64 - 365 * 86_400, 0)
            .ok_or("invalid_clock")?
            .format("%Y-%m-%d")
            .to_string();
        let dates = git_text(
            &root,
            &[
                "log",
                "--max-count=20001",
                &format!("--since={since}"),
                "--format=%cs",
                "HEAD",
                "--",
            ],
        )?;
        let mut counts = BTreeMap::new();
        for date in dates.lines().take(20_000) {
            if date.len() == 10 {
                *counts.entry(date.to_string()).or_insert(0_u32) += 1;
            }
        }
        value.activity_limited = dates.lines().count() > 20_000;
        value.days = counts
            .into_iter()
            .map(|(date, count)| ActivityDay {
                date,
                count,
                level: match count {
                    0 => 0,
                    1 => 1,
                    2..=3 => 2,
                    4..=7 => 3,
                    _ => 4,
                },
            })
            .collect();
    }
    *cached = Some(Cached {
        value: value.clone(),
        at: Instant::now(),
    });
    Ok(value)
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitOutcome {
    pub hash: String,
    pub summary: String,
}

/// Commits exactly the given changed files. Every path must be a current change of the repository, so the
/// webview cannot make Git touch anything else.
pub fn commit(directory: &str, paths: &[String], message: &str) -> Result<CommitOutcome, String> {
    let message = message.trim();
    if message.is_empty() || paths.is_empty() {
        return Err("nothing_to_commit".into());
    }
    let value = snapshot(directory, true)?;
    for path in paths {
        let known = value
            .files
            .iter()
            .any(|file| file.path == *path && !file.conflict);
        if !known {
            return Err("file_no_longer_changed".into());
        }
        if Path::new(path).is_absolute()
            || Path::new(path)
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            return Err("invalid_path".into());
        }
    }
    let root = Path::new(&value.root);
    let timeout = Duration::from_secs(60);
    let mut add = vec!["add", "--"];
    add.extend(paths.iter().map(String::as_str));
    git_within(root, &add, timeout)?;
    let mut commit = vec!["commit", "--only", "-m", message, "--"];
    commit.extend(paths.iter().map(String::as_str));
    git_within(root, &commit, timeout)?;
    let hash = git_text(root, &["rev-parse", "--short", "HEAD"])?;
    let summary = git_text(root, &["log", "-1", "--format=%s"])?;
    Ok(CommitOutcome { hash, summary })
}

// ── History ──────────────────────────────────────────────────────────────────

/// One commit of the history graph: its parents (to draw the lines), the branches and tags that point at it.
#[derive(Clone, Serialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HistoryCommit {
    pub oid: String,
    pub parents: Vec<String>,
    pub author: String,
    pub at: i64,
    pub refs: Vec<String>,
    pub subject: String,
}

#[derive(Clone, Serialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CommitFile {
    pub path: String,
    pub added: Option<u32>,
    pub removed: Option<u32>,
}

#[derive(Clone, Serialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CommitDetail {
    pub oid: String,
    pub message: String,
    pub files: Vec<CommitFile>,
}

const HISTORY_PAGE: usize = 200;

fn valid_oid(oid: &str) -> bool {
    (7..=64).contains(&oid.len()) && oid.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// `git log` records (`%H %P %an %at %D %s`, joined by unit separators), newest first, in topological order so the
/// lines of the graph never cross a commit that comes later.
fn parse_history(bytes: &[u8]) -> Vec<HistoryCommit> {
    String::from_utf8_lossy(bytes)
        .split('\0')
        .filter_map(|record| {
            let record = record.trim_start_matches(['\n', '\r']);
            let mut fields = record.splitn(6, '\u{1f}');
            let oid = fields.next().filter(|oid| valid_oid(oid))?.to_string();
            let parents = fields.next()?.split_whitespace().map(str::to_string).collect();
            let author = fields.next()?.to_string();
            let at = fields.next()?.parse().ok()?;
            let refs = fields
                .next()?
                .split(", ")
                .filter(|name| !name.is_empty() && !name.ends_with("/HEAD"))
                .map(|name| name.trim_start_matches("HEAD -> ").trim_start_matches("tag: ").to_string())
                .collect();
            let subject = fields.next()?.to_string();
            Some(HistoryCommit { oid, parents, author, at, refs, subject })
        })
        .collect()
}

/// The commits of every branch, tag and remote, a page at a time.
pub fn history(directory: &str, skip: usize) -> Result<Vec<HistoryCommit>, String> {
    let root = root_for(directory)?;
    if git_text(&root, &["rev-parse", "--verify", "-q", "HEAD"]).is_err() {
        return Ok(Vec::new());
    }
    let skip = skip.min(100_000).to_string();
    let limit = HISTORY_PAGE.to_string();
    let bytes = git(
        &root,
        &[
            "log",
            "--all",
            "--topo-order",
            "-z",
            "--decorate=short",
            "--skip",
            &skip,
            "-n",
            &limit,
            "--format=%H%x1f%P%x1f%an%x1f%at%x1f%D%x1f%s",
        ],
    )?;
    Ok(parse_history(&bytes))
}

/// A commit's full message and the files it changed with their line counts.
pub fn commit_detail(directory: &str, oid: &str) -> Result<CommitDetail, String> {
    if !valid_oid(oid) {
        return Err("invalid_commit".into());
    }
    let root = root_for(directory)?;
    let message = git_text(&root, &["show", "-s", "--format=%B", oid])?;
    let stat = git(&root, &["show", "--format=", "--numstat", "--no-renames", "-z", oid])?;
    let files = String::from_utf8_lossy(&stat)
        .split('\0')
        .filter_map(|record| {
            let mut parts = record.trim_start_matches(['\n', '\r']).splitn(3, '\t');
            let added = parts.next()?;
            let removed = parts.next()?;
            let path = parts.next()?.to_string();
            Some(CommitFile { path, added: added.parse().ok(), removed: removed.parse().ok() })
        })
        .take(500)
        .collect();
    Ok(CommitDetail { oid: oid.to_string(), message: message.trim().to_string(), files })
}

/// One file's diff in a commit.
pub fn commit_diff(directory: &str, oid: &str, path: &str) -> Result<RepositoryDiff, String> {
    if !valid_oid(oid)
        || Path::new(path).is_absolute()
        || Path::new(path).components().any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err("invalid_path".into());
    }
    let root = root_for(directory)?;
    let bytes = git(&root, &["show", "--format=", "--no-ext-diff", "--no-textconv", "--no-renames", oid, "--", path])?;
    if bytes.len() > OUTPUT_LIMIT {
        return Err("command_output_limit".into());
    }
    let binary = bytes.contains(&0);
    Ok(RepositoryDiff {
        path: path.into(),
        diff: if binary { String::new() } else { String::from_utf8_lossy(&bytes).into_owned() },
        binary,
        untracked: false,
    })
}

// ── Git actions ──────────────────────────────────────────────────────────────

const REMOTE_TIMEOUT: Duration = Duration::from_secs(90);

fn invalidate(directory: &str) -> Result<RepositorySnapshot, String> {
    snapshot(directory, true)
}

/// Downloads what the remote has, without touching the working tree. Prunes the branches deleted there.
pub fn fetch(directory: &str) -> Result<RepositorySnapshot, String> {
    let root = root_for(directory)?;
    git_within(&root, &["fetch", "--prune", "--quiet"], REMOTE_TIMEOUT)?;
    invalidate(directory)
}

/// Brings the branch up to its upstream, and only by moving it forward: a diverged branch is refused, never merged.
pub fn pull(directory: &str) -> Result<RepositorySnapshot, String> {
    let root = root_for(directory)?;
    let before = snapshot(directory, true)?;
    if before.branch == "(detached)" || before.upstream.is_none() {
        return Err("no_upstream".into());
    }
    if before.conflicts > 0 {
        return Err("has_conflicts".into());
    }
    git_within(&root, &["fetch", "--quiet"], REMOTE_TIMEOUT)?;
    git_within(&root, &["merge", "--ff-only", "--quiet", "@{upstream}"], REMOTE_TIMEOUT)?;
    invalidate(directory)
}

#[derive(Clone, Serialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StashEntry {
    pub oid: String,
    pub index: usize,
    pub message: String,
    pub at: u64,
}

pub fn stashes(directory: &str) -> Result<Vec<StashEntry>, String> {
    let root = root_for(directory)?;
    let text = git_text(&root, &["stash", "list", "--format=%H%x1f%ct%x1f%gs"])?;
    Ok(text
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let mut fields = line.split('\u{1f}');
            let oid = fields.next()?.to_string();
            let at = fields.next()?.parse().ok()?;
            let message = fields.next()?.to_string();
            Some(StashEntry { oid, index, message, at })
        })
        .collect())
}

/// Puts the tracked and untracked changes away so the tree is clean.
pub fn stash_push(directory: &str, message: &str) -> Result<Vec<StashEntry>, String> {
    let root = root_for(directory)?;
    let message: String = message.chars().filter(|c| !c.is_control()).take(120).collect();
    let message = if message.trim().is_empty() { "Lume".to_string() } else { message };
    git_within(
        &root,
        &["stash", "push", "--include-untracked", "--quiet", "-m", &message],
        Duration::from_secs(60),
    )?;
    invalidate(directory)?;
    stashes(directory)
}

/// A stash is addressed by its commit, so a list that changed since it was shown can never make Git act on another one.
fn stash_ref(root: &Path, oid: &str, index: usize) -> Result<String, String> {
    if !valid_oid(oid) {
        return Err("invalid_oid".into());
    }
    let reference = format!("stash@{{{index}}}");
    let current = git_text(root, &["rev-parse", "--verify", "--quiet", &reference])
        .map_err(|_| "stash_changed".to_string())?;
    if current != oid {
        return Err("stash_changed".into());
    }
    Ok(reference)
}

/// Applies a stash and keeps it, so a conflict never loses the changes.
pub fn stash_apply(directory: &str, oid: &str, index: usize) -> Result<RepositorySnapshot, String> {
    let root = root_for(directory)?;
    let reference = stash_ref(&root, oid, index)?;
    git_within(&root, &["stash", "apply", "--quiet", &reference], Duration::from_secs(60))?;
    invalidate(directory)
}

pub fn stash_drop(directory: &str, oid: &str, index: usize) -> Result<Vec<StashEntry>, String> {
    let root = root_for(directory)?;
    let reference = stash_ref(&root, oid, index)?;
    git_within(&root, &["stash", "drop", "--quiet", &reference], Duration::from_secs(15))?;
    stashes(directory)
}

#[derive(Clone, Serialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LocalBranch {
    pub name: String,
    pub current: bool,
    pub upstream: Option<String>,
    pub at: u64,
}

pub fn branches(directory: &str) -> Result<Vec<LocalBranch>, String> {
    let root = root_for(directory)?;
    let text = git_text(
        &root,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname:short)%1f%(HEAD)%1f%(upstream:short)%1f%(committerdate:unix)",
            "refs/heads",
        ],
    )?;
    Ok(text
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\u{1f}');
            let name = fields.next()?.to_string();
            let current = fields.next()? == "*";
            let upstream = fields.next().filter(|value| !value.is_empty()).map(String::from);
            let at = fields.next()?.parse().unwrap_or(0);
            Some(LocalBranch { name, current, upstream, at })
        })
        .collect())
}

/// Switches to another local branch, only with no change that the switch could carry along or lose.
pub fn switch_branch(directory: &str, name: &str) -> Result<RepositorySnapshot, String> {
    let root = root_for(directory)?;
    if !branches(directory)?.iter().any(|branch| branch.name == name) {
        return Err("unknown_branch".into());
    }
    let before = snapshot(directory, true)?;
    if before.conflicts > 0 || before.staged > 0 || before.modified > 0 {
        return Err("dirty_tree".into());
    }
    git_within(&root, &["switch", "--quiet", name], Duration::from_secs(30))?;
    invalidate(directory)
}

pub fn diff(directory: &str, path: &str) -> Result<RepositoryDiff, String> {
    let value = snapshot(directory, true)?;
    let file = value
        .files
        .iter()
        .find(|file| file.path == path)
        .ok_or("file_no_longer_changed")?;
    let root = Path::new(&value.root);
    if Path::new(path).is_absolute()
        || Path::new(path)
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err("invalid_path".into());
    }
    let bytes = if file.untracked {
        let target = root
            .join(path)
            .canonicalize()
            .map_err(|_| "file_unavailable")?;
        if !target.starts_with(root) || !target.is_file() {
            return Err("invalid_path".into());
        }
        let mut bytes = Vec::new();
        std::fs::File::open(target)
            .map_err(|_| "file_unavailable")?
            .take((OUTPUT_LIMIT + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| "file_unavailable")?;
        if bytes.len() > OUTPUT_LIMIT {
            return Err("command_output_limit".into());
        }
        bytes
    } else if value.head.is_some() {
        git(
            root,
            &["diff", "--no-ext-diff", "--no-textconv", "HEAD", "--", path],
        )?
    } else {
        let mut bytes = git(
            root,
            &[
                "diff",
                "--no-ext-diff",
                "--no-textconv",
                "--cached",
                "--",
                path,
            ],
        )?;
        bytes.extend(git(
            root,
            &["diff", "--no-ext-diff", "--no-textconv", "--", path],
        )?);
        if bytes.len() > OUTPUT_LIMIT {
            return Err("command_output_limit".into());
        }
        bytes
    };
    let binary = bytes.contains(&0);
    Ok(RepositoryDiff {
        path: path.into(),
        diff: if binary {
            String::new()
        } else {
            String::from_utf8_lossy(&bytes).into_owned()
        },
        binary,
        untracked: file.untracked,
    })
}

fn gh(args: &[String]) -> Result<Value, String> {
    let mut command = crate::executables::command("gh").map_err(|_| "cli_missing")?;
    command
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_PAGER", "cat")
        .env("NO_COLOR", "1")
        .args(args);
    let bytes = run(command, Duration::from_secs(12))?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| "invalid_response")?;
    if value.get("errors").is_some() {
        return Err("access_limited".into());
    }
    Ok(value)
}

fn graphql(query: &str, fields: &[(&str, &str)]) -> Result<Value, String> {
    let mut args = vec![
        "api".into(),
        "--hostname".into(),
        "github.com".into(),
        "graphql".into(),
        "-f".into(),
        format!("query={query}"),
    ];
    for (key, value) in fields {
        args.extend(["-f".into(), format!("{key}={value}")]);
    }
    gh(&args)
}

fn cached_remote(
    key: String,
    refresh: bool,
    read: impl FnOnce() -> Value,
) -> Result<Value, String> {
    let entry = {
        let mut cache = REMOTE
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| "repository_busy")?;
        if cache.len() >= 64 && !cache.contains_key(&key) {
            cache.clear();
        }
        cache.entry(key).or_default().clone()
    };
    let mut cached = entry.lock().map_err(|_| "repository_busy")?;
    if let Some(value) = cached
        .as_ref()
        .filter(|value| value.at.elapsed() < REMOTE_TTL && !refresh)
    {
        return Ok(value.value.clone());
    }
    let value = read();
    *cached = Some(Cached {
        value: value.clone(),
        at: Instant::now(),
    });
    Ok(value)
}

const REPO_QUERY: &str = r#"query($owner:String!,$name:String!,$ref:String!) {
  repository(owner:$owner,name:$name) {
    nameWithOwner url description isPrivate defaultBranchRef { name }
    issues(first:15,states:OPEN,orderBy:{field:UPDATED_AT,direction:DESC}) { totalCount nodes { number title url updatedAt author { login } } }
    pullRequests(first:15,states:OPEN,orderBy:{field:UPDATED_AT,direction:DESC}) { totalCount nodes { number title url isDraft headRefName baseRefName reviewDecision author { login } } }
    object(expression:$ref) { ... on Commit { statusCheckRollup { state } } }
  }
}"#;

pub fn github_repository(directory: &str, refresh: bool) -> Result<Value, String> {
    let local = snapshot(directory, false)?;
    let name = local.github.ok_or("no_github_remote")?;
    let head = local.head.unwrap_or_else(|| "HEAD".into());
    cached_remote(format!("repo:{name}:{head}"), refresh, || {
        let Some((owner, repo)) = name.split_once('/') else {
            return json!({"state":"unavailable"});
        };
        match graphql(
            REPO_QUERY,
            &[("owner", owner), ("name", repo), ("ref", &head)],
        ) {
            Ok(response) => {
                let repository = &response["data"]["repository"];
                if repository.is_null() {
                    return json!({"state":"not_found"});
                }
                let runs = gh(&[
                    "api".into(),
                    "--hostname".into(),
                    "github.com".into(),
                    "--method".into(),
                    "GET".into(),
                    format!("repos/{name}/actions/runs?per_page=10"),
                ]);
                let (workflows, workflows_error) = match runs {
                    Ok(value) => (value["workflow_runs"].as_array().map(|runs| runs.iter().map(|run| json!({
                        "id":run["id"],"title":run["display_title"],"name":run["name"],"url":run["html_url"],"branch":run["head_branch"],"status":run["status"],"conclusion":run["conclusion"],"updatedAt":run["updated_at"]
                    })).collect::<Vec<_>>()).unwrap_or_default(), None),
                    Err(error) => (Vec::new(), Some(error)),
                };
                json!({"state":"connected","repository":repository,"workflows":workflows,"workflowsError":workflows_error,"fetchedAt":timestamp()})
            }
            Err(error) => json!({"state":error,"fetchedAt":timestamp()}),
        }
    })
}

const ACCOUNT_QUERY: &str = r#"query {
  viewer {
    login name url avatarUrl
    contributionsCollection {
      contributionCalendar { totalContributions weeks { contributionDays { date contributionCount contributionLevel } } }
      commitContributionsByRepository(maxRepositories:10) { repository { nameWithOwner url isPrivate openGraphImageUrl usesCustomOpenGraphImage owner { avatarUrl(size:64) } } contributions { totalCount } }
    }
  }
}"#;

pub fn github_account(refresh: bool) -> Result<Value, String> {
    cached_remote("account".into(), refresh, || {
        match graphql(ACCOUNT_QUERY, &[]) {
            Ok(response) => {
                let viewer = &response["data"]["viewer"];
                if viewer.is_null() {
                    return json!({"state":"auth_required"});
                }
                let collection = &viewer["contributionsCollection"];
                let days: Vec<_> = collection["contributionCalendar"]["weeks"].as_array().into_iter().flatten()
                .flat_map(|week| week["contributionDays"].as_array().into_iter().flatten())
                .map(|day| json!({"date":day["date"],"count":day["contributionCount"],"level":match day["contributionLevel"].as_str().unwrap_or("") {
                    "FIRST_QUARTILE"=>1,"SECOND_QUARTILE"=>2,"THIRD_QUARTILE"=>3,"FOURTH_QUARTILE"=>4,_=>0
                }})).collect();
                json!({"state":"connected","login":viewer["login"],"name":viewer["name"],"url":viewer["url"],"avatarUrl":viewer["avatarUrl"],
                "totalContributions":collection["contributionCalendar"]["totalContributions"],"days":days,
                "topRepositories":collection["commitContributionsByRepository"],"fetchedAt":timestamp()})
            }
            Err(error) => json!({"state":error,"fetchedAt":timestamp()}),
        }
    })
}

const REFERENCE_QUERY: &str = r#"query($owner:String!,$name:String!,$number:Int!) {
  repository(owner:$owner,name:$name) {
    issueOrPullRequest(number:$number) {
      __typename
      ... on PullRequest {
        title state isDraft merged url body createdAt updatedAt headRefName baseRefName additions deletions changedFiles reviewDecision
        author { login } comments { totalCount } labels(first:6) { nodes { name color } }
        commits(last:1) { nodes { commit { statusCheckRollup { state } } } }
      }
      ... on Issue {
        title state stateReason url body createdAt updatedAt
        author { login } comments { totalCount } labels(first:6) { nodes { name color } }
      }
    }
  }
}"#;

fn reference_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
}

/// A pull request or issue mentioned in a chat, read through the GitHub CLI.
pub fn github_reference(
    owner: &str,
    repo: &str,
    number: u32,
    refresh: bool,
) -> Result<Value, String> {
    if !reference_name(owner) || !reference_name(repo) || number == 0 {
        return Err("invalid_reference".into());
    }
    cached_remote(format!("ref:{owner}/{repo}#{number}"), refresh, || {
        let response = gh(&[
            "api".into(),
            "--hostname".into(),
            "github.com".into(),
            "graphql".into(),
            "-f".into(),
            format!("query={REFERENCE_QUERY}"),
            "-f".into(),
            format!("owner={owner}"),
            "-f".into(),
            format!("name={repo}"),
            "-F".into(),
            format!("number={number}"),
        ]);
        let item = match response {
            Ok(response) => response["data"]["repository"]["issueOrPullRequest"].clone(),
            Err(error) => return json!({"state": error}),
        };
        if item.is_null() {
            return json!({"state": "not_found"});
        }
        let pull = item["__typename"] == "PullRequest";
        let status = if pull && item["merged"] == true {
            "merged"
        } else if item["state"] == "OPEN" {
            "open"
        } else {
            "closed"
        };
        let body: String = item["body"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(6000)
            .collect();
        json!({
            "state": "connected",
            "kind": if pull { "pull_request" } else { "issue" },
            "owner": owner, "repo": repo, "number": number,
            "title": item["title"], "status": status, "draft": item["isDraft"],
            "url": item["url"], "body": body,
            "author": item["author"]["login"],
            "labels": item["labels"]["nodes"],
            "comments": item["comments"]["totalCount"],
            "createdAt": item["createdAt"], "updatedAt": item["updatedAt"],
            "head": item["headRefName"], "base": item["baseRefName"],
            "additions": item["additions"], "deletions": item["deletions"], "changedFiles": item["changedFiles"],
            "review": item["reviewDecision"],
            "checks": item["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["state"],
            "fetchedAt": timestamp(),
        })
    })
}

#[cfg(test)]
mod reference_tests {
    use super::*;

    #[test]
    fn only_plain_owner_and_repository_names_are_queried() {
        assert!(reference_name("Lume-agents") && reference_name("Lume.app"));
        assert!(!reference_name("a/b") && !reference_name("a b") && !reference_name(""));
        assert_eq!(github_reference("a/b", "repo", 1, false).unwrap_err(), "invalid_reference");
        assert_eq!(github_reference("owner", "repo", 0, false).unwrap_err(), "invalid_reference");
    }

    #[test]
    #[ignore = "needs the GitHub CLI signed in"]
    fn live_pull_request_reference() {
        let value = github_reference("Lume-agents", "Lume", 26, true).expect("reference");
        assert_eq!(value["state"], "connected", "{value}");
    }
}

#[cfg(test)]
mod history_tests {
    use super::*;

    fn run(root: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .current_dir(root)
            .args(["-c", "user.name=Ana", "-c", "user.email=ana@example.com", "-c", "commit.gpgsign=false"])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .expect("git runs");
        assert!(status.status.success(), "{args:?}: {}", String::from_utf8_lossy(&status.stderr));
    }

    #[test]
    fn the_history_lists_every_branch_with_its_parents_refs_and_files() {
        let root = std::env::temp_dir().join(format!("lume-history-{}-{}", std::process::id(), timestamp()));
        std::fs::create_dir_all(&root).unwrap();
        run(&root, &["init", "-q", "-b", "main"]);
        std::fs::write(root.join("a.txt"), "one\n").unwrap();
        run(&root, &["add", "."]);
        run(&root, &["commit", "-q", "-m", "first"]);
        run(&root, &["checkout", "-q", "-b", "feature"]);
        std::fs::write(root.join("b.txt"), "two\nthree\n").unwrap();
        run(&root, &["add", "."]);
        run(&root, &["commit", "-q", "-m", "on a branch"]);
        run(&root, &["checkout", "-q", "main"]);
        std::fs::write(root.join("a.txt"), "one\nmore\n").unwrap();
        run(&root, &["commit", "-q", "-am", "on main"]);
        run(&root, &["merge", "-q", "--no-ff", "-m", "merge feature", "feature"]);
        run(&root, &["tag", "v1"]);
        let directory = root.to_string_lossy().into_owned();

        let commits = history(&directory, 0).unwrap();
        assert_eq!(commits.iter().map(|commit| commit.subject.as_str()).collect::<Vec<_>>().len(), 4);
        let merge = &commits[0];
        assert_eq!(merge.subject, "merge feature");
        assert_eq!(merge.parents.len(), 2, "a merge has two parents");
        assert!(merge.refs.contains(&"main".to_string()) && merge.refs.contains(&"v1".to_string()), "{:?}", merge.refs);
        assert!(commits.iter().any(|commit| commit.refs.contains(&"feature".to_string())));
        assert_eq!(commits.last().unwrap().parents.len(), 0, "the root commit has none");
        assert_eq!(history(&directory, 3).unwrap().len(), 1, "a page can start later");

        let on_branch = commits.iter().find(|commit| commit.subject == "on a branch").unwrap();
        let detail = commit_detail(&directory, &on_branch.oid).unwrap();
        assert_eq!(detail.message, "on a branch");
        assert_eq!(detail.files, [CommitFile { path: "b.txt".into(), added: Some(2), removed: Some(0) }]);
        let diff = commit_diff(&directory, &on_branch.oid, "b.txt").unwrap();
        assert!(diff.diff.contains("+three") && !diff.binary);
        assert!(commit_diff(&directory, &on_branch.oid, "../x").is_err());
        assert!(commit_diff(&directory, "--output=x", "b.txt").is_err());
        assert!(commit_detail(&directory, "not a hash").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_empty_repository_has_no_history_and_a_folder_that_is_not_one_is_refused() {
        let root = std::env::temp_dir().join(format!("lume-history-empty-{}-{}", std::process::id(), timestamp()));
        std::fs::create_dir_all(&root).unwrap();
        assert!(history(&root.to_string_lossy(), 0).is_err());
        run(&root, &["init", "-q"]);
        assert_eq!(history(&root.to_string_lossy(), 0).unwrap(), []);
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod action_tests {
    use super::*;

    fn run(root: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .current_dir(root)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .expect("git runs");
        assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
    }

    fn identity(root: &Path) {
        run(root, &["config", "user.name", "Ana"]);
        run(root, &["config", "user.email", "ana@example.com"]);
        run(root, &["config", "commit.gpgsign", "false"]);
    }

    fn commit_file(root: &Path, name: &str, text: &str) {
        std::fs::write(root.join(name), text).unwrap();
        run(root, &["add", "."]);
        run(root, &["commit", "-q", "-m", name]);
    }

    #[test]
    fn fetch_and_pull_only_move_a_branch_forward_and_refuse_a_diverged_one() {
        let base = std::env::temp_dir().join(format!("lume-actions-{}-{}", std::process::id(), timestamp()));
        std::fs::create_dir_all(&base).unwrap();
        let origin = base.join("origin.git");
        std::fs::create_dir_all(&origin).unwrap();
        run(&origin, &["init", "-q", "--bare", "-b", "main"]);
        let mine = base.join("mine");
        let other = base.join("other");
        let origin_path = origin.to_string_lossy().into_owned();
        run(&base, &["clone", "-q", &origin_path, "mine"]);
        identity(&mine);
        commit_file(&mine, "a.txt", "one\n");
        run(&mine, &["push", "-q", "-u", "origin", "main"]);
        run(&base, &["clone", "-q", &origin_path, "other"]);
        identity(&other);
        commit_file(&other, "b.txt", "two\n");
        run(&other, &["push", "-q", "origin", "main"]);
        let directory = mine.to_string_lossy().into_owned();

        assert_eq!(snapshot(&directory, true).unwrap().behind, 0, "nothing is known before the fetch");
        assert_eq!(fetch(&directory).unwrap().behind, 1);
        let pulled = pull(&directory).unwrap();
        assert_eq!((pulled.behind, pulled.ahead), (0, 0));
        assert!(mine.join("b.txt").exists());

        commit_file(&mine, "mine.txt", "local\n");
        commit_file(&other, "c.txt", "three\n");
        run(&other, &["push", "-q", "origin", "main"]);
        let head = git_text(&mine, &["rev-parse", "HEAD"]).unwrap();
        assert_eq!(pull(&directory).err().unwrap(), "diverged");
        assert_eq!(git_text(&mine, &["rev-parse", "HEAD"]).unwrap(), head, "a refused pull leaves the branch alone");
        assert!(!mine.join("c.txt").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn switching_needs_a_clean_tree_and_a_stash_can_be_applied_then_dropped() {
        let root = std::env::temp_dir().join(format!("lume-stash-{}-{}", std::process::id(), timestamp()));
        std::fs::create_dir_all(&root).unwrap();
        run(&root, &["init", "-q", "-b", "main"]);
        identity(&root);
        commit_file(&root, "a.txt", "one\n");
        run(&root, &["branch", "feature"]);
        let directory = root.to_string_lossy().into_owned();

        let listed = branches(&directory).unwrap();
        assert_eq!(listed.len(), 2);
        assert!(listed.iter().any(|branch| branch.name == "main" && branch.current));

        std::fs::write(root.join("a.txt"), "changed\n").unwrap();
        std::fs::write(root.join("new.txt"), "untracked\n").unwrap();
        assert_eq!(switch_branch(&directory, "feature").err().unwrap(), "dirty_tree");
        assert_eq!(switch_branch(&directory, "missing").err().unwrap(), "unknown_branch");
        assert_eq!(switch_branch(&directory, "--detach").err().unwrap(), "unknown_branch");

        let saved = stash_push(&directory, "before switching").unwrap();
        assert_eq!(saved.len(), 1);
        assert!(saved[0].message.contains("before switching"), "{:?}", saved[0]);
        assert!(!root.join("new.txt").exists(), "untracked files go away with the stash");
        let switched = switch_branch(&directory, "feature").unwrap();
        assert_eq!(switched.branch, "feature");

        assert_eq!(stash_apply(&directory, &saved[0].oid, 3).err().unwrap(), "stash_changed");
        assert_eq!(stash_drop(&directory, "not a hash", 0).err().unwrap(), "invalid_oid");
        let applied = stash_apply(&directory, &saved[0].oid, 0).unwrap();
        assert_eq!(applied.modified, 1);
        assert!(root.join("new.txt").exists());
        assert_eq!(stashes(&directory).unwrap().len(), 1, "applying keeps the stash");
        assert!(stash_drop(&directory, &saved[0].oid, 0).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
