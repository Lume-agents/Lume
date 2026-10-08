use std::path::PathBuf;

pub fn is_codex_internal_workspace(path: &str) -> bool {
    let normalized = normalize(path);
    if normalized.contains("/.codex/memories") {
        return true;
    }
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .map(|root| normalize(&root.join("memories").to_string_lossy()))
        .is_some_and(|memories| {
            normalized == memories || normalized.starts_with(&format!("{memories}/"))
        })
}

/// The system temp folder, where tools start throwaway agent conversations.
pub fn is_temp_workspace(path: &str) -> bool {
    let normalized = normalize(path);
    if normalized.is_empty() {
        return false;
    }
    let mut roots = vec![
        "/tmp".to_string(),
        "/var/tmp".to_string(),
        "/private/tmp".to_string(),
    ];
    roots.push(normalize(&std::env::temp_dir().to_string_lossy()));
    roots
        .iter()
        .any(|root| normalized == *root || normalized.starts_with(&format!("{root}/")))
}

/// The home folder itself (not a project inside it).
pub fn is_home_workspace(path: &str) -> bool {
    let normalized = normalize(path);
    !normalized.is_empty()
        && ["HOME", "USERPROFILE"]
            .into_iter()
            .filter_map(std::env::var_os)
            .any(|home| normalize(&home.to_string_lossy()) == normalized)
}

fn normalize(path: &str) -> String {
    path.replace('\\', "/").trim_end_matches('/').to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_workspaces_are_the_temp_folders() {
        assert!(is_temp_workspace("/tmp"));
        assert!(is_temp_workspace(
            "/tmp/lume-native-handshake-18-1790644127901"
        ));
        assert!(is_temp_workspace("/var/tmp/"));
        assert!(!is_temp_workspace("/tmpfiles/project"));
        assert!(!is_temp_workspace("/home/user/Documents/Projetos/Lume"));
        assert!(!is_temp_workspace(""));
    }

    #[test]
    fn recognizes_only_the_internal_codex_memories_workspace() {
        assert!(is_codex_internal_workspace("/home/user/.codex/memories"));
        assert!(is_codex_internal_workspace(
            "C:\\Users\\user\\.codex\\memories\\rollout_summaries"
        ));
        assert!(!is_codex_internal_workspace(
            "/home/user/Documents/memories"
        ));
    }
}
