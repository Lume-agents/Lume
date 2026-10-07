//! Read-only queries to the Claude Code CLI through its stream-json control
//! protocol. A probe never sends a user message, so it makes no model request,
//! and it disables hooks so it never reaches Lume as a session.

use std::{
    collections::{HashMap, HashSet},
    io::{BufRead, BufReader, Write},
    process::Stdio,
    sync::{mpsc, Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};

use serde::Serialize;
use serde_json::{json, Value};

use crate::domain::{AgentRateLimit, PermissionSettings, SessionModelOverride};

const PROBE_TIMEOUT: Duration = Duration::from_secs(15);
const INITIALIZE_CACHE_TTL: Duration = Duration::from_secs(300);
const FALLBACK_EFFORT: &str = "high";

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeEffortOption {
    pub value: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeModelOption {
    pub model: String,
    pub display_name: String,
    pub description: String,
    pub is_default: bool,
    pub default_reasoning_effort: String,
    pub supported_reasoning_efforts: Vec<ClaudeEffortOption>,
    pub supports_auto_mode: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeModelSettings {
    pub model: String,
    pub reasoning_effort: Option<String>,
    pub models: Vec<ClaudeModelOption>,
}

/// Models the CLI offers, keyed by their versioned ID, plus every alias the
/// CLI accepts (`default`, `opus`, `sonnet`...) resolved to that ID.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClaudeModelCatalog {
    pub options: Vec<ClaudeModelOption>,
    pub aliases: HashMap<String, String>,
}

/// Where the effective model comes from, in priority order.
#[derive(Default)]
pub struct ClaudeModelSources<'a> {
    pub session_override: Option<&'a SessionModelOverride>,
    pub current_model: Option<String>,
    pub last_session_model: Option<String>,
    pub settings: Option<&'a Value>,
}

/// What `--permission-mode` accepts that Lume offers, from asking to bypassing.
/// `default` is the CLI's normal mode, `auto` lets its classifier approve for you.
pub const PERMISSION_MODES: [&str; 5] = [
    "default",
    "acceptEdits",
    "plan",
    "auto",
    "bypassPermissions",
];

pub fn is_permission_mode(mode: &str) -> bool {
    PERMISSION_MODES.contains(&mode)
}

/// The modes to offer for a model, always including the one already in effect.
pub fn permission_settings(mode: &str, supports_auto_mode: bool) -> PermissionSettings {
    let mut modes = PERMISSION_MODES
        .iter()
        .filter(|candidate| supports_auto_mode || **candidate != "auto")
        .map(|candidate| candidate.to_string())
        .collect::<Vec<_>>();
    if !modes.iter().any(|candidate| candidate == mode) {
        modes.push(mode.to_string());
    }
    PermissionSettings {
        mode: mode.to_string(),
        modes,
    }
}

fn probe_pids() -> &'static Mutex<HashSet<u32>> {
    static PIDS: OnceLock<Mutex<HashSet<u32>>> = OnceLock::new();
    PIDS.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Discovery must not mistake a short-lived probe for a Claude session.
pub fn is_probe_process(pid: u32) -> bool {
    probe_pids()
        .lock()
        .map(|pids| pids.contains(&pid))
        .unwrap_or(false)
}

/// The `initialize` response (`commands`, `models`, `agents`...), cached per folder.
pub fn initialize(working_directory: &str) -> Result<Value, String> {
    static CACHE: OnceLock<Mutex<HashMap<String, (Instant, Value)>>> = OnceLock::new();
    static PROBES: Mutex<()> = Mutex::new(());
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let cached = || {
        cache
            .lock()
            .ok()
            .and_then(|cache| cache.get(working_directory).cloned())
            .filter(|(created, _)| created.elapsed() < INITIALIZE_CACHE_TTL)
            .map(|(_, value)| value)
    };
    if let Some(value) = cached() {
        return Ok(value);
    }
    // Several panes opening at once ask the same thing: one probe runs, the others wait
    // for it and read its answer from the cache.
    let _probe = PROBES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(value) = cached() {
        return Ok(value);
    }
    let value = request(working_directory, &["initialize"])?
        .remove("initialize")
        .ok_or("Claude did not answer the initialize request")?;
    if let Ok(mut cache) = cache.lock() {
        cache.insert(
            working_directory.to_string(),
            (Instant::now(), value.clone()),
        );
    }
    Ok(value)
}

/// The account limits that `/usage` shows.
pub fn usage(working_directory: &str) -> Result<Vec<AgentRateLimit>, String> {
    let response = request(working_directory, &["initialize", "get_usage"])?
        .remove("get_usage")
        .ok_or("Claude did not answer the usage request")?;
    Ok(rate_limits(&response))
}

fn request(working_directory: &str, subtypes: &[&str]) -> Result<HashMap<String, Value>, String> {
    let mut command = crate::executables::command("claude")?;
    command
        .args([
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--settings",
            r#"{"disableAllHooks":true}"#,
        ])
        .current_dir(working_directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("Could not start Claude: {error}"))?;
    let pid = child.id();
    if let Ok(mut pids) = probe_pids().lock() {
        pids.insert(pid);
    }
    let result = exchange(&mut child, subtypes);
    let _ = child.kill();
    let _ = child.wait();
    if let Ok(mut pids) = probe_pids().lock() {
        pids.remove(&pid);
    }
    result
}

fn exchange(
    child: &mut std::process::Child,
    subtypes: &[&str],
) -> Result<HashMap<String, Value>, String> {
    let stdout = child.stdout.take().ok_or("Claude closed its output")?;
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    // Claude exits once stdin closes, so it stays open until every answer arrives.
    let mut stdin = child.stdin.take().ok_or("Claude closed its input")?;
    for subtype in subtypes {
        let request = json!({
            "type": "control_request",
            "request_id": subtype,
            "request": { "subtype": subtype },
        });
        writeln!(stdin, "{request}").map_err(|error| error.to_string())?;
    }
    stdin.flush().map_err(|error| error.to_string())?;

    let deadline = Instant::now() + PROBE_TIMEOUT;
    let mut responses = HashMap::new();
    while responses.len() < subtypes.len() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let line = receiver
            .recv_timeout(remaining)
            .map_err(|_| "Claude did not answer in time".to_string())?;
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if message.get("type").and_then(Value::as_str) != Some("control_response") {
            continue;
        }
        let response = &message["response"];
        let Some(id) = response.get("request_id").and_then(Value::as_str) else {
            continue;
        };
        if response.get("subtype").and_then(Value::as_str) == Some("error") {
            return Err(response
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("Claude rejected the request")
                .to_string());
        }
        responses.insert(id.to_string(), response["response"].clone());
    }
    drop(stdin);
    Ok(responses)
}

