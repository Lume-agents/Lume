//! What each coding agent has been extended with — plugins, skills, MCP servers, instruction files,
//! custom commands and subagents — read from the places the agent itself reads them, and the plugin
//! catalogs of their marketplaces. Plugins are installed, removed and toggled through each agent's own
//! CLI, so Lume never edits an agent's configuration by hand.

use std::{
    collections::HashMap,
    env, fs,
    io::Read,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};

use serde::Serialize;
use serde_json::Value;
use base64::Engine;

use crate::{agent_commands::frontmatter, agent_mcp};

const CLI_TIMEOUT: Duration = Duration::from_secs(180);
const CATALOG_TTL: Duration = Duration::from_secs(10 * 60);
const MAX_PREVIEW_BYTES: u64 = 512 * 1024;

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionItem {
    /// Unique within the agent: kind plus path or plugin id.
    pub id: String,
    /// `plugin`, `skill`, `mcp`, `instruction`, `command` or `subagent`.
    pub kind: String,
    pub name: String,
    pub description: String,
    /// `user`, `project`, `local`, `account` or the plugin's marketplace scope.
    pub scope: String,
    /// The file that defines it, when there is one to open.
    pub path: Option<String>,
    pub enabled: Option<bool>,
    pub version: Option<String>,
    /// Where it came from: a marketplace, or how an MCP server is reached.
    pub source: Option<String>,
    pub icon_url: Option<String>,
    pub icon_dark_url: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentExtensions {
    pub agent: String,
    pub label: String,
    /// The agent's CLI is installed (needed to manage plugins).
    pub cli: bool,
    /// Plugins can be installed from a catalog and removed.
    pub manages_plugins: bool,
    /// Installed plugins can be switched on and off.
    pub toggles_plugins: bool,
    pub items: Vec<ExtensionItem>,
    /// Sources that could not be read, shown rather than hidden.
    pub problems: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogPlugin {
    pub id: String,
    pub name: String,
    pub description: String,
    pub marketplace: String,
    pub category: Option<String>,
    pub installs: Option<u64>,
    pub downloads: Option<u64>,
    pub rating: Option<f64>,
    pub updated_at: Option<i64>,
    pub icon_url: Option<String>,
    pub icon_dark_url: Option<String>,
    pub github_repository: Option<String>,
    pub installed: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginActionResult {
    pub ok: bool,
    pub message: String,
    /// A command the marketplace runs to install the plugin; it runs only after the person accepts it.
    pub confirm: Option<PendingCommand>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingCommand {
    pub command: String,
    pub sha256: String,
}

struct AgentSpec {
    id: &'static str,
    label: &'static str,
    cli: &'static str,
    config: &'static str,
}

const AGENTS: &[AgentSpec] = &[
    AgentSpec {
        id: "claude",
        label: "Claude Code",
        cli: "claude",
        config: ".claude",
    },
    AgentSpec {
        id: "codex",
        label: "Codex",
        cli: "codex",
        config: ".codex",
    },
    AgentSpec {
        id: "gemini",
        label: "Gemini CLI",
        cli: "gemini",
        config: ".gemini",
    },
    AgentSpec {
        id: "opencode",
        label: "OpenCode",
        cli: "opencode",
        config: ".config/opencode",
    },
    AgentSpec { id: "antigravity", label: "Antigravity", cli: "agy", config: ".gemini/antigravity-cli" },
];

fn home() -> Option<PathBuf> {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn codex_home() -> Option<PathBuf> {
    env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| home().map(|home| home.join(".codex")))
}

/// Every agent that is installed or has been configured on this machine, with what extends it.
pub fn list(project: Option<&str>) -> Vec<AgentExtensions> {
    let Some(home) = home() else {
        return Vec::new();
    };
    let project = project.map(Path::new).filter(|path| path.is_dir());
    AGENTS
        .iter()
        .filter_map(|spec| {
            let cli = crate::executables::available(spec.cli);
            if !cli && !home.join(spec.config).is_dir() {
                return None;
            }
            let mut agent = AgentExtensions {
                agent: spec.id.into(),
                label: spec.label.into(),
                cli,
                manages_plugins: cli && matches!(spec.id, "claude" | "codex" | "antigravity"),
                toggles_plugins: cli && matches!(spec.id, "claude" | "gemini" | "antigravity"),
                items: Vec::new(),
                problems: Vec::new(),
            };
            match spec.id {
                "claude" => claude(&home, project, &mut agent),
                "codex" => codex(&home, project, &mut agent),
                "gemini" => gemini(&home, project, &mut agent),
                "antigravity" => antigravity(&home, project, &mut agent),
                _ => opencode(&home, project, &mut agent),
            }
            Some(agent)
        })
        .collect()
}

fn claude(home: &Path, project: Option<&Path>, agent: &mut AgentExtensions) {
    if agent.cli {
        match run_cli(
            "claude",
            &["plugin", "list", "--json"],
            Duration::from_secs(60),
        ) {
            Ok(output) => agent.items.extend(installed_plugins(&output)),
            Err(error) => agent.problems.push(format!("plugins: {error}")),
        }
    }
    let user = home.join(".claude");
    skills_in(&user.join("skills"), "user", agent);
    // Skills synced from claude.ai sit one level deeper, per account.
    for synced in sorted_dirs(&user.join("skills/synced")) {
        skills_in(&synced, "account", agent);
    }
    mcp(
        agent_mcp::claude_configured(project.and_then(Path::to_str)),
        agent,
    );
    instruction(&user.join("CLAUDE.md"), "user", agent);
    markdown_items(&user.join("rules"), "instruction", "user", agent);
    markdown_items(&user.join("commands"), "command", "user", agent);
    markdown_items(&user.join("agents"), "subagent", "user", agent);
    if let Some(project) = project {
        let local = project.join(".claude");
        skills_in(&local.join("skills"), "project", agent);
        for file in [project.join("CLAUDE.md"), local.join("CLAUDE.md")] {
            instruction(&file, "project", agent);
        }
        instruction(&project.join("CLAUDE.local.md"), "local", agent);
        markdown_items(&local.join("rules"), "instruction", "project", agent);
        markdown_items(&local.join("commands"), "command", "project", agent);
        markdown_items(&local.join("agents"), "subagent", "project", agent);
    }
}

fn codex(home: &Path, project: Option<&Path>, agent: &mut AgentExtensions) {
    if agent.cli {
        match run_cli(
            "codex",
            &["plugin", "list", "--json"],
            Duration::from_secs(60),
        ) {
            Ok(output) => agent.items.extend(installed_plugins(&output)),
            Err(error) => agent.problems.push(format!("plugins: {error}")),
        }
    }
    let user = codex_home().unwrap_or_else(|| home.join(".codex"));
    skills_in(&user.join("skills"), "user", agent);
    // ~/.agents/skills is shared by the agents that follow the open skills layout.
    skills_in(&home.join(".agents/skills"), "user", agent);
    mcp(
        agent_mcp::codex_configured(project.and_then(Path::to_str)),
        agent,
    );
    instruction(&user.join("AGENTS.md"), "user", agent);
    markdown_items(&user.join("prompts"), "command", "user", agent);
    if let Some(project) = project {
        skills_in(&project.join(".agents/skills"), "project", agent);
        skills_in(&project.join(".codex/skills"), "project", agent);
        instruction(&project.join("AGENTS.md"), "project", agent);
        instruction(&project.join("AGENTS.override.md"), "project", agent);
    }
}

fn gemini(home: &Path, project: Option<&Path>, agent: &mut AgentExtensions) {
    let user = home.join(".gemini");
    for directory in sorted_dirs(&user.join("extensions")) {
        let Some(manifest) = read_json(&directory.join("gemini-extension.json")) else {
            continue;
        };
        let name = text(&manifest, "name").unwrap_or_else(|| file_name(&directory));
        agent.items.push(ExtensionItem {
            id: format!("plugin:{name}"),
            kind: "plugin".into(),
            description: text(&manifest, "description").unwrap_or_default(),
            name,
            scope: "user".into(),
            path: Some(
                directory
                    .join("gemini-extension.json")
                    .to_string_lossy()
                    .into_owned(),
            ),
            version: text(&manifest, "version"),
            ..Default::default()
        });
    }
    skills_in(&user.join("skills"), "user", agent);
    mcp(
        agent_mcp::gemini_configured(project.and_then(Path::to_str)),
        agent,
    );
    instruction(&user.join("GEMINI.md"), "user", agent);
    toml_commands(&user.join("commands"), "user", agent);
    if let Some(project) = project {
        skills_in(&project.join(".gemini/skills"), "project", agent);
        instruction(&project.join("GEMINI.md"), "project", agent);
        toml_commands(&project.join(".gemini/commands"), "project", agent);
    }
}

fn opencode(home: &Path, project: Option<&Path>, agent: &mut AgentExtensions) {
    let user = home.join(".config/opencode");
    for name in ["skill", "skills"] {
        skills_in(&user.join(name), "user", agent);
    }
    mcp(
        agent_mcp::opencode_configured(project.and_then(Path::to_str)),
        agent,
    );
    instruction(&user.join("AGENTS.md"), "user", agent);
    for name in ["command", "commands"] {
        markdown_items(&user.join(name), "command", "user", agent);
    }
    for name in ["agent", "agents"] {
        markdown_items(&user.join(name), "subagent", "user", agent);
    }
    if let Some(project) = project {
        instruction(&project.join("AGENTS.md"), "project", agent);
        for name in ["skill", "skills"] {
            skills_in(&project.join(".opencode").join(name), "project", agent);
        }
        for name in ["command", "commands"] {
            markdown_items(
                &project.join(".opencode").join(name),
                "command",
                "project",
                agent,
            );
        }
    }
}

fn antigravity(home: &Path, project: Option<&Path>, agent: &mut AgentExtensions) {
    let user = home.join(".gemini/antigravity-cli");
    if agent.cli {
        match run_cli("agy", &["plugin", "list", "--json"], Duration::from_secs(5)) {
            Ok(output) => agent.items.extend(installed_plugins(&output)),
            Err(error) => agent.problems.push(format!("plugins: {error}")),
        }
    }
    for directory in sorted_dirs(&user.join("plugins")) {
        let manifest_path = directory.join("plugin.json");
        let Some(manifest) = read_json(&manifest_path) else { continue };
        let name = text(&manifest, "name").unwrap_or_else(|| file_name(&directory));
        if !agent.items.iter().any(|item| item.kind == "plugin" && item.name == name) {
            agent.items.push(ExtensionItem {
                id: format!("plugin:{name}"), kind: "plugin".into(), name,
                description: text(&manifest, "description").unwrap_or_default(),
                scope: "user".into(), path: Some(manifest_path.to_string_lossy().into_owned()),
                version: text(&manifest, "version"), ..Default::default()
            });
        }
        skills_in(&directory.join("skills"), "plugin", agent);
    }
    skills_in(&user.join("skills"), "user", agent);
    instruction(&user.join("GEMINI.md"), "user", agent);
    markdown_items(&user.join("agents"), "subagent", "user", agent);
    markdown_items(&user.join("rules"), "instruction", "user", agent);
    if let Some(project) = project {
        skills_in(&project.join(".agents/skills"), "project", agent);
        skills_in(&project.join(".gemini/skills"), "project", agent);
    }
}

fn mcp(servers: Vec<agent_mcp::McpServer>, agent: &mut AgentExtensions) {
    for server in servers {
        agent.items.push(ExtensionItem {
            id: format!("mcp:{}:{}", server.scope, server.name),
            kind: "mcp".into(),
            description: server.target.clone(),
            enabled: Some(server.status != "disabled"),
            source: Some(server.transport),
            name: server.name,
            scope: server.scope,
            ..Default::default()
        });
    }
}

fn instruction(path: &Path, scope: &str, agent: &mut AgentExtensions) {
    let Ok(meta) = fs::metadata(path) else { return };
    if !meta.is_file() {
        return;
    }
    let description = fs::read_to_string(path)
        .ok()
        .and_then(|text| {
            text.lines()
                .map(str::trim)
                .find(|line| !line.is_empty() && *line != "---")
                .map(|line| line.trim_start_matches('#').trim().to_string())
        })
        .unwrap_or_default();
    push_file(
        agent,
        "instruction",
        file_name(path),
        description,
        scope,
        path,
    );
}

fn skills_in(directory: &Path, scope: &str, agent: &mut AgentExtensions) {
    for skill in sorted_dirs(directory) {
        let file = skill.join("SKILL.md");
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        let fields = frontmatter(&text);
        let name = fields
            .get("name")
            .cloned()
            .unwrap_or_else(|| file_name(&skill));
        let description = fields.get("description").cloned().unwrap_or_default();
        push_file(agent, "skill", name, description, scope, &file);
    }
}

/// Markdown files with a `description` in their frontmatter: commands, subagents and rules.
fn markdown_items(directory: &Path, kind: &str, scope: &str, agent: &mut AgentExtensions) {
    for file in files_with(directory, "md", 2) {
        let text = fs::read_to_string(&file).unwrap_or_default();
        let fields = frontmatter(&text);
        let stem = file
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let name = fields.get("name").cloned().unwrap_or(stem);
        let description = fields.get("description").cloned().unwrap_or_default();
        push_file(agent, kind, name, description, scope, &file);
    }
}

/// Gemini's custom commands: a TOML file per command, its name from the path (`git/commit.toml` is `/git:commit`).
fn toml_commands(directory: &Path, scope: &str, agent: &mut AgentExtensions) {
    for file in files_with(directory, "toml", 3) {
        let name = file
            .strip_prefix(directory)
            .ok()
            .map(|relative| {
                relative
                    .with_extension("")
                    .to_string_lossy()
                    .replace(['/', '\\'], ":")
            })
            .unwrap_or_default();
        let description = fs::read_to_string(&file)
            .ok()
            .and_then(|text| {
                text.lines()
                    .find_map(|line| line.trim().strip_prefix("description").map(str::trim))
                    .and_then(|rest| rest.strip_prefix('='))
                    .map(|value| value.trim().trim_matches(['"', '\'']).to_string())
            })
            .unwrap_or_default();
        push_file(agent, "command", name, description, scope, &file);
    }
}

fn push_file(
    agent: &mut AgentExtensions,
    kind: &str,
    name: String,
    description: String,
    scope: &str,
    path: &Path,
) {
    let path = path.to_string_lossy().into_owned();
    if agent
        .items
        .iter()
        .any(|item| item.path.as_deref() == Some(path.as_str()))
    {
        return;
    }
    agent.items.push(ExtensionItem {
        id: format!("{kind}:{path}"),
        kind: kind.into(),
        name,
        description,
        scope: scope.into(),
        path: Some(path),
        ..Default::default()
    });
}

/// The installed plugins in a `plugin list --json` answer (Claude Code and Codex share its shape).
fn installed_plugins(output: &str) -> Vec<ExtensionItem> {
    let Ok(value) = serde_json::from_str::<Value>(output) else {
        return Vec::new();
    };
    let list = value
        .get("installed")
        .or(Some(&value))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    list.iter()
        .filter_map(|plugin| {
            let id = text(plugin, "pluginId").or_else(|| text(plugin, "id")).or_else(|| text(plugin, "name"))?;
            Some(ExtensionItem {
                id: format!("plugin:{id}"),
                kind: "plugin".into(),
                name: text(plugin, "name")
                    .unwrap_or_else(|| id.split('@').next().unwrap_or(&id).to_string()),
                description: text(plugin, "description").unwrap_or_default(),
                scope: text(plugin, "scope").unwrap_or_else(|| "user".into()),
                path: text(plugin, "installPath").or_else(|| text(plugin, "path")),
                enabled: plugin.get("enabled").and_then(Value::as_bool),
                version: text(plugin, "version"),
                source: text(plugin, "marketplaceName")
                    .or_else(|| text(plugin, "source"))
                    .or_else(|| id.split_once('@').map(|(_, market)| market.to_string())),
                icon_url: installed_icon(plugin, false),
                icon_dark_url: installed_icon(plugin, true),
            })
        })
        .collect()
}

type CatalogCache = Mutex<HashMap<String, (Instant, Vec<CatalogPlugin>)>>;

fn catalog_cache() -> &'static CatalogCache {
    static CACHE: OnceLock<CatalogCache> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The plugins an agent's marketplaces offer, most installed first.
pub fn catalog(agent: &str, refresh: bool) -> Result<Vec<CatalogPlugin>, String> {
    if !refresh {
        if let Some((at, plugins)) = catalog_cache()
            .lock()
            .ok()
            .and_then(|cache| cache.get(agent).cloned())
        {
            if at.elapsed() < CATALOG_TTL {
                return Ok(plugins);
            }
        }
    }
    let output = match agent {
        "claude" => run_cli(
            "claude",
            &["plugin", "list", "--json", "--available"],
            CLI_TIMEOUT,
        )?,
        "codex" => run_cli("codex", &["plugin", "list", "--json", "--available"], CLI_TIMEOUT)?,
        "antigravity" => {
            let root = home().ok_or("Home directory is unavailable")?;
            let path = root.join(".gemini/antigravity-cli/cache/marketplaces/antigravity-plugins-official/marketplace.json");
            let manifest = read_json(&path).ok_or("O catálogo do Antigravity ainda não está disponível neste computador. Abra /plugins na CLI para sincronizar o marketplace oficial.")?;
            return Ok(antigravity_catalog(&manifest));
        }
        _ => return Ok(Vec::new()),
    };
    let plugins = parse_catalog(&output);
    if let Ok(mut cache) = catalog_cache().lock() {
        cache.insert(agent.into(), (Instant::now(), plugins.clone()));
    }
    Ok(plugins)
}

fn parse_catalog(output: &str) -> Vec<CatalogPlugin> {
    let Ok(value) = serde_json::from_str::<Value>(output) else {
        return Vec::new();
    };
    let installed: Vec<String> = value
        .get("installed")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|plugin| text(plugin, "pluginId"))
        .collect();
    let installed_images: HashMap<String, (Option<String>, Option<String>)> = value.get("installed")
        .and_then(Value::as_array).into_iter().flatten().filter_map(|plugin| {
            Some((text(plugin, "pluginId")?, (installed_icon(plugin, false), installed_icon(plugin, true))))
        }).collect();
    let mut repositories = cached_marketplace_repositories(&value);
    for plugin in value.get("installed").and_then(Value::as_array).into_iter().flatten() {
        if let (Some(id), Some(repository)) = (text(plugin, "pluginId"), plugin_repository(plugin).or_else(|| installed_repository(plugin))) {
            repositories.insert(id, repository);
        }
    }
    let mut plugins: Vec<CatalogPlugin> = value
        .get("available")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|plugin| {
            let id = text(plugin, "pluginId")?;
            Some(CatalogPlugin {
                name: text(plugin, "name").unwrap_or_else(|| id.clone()),
                description: text(plugin, "description").unwrap_or_default(),
                marketplace: text(plugin, "marketplaceName").unwrap_or_default(),
                category: text(plugin, "category"),
                installs: plugin.get("installCount").and_then(Value::as_u64),
                downloads: plugin_count(plugin, &["downloadCount", "downloads"]),
                rating: plugin_rating(plugin),
                updated_at: plugin_date(plugin),
                icon_url: plugin_icon(plugin).or_else(|| installed_images.get(&id).and_then(|icons| icons.0.clone())),
                icon_dark_url: plugin_icon_fields(plugin, &["iconDarkUrl", "logoDark"]).or_else(|| installed_images.get(&id).and_then(|icons| icons.1.clone())),
                github_repository: plugin_repository(plugin).or_else(|| repositories.get(&id).cloned()),
                installed: installed.contains(&id)
                    || plugin.get("installed").and_then(Value::as_bool) == Some(true),
                id,
            })
        })
        .collect();
    plugins.sort_by(|left, right| {
        right
            .installs
            .unwrap_or(0)
            .cmp(&left.installs.unwrap_or(0))
            .then_with(|| left.name.cmp(&right.name))
    });
    plugins
}

fn antigravity_catalog(manifest: &Value) -> Vec<CatalogPlugin> {
    manifest.get("plugins").and_then(Value::as_array).into_iter().flatten().filter_map(|plugin| {
        let name = text(plugin, "name")?;
        if !valid_antigravity_name(&name) { return None; }
        Some(CatalogPlugin {
            id: format!("{name}@antigravity-plugins-official"),
            name: text(plugin, "displayName").unwrap_or(name),
            description: text(plugin, "description").unwrap_or_default(),
            marketplace: "antigravity-plugins-official".into(),
            category: text(plugin, "category"),
            installs: plugin_count(plugin, &["installCount", "installs"]),
            downloads: plugin_count(plugin, &["downloadCount", "downloads"]),
            rating: plugin_rating(plugin), updated_at: plugin_date(plugin),
            icon_url: plugin_icon(plugin), installed: false,
            icon_dark_url: plugin_icon_fields(plugin, &["iconDarkUrl", "logoDark"]),
            github_repository: plugin_repository(plugin),
        })
    }).collect()
}

fn valid_antigravity_name(name: &str) -> bool {
    name.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
        && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn plugin_repository(plugin: &Value) -> Option<String> {
    let repository = &plugin["repository"];
    repository.as_str().or_else(|| repository["url"].as_str())
        .and_then(crate::extension_github::repository)
        .or_else(|| plugin["source"]["url"].as_str().and_then(crate::extension_github::repository))
        .or_else(|| (plugin["source"]["source"] == "github")
            .then(|| plugin["source"]["repo"].as_str().and_then(crate::extension_github::repository)).flatten())
}

fn cached_marketplace_repositories(value: &Value) -> HashMap<String, String> {
    let mut repositories = HashMap::new();
    let Some(user) = home() else { return repositories; };
    let Ok(base) = user.join(".claude/plugins/marketplaces").canonicalize() else { return repositories; };
    let marketplaces: std::collections::HashSet<_> = value["available"].as_array().into_iter().flatten()
        .filter_map(|plugin| plugin["marketplaceName"].as_str())
        .filter(|name| !name.is_empty() && *name != "." && *name != ".." && name.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))).collect();
    for marketplace in marketplaces {
        let Ok(path) = base.join(marketplace).join(".claude-plugin/marketplace.json").canonicalize() else { continue; };
        if !path.starts_with(&base) || !fs::metadata(&path).is_ok_and(|meta| meta.len() <= 8 * 1024 * 1024) { continue; }
        let Some(manifest) = read_json(&path) else { continue; };
        if manifest["name"].as_str() != Some(marketplace) { continue; }
        for plugin in manifest["plugins"].as_array().into_iter().flatten() {
            if let (Some(name), Some(repository)) = (plugin["name"].as_str(), plugin_repository(plugin)) {
                repositories.insert(format!("{name}@{marketplace}"), repository);
            }
        }
    }
    repositories
}

fn installed_repository(plugin: &Value) -> Option<String> {
    let roots = [home()?.join(".claude/plugins/cache"), codex_home()?.join("plugins/cache")];
    installed_manifests_in(plugin, &roots).iter().find_map(|(_, manifest)| plugin_repository(manifest))
}

fn plugin_count(plugin: &Value, fields: &[&str]) -> Option<u64> {
    fields.iter().find_map(|field| plugin.get(*field).and_then(Value::as_u64))
}

fn plugin_rating(plugin: &Value) -> Option<f64> {
    ["averageRating", "rating"].iter().find_map(|field| plugin.get(*field).and_then(Value::as_f64))
        .filter(|rating| rating.is_finite() && *rating >= 0.0)
}

fn plugin_date(plugin: &Value) -> Option<i64> {
    ["updatedAt", "lastUpdated", "publishedAt", "createdAt"].iter().find_map(|field| {
        let value = plugin.get(*field)?;
        if let Some(stamp) = value.as_i64().filter(|stamp| *stamp > 0) {
            return if stamp < 100_000_000_000 { stamp.checked_mul(1000) } else { Some(stamp) };
        }
        chrono::DateTime::parse_from_rfc3339(value.as_str()?).ok().map(|date| date.timestamp_millis())
    })
}

fn public_icon_host(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']).trim_end_matches('.');
    if host == "localhost" || [".localhost", ".local", ".internal"].iter().any(|suffix| host.ends_with(suffix)) {
        return false;
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        let ipv4 = match ip {
            std::net::IpAddr::V4(ip) => Some(ip),
            std::net::IpAddr::V6(ip) => {
                if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() || ip.is_unique_local() || ip.is_unicast_link_local() { return false; }
                ip.to_ipv4_mapped()
            }
        };
        return ipv4.is_none_or(|ip| !ip.is_loopback() && !ip.is_private() && !ip.is_link_local() && !ip.is_unspecified() && !ip.is_multicast() && !ip.is_broadcast() && ip.octets()[0] != 0);
    }
    host.contains('.')
}

fn plugin_icon(plugin: &Value) -> Option<String> {
    plugin_icon_fields(plugin, &["iconUrl", "logoUrl", "logo", "icon", "iconSmall", "iconLarge", "composerIcon"])
}

fn plugin_icon_fields(plugin: &Value, fields: &[&str]) -> Option<String> {
    [Some(plugin), plugin.get("interface")].into_iter().flatten().find_map(|metadata| {
        fields.iter().filter_map(|field| metadata.get(*field).and_then(Value::as_str)).find_map(|value| {
            if value.len() > 4096 { return None; }
            let url = tauri::Url::parse(value).ok()?;
            (url.scheme() == "https" && url.username().is_empty() && url.password().is_none()
                && url.host_str().is_some_and(public_icon_host)).then(|| url.to_string())
        })
    })
}

fn installed_icon(plugin: &Value, dark: bool) -> Option<String> {
    let fields: &[&str] = if dark { &["iconDarkUrl", "logoDark"] } else { &["iconUrl", "logoUrl", "logo", "icon", "iconSmall", "composerIcon"] };
    if let Some(url) = plugin_icon_fields(plugin, fields) { return Some(url); }
    let user = home()?;
    let roots = [user.join(".claude/plugins/cache"), codex_home()?.join("plugins/cache")];
    installed_icon_in(plugin, dark, &roots)
}

fn installed_icon_in(plugin: &Value, dark: bool, roots: &[PathBuf]) -> Option<String> {
    let fields: &[&str] = if dark { &["iconDarkUrl", "logoDark"] } else { &["iconUrl", "logoUrl", "logo", "icon", "iconSmall", "composerIcon"] };
    for (root, manifest) in installed_manifests_in(plugin, roots) {
        if let Some(url) = plugin_icon_fields(&manifest, fields) { return Some(url); }
        for metadata in [Some(&manifest), manifest.get("interface")].into_iter().flatten() {
            for field in fields {
                let Some(relative) = metadata.get(*field).and_then(Value::as_str) else { continue };
                if let Some(url) = local_icon_data(&root, relative) { return Some(url); }
            }
        }
    }
    None
}

fn installed_manifests_in(plugin: &Value, roots: &[PathBuf]) -> Vec<(PathBuf, Value)> {
    let mut manifests = Vec::new();
    let allowed: Vec<PathBuf> = roots.iter().filter_map(|path| path.canonicalize().ok()).collect();
    let mut candidates: Vec<PathBuf> = ["installPath", "path"].iter().filter_map(|key| text(plugin, key).map(PathBuf::from)).collect();
    if let Some((name, marketplace)) = plugin.get("pluginId").and_then(Value::as_str).and_then(|id| id.split_once('@')) {
        let component = |value: &str| !value.is_empty() && value != "." && value != ".." && value.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'));
        if component(name) && component(marketplace) {
            if let Some(version) = plugin.get("version").and_then(Value::as_str) {
                for base in &allowed {
                    candidates.extend(sorted_dirs(&base.join(marketplace).join(name)).into_iter().filter(|path| {
                        let folder = file_name(path);
                        folder == version || folder.strip_prefix(version).is_some_and(|suffix| suffix.starts_with('-'))
                    }));
                }
            }
        }
    }
    for root in candidates {
        let Ok(root) = root.canonicalize() else { continue };
        if !allowed.iter().any(|base| root.starts_with(base) && root != *base) { continue; }
        for name in [".codex-plugin/plugin.json", ".claude-plugin/plugin.json", "plugin.json"] {
            let Ok(manifest_path) = root.join(name).canonicalize() else { continue };
            if !manifest_path.starts_with(&root) { continue; }
            if !fs::metadata(&manifest_path).is_ok_and(|meta| meta.len() <= MAX_PREVIEW_BYTES) { continue; }
            let Some(manifest) = read_json(&manifest_path) else { continue };
            manifests.push((root.clone(), manifest));
        }
    }
    manifests
}

fn local_icon_data(root: &Path, relative: &str) -> Option<String> {
    if Path::new(relative).is_absolute() { return None; }
    let path = root.join(relative).canonicalize().ok()?;
    if !path.starts_with(root) { return None; }
    let mime = match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => "image/png", "jpg" | "jpeg" => "image/jpeg", "webp" => "image/webp",
        "gif" => "image/gif", "avif" => "image/avif", "svg" => "image/svg+xml", _ => return None,
    };
    let mut bytes = Vec::new();
    fs::File::open(path).ok()?.take(96 * 1024 + 1).read_to_end(&mut bytes).ok()?;
    if bytes.is_empty() || bytes.len() > 96 * 1024 { return None; }
    Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

/// Installs, removes or toggles a plugin with the agent's own CLI.
pub fn manage_plugin(
    agent: &str,
    action: &str,
    plugin: &str,
    accept_command: Option<&str>,
) -> Result<PluginActionResult, String> {
    if plugin.is_empty() || plugin.starts_with('-') || plugin.chars().any(char::is_whitespace) {
        return Err("Invalid plugin id".into());
    }
    let mut args: Vec<&str> = match (agent, action) {
        ("claude", "install") => vec!["plugin", "install", plugin, "--json"],
        ("claude", "uninstall") => vec!["plugin", "uninstall", plugin, "--json"],
        ("claude", "enable") => vec!["plugin", "enable", plugin],
        ("claude", "disable") => vec!["plugin", "disable", plugin],
        ("codex", "install") => vec!["plugin", "add", plugin, "--json"],
        ("codex", "uninstall") => vec!["plugin", "remove", plugin, "--json"],
        ("antigravity", "install") if plugin.strip_suffix("@antigravity-plugins-official").is_some_and(valid_antigravity_name) => vec!["plugin", "install", plugin],
        ("antigravity", "uninstall") => vec!["plugin", "uninstall", plugin.split('@').next().unwrap_or(plugin)],
        ("antigravity", "enable") => vec!["plugin", "enable", plugin.split('@').next().unwrap_or(plugin)],
        ("antigravity", "disable") => vec!["plugin", "disable", plugin.split('@').next().unwrap_or(plugin)],
        ("gemini", "uninstall") => vec!["extensions", "uninstall", plugin],
        ("gemini", "enable") => vec!["extensions", "enable", plugin],
        ("gemini", "disable") => vec!["extensions", "disable", plugin],
        _ => return Err(format!("{agent} cannot {action} plugins from Lume")),
    };
    if let Some(sha) = accept_command.filter(|sha| sha.chars().all(|c| c.is_ascii_hexdigit())) {
        args.extend(["--accept-command", sha]);
    }
    let cli = if agent == "antigravity" { "agy" } else { agent };
    let outcome = run_cli_status(cli, &args, CLI_TIMEOUT)?;
    if let Ok(mut cache) = catalog_cache().lock() {
        cache.remove(agent);
    }
    Ok(action_result(outcome))
}

fn action_result((ok, stdout, stderr): (bool, String, String)) -> PluginActionResult {
    let json = serde_json::from_str::<Value>(stdout.trim()).ok().or_else(|| stdout.lines()
        .rev()
        .find_map(|line| serde_json::from_str::<Value>(line.trim()).ok()));
    let confirm = json
        .as_ref()
        .and_then(|value| value.get("shownCommand"))
        .and_then(|shown| {
            Some(PendingCommand {
                sha256: text(shown, "sha256")?,
                command: ["command", "text", "display"]
                    .iter()
                    .find_map(|key| text(shown, key))
                    .unwrap_or_else(|| shown.to_string()),
            })
        });
    let message = json
        .as_ref()
        .and_then(|value| {
            ["message", "error", "status"]
                .iter()
                .find_map(|key| text(value, key))
        })
        .unwrap_or_else(|| {
            let stream = if !ok && !stderr.trim().is_empty() {
                &stderr
            } else {
                &stdout
            };
            stream
                .lines()
                .map(str::trim)
                .rfind(|line| !line.is_empty())
                .unwrap_or_default()
                .to_string()
        });
    let ok = ok
        && json
            .as_ref()
            .and_then(|value| value.get("success").or(value.get("ok")))
            .and_then(Value::as_bool)
            != Some(false);
    PluginActionResult {
        ok: ok && confirm.is_none(),
        message,
        confirm: if ok { None } else { confirm },
    }
}

/// The text of an extension's file, for its preview. Only small text files in the agents' own
/// folders or a project are read.
pub fn read_file(path: &str, project: Option<&str>) -> Result<String, String> {
    let path = fs::canonicalize(path).map_err(|error| error.to_string())?;
    let home = home().ok_or("No home folder")?;
    let allowed = [
        ".claude",
        ".codex",
        ".gemini",
        ".agents",
        ".config/opencode",
    ]
    .iter()
    .map(|root| home.join(root))
    .chain(project.map(PathBuf::from))
    .filter_map(|root| fs::canonicalize(root).ok())
    .any(|root| path.starts_with(root));
    if !allowed {
        return Err("This file is outside the agents' folders".into());
    }
    let meta = fs::metadata(&path).map_err(|error| error.to_string())?;
    if !meta.is_file() || meta.len() > MAX_PREVIEW_BYTES {
        return Err("This file is too large to preview".into());
    }
    fs::read_to_string(&path).map_err(|error| error.to_string())
}

fn run_cli(name: &str, args: &[&str], timeout: Duration) -> Result<String, String> {
    let (ok, stdout, stderr) = run_cli_status(name, args, timeout)?;
    if ok {
        Ok(stdout)
    } else {
        Err(stderr
            .lines()
            .map(str::trim)
            .rfind(|line| !line.is_empty())
            .unwrap_or("failed")
            .to_string())
    }
}

fn run_cli_status(
    name: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<(bool, String, String), String> {
    let mut child = crate::executables::command(name)?
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let mut stdout = child.stdout.take().ok_or("no stdout")?;
    let mut stderr = child.stderr.take().ok_or("no stderr")?;
    let out = thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        text
    });
    let err = thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("`{name} {}` took too long", args.join(" ")));
        }
        thread::sleep(Duration::from_millis(50));
    };
    Ok((
        status.success(),
        out.join().unwrap_or_default(),
        err.join().unwrap_or_default(),
    ))
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

