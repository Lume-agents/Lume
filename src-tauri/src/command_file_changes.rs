//! Files a shell command changed. Edit tools report their own diffs, but a script run in the terminal
//! (`sed -i`, a Python rewrite, a code generator) only reports its output. Before the command runs, the
//! hook keeps a copy of the repository's changed files; after it runs, anything whose content moved is
//! reported as a file edit with its diff.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, SystemTime},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A working tree with more changes than this is being rewritten wholesale; listing it is noise.
const MAX_CHANGED_PATHS: usize = 400;
const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_SNAPSHOT_BYTES: usize = 12 * 1024 * 1024;
const MAX_DIFF_CHARS: usize = 48 * 1024;

/// Git commands rewrite the tree as git work (a stash, a checkout, a merge), not as edits.
const TREE_MOVING_GIT: &[&str] = &[
    "stash",
    "checkout",
    "switch",
    "reset",
    "restore",
    "merge",
    "rebase",
    "pull",
    "cherry-pick",
    "revert",
    "commit",
    "am",
    "clean",
];

#[derive(Serialize, Deserialize)]
struct Snapshot {
    root: String,
    /// Content of each file that differed from HEAD before the command; `None` when it was too large,
    /// binary or missing (deleted).
    files: BTreeMap<String, Option<String>>,
    /// Hash of each of those files, so a large one can still be compared.
    hashes: BTreeMap<String, String>,
}

/// The files the command changed and their combined unified diff.
pub(crate) struct CommandFileChanges {
    pub files: Vec<String>,
    pub diff: String,
}

fn snapshot_dir() -> PathBuf {
    std::env::temp_dir().join("lume-command-snapshots")
}

fn snapshot_path(key: &str) -> PathBuf {
    let digest = Sha256::digest(key.as_bytes());
    snapshot_dir().join(format!("{:x}.json", digest))
}

fn git(root: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let output = crate::executables::command("git")
        .ok()?
        .current_dir(root)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .args([
            "--no-pager",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.quotePath=false",
            "-c",
            "color.ui=false",
        ])
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    // `diff --no-index` exits 1 when the files differ.
    matches!(output.status.code(), Some(0 | 1)).then_some(output.stdout)
}

fn repository_root(cwd: &str) -> Option<PathBuf> {
    let output = git(Path::new(cwd), &["rev-parse", "--show-toplevel"])?;
    let root = String::from_utf8(output).ok()?.trim().to_string();
    (!root.is_empty()).then(|| PathBuf::from(root))
}

/// Paths (relative to the root) that differ from HEAD, untracked files included.
fn changed_paths(root: &Path) -> Option<Vec<String>> {
    let output = git(
        root,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--no-renames",
        ],
    )?;
    let paths: Vec<String> = output
        .split(|byte| *byte == 0)
        .filter(|entry| entry.len() > 3)
        .filter_map(|entry| String::from_utf8(entry[3..].to_vec()).ok())
        .collect();
    (paths.len() <= MAX_CHANGED_PATHS).then_some(paths)
}

fn read_text(path: &Path) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    if bytes.contains(&0) {
        return None;
    }
    String::from_utf8(bytes).ok()
}

fn hash_file(path: &Path) -> String {
    fs::read(path)
        .map(|bytes| format!("{:x}", Sha256::digest(&bytes)))
        .unwrap_or_else(|_| "missing".into())
}

fn head_text(root: &Path, relative: &str) -> Option<String> {
    let bytes = git(root, &["show", &format!("HEAD:{relative}")])?;
    (!bytes.contains(&0) && bytes.len() as u64 <= MAX_FILE_BYTES)
        .then(|| String::from_utf8(bytes).ok())
        .flatten()
}

fn head_hash(root: &Path, relative: &str) -> String {
    git(root, &["show", &format!("HEAD:{relative}")])
        .map(|bytes| format!("{:x}", Sha256::digest(&bytes)))
        .unwrap_or_else(|| "missing".into())
}

fn moves_the_tree(command: &str) -> bool {
    command
        .split(['&', ';', '|', '\n'])
        .map(str::trim)
        .filter_map(|segment| segment.strip_prefix("git "))
        .any(|rest| {
            // The subcommand is the first word that is not an option (`-c key=value` and `-C dir` take one).
            let mut words = rest.split_whitespace();
            while let Some(word) = words.next() {
                if word == "-c" || word == "-C" {
                    words.next();
                } else if !word.starts_with('-') {
                    return TREE_MOVING_GIT.contains(&word);
                }
            }
            false
        })
}

