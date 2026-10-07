//! Slash commands as each agent reports them, instead of a fixed catalog.
//!
//! Claude answers the stream-json `initialize` control request, Codex lists its
//! skills through the app-server, OpenCode announces `available_commands_update`
//! over ACP, and Antigravity resolves skills and workflows from disk.

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use serde::Serialize;
use serde_json::{json, Value};

const CACHE_TTL: Duration = Duration::from_secs(5 * 60);
const DESCRIPTION_LIMIT: usize = 320;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSlashCommand {
    pub name: String,
    pub description: String,
    pub argument_hint: Option<String>,
    /// Text that invokes the command: `/` for commands, `$` for Codex skills.
    pub prefix: String,
    pub kind: String,
}

fn command(name: &str, description: &str, prefix: &str, kind: &str) -> AgentSlashCommand {
    AgentSlashCommand {
        name: name.trim().to_string(),
        description: description.trim().chars().take(DESCRIPTION_LIMIT).collect(),
        argument_hint: None,
        prefix: prefix.into(),
        kind: kind.into(),
    }
}

type Cache = Mutex<HashMap<String, (Instant, Vec<AgentSlashCommand>)>>;

fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cached(
    key: String,
    load: impl FnOnce() -> Result<Vec<AgentSlashCommand>, String>,
) -> Result<Vec<AgentSlashCommand>, String> {
    if let Some((loaded_at, commands)) = cache()
        .lock()
        .map_err(|_| "Could not read the agent command cache")?
        .get(&key)
    {
        if loaded_at.elapsed() < CACHE_TTL {
            return Ok(commands.clone());
        }
    }
    let commands = load()?;
    cache()
        .lock()
        .map_err(|_| "Could not update the agent command cache")?
        .insert(key, (Instant::now(), commands.clone()));
    Ok(commands)
}

pub fn claude_commands(cwd: &str) -> Result<Vec<AgentSlashCommand>, String> {
    cached(format!("claude:{cwd}"), || {
        Ok(parse_claude_commands(&crate::claude_control::initialize(
            cwd,
        )?))
    })
}

fn parse_claude_commands(response: &Value) -> Vec<AgentSlashCommand> {
    // Accepts the whole control_response or the initialize payload inside it.
    response
        .pointer("/response/response/commands")
        .or_else(|| response.get("commands"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let name = entry.get("name").and_then(Value::as_str)?;
            if name.trim().is_empty() || name.starts_with("__") {
                return None;
            }
            let mut item = command(
                name,
                entry
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
                "/",
                "command",
            );
            item.argument_hint = entry
                .get("argumentHint")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|hint| !hint.is_empty())
                .map(str::to_string);
            Some(item)
        })
        .collect()
}

pub fn codex_commands(
    cwd: &str,
    list: impl FnOnce() -> Result<Value, String>,
) -> Result<Vec<AgentSlashCommand>, String> {
    let skills = cached(format!("codex:{cwd}"), || Ok(parse_codex_skills(&list()?)))?;
    if let Ok(mut known) = codex_skill_paths().lock() {
        for skill in &skills {
            if let Some(path) = &skill.argument_hint {
                known.insert(skill.name.clone(), path.clone());
            }
        }
    }
    Ok(skills
        .into_iter()
        .map(|mut skill| {
            skill.argument_hint = None;
            skill
        })
        .collect())
}

fn codex_skill_paths() -> &'static Mutex<HashMap<String, String>> {
    static PATHS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    PATHS.get_or_init(|| Mutex::new(HashMap::new()))
}