fn text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn sorted_dirs(directory: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(directory)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect()
        })
        .unwrap_or_default();
    dirs.sort();
    dirs
}

fn files_with(directory: &Path, extension: &str, depth: usize) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(directory) else {
        return found;
    };
    let mut entries: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() && depth > 1 {
            found.extend(files_with(&path, extension, depth - 1));
        } else if path.extension().is_some_and(|ext| ext == extension) {
            found.push(path);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_repository_comes_from_explicit_plugin_metadata_only() {
        assert_eq!(plugin_repository(&serde_json::json!({"repository":"https://github.com/OpenAI/plugins.git"})).as_deref(), Some("openai/plugins"));
        assert_eq!(plugin_repository(&serde_json::json!({"source":{"source":"github","repo":"owner/plugin"}})).as_deref(), Some("owner/plugin"));
        assert_eq!(plugin_repository(&serde_json::json!({"source":{"source":"git-subdir","url":"https://github.com/owner/collection.git","path":"plugins/example"}})).as_deref(), Some("owner/collection"));
        for value in [serde_json::json!({"name":"github","homepage":"https://github.com/"}), serde_json::json!({"source":{"source":"remote","id":"owner/plugin"}}), serde_json::json!({"repository":"https://example.com/owner/plugin"})] {
            assert!(plugin_repository(&value).is_none());
        }
        let plugins = parse_catalog(r#"{"available":[{"pluginId":"explicit@test-repos","repository":"https://github.com/owner/plugin.git"},{"pluginId":"unknown@test-repos","name":"popular-project"}]}"#);
        assert_eq!(plugins.iter().find(|p| p.id == "explicit@test-repos").unwrap().github_repository.as_deref(), Some("owner/plugin"));
        assert!(plugins.iter().find(|p| p.id == "unknown@test-repos").unwrap().github_repository.is_none());
    }

    #[test]
    fn installed_plugin_images_stay_inside_the_cache_and_respect_size_limits() {
        let temporary = std::env::temp_dir().join(format!("lume-plugin-images-{}-{}", std::process::id(), crate::state::now_millis()));
        fs::create_dir(&temporary).unwrap();
        let cache = temporary.join("cache");
        let root = cache.join("official/demo/1.0.0");
        fs::create_dir_all(root.join(".codex-plugin")).unwrap();
        fs::create_dir(root.join("assets")).unwrap();
        fs::write(root.join("assets/logo.png"), b"image bytes").unwrap();
        fs::write(root.join("assets/dark.png"), b"dark bytes").unwrap();
        fs::write(temporary.join("outside.png"), b"private").unwrap();
        fs::write(root.join(".codex-plugin/plugin.json"), r#"{"interface":{"logo":"./assets/logo.png","logoDark":"./assets/dark.png"}}"#).unwrap();
        let entry = serde_json::json!({"pluginId":"demo@official", "version":"1.0.0"});
        let light = installed_icon_in(&entry, false, std::slice::from_ref(&cache)).unwrap();
        let dark = installed_icon_in(&entry, true, std::slice::from_ref(&cache)).unwrap();
        assert!(light.starts_with("data:image/png;base64,"));
        assert_ne!(light, dark);
        let canonical = root.canonicalize().unwrap();
        assert!(local_icon_data(&canonical, "../../../../outside.png").is_none());
        fs::write(root.join("assets/large.png"), vec![0; 96 * 1024 + 1]).unwrap();
        assert!(local_icon_data(&canonical, "assets/large.png").is_none());
        #[cfg(unix)] {
            std::os::unix::fs::symlink(temporary.join("outside.png"), root.join("assets/link.png")).unwrap();
            assert!(local_icon_data(&canonical, "assets/link.png").is_none());
        }
        fs::remove_dir_all(temporary).unwrap();
    }

    #[test]
    fn catalog_metadata_is_optional_and_preserves_real_metrics_and_safe_icons() {
        let catalog = parse_catalog(r#"{"available":[
          {"pluginId":"a@official","name":"A","installCount":0,"downloadCount":123,"rating":4.5,"updatedAt":"2026-10-23T12:00:00Z","iconUrl":"https://cdn.example.com/a.png"},
          {"pluginId":"b@community","name":"B","downloads":8,"updatedAt":1792756800,"logo":"https://localhost/b.png"},
          {"pluginId":"empty@community","name":"Empty"}
        ]}"#);
        let a = catalog.iter().find(|plugin| plugin.id == "a@official").unwrap();
        assert_eq!(a.installs, Some(0));
        assert_eq!(a.downloads, Some(123));
        assert_eq!(a.rating, Some(4.5));
        assert_eq!(a.updated_at, Some(1_792_756_800_000));
        assert_eq!(a.icon_url.as_deref(), Some("https://cdn.example.com/a.png"));
        let b = catalog.iter().find(|plugin| plugin.id == "b@community").unwrap();
        assert_eq!(b.updated_at, a.updated_at);
        assert!(b.icon_url.is_none());
        let empty = catalog.iter().find(|plugin| plugin.id == "empty@community").unwrap();
        assert!(empty.downloads.is_none() && empty.rating.is_none() && empty.updated_at.is_none());
        for url in ["file:///home/user/.env", "https://127.0.0.1/a", "https://2130706433/a", "https://[::ffff:127.0.0.1]/a", "https://host.internal/a", "https://user:password@example.com/a"] {
            assert!(plugin_icon(&serde_json::json!({"iconUrl":url})).is_none(), "{url}");
        }
    }

    #[test]
    fn antigravity_catalog_uses_native_selectors_and_ignores_unsafe_names() {
        let catalog = antigravity_catalog(&serde_json::json!({"plugins": [
            {"name": "gcalendar", "displayName": "Google Calendar", "description": "Calendar skills"},
            {"name": "../outside"}, {"name": "foreign@other-market"}, {"name": "--flag"}
        ]}));
        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog[0].id, "gcalendar@antigravity-plugins-official");
        assert_eq!(catalog[0].name, "Google Calendar");
        // A flag-like native name cannot pass manage_plugin's selector validation.
        assert!(manage_plugin("antigravity", "install", "--flag", None).is_err());
        assert!(manage_plugin("antigravity", "install", "../outside@antigravity-plugins-official", None).is_err());
        let items = installed_plugins(r#"[{"name":"gcalendar","enabled":true,"source":"antigravity-plugins-official","path":"/home/test/.gemini/antigravity-cli/plugins/gcalendar"}]"#);
        assert_eq!(items[0].name, "gcalendar");
        assert_eq!(items[0].enabled, Some(true));
        assert_eq!(items[0].source.as_deref(), Some("antigravity-plugins-official"));
    }

    #[test]
    fn plugin_lists_from_claude_and_codex_read_the_same_way() {
        let codex = r#"{"installed":[{"pluginId":"github@openai-curated-remote","name":"github","marketplaceName":"openai-curated-remote","version":"0.1.12","installed":true,"enabled":true}],"available":[]}"#;
        let items = installed_plugins(codex);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "github");
        assert_eq!(items[0].enabled, Some(true));
        assert_eq!(items[0].source.as_deref(), Some("openai-curated-remote"));

        let claude = r#"{"installed":[],"available":[{"pluginId":"b@m","name":"b","installCount":3},{"pluginId":"a@m","name":"a","installCount":90,"description":"Alpha"}]}"#;
        let plugins = parse_catalog(claude);
        assert_eq!(
            plugins
                .iter()
                .map(|plugin| plugin.name.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert!(!plugins[0].installed);
    }

    #[test]
    fn an_install_waiting_on_a_command_asks_before_running_it() {
        let pretty = action_result((false, serde_json::to_string_pretty(&serde_json::json!({
            "success": false, "shownCommand": {"command": "npm install example", "sha256": "abc123"}
        })).unwrap(), String::new()));
        assert_eq!(pretty.confirm.unwrap().command, "npm install example");
        let result = action_result((false, r#"{"success":false,"message":"Confirm the command","shownCommand":{"command":"npx thing","sha256":"abc123"}}"#.into(), String::new()));
        assert!(!result.ok);
        let confirm = result.confirm.expect("confirm");
        assert_eq!(confirm.command, "npx thing");
        assert_eq!(confirm.sha256, "abc123");
        let done = action_result((true, "Installed github\n".into(), String::new()));
        assert!(done.ok);
        assert_eq!(done.message, "Installed github");
    }

    #[test]
    fn invalid_plugin_ids_never_reach_the_cli() {
        assert!(manage_plugin("claude", "install", "--yes", None).is_err());
        assert!(manage_plugin("claude", "install", "a b", None).is_err());
        assert!(manage_plugin("codex", "enable", "x@y", None).is_err());
    }
}

#[cfg(test)]
mod machine {
    // Reads this machine's agents: cargo test agent_extensions::machine -- --ignored --nocapture
    #[test]
    #[ignore]
    fn lists_this_machines_extensions() {
        for agent in super::list(Some(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))) {
            println!(
                "{} cli={} problems={:?}",
                agent.label, agent.cli, agent.problems
            );
            for item in agent.items.iter().take(40) {
                println!(
                    "  {:<11} {:<8} {} — {}",
                    item.kind,
                    item.scope,
                    item.name,
                    item.description.chars().take(60).collect::<String>()
                );
            }
        }
    }
}