pub fn model_catalog(initialize: &Value) -> ClaudeModelCatalog {
    let models = initialize
        .get("models")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let resolved = |model: &Value| {
        model
            .get("resolvedModel")
            .or_else(|| model.get("value"))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let default_model = models
        .iter()
        .find(|model| model.get("value").and_then(Value::as_str) == Some("default"))
        .and_then(resolved);
    let mut catalog = ClaudeModelCatalog::default();
    for model in &models {
        let (Some(value), Some(id)) = (model.get("value").and_then(Value::as_str), resolved(model))
        else {
            continue;
        };
        catalog.aliases.insert(value.to_string(), id.clone());
        // `default` is an alias of another listed model, not a model of its own.
        if value == "default" || catalog.options.iter().any(|option| option.model == id) {
            continue;
        }
        let efforts = model
            .get("supportedEffortLevels")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|effort| ClaudeEffortOption {
                value: effort.to_string(),
                description: String::new(),
            })
            .collect::<Vec<_>>();
        let default_effort = if efforts.iter().any(|effort| effort.value == FALLBACK_EFFORT) {
            FALLBACK_EFFORT.to_string()
        } else {
            efforts
                .last()
                .map(|effort| effort.value.clone())
                .unwrap_or_default()
        };
        catalog.options.push(ClaudeModelOption {
            is_default: default_model.as_deref() == Some(id.as_str()),
            model: id,
            display_name: model
                .get("displayName")
                .and_then(Value::as_str)
                .unwrap_or(value)
                .to_string(),
            description: model
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            default_reasoning_effort: default_effort,
            supported_reasoning_efforts: efforts,
            supports_auto_mode: model
                .get("supportsAutoMode")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        });
    }
    catalog
}

impl ClaudeModelCatalog {
    pub fn option(&self, model: &str) -> Option<&ClaudeModelOption> {
        let model = model.trim();
        let id = self
            .aliases
            .get(model)
            .or_else(|| self.aliases.get(&model.to_ascii_lowercase()))
            .map(String::as_str)
            .unwrap_or(model);
        self.options.iter().find(|option| option.model == id)
    }
}