// The skill path travels in `argument_hint` only inside the cache; it is
// removed before the list reaches the frontend.
fn parse_codex_skills(response: &Value) -> Vec<AgentSlashCommand> {
    let mut seen = HashSet::new();
    response
        .pointer("/result/data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|entry| {
            entry
                .get("skills")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter(|skill| skill.get("enabled").and_then(Value::as_bool) != Some(false))
        .filter_map(|skill| {
            let name = skill.get("name").and_then(Value::as_str)?;
            if !seen.insert(name.to_string()) {
                return None;
            }
            let description = skill
                .get("shortDescription")
                .and_then(Value::as_str)
                .or_else(|| {
                    skill
                        .pointer("/interface/shortDescription")
                        .and_then(Value::as_str)
                })
                .or_else(|| skill.get("description").and_then(Value::as_str))
                .unwrap_or("");
            let mut item = command(name, description, "$", "skill");
            item.argument_hint = skill
                .get("path")
                .and_then(Value::as_str)
                .map(str::to_string);
            Some(item)
        })
        .collect()
}

/// Codex invokes a skill through a structured input item, not through `$name`
/// text alone. Adds one for every known skill the prompt mentions.
pub fn codex_skill_inputs(prompt: &str) -> Vec<Value> {
    let Ok(known) = codex_skill_paths().lock() else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    prompt
        .split(char::is_whitespace)
        .filter_map(|token| token.strip_prefix('$'))
        .map(|name| name.trim_end_matches([',', '.', ';', ':', '!', '?']))
        .filter_map(|name| known.get(name).map(|path| (name, path)))
        .filter(|(name, _)| seen.insert(name.to_string()))
        .map(|(name, path)| json!({ "type": "skill", "name": name, "path": path }))
        .collect()
}

fn opencode_store() -> &'static Mutex<HashMap<String, Vec<AgentSlashCommand>>> {
    static STORE: OnceLock<Mutex<HashMap<String, Vec<AgentSlashCommand>>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn record_opencode_commands(session_id: &str, available: &Value) {
    let commands = available
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let name = entry.get("name").and_then(Value::as_str)?;
            let mut item = command(
                name,
                entry
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
                "/",
                "command",
            );
            item.argument_hint = entry
                .pointer("/input/hint")
                .and_then(Value::as_str)
                .map(str::to_string);
            Some(item)
        })
        .collect();
    if let Ok(mut store) = opencode_store().lock() {
        store.insert(session_id.into(), commands);
    }
}

pub fn opencode_commands(session_id: &str) -> Option<Vec<AgentSlashCommand>> {
    opencode_store().lock().ok()?.get(session_id).cloned()
}

pub fn antigravity_commands(cwd: &str) -> Result<Vec<AgentSlashCommand>, String> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or("Could not find the home directory")?;
    cached(format!("antigravity:{cwd}"), || {
        Ok(antigravity_commands_from(&home, Path::new(cwd)))
    })
}

// Project definitions come first so they win over global ones with the same name.
fn antigravity_commands_from(home: &Path, cwd: &Path) -> Vec<AgentSlashCommand> {
    let mut seen = HashSet::new();
    let mut commands = Vec::new();
    let mut add = |item: AgentSlashCommand| {
        if !item.name.is_empty() && seen.insert(item.name.clone()) {
            commands.push(item);
        }
    };
    for root in [".agents", "_agents", ".agent", "_agent"] {
        workflow_commands(&cwd.join(root).join("workflows"))
            .into_iter()
            .for_each(&mut add);
        skill_commands(&cwd.join(root).join("skills"))
            .into_iter()
            .for_each(&mut add);
    }
    let config = home.join(".gemini/config");
    for directory in ["global_workflows", "workflows"] {
        workflow_commands(&config.join(directory))
            .into_iter()
            .for_each(&mut add);
    }
    for directory in [
        config.join("skills"),
        home.join(".gemini/antigravity-cli/builtin/skills"),
    ] {
        skill_commands(&directory).into_iter().for_each(&mut add);
    }
    commands
}

fn workflow_commands(directory: &Path) -> Vec<AgentSlashCommand> {
    sorted_entries(directory)
        .into_iter()
        .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
        .filter_map(|path| {
            let text = fs::read_to_string(&path).ok()?;
            let fields = frontmatter(&text);
            let stem = path.file_stem()?.to_str()?;
            Some(command(
                stem,
                fields.get("description").map(String::as_str).unwrap_or(""),
                "/",
                "workflow",
            ))
        })
        .collect()
}

fn skill_commands(directory: &Path) -> Vec<AgentSlashCommand> {
    sorted_entries(directory)
        .into_iter()
        .filter_map(|path| {
            let text = fs::read_to_string(path.join("SKILL.md")).ok()?;
            let fields = frontmatter(&text);
            let name = fields
                .get("name")
                .cloned()
                .or_else(|| path.file_name()?.to_str().map(str::to_string))?;
            Some(command(
                &name,
                fields.get("description").map(String::as_str).unwrap_or(""),
                "/",
                "skill",
            ))
        })
        .collect()
}

fn sorted_entries(directory: &Path) -> Vec<PathBuf> {
    let mut entries = fs::read_dir(directory)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    entries.sort();
    entries
}

