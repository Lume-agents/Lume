//! Files and folders a prompt can reference with `@`, as the agent CLIs do.

use std::{
    collections::{BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use serde::Serialize;

const CACHE_TTL: Duration = Duration::from_secs(15);
const MAX_PATHS: usize = 50_000;
const MAX_WALK_DEPTH: usize = 8;
const SKIPPED_DIRECTORIES: [&str; 8] = [
    ".git",
    "node_modules",
    "target",
    ".svelte-kit",
    "dist",
    "build",
    ".venv",
    "__pycache__",
];

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathMention {
    pub path: String,
    pub is_directory: bool,
}

pub fn search(directory: &str, query: &str, limit: usize) -> Result<Vec<PathMention>, String> {
    let paths = cached_paths(directory)?;
    Ok(rank(&paths, query, limit))
}

type PathCache = Mutex<HashMap<String, (Instant, Vec<PathMention>)>>;

fn cached_paths(directory: &str) -> Result<Vec<PathMention>, String> {
    static CACHE: OnceLock<PathCache> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some((_, paths)) = cache
        .lock()
        .ok()
        .and_then(|cache| cache.get(directory).cloned())
        .filter(|(created, _)| created.elapsed() < CACHE_TTL)
    {
        return Ok(paths);
    }
    let root = Path::new(directory);
    if !root.is_dir() {
        return Err("The session folder is not available".into());
    }
    let files = git_files(root).unwrap_or_else(|| walked_files(root));
    let paths = with_directories(files);
    if let Ok(mut cache) = cache.lock() {
        cache.insert(directory.to_string(), (Instant::now(), paths.clone()));
    }
    Ok(paths)
}

/// Tracked and untracked files, minus what .gitignore excludes, relative to `root`.
fn git_files(root: &Path) -> Option<Vec<String>> {
    let output = crate::repository::git(
        root,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    )
    .ok()?;
    Some(
        output
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
            .take(MAX_PATHS)
            .map(|path| String::from_utf8_lossy(path).into_owned())
            .collect(),
    )
}

fn walked_files(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    let mut pending = vec![(PathBuf::new(), 0usize)];
    while let Some((relative, depth)) = pending.pop() {
        let Ok(entries) = fs::read_dir(root.join(&relative)) else {
            continue;
        };
        for entry in entries.flatten() {
            if files.len() >= MAX_PATHS {
                return files;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = relative.join(&name);
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if depth < MAX_WALK_DEPTH && !SKIPPED_DIRECTORIES.contains(&name.as_str()) {
                    pending.push((path, depth + 1));
                }
            } else {
                files.push(path.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    files
}

fn with_directories(files: Vec<String>) -> Vec<PathMention> {
    let directories = files
        .iter()
        .flat_map(|file| {
            file.match_indices('/')
                .map(|(index, _)| format!("{}/", &file[..index]))
                .collect::<Vec<_>>()
        })
        .collect::<BTreeSet<_>>();
    directories
        .into_iter()
        .map(|path| PathMention {
            path,
            is_directory: true,
        })
        .chain(files.into_iter().map(|path| PathMention {
            path,
            is_directory: false,
        }))
        .collect()
}

/// Basename matches first, then path substrings, then in-order characters,
/// each favoring shorter paths; an empty query lists the top level.
fn rank(paths: &[PathMention], query: &str, limit: usize) -> Vec<PathMention> {
    let query = query.trim().trim_start_matches("./").to_lowercase();
    let mut scored = paths
        .iter()
        .filter_map(|mention| {
            let path = mention.path.to_lowercase();
            let depth = path.trim_end_matches('/').matches('/').count();
            if query.is_empty() {
                return (depth == 0).then_some((0, path.len(), mention));
            }
            let name = path
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or_default();
            let tier = if name.starts_with(&query) {
                0
            } else if name.contains(&query) {
                1
            } else if path.contains(&query) {
                2
            } else if is_subsequence(&query, &path) {
                3
            } else {
                return None;
            };
            Some((tier, path.len(), mention))
        })
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| {
        (left.0, left.1, &left.2.path).cmp(&(right.0, right.1, &right.2.path))
    });
    scored
        .into_iter()
        .take(limit)
        .map(|(_, _, mention)| mention.clone())
        .collect()
}

fn is_subsequence(needle: &str, haystack: &str) -> bool {
    let mut characters = haystack.chars();
    needle
        .chars()
        .all(|wanted| characters.by_ref().any(|character| character == wanted))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mentions() -> Vec<PathMention> {
        with_directories(
            [
                "README.md",
                "src/lib/WorkspaceSessionPane.svelte",
                "src/lib/promptHistory.ts",
                "src-tauri/src/lib.rs",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
        )
    }

    fn found(query: &str) -> Vec<String> {
        rank(&mentions(), query, 10)
            .into_iter()
            .map(|mention| mention.path)
            .collect()
    }

    #[test]
    fn directories_are_derived_from_files() {
        let directories = mentions()
            .into_iter()
            .filter(|mention| mention.is_directory)
            .map(|mention| mention.path)
            .collect::<Vec<_>>();
        assert_eq!(
            directories,
            vec!["src-tauri/", "src-tauri/src/", "src/", "src/lib/"]
        );
    }

    #[test]
    fn empty_query_lists_the_top_level() {
        assert_eq!(found(""), vec!["src/", "README.md", "src-tauri/"]);
    }

    #[test]
    fn basename_matches_rank_before_path_and_fuzzy_matches() {
        assert_eq!(found("lib")[..2], ["src/lib/", "src-tauri/src/lib.rs"]);
        assert_eq!(found("wsp"), vec!["src/lib/WorkspaceSessionPane.svelte"]);
        assert!(found("zzz").is_empty());
    }
}