/// The model and effort Claude will run with: this session's Lume choice, the
/// model the CLI is using in this conversation, the one used in the most recent
/// Claude session, then the CLI default.
pub fn effective_settings(
    catalog: &ClaudeModelCatalog,
    sources: ClaudeModelSources,
) -> ClaudeModelSettings {
    let settings_model = sources
        .settings
        .and_then(|settings| settings.get("model"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let option = [
        sources
            .session_override
            .and_then(|value| value.model.clone()),
        sources.current_model,
        sources.last_session_model,
        settings_model,
        Some("default".to_string()),
    ]
    .into_iter()
    .flatten()
    .find_map(|model| catalog.option(&model))
    .or_else(|| catalog.options.first());
    let Some(option) = option else {
        return ClaudeModelSettings {
            model: String::new(),
            reasoning_effort: None,
            models: Vec::new(),
        };
    };
    let supports = |effort: &str| {
        option
            .supported_reasoning_efforts
            .iter()
            .any(|supported| supported.value == effort)
    };
    let saved_effort = |settings: &Value| {
        settings
            .pointer(&format!("/modelSettings/{}/effortLevel", option.model))
            .or_else(|| settings.get("effortLevel"))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let effort = [
        sources
            .session_override
            .and_then(|value| value.reasoning_effort.clone()),
        sources.settings.and_then(saved_effort),
        Some(option.default_reasoning_effort.clone()),
    ]
    .into_iter()
    .flatten()
    .find(|effort| supports(effort));
    ClaudeModelSettings {
        model: option.model.clone(),
        reasoning_effort: effort,
        models: catalog.options.clone(),
    }
}

pub fn rate_limits(usage: &Value) -> Vec<AgentRateLimit> {
    let windows = usage
        .pointer("/rate_limits/limits")
        .and_then(Value::as_array);
    let Some(windows) = windows else {
        return legacy_rate_limits(usage);
    };
    windows
        .iter()
        .filter_map(|window| {
            let kind = window.get("kind").and_then(Value::as_str)?;
            let group = window.get("group").and_then(Value::as_str).unwrap_or(kind);
            let (label, minutes) = match group {
                "session" => ("5h", Some(5 * 60)),
                "weekly" => ("7d", Some(7 * 24 * 60)),
                _ => (group, None),
            };
            Some(AgentRateLimit {
                id: format!("claude:{kind}"),
                label: label.to_string(),
                used_percent: percent(window.get("percent")),
                resets_at: reset_millis(window.get("resets_at")),
                window_minutes: minutes,
            })
        })
        .collect()
}

fn legacy_rate_limits(usage: &Value) -> Vec<AgentRateLimit> {
    [
        ("five_hour", "5h", 5 * 60),
        ("seven_day", "7d", 7 * 24 * 60),
    ]
    .into_iter()
    .filter_map(|(key, label, minutes)| {
        let window = usage
            .pointer(&format!("/rate_limits/{key}"))
            .filter(|window| !window.is_null())?;
        Some(AgentRateLimit {
            id: format!("claude:{key}"),
            label: label.to_string(),
            used_percent: percent(window.get("utilization")),
            resets_at: reset_millis(window.get("resets_at")),
            window_minutes: Some(minutes),
        })
    })
    .collect()
}

fn percent(value: Option<&Value>) -> u8 {
    value
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        .round()
        .clamp(0.0, 100.0) as u8
}

fn reset_millis(value: Option<&Value>) -> Option<i64> {
    value
        .and_then(Value::as_str)
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn initialize_response() -> Value {
        json!({ "models": [
            { "value": "default", "resolvedModel": "claude-sonnet-5-5", "displayName": "Default (recommended)",
              "supportsEffort": true, "supportedEffortLevels": ["low", "medium", "high", "xhigh", "max"] },
            { "value": "opus", "resolvedModel": "claude-opus-5-5", "displayName": "Opus 5.5", "supportsAutoMode": true,
              "supportsEffort": true, "supportedEffortLevels": ["low", "medium", "high", "xhigh", "max", "ultra"] },
            { "value": "sonnet", "resolvedModel": "claude-sonnet-5-5", "displayName": "Sonnet 5.5",
              "supportsEffort": true, "supportedEffortLevels": ["low", "medium", "high", "xhigh", "max"] },
            { "value": "haiku", "resolvedModel": "claude-haiku-4-5-20251001", "displayName": "Haiku 4.5" },
            { "value": "claude-sonnet-4-6", "resolvedModel": "claude-sonnet-4-6", "displayName": "Sonnet 4.6",
              "supportsEffort": true, "supportedEffortLevels": ["low", "medium", "high", "max"] }
        ]})
    }

    #[test]
    fn catalog_lists_versioned_models_with_their_own_efforts() {
        let catalog = model_catalog(&initialize_response());
        let models = catalog
            .options
            .iter()
            .map(|option| (option.model.as_str(), option.is_default))
            .collect::<Vec<_>>();
        assert_eq!(
            models,
            vec![
                ("claude-opus-5-5", false),
                ("claude-sonnet-5-5", true),
                ("claude-haiku-4-5-20251001", false),
                ("claude-sonnet-4-6", false),
            ]
        );
        let efforts = |model: &str| {
            catalog
                .option(model)
                .unwrap()
                .supported_reasoning_efforts
                .iter()
                .map(|effort| effort.value.clone())
                .collect::<Vec<_>>()
        };
        assert!(efforts("opus").contains(&"ultra".to_string()));
        assert!(catalog.option("opus").unwrap().supports_auto_mode);
        assert!(!catalog.option("haiku").unwrap().supports_auto_mode);
        assert!(!efforts("claude-sonnet-4-6").contains(&"xhigh".to_string()));
        assert!(efforts("haiku").is_empty());
    }

    #[test]
    fn effective_model_follows_session_then_cli_then_last_session_then_default() {
        let catalog = model_catalog(&initialize_response());
        let settings = json!({ "model": "opus", "modelSettings": { "claude-opus-5-5": { "effortLevel": "max" } } });
        let resolve = |sources| effective_settings(&catalog, sources);

        let chosen = SessionModelOverride {
            model: Some("haiku".into()),
            reasoning_effort: Some("high".into()),
        };
        let from_session = resolve(ClaudeModelSources {
            session_override: Some(&chosen),
            current_model: Some("claude-sonnet-4-6".into()),
            ..Default::default()
        });
        assert_eq!(from_session.model, "claude-haiku-4-5-20251001");
        assert_eq!(
            from_session.reasoning_effort, None,
            "Haiku has no effort levels"
        );

        let from_cli = resolve(ClaudeModelSources {
            current_model: Some("claude-sonnet-4-6".into()),
            last_session_model: Some("claude-opus-5-5".into()),
            settings: Some(&settings),
            ..Default::default()
        });
        assert_eq!(from_cli.model, "claude-sonnet-4-6");
        assert_eq!(from_cli.reasoning_effort.as_deref(), Some("high"));

        let from_last = resolve(ClaudeModelSources {
            last_session_model: Some("claude-opus-5-5".into()),
            settings: Some(&settings),
            ..Default::default()
        });
        assert_eq!(from_last.model, "claude-opus-5-5");
        assert_eq!(from_last.reasoning_effort.as_deref(), Some("max"));

        let from_default = resolve(ClaudeModelSources::default());
        assert_eq!(from_default.model, "claude-sonnet-5-5");
    }

    #[test]
    fn permission_modes_follow_what_the_model_supports() {
        let without_auto = permission_settings("default", false);
        assert_eq!(
            without_auto.modes,
            ["default", "acceptEdits", "plan", "bypassPermissions"]
        );
        let with_auto = permission_settings("auto", true);
        assert_eq!(
            with_auto.modes,
            [
                "default",
                "acceptEdits",
                "plan",
                "auto",
                "bypassPermissions"
            ]
        );
        // A mode the CLI is in that Lume does not list stays selectable.
        assert_eq!(
            permission_settings("dontAsk", true)
                .modes
                .last()
                .map(String::as_str),
            Some("dontAsk")
        );
        assert!(is_permission_mode("auto") && !is_permission_mode("manual"));
    }

    #[test]
    fn usage_maps_session_and_weekly_limits() {
        let usage = json!({ "rate_limits": { "limits": [
            { "kind": "session", "group": "session", "percent": 25, "resets_at": "2026-10-06T20:40:00+00:00" },
            { "kind": "weekly_all", "group": "weekly", "percent": 4.4, "resets_at": "2026-10-11T18:00:00+00:00" }
        ]}});
        let limits = rate_limits(&usage);
        assert_eq!(limits.len(), 2);
        assert_eq!(
            (
                limits[0].label.as_str(),
                limits[0].used_percent,
                limits[0].window_minutes
            ),
            ("5h", 25, Some(300))
        );
        assert_eq!(
            (
                limits[1].label.as_str(),
                limits[1].used_percent,
                limits[1].window_minutes
            ),
            ("7d", 4, Some(10080))
        );
        assert_eq!(limits[0].resets_at, Some(1_791_319_200_000));
    }
}