fn frontmatter(text: &str) -> HashMap<String, String> {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return HashMap::new();
    }
    lines
        .take_while(|line| line.trim() != "---")
        .filter(|line| !line.starts_with([' ', '\t']))
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| {
            (
                key.trim().to_string(),
                value.trim().trim_matches(['"', '\'']).to_string(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Talks to the installed CLIs: cargo test agent_commands -- --ignored --nocapture
    #[test]
    #[ignore]
    fn installed_agents_report_their_commands() {
        let cwd = env!("CARGO_MANIFEST_DIR");
        let claude = claude_commands(cwd).expect("Claude commands");
        println!(
            "claude: {} commands, e.g. {:?}",
            claude.len(),
            claude
                .iter()
                .take(5)
                .map(|item| &item.name)
                .collect::<Vec<_>>()
        );
        assert!(claude.iter().any(|item| item.name == "compact"));
        let antigravity = antigravity_commands(cwd).expect("Antigravity commands");
        println!(
            "antigravity: {} commands, e.g. {:?}",
            antigravity.len(),
            antigravity
                .iter()
                .take(5)
                .map(|item| &item.name)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn claude_initialize_response_becomes_commands() {
        let response = json!({
            "type": "control_response",
            "response": { "subtype": "success", "request_id": "lume-commands", "response": {
                "commands": [
                    { "name": "compact", "description": "Compact the conversation", "argumentHint": "", "builtin": true },
                    { "name": "code-review", "description": "Review the diff", "argumentHint": "[low|high]" },
                    { "name": "__remote-workflow", "description": "internal" }
                ]
            }}
        });
        let commands = parse_claude_commands(&response);
        assert_eq!(
            commands
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
            ["compact", "code-review"]
        );
        assert_eq!(commands[0].argument_hint, None);
        assert_eq!(commands[1].argument_hint.as_deref(), Some("[low|high]"));
        assert!(commands.iter().all(|item| item.prefix == "/"));
        assert_eq!(
            parse_claude_commands(&response["response"]["response"]),
            commands
        );
    }

    #[test]
    fn codex_skills_are_invoked_with_a_structured_input() {
        let response = json!({ "result": { "data": [{
            "cwd": "/work",
            "errors": [],
            "skills": [
                { "name": "lint", "description": "Long", "shortDescription": "Run lint", "path": "/s/lint/SKILL.md", "enabled": true },
                { "name": "off", "description": "Disabled", "path": "/s/off/SKILL.md", "enabled": false }
            ]
        }]}});
        let commands = codex_commands("/work-test", || Ok(response)).expect("skills");
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].prefix, "$");
        assert_eq!(commands[0].description, "Run lint");
        assert_eq!(commands[0].argument_hint, None);
        assert_eq!(
            codex_skill_inputs("use $lint, then $unknown"),
            vec![json!({ "type": "skill", "name": "lint", "path": "/s/lint/SKILL.md" })]
        );
    }

    #[test]
    fn opencode_announcement_is_stored_per_session() {
        record_opencode_commands(
            "acp-1",
            &json!([{ "name": "review", "description": "review changes", "input": { "hint": "[commit]" } }]),
        );
        let commands = opencode_commands("acp-1").expect("comandos");
        assert_eq!(commands[0].name, "review");
        assert_eq!(commands[0].argument_hint.as_deref(), Some("[commit]"));
        assert_eq!(opencode_commands("acp-2"), None);
    }

    #[test]
    fn antigravity_reads_project_and_global_skills_and_workflows() {
        let root = std::env::temp_dir().join(format!("lume-agy-commands-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let home = root.join("home");
        let project = root.join("project");
        let write = |path: PathBuf, text: &str| {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        };
        write(
            project.join(".agents/workflows/deploy.md"),
            "---\ndescription: Deploy the app\n---\nSteps",
        );
        write(
            project.join(".agents/skills/lint/SKILL.md"),
            "---\nname: lint\ndescription: \"Project lint\"\n---",
        );
        write(
            home.join(".gemini/config/skills/lint/SKILL.md"),
            "---\nname: lint\ndescription: Global lint\n---",
        );
        write(
            home.join(".gemini/antigravity-cli/builtin/skills/plugin/SKILL.md"),
            "---\nname: plugin\ndescription: Plugins\n---",
        );
        let commands = antigravity_commands_from(&home, &project);
        let _ = fs::remove_dir_all(&root);
        let summary = commands
            .iter()
            .map(|item| {
                (
                    item.name.as_str(),
                    item.description.as_str(),
                    item.kind.as_str(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            summary,
            [
                ("deploy", "Deploy the app", "workflow"),
                ("lint", "Project lint", "skill"),
                ("plugin", "Plugins", "skill"),
            ]
        );
    }
}
