//! Optional, public repository popularity. Never uses agent credentials or delays catalog discovery.
use std::{collections::HashMap, sync::{Mutex, OnceLock}, time::{Duration, Instant}};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct RepositoryStars {
    pub repository: String,
    pub stars: Option<u64>,
}

#[derive(Default)]
struct Cache {
    entries: HashMap<String, (Instant, Option<u64>)>,
    budget: Option<(Instant, usize)>,
    blocked_until: Option<Instant>,
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Only explicit GitHub repositories: no inferred names, arbitrary hosts, credentials or API paths.
pub fn repository(value: &str) -> Option<String> {
    let value = value.trim();
    let path = if value.contains("://") {
        let url = tauri::Url::parse(value.strip_prefix("git+").unwrap_or(value)).ok()?;
        if !matches!(url.scheme(), "https" | "ssh") || url.host_str() != Some("github.com")
            || url.port().is_some() || url.password().is_some() || url.query().is_some() || url.fragment().is_some()
            || (!url.username().is_empty() && !(url.scheme() == "ssh" && url.username() == "git")) { return None; }
        url.path().to_owned()
    } else {
        value.strip_prefix("git@github.com:").unwrap_or(value).to_owned()
    };
    let path = path.trim_matches('/').strip_suffix(".git").unwrap_or(path.trim_matches('/'));
    let (owner, name) = path.split_once('/')?;
    let valid = |part: &str| !part.is_empty() && part.len() <= 100 && part != "." && part != ".."
        && part.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    (valid(owner) && owner.as_bytes()[0].is_ascii_alphanumeric() && valid(name)).then(|| format!("{owner}/{name}").to_ascii_lowercase())
}

/// One bounded background batch, deduplicated by repository. Unknown is never reported as zero.
pub async fn stars(repositories: Vec<String>) -> Vec<RepositoryStars> {
    // Match the updater's existing Ring provider instead of adding a second TLS backend.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let Ok(client) = reqwest::Client::builder().https_only(true)
        .redirect(reqwest::redirect::Policy::none()).connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(5)).user_agent("Lume/plugin-repository-stars").build() else { return Vec::new(); };
    let mut names: Vec<_> = repositories.iter().take(40).filter_map(|name| repository(name)).collect();
    names.sort(); names.dedup();
    let mut result = Vec::new();
    let mut jobs = Vec::new();
    if let Ok(mut cache) = cache().lock() { cache.entries.retain(|_, (expires, _)| *expires > Instant::now()); }
    for name in names {
        let request = {
            let Ok(mut cache) = cache().lock() else { continue; };
            if let Some((_, value)) = cache.entries.get(&name).filter(|(expires, _)| *expires > Instant::now()) {
                result.push(RepositoryStars { repository: name, stars: *value });
                continue;
            }
            let allowed = if cache.blocked_until.is_some_and(|until| until > Instant::now()) { false }
            else {
                let (start, used) = cache.budget.get_or_insert((Instant::now(), 0));
                if start.elapsed() >= Duration::from_secs(3600) { *start = Instant::now(); *used = 0; }
                if *used >= 40 { false } else { *used += 1; true }
            };
            if allowed {
                // Atomic reservation also deduplicates simultaneous panels.
                cache.entries.insert(name.clone(), (Instant::now() + Duration::from_secs(30 * 60), None));
            }
            allowed
        };
        if !request { result.push(RepositoryStars { repository: name, stars: None }); continue; }
        let client = client.clone();
        jobs.push(tauri::async_runtime::spawn(async move {
            let response = client.get(format!("https://api.github.com/repos/{name}"))
                .header("Accept", "application/vnd.github+json").send().await;
            let mut count = None;
            if let Ok(mut response) = response {
                if matches!(response.status().as_u16(), 403 | 429) {
                    if let Ok(mut cache) = cache().lock() { cache.blocked_until = Some(Instant::now() + Duration::from_secs(3600)); }
                } else if response.status().is_success() && response.content_length().is_none_or(|size| size <= 128 * 1024) {
                    let mut bytes = Vec::new();
                    while let Ok(Some(chunk)) = response.chunk().await {
                        if bytes.len() + chunk.len() > 128 * 1024 { bytes.clear(); break; }
                        bytes.extend_from_slice(&chunk);
                    }
                    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                        count = public_stars(&value, &name);
                    }
                }
            }
            if let Ok(mut cache) = cache().lock() {
                cache.entries.insert(name.clone(), (Instant::now() + Duration::from_secs(if count.is_some() { 12 * 3600 } else { 30 * 60 }), count));
            }
            RepositoryStars { repository: name, stars: count }
        }));
        // Four concurrent requests at most, without holding the cache lock across I/O.
        if jobs.len() == 4 {
            for job in jobs.drain(..) { if let Ok(value) = job.await { result.push(value); } }
        }
    }
    for job in jobs { if let Ok(value) = job.await { result.push(value); } }
    result
}

fn public_stars(value: &serde_json::Value, name: &str) -> Option<u64> {
    if value["private"].as_bool() != Some(false)
        || value["full_name"].as_str().and_then(repository).as_deref() != Some(name) { return None; }
    value["stargazers_count"].as_u64()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_client_uses_the_existing_tls_provider_without_network_for_empty_input() {
        assert!(tauri::async_runtime::block_on(stars(Vec::new())).is_empty());
    }
    #[test]
    fn only_explicit_github_repositories_are_accepted() {
        for value in ["OpenAI/plugins", "https://github.com/OpenAI/plugins.git", "git+https://github.com/OpenAI/plugins.git", "git@github.com:OpenAI/plugins.git", "ssh://git@github.com/OpenAI/plugins"] {
            assert_eq!(repository(value).as_deref(), Some("openai/plugins"), "{value}");
        }
        for value in ["plugins", "https://example.com/openai/plugins", "https://github.com", "https://github.com/openai/plugins/tree/main", "https://github.com/openai/plugins?token=secret", "https://user:password@github.com/openai/plugins", "http://github.com/openai/plugins", "owner/../secret", "owner/repo/extra", "owner/%2e%2e", "https://api.github.com/repos/openai/plugins", "--option/repo"] {
            assert!(repository(value).is_none(), "{value}");
        }
    }

    #[test]
    fn counts_must_belong_to_the_public_repository_and_zero_is_real() {
        let mut value = serde_json::json!({"full_name":"Owner/Plugin","private":false,"stargazers_count":0});
        assert_eq!(public_stars(&value, "owner/plugin"), Some(0));
        value["stargazers_count"] = 1234.into();
        assert_eq!(public_stars(&value, "owner/plugin"), Some(1234));
        assert_eq!(public_stars(&value, "owner/other"), None);
        value["private"] = true.into();
        assert_eq!(public_stars(&value, "owner/plugin"), None);
        assert_eq!(public_stars(&serde_json::json!({"message":"API rate limit exceeded"}), "owner/plugin"), None);
    }
}