fn clear_stale_snapshots() {
    let Ok(entries) = fs::read_dir(snapshot_dir()) else {
        return;
    };
    let day = Duration::from_secs(24 * 60 * 60);
    for entry in entries.flatten() {
        let stale = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| age > day);
        if stale {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Before the command runs: remember the files that already differ from HEAD.
pub(crate) fn remember_before(key: &str, cwd: &str, command: &str) {
    if moves_the_tree(command) {
        return;
    }
    let Some(root) = repository_root(cwd) else {
        return;
    };
    let Some(paths) = changed_paths(&root) else {
        return;
    };
    clear_stale_snapshots();
    let mut files = BTreeMap::new();
    let mut hashes = BTreeMap::new();
    let mut budget = MAX_SNAPSHOT_BYTES;
    for relative in paths {
        let path = root.join(&relative);
        let text = read_text(&path).filter(|text| text.len() <= budget);
        if let Some(text) = &text {
            budget -= text.len();
        }
        hashes.insert(relative.clone(), hash_file(&path));
        files.insert(relative, text);
    }
    let snapshot = Snapshot {
        root: root.to_string_lossy().into_owned(),
        files,
        hashes,
    };
    if fs::create_dir_all(snapshot_dir()).is_ok() {
        if let Ok(json) = serde_json::to_vec(&snapshot) {
            let _ = fs::write(snapshot_path(key), json);
        }
    }
}

/// After the command ran: the files whose content it changed, with their diff.
pub(crate) fn changes_after(key: &str) -> Option<CommandFileChanges> {
    let path = snapshot_path(key);
    let snapshot: Snapshot = serde_json::from_slice(&fs::read(&path).ok()?).ok()?;
    let _ = fs::remove_file(&path);
    let root = PathBuf::from(&snapshot.root);
    let now = changed_paths(&root)?;
    let mut candidates: Vec<String> = snapshot.hashes.keys().cloned().collect();
    candidates.extend(
        now.into_iter()
            .filter(|relative| !snapshot.hashes.contains_key(relative)),
    );

    let mut files = Vec::new();
    let mut diff = String::new();
    for relative in candidates {
        let absolute = root.join(&relative);
        let before_hash = snapshot
            .hashes
            .get(&relative)
            .cloned()
            .unwrap_or_else(|| head_hash(&root, &relative));
        if before_hash == hash_file(&absolute) {
            continue;
        }
        let display = absolute.to_string_lossy().into_owned();
        let before = match snapshot.files.get(&relative) {
            Some(text) => text.clone(),
            None => head_text(&root, &relative),
        };
        let existed = before_hash != "missing";
        let exists = absolute.exists();
        if let Some(chunk) = file_diff(
            &display,
            existed.then_some(before).flatten().as_deref(),
            existed,
            exists.then(|| read_text(&absolute)).flatten().as_deref(),
            exists,
        ) {
            if diff.len() + chunk.len() <= MAX_DIFF_CHARS {
                diff.push_str(&chunk);
            }
        }
        files.push(display);
    }
    (!files.is_empty()).then_some(CommandFileChanges { files, diff })
}

/// A unified diff of one file, in the same shape the edit tools' diffs are recorded in.
fn file_diff(
    path: &str,
    before: Option<&str>,
    existed: bool,
    after: Option<&str>,
    exists: bool,
) -> Option<String> {
    let workspace = snapshot_dir().join(format!(
        "diff-{}-{:x}",
        std::process::id(),
        Sha256::digest(path.as_bytes())
    ));
    fs::create_dir_all(&workspace).ok()?;
    let written = fs::write(workspace.join("before"), before.unwrap_or_default())
        .and_then(|_| fs::write(workspace.join("after"), after.unwrap_or_default()));
    let output = written.ok().and_then(|_| {
        git(
            &workspace,
            &[
                "diff",
                "--no-index",
                "--no-color",
                "--unified=3",
                "before",
                "after",
            ],
        )
    });
    let _ = fs::remove_dir_all(&workspace);
    let output = String::from_utf8(output?).ok()?;
    let hunks = output.find("\n@@").map(|at| &output[at + 1..])?;
    let from = if existed {
        format!("a/{path}")
    } else {
        "/dev/null".into()
    };
    let to = if exists {
        format!("b/{path}")
    } else {
        "/dev/null".into()
    };
    let mut diff = format!("diff --git a/{path} b/{path}\n--- {from}\n+++ {to}\n{hunks}");
    if !diff.ends_with('\n') {
        diff.push('\n');
    }
    Some(diff)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(root: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .current_dir(root)
            .args(args)
            .status()
            .expect("git");
        assert!(status.success());
    }

    #[test]
    fn a_script_that_rewrites_a_file_is_reported_with_its_diff() {
        let repo = std::env::temp_dir().join(format!("lume-command-files-{}", std::process::id()));
        let _ = fs::remove_dir_all(&repo);
        fs::create_dir_all(&repo).unwrap();
        let root = repo.as_path();
        run(root, &["init", "-q"]);
        run(
            root,
            &[
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "init",
            ],
        );
        fs::write(root.join("clean.txt"), "one\n").unwrap();
        run(root, &["add", "clean.txt"]);
        run(
            root,
            &[
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "commit",
                "-q",
                "-m",
                "clean",
            ],
        );
        fs::write(root.join("dirty.txt"), "a\nb\n").unwrap();
        fs::write(root.join("untouched.txt"), "same\n").unwrap();

        let key = format!("test:{}", root.display());
        remember_before(&key, &root.to_string_lossy(), "python3 rewrite.py");
        fs::write(root.join("clean.txt"), "one\ntwo\n").unwrap();
        fs::write(root.join("dirty.txt"), "a\nB\n").unwrap();
        fs::write(root.join("new.txt"), "fresh\n").unwrap();

        let changes = changes_after(&key).expect("changes");
        let names: Vec<_> = changes
            .files
            .iter()
            .map(|path| {
                Path::new(path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(names, ["dirty.txt", "clean.txt", "new.txt"]);
        assert!(changes.diff.contains("+two"));
        assert!(changes.diff.contains("-b\n+B"));
        assert!(changes.diff.contains("--- /dev/null"));
        assert!(!changes.diff.contains("untouched"));
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn git_commands_that_move_the_tree_are_left_to_the_git_events() {
        assert!(moves_the_tree("cd repo && git stash"));
        assert!(moves_the_tree("git -c x=y checkout main"));
        assert!(!moves_the_tree("git status --short; python3 edit.py"));
    }
}
