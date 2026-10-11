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
