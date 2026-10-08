//! The MCP servers each agent has configured, and whether they are connected.
//!
//! Configuration is read from the files each agent owns. Claude Code can also report live
//! health through `claude mcp list`, which is slow, so that probe is separate and cached.

use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};

use serde::Serialize;
use serde_json::Value;

use crate::domain::AgentKind;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServer {
    pub name: String,
    /// `stdio`, `http` or `sse`.
    pub transport: String,
    /// The URL, or the command with its arguments. Never environment values or headers.
    pub target: String,
    /// `user`, `project`, `local` or `account`.
    pub scope: String,
    /// `connected`, `needs_auth`, `failed`, `disabled` or `unknown`.
    pub status: String,
}

fn home() -> Option<PathBuf> {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn server(name: &str, transport: &str, target: String, scope: &str, status: &str) -> McpServer {
    McpServer {
        name: name.into(),
        transport: transport.into(),
        target,
        scope: scope.into(),
        status: status.into(),
    }
}

fn from_json_entry(name: &str, entry: &Value, scope: &str) -> McpServer {
    let url = entry.get("url").and_then(Value::as_str);
    let transport = entry
        .get("type")
        .and_then(Value::as_str)
        .map(|kind| match kind {
            "local" => "stdio",
            "remote" => "http",
            other => other,
        })
        .unwrap_or(if url.is_some() { "http" } else { "stdio" });
    let command = match entry.get("command") {
        Some(Value::String(command)) => Some(command.clone()),
        Some(Value::Array(parts)) => Some(
            parts
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" "),
        ),
        _ => None,
    };
    let args = entry
        .get("args")
        .and_then(Value::as_array)
        .map(|args| {
            args.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    let target = url.map(str::to_string).unwrap_or_else(|| {
        format!("{} {}", command.unwrap_or_default(), args)
            .trim()
            .to_string()
    });
    let disabled = entry.get("enabled").and_then(Value::as_bool) == Some(false)
        || entry.get("disabled").and_then(Value::as_bool) == Some(true);
    server(
        name,
        transport,
        target,
        scope,
        if disabled { "disabled" } else { "unknown" },
    )
}

fn json_servers(value: &Value, key: &str, scope: &str) -> Vec<McpServer> {
    value
        .get(key)
        .and_then(Value::as_object)
        .map(|servers| {
            servers
                .iter()
                .map(|(name, entry)| from_json_entry(name, entry, scope))
                .collect()
        })
        .unwrap_or_default()
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

pub fn claude_configured(cwd: Option<&str>) -> Vec<McpServer> {
    let mut servers = Vec::new();
    if let Some(config) = home().and_then(|home| read_json(&home.join(".claude.json"))) {
        servers.extend(json_servers(&config, "mcpServers", "user"));
        if let Some(project) =
            cwd.and_then(|cwd| config.pointer(&format!("/projects/{}", pointer_escape(cwd))))
        {
            servers.extend(json_servers(project, "mcpServers", "local"));
        }
    }
    if let Some(project) = cwd.and_then(|cwd| read_json(&Path::new(cwd).join(".mcp.json"))) {
        servers.extend(json_servers(&project, "mcpServers", "project"));
    }
    servers
}

fn pointer_escape(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

/// Removes `//` and `/* */` comments outside of strings so JSONC parses as JSON.
fn strip_jsonc(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    while let Some(character) = chars.next() {
        if in_string {
            output.push(character);
            if character == '\\' {
                if let Some(next) = chars.next() {
                    output.push(next);
                }
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        match (character, chars.peek()) {
            ('"', _) => {
                in_string = true;
                output.push(character);
            }
            ('/', Some('/')) => {
                for next in chars.by_ref() {
                    if next == '\n' {
                        output.push('\n');
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                chars.next();
                let mut previous = ' ';
                for next in chars.by_ref() {
                    if previous == '*' && next == '/' {
                        break;
                    }
                    previous = next;
                }
            }
            _ => output.push(character),
        }
    }
    output
}

pub fn opencode_configured(cwd: Option<&str>) -> Vec<McpServer> {
    let mut servers = Vec::new();
    let mut read = |path: PathBuf, scope: &str| {
        if let Ok(text) = fs::read_to_string(path) {
            if let Ok(value) = serde_json::from_str::<Value>(&strip_jsonc(&text)) {
                servers.extend(json_servers(&value, "mcp", scope));
            }
        }
    };
    if let Some(home) = home() {
        read(home.join(".config/opencode/opencode.jsonc"), "user");
        read(home.join(".config/opencode/opencode.json"), "user");
    }
    if let Some(cwd) = cwd {
        read(Path::new(cwd).join("opencode.jsonc"), "project");
        read(Path::new(cwd).join("opencode.json"), "project");
    }
    servers
}

/// `[mcp_servers.name]` tables of a Codex `config.toml`. Only the keys shown are read, so a
/// small line parser is enough and keeps secrets in `env` tables untouched.
pub fn parse_codex_config(text: &str, scope: &str) -> Vec<McpServer> {
    let mut servers: Vec<McpServer> = Vec::new();
    let mut current: Option<usize> = None;
    let mut command = String::new();
    let mut args = String::new();
    let flush = |servers: &mut Vec<McpServer>,
                     index: Option<usize>,
                     command: &mut String,
                     args: &mut String| {
        if let Some(index) = index {
            if servers[index].target.is_empty() {
                servers[index].target = format!("{command} {args}").trim().to_string();
            }
        }
        command.clear();
        args.clear();
    };
    for raw in text.lines() {
        let line = raw.trim();
        if let Some(header) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            flush(&mut servers, current, &mut command, &mut args);
            current = None;
            if let Some(rest) = header.strip_prefix("mcp_servers.") {
                if !rest.contains('.')
                    || rest.starts_with('"')
                        && rest.matches('"').count() == 2
                        && rest.ends_with('"')
                {
                    let name = rest.trim_matches('"').to_string();
                    servers.push(server(&name, "stdio", String::new(), scope, "unknown"));
                    current = Some(servers.len() - 1);
                }
            }
            continue;
        }
        let Some(index) = current else { continue };
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        let unquoted = value.trim_matches('"').to_string();
        match key {
            "url" => {
                servers[index].transport = "http".into();
                servers[index].target = unquoted;
            }
            "command" => command = unquoted,
            "args" => {
                args = value
                    .trim_matches(['[', ']'])
                    .split(',')
                    .map(|part| part.trim().trim_matches('"'))
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
            }
            "enabled" if value == "false" => servers[index].status = "disabled".into(),
            _ => {}
        }
    }
    flush(&mut servers, current, &mut command, &mut args);
    servers
}

pub fn codex_configured(cwd: Option<&str>) -> Vec<McpServer> {
    let mut servers = Vec::new();
    let root = env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| home().map(|home| home.join(".codex")));
    if let Some(text) = root.and_then(|root| fs::read_to_string(root.join("config.toml")).ok()) {
        servers.extend(parse_codex_config(&text, "user"));
    }
    if let Some(text) =
        cwd.and_then(|cwd| fs::read_to_string(Path::new(cwd).join(".codex/config.toml")).ok())
    {
        servers.extend(parse_codex_config(&text, "project"));
    }
    servers
}

/// One status per line of `claude mcp list`: `name: target - ✔ Connected`.
pub fn parse_claude_health(output: &str) -> Vec<(String, String, String)> {
    output
        .lines()
        .filter_map(|line| {
            let (head, status) = line.rsplit_once(" - ")?;
            let (name, target) = head.split_once(": ")?;
            let lower = status.to_lowercase();
            let status = if lower.contains("connected") && !lower.contains("failed") {
                "connected"
            } else if lower.contains("auth") {
                "needs_auth"
            } else if lower.contains("fail") {
                "failed"
            } else {
                "unknown"
            };
            Some((
                name.trim().to_string(),
                target.trim().to_string(),
                status.to_string(),
            ))
        })
        .collect()
}

fn health_cache() -> &'static Mutex<HashMap<String, (Instant, Vec<(String, String, String)>)>> {
    static CACHE: OnceLock<Mutex<HashMap<String, (Instant, Vec<(String, String, String)>)>>> =
        OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn claude_health(cwd: &str) -> Result<Vec<(String, String, String)>, String> {
    if let Ok(cache) = health_cache().lock() {
        if let Some((at, value)) = cache.get(cwd) {
            if at.elapsed() < Duration::from_secs(60) {
                return Ok(value.clone());
            }
        }
    }
    let mut command = crate::executables::command("claude")?;
    command
        .args(["mcp", "list"])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|error| format!("Could not start Claude: {error}"))?;
    let stdout = child.stdout.take().ok_or("Claude closed its output")?;
    let reader = thread::spawn(move || {
        let mut text = String::new();
        let _ = std::io::Read::read_to_string(&mut std::io::BufReader::new(stdout), &mut text);
        text
    });
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(45) {
        if child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            break;
        }
        thread::sleep(Duration::from_millis(150));
    }
    let _ = child.kill();
    let _ = child.wait();
    let parsed = parse_claude_health(&reader.join().unwrap_or_default());
    if let Ok(mut cache) = health_cache().lock() {
        cache.insert(cwd.to_string(), (Instant::now(), parsed.clone()));
    }
    Ok(parsed)
}

/// The agent's MCP servers. With `probe`, Claude Code's live health is merged in and servers
/// that only exist on the account (claude.ai connectors) are added.
pub fn servers_for(
    agent: &AgentKind,
    cwd: Option<&str>,
    probe: bool,
) -> Result<Vec<McpServer>, String> {
    let mut servers = match agent {
        AgentKind::ClaudeCode => claude_configured(cwd),
        AgentKind::Codex => codex_configured(cwd),
        AgentKind::OpenCode => opencode_configured(cwd),
        _ => Vec::new(),
    };
    if probe && *agent == AgentKind::ClaudeCode {
        if let Some(cwd) = cwd {
            for (name, target, status) in claude_health(cwd)? {
                match servers.iter_mut().find(|item| item.name == name) {
                    Some(item) => item.status = status,
                    None => servers.push(server(
                        &name,
                        if target.starts_with("http") {
                            "http"
                        } else {
                            "stdio"
                        },
                        target,
                        "account",
                        &status,
                    )),
                }
            }
        }
    }
    servers.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    servers.dedup_by(|left, right| left.name == right.name && left.scope == right.scope);
    Ok(servers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn codex_config_lists_servers_without_secrets() {
        let servers = parse_codex_config(
            "model = \"x\"\n[mcp_servers.docs]\nurl = \"https://mcp.example.com/mcp\"\n[mcp_servers.atlassian]\ncommand = \"npx\"\nargs = [\"-y\", \"mcp-remote\"]\nenabled = false\n[mcp_servers.atlassian.env]\nTOKEN = \"secret\"\n[other]\nkey = 1\n",
            "user",
        );
        assert_eq!(servers.len(), 2);
        assert_eq!(
            (servers[0].transport.as_str(), servers[0].target.as_str()),
            ("http", "https://mcp.example.com/mcp")
        );
        assert_eq!(servers[1].target, "npx -y mcp-remote");
        assert_eq!(servers[1].status, "disabled");
        assert!(servers.iter().all(|item| !item.target.contains("secret")));
    }

    #[test]
    fn jsonc_comments_are_removed_but_urls_survive() {
        let text = "{ // comment\n \"mcp\": { \"a\": { \"type\": \"remote\", \"url\": \"https://x.test/mcp\" /* c */ } } }";
        let value: Value = serde_json::from_str(&strip_jsonc(text)).expect("json");
        let servers = json_servers(&value, "mcp", "user");
        assert_eq!(servers[0].target, "https://x.test/mcp");
        assert_eq!(servers[0].transport, "http");
    }

    #[test]
    fn claude_entries_and_health_lines_are_understood() {
        let entry = json!({ "command": "node", "args": ["server.js"], "env": { "KEY": "secret" } });
        let item = from_json_entry("local", &entry, "user");
        assert_eq!(
            (item.transport.as_str(), item.target.as_str()),
            ("stdio", "node server.js")
        );
        let health = parse_claude_health(
            "Checking MCP server health…\n\nclaude.ai Claude Docs: https://api.anthropic.com/v1/pages/mcp - ✔ Connected\nplaywright: npx playwright-mcp - ✗ Failed to connect\nnotion: https://mcp.notion.com/mcp - ⚠ Needs authentication\n",
        );
        assert_eq!(
            health
                .iter()
                .map(|item| item.2.as_str())
                .collect::<Vec<_>>(),
            ["connected", "failed", "needs_auth"]
        );
        assert_eq!(health[0].0, "claude.ai Claude Docs");
    }
}
