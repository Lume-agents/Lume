//! Keeps the account usage of Codex and Claude Code fresh without anyone opening the inspector,
//! and tells the user (desktop notification) when a window runs low or is used up.
use std::{
    collections::HashSet,
    sync::{Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};

use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::{
    codex_bridge::CodexBridge,
    domain::{AgentKind, AgentRateLimit, SessionStatus},
    protocol,
    state::AppState,
};

/// A window with this much (or less) left raises an alert.
pub const LOW_REMAINING_PERCENT: i64 = 20;
const TICK: Duration = Duration::from_secs(15);
const IDLE_REFRESH: Duration = Duration::from_secs(120);
const ACTIVE_REFRESH: Duration = Duration::from_secs(45);
const TIGHT_REFRESH: Duration = Duration::from_secs(25);

fn notified() -> &'static Mutex<HashSet<String>> {
    static NOTIFIED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    NOTIFIED.get_or_init(|| Mutex::new(HashSet::new()))
}

fn remaining(limit: &AgentRateLimit) -> i64 {
    100 - i64::from(limit.used_percent.min(100))
}

fn agent_label(agent: &AgentKind) -> &'static str {
    match agent {
        AgentKind::Codex => "Codex",
        AgentKind::ClaudeCode => "Claude Code",
        _ => "Agente",
    }
}

/// Stores the limits, announces any window that newly ran low, and refreshes the UI.
pub fn publish(
    app: &AppHandle,
    state: &AppState,
    agent: AgentKind,
    limits: Vec<AgentRateLimit>,
) -> Result<bool, String> {
    let alerts = new_alerts(&agent, &limits);
    let changed = state.set_agent_rate_limits(agent, limits)?;
    if changed {
        protocol::emit_sessions_changed(app);
    }
    if !alerts.is_empty()
        && state
            .preferences()
            .map(|p| p.popup_notifications_enabled)
            .unwrap_or(false)
    {
        let english = state
            .preferences()
            .map(|p| p.language != "pt-BR")
            .unwrap_or(false);
        for (title, body) in alerts.iter().map(|alert| alert.text(english)) {
            let _ = app.notification().builder().title(title).body(body).show();
        }
    }
    Ok(changed)
}

struct UsageAlert {
    agent: AgentKind,
    window: String,
    remaining: i64,
}

impl UsageAlert {
    fn text(&self, english: bool) -> (String, String) {
        let name = agent_label(&self.agent);
        let window = if self.window.trim().is_empty() {
            if english {
                "current window"
            } else {
                "janela atual"
            }
            .to_string()
        } else {
            self.window.clone()
        };
        match (self.remaining == 0, english) {
            (true, true) => (
                "Lume · Usage limit reached".into(),
                format!("{name} used up its {window} limit."),
            ),
            (true, false) => (
                "Lume · Limite de uso atingido".into(),
                format!("{name} esgotou o limite de {window}."),
            ),
            (false, true) => (
                "Lume · Usage running low".into(),
                format!(
                    "{name}: only {}% of the {window} limit is left.",
                    self.remaining
                ),
            ),
            (false, false) => (
                "Lume · Uso quase no limite".into(),
                format!(
                    "{name}: restam apenas {}% do limite de {window}.",
                    self.remaining
                ),
            ),
        }
    }
}

/// Windows that are low (or empty) and were not announced yet for this reset period.
fn new_alerts(agent: &AgentKind, limits: &[AgentRateLimit]) -> Vec<UsageAlert> {
    let Ok(mut seen) = notified().lock() else {
        return Vec::new();
    };
    limits
        .iter()
        .filter(|limit| remaining(limit) <= LOW_REMAINING_PERCENT)
        .filter(|limit| {
            let level = if remaining(limit) == 0 {
                "exhausted"
            } else {
                "low"
            };
            let key = format!(
                "{agent:?}:{}:{}:{level}",
                limit.id,
                limit
                    .resets_at
                    .map_or("current".to_string(), |at| at.to_string())
            );
            seen.insert(key)
        })
        .map(|limit| UsageAlert {
            agent: agent.clone(),
            window: limit.label.clone(),
            remaining: remaining(limit),
        })
        .collect()
}

/// Polls the accounts of the agents in use: faster while a session runs or a window is tight.
pub fn start(state: AppState, app: AppHandle) {
    thread::spawn(move || {
        let mut last: [(AgentKind, Option<Instant>); 2] =
            [(AgentKind::Codex, None), (AgentKind::ClaudeCode, None)];
        loop {
            let sessions = state.sessions().unwrap_or_default();
            for (agent, refreshed) in &mut last {
                let mine: Vec<_> = sessions
                    .iter()
                    .filter(|session| session.agent == *agent)
                    .collect();
                if mine.is_empty() {
                    continue;
                }
                let working = mine.iter().any(|session| {
                    matches!(
                        session.status,
                        SessionStatus::Running | SessionStatus::PermissionRequired
                    )
                });
                let tight = mine
                    .iter()
                    .flat_map(|session| session.rate_limits.iter())
                    .any(|limit| remaining(limit) <= LOW_REMAINING_PERCENT + 10);
                let every = if tight {
                    TIGHT_REFRESH
                } else if working {
                    ACTIVE_REFRESH
                } else {
                    IDLE_REFRESH
                };
                if refreshed.is_some_and(|at| at.elapsed() < every) {
                    continue;
                }
                *refreshed = Some(Instant::now());
                match &*agent {
                    AgentKind::Codex => {
                        let bridge = app.state::<CodexBridge>();
                        let _ = bridge.refresh_rate_limits(&state, &app);
                    }
                    _ => crate::refresh_claude_rate_limits(app.clone(), state.clone()),
                }
            }
            thread::sleep(TICK);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limit(id: &str, used: u8) -> AgentRateLimit {
        AgentRateLimit {
            id: id.into(),
            label: "5h".into(),
            used_percent: used,
            resets_at: Some(10),
            window_minutes: Some(300),
        }
    }

    #[test]
    fn only_low_windows_alert_and_only_once_per_level() {
        let first = new_alerts(
            &AgentKind::Codex,
            &[limit("test-a", 50), limit("test-b", 85)],
        );
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].remaining, 15);
        assert!(new_alerts(&AgentKind::Codex, &[limit("test-b", 86)]).is_empty());
        let empty = new_alerts(&AgentKind::Codex, &[limit("test-b", 100)]);
        assert_eq!(empty.len(), 1);
        assert_eq!(empty[0].remaining, 0);
    }
}
