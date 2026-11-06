//! Keeps the account usage of Codex and Claude Code fresh without anyone opening the inspector,
//! and tells the user (desktop notification) when a window runs low or is used up.
use std::{
    collections::HashMap,
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

fn tracker() -> &'static Mutex<UsageTracker> {
    static TRACKER: OnceLock<Mutex<UsageTracker>> = OnceLock::new();
    TRACKER.get_or_init(|| Mutex::new(UsageTracker::default()))
}

fn remaining(limit: &AgentRateLimit) -> i64 {
    100 - i64::from(limit.used_percent.min(100))
}

fn agent_label(agent: &AgentKind) -> &'static str {
    match agent {
        AgentKind::Codex => "Codex",
        AgentKind::ClaudeCode => "Claude Code",
        AgentKind::Antigravity => "Antigravity",
        AgentKind::Gemini => "Gemini",
        AgentKind::OpenCode => "OpenCode",
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
    let (changed, alerts, resets) = {
        let mut tracker = tracker()
            .lock()
            .map_err(|_| "Could not monitor agent usage".to_string())?;
        let (limits, alerts, resets) = tracker.observe(&agent, limits, crate::state::now_millis());
        let changed = state.set_agent_rate_limits(agent.clone(), limits)?;
        (changed, alerts, resets)
    };
    if changed {
        protocol::emit_sessions_changed(app);
    }
    for reset in resets {
        let available = remaining(&reset) as u8;
        protocol::emit_usage_reset(app, agent.clone(), reset.id, reset.label, available);
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

/// Shows why the limits could not be read instead of leaving the gauges empty.
pub fn publish_error(
    app: &AppHandle,
    state: &AppState,
    agent: AgentKind,
    error: String,
) -> Result<bool, String> {
    let changed = state.set_agent_rate_limits_error(agent, error)?;
    if changed {
        protocol::emit_sessions_changed(app);
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

#[derive(Default)]
struct UsageTracker {
    windows: HashMap<(AgentKind, String), TrackedWindow>,
}

#[derive(Default)]
struct TrackedWindow {
    reset_at: Option<i64>,
    warned_low: bool,
    warned_exhausted: bool,
}

/// A reset is reported only after the previous deadline and when the provider reports a new,
/// later window. A first snapshot or a drifting clock is not a reset.
impl UsageTracker {
    fn tracks(&self, agent: &AgentKind) -> bool {
        self.windows.keys().any(|(kind, _)| kind == agent)
    }

    fn reset_due(&self, agent: &AgentKind, now: i64) -> bool {
        self.windows
            .iter()
            .any(|((kind, _), window)| kind == agent && window.reset_at.is_some_and(|at| at <= now))
    }

    fn observe(
        &mut self,
        agent: &AgentKind,
        mut limits: Vec<AgentRateLimit>,
        now: i64,
    ) -> (Vec<AgentRateLimit>, Vec<UsageAlert>, Vec<AgentRateLimit>) {
        let mut alerts = Vec::new();
        let mut resets = Vec::new();
        for limit in &mut limits {
            let key = (agent.clone(), limit.id.clone());
            let known = self.windows.contains_key(&key);
            if !known && limit.resets_at.is_some_and(|reset_at| reset_at <= now) {
                continue;
            }
            let window = self.windows.entry(key).or_default();
            let tolerance = limit
                .window_minutes
                .unwrap_or(10)
                .saturating_mul(60_000)
                .saturating_div(5)
                .clamp(2_000, 120_000);
            let reset = known
                && window
                    .reset_at
                    .zip(limit.resets_at)
                    .is_some_and(|(previous, next)| {
                        previous <= now && next > now && next.saturating_sub(previous) > tolerance
                    });
            if reset {
                window.reset_at = limit.resets_at;
                window.warned_low = false;
                window.warned_exhausted = false;
                resets.push(limit.clone());
            } else if let Some(previous) = window.reset_at {
                // Small clock drift is the same deadline. If a provider materially corrects a
                // future deadline, show the correction without inventing a completed reset.
                match limit.resets_at {
                    Some(next)
                        if previous.saturating_sub(now) > tolerance
                            && next > now
                            && next.abs_diff(previous) > tolerance as u64 =>
                    {
                        window.reset_at = Some(next);
                    }
                    _ => limit.resets_at = Some(previous),
                }
            } else if limit.resets_at.is_some() {
                window.reset_at = limit.resets_at;
            }
            let remaining = remaining(limit);
            if limit.resets_at.is_some_and(|reset_at| reset_at <= now)
                || remaining > LOW_REMAINING_PERCENT
            {
                continue;
            }
            let already_warned = if remaining == 0 {
                &mut window.warned_exhausted
            } else {
                &mut window.warned_low
            };
            if !*already_warned {
                *already_warned = true;
                alerts.push(UsageAlert {
                    agent: agent.clone(),
                    window: limit.label.clone(),
                    remaining,
                });
            }
        }
        (limits, alerts, resets)
    }
}

/// Polls the accounts of the agents in use: faster while a session runs or a window is tight.
pub fn start(state: AppState, app: AppHandle) {
    thread::spawn(move || {
        let mut last: [(AgentKind, Option<Instant>); 3] = [
            (AgentKind::Codex, None),
            (AgentKind::ClaudeCode, None),
            (AgentKind::Antigravity, None),
        ];
        loop {
            let sessions = state.sessions().unwrap_or_default();
            for (agent, refreshed) in &mut last {
                let mine: Vec<_> = sessions
                    .iter()
                    .filter(|session| session.agent == *agent)
                    .collect();
                let (tracked, reset_due) = tracker()
                    .lock()
                    .map(|tracker| {
                        (
                            tracker.tracks(agent),
                            tracker.reset_due(agent, crate::state::now_millis()),
                        )
                    })
                    .unwrap_or_default();
                if mine.is_empty() && !tracked {
                    continue;
                }
                // Antigravity's usage probe starts a CLI process. Only run it when an already-read
                // window reaches its reset time, rather than creating a process on every poll.
                if *agent == AgentKind::Antigravity && !reset_due {
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
                let every = if *agent == AgentKind::Antigravity {
                    IDLE_REFRESH
                } else if tight {
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
                    AgentKind::ClaudeCode => {
                        crate::refresh_claude_rate_limits(app.clone(), state.clone())
                    }
                    AgentKind::Antigravity => {
                        crate::refresh_antigravity_rate_limits(app.clone(), state.clone())
                    }
                    _ => {}
                }
            }
            thread::sleep(TICK);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limit(id: &str, used: u8, resets_at: Option<i64>) -> AgentRateLimit {
        AgentRateLimit {
            id: id.into(),
            label: "5h".into(),
            used_percent: used,
            resets_at,
            window_minutes: Some(300),
        }
    }

    #[test]
    fn warnings_are_once_per_level_and_timestamp_drift_does_not_reopen_a_banner() {
        let mut tracker = UsageTracker::default();
        let now = 1_800_000_000_000;
        let reset = now + 1_800_000 - 1_000;
        let (_, first, resets) = tracker.observe(
            &AgentKind::Codex,
            vec![limit("primary", 85, Some(reset))],
            now,
        );
        assert_eq!(first.len(), 1);
        assert!(resets.is_empty());
        let (stabilized, alerts, resets) = tracker.observe(
            &AgentKind::Codex,
            vec![limit("primary", 86, Some(reset + 4_000))],
            now + 4_000,
        );
        assert_eq!(stabilized[0].resets_at, Some(reset));
        assert!(alerts.is_empty() && resets.is_empty());
        let (_, empty, _) = tracker.observe(
            &AgentKind::Codex,
            vec![limit("primary", 100, Some(reset + 5_000))],
            now + 5_000,
        );
        assert_eq!(empty.len(), 1);
        assert_eq!(empty[0].remaining, 0);
        assert!(tracker
            .observe(
                &AgentKind::Codex,
                vec![limit("primary", 100, Some(reset + 10_000))],
                now + 10_000
            )
            .1
            .is_empty());
    }

    #[test]
    fn confirmed_reset_is_emitted_once_and_rearms_only_that_window() {
        let mut tracker = UsageTracker::default();
        let now = 1_800_000_000_000;
        let reset = now + 60_000;
        tracker.observe(
            &AgentKind::ClaudeCode,
            vec![
                limit("5h", 100, Some(reset)),
                limit("7d", 100, Some(reset + 500_000_000)),
            ],
            now,
        );
        let next = reset + 5 * 60 * 60_000;
        let (_, alerts, resets) = tracker.observe(
            &AgentKind::ClaudeCode,
            vec![limit("5h", 3, Some(next))],
            reset + 1,
        );
        assert!(alerts.is_empty());
        assert_eq!(
            resets
                .iter()
                .map(|limit| limit.id.as_str())
                .collect::<Vec<_>>(),
            ["5h"]
        );
        assert!(tracker
            .observe(
                &AgentKind::ClaudeCode,
                vec![limit("5h", 3, Some(next + 2_000))],
                reset + 3_000
            )
            .2
            .is_empty());
        assert_eq!(
            tracker
                .observe(
                    &AgentKind::ClaudeCode,
                    vec![limit("5h", 100, Some(next))],
                    reset + 4_000
                )
                .1
                .len(),
            1
        );
        assert!(tracker
            .observe(
                &AgentKind::ClaudeCode,
                vec![limit("7d", 100, Some(reset + 500_000_000))],
                reset + 5_000
            )
            .1
            .is_empty());
    }

    #[test]
    fn stale_data_missing_deadlines_and_uncertain_corrections_do_not_invent_resets() {
        let mut tracker = UsageTracker::default();
        let now = 1_800_000_000_000;
        assert!(tracker
            .observe(
                &AgentKind::Codex,
                vec![limit("stale", 100, Some(now - 1))],
                now
            )
            .1
            .is_empty());
        assert!(
            tracker
                .observe(
                    &AgentKind::Codex,
                    vec![limit("stale", 0, Some(now + 60_000))],
                    now
                )
                .2
                .is_empty(),
            "the first fresh reading is a baseline"
        );
        tracker.observe(&AgentKind::Codex, vec![limit("no-time", 100, None)], now);
        assert!(tracker
            .observe(
                &AgentKind::Codex,
                vec![limit("no-time", 0, None)],
                now + 1_000
            )
            .2
            .is_empty());
        let previous = now + 600_000;
        tracker.observe(
            &AgentKind::Codex,
            vec![limit("corrected", 100, Some(previous))],
            now,
        );
        let corrected = previous + 5 * 60 * 60_000;
        let (limits, _, resets) = tracker.observe(
            &AgentKind::Codex,
            vec![limit("corrected", 40, Some(corrected))],
            now + 1_000,
        );
        assert!(
            resets.is_empty(),
            "a new timestamp alone before the deadline is not a reset"
        );
        assert_eq!(
            limits[0].resets_at,
            Some(corrected),
            "material deadline corrections remain visible"
        );
        assert!(tracker
            .observe(
                &AgentKind::Codex,
                vec![limit("corrected", 0, Some(corrected))],
                previous + 2
            )
            .2
            .is_empty());
        let next = corrected + 5 * 60 * 60_000;
        let (_, alerts, resets) = tracker.observe(
            &AgentKind::Codex,
            vec![limit("corrected", 100, Some(next))],
            corrected + 1,
        );
        assert_eq!(
            resets.len(),
            1,
            "a new window is reported even if already exhausted"
        );
        assert_eq!(remaining(&resets[0]), 0);
        assert_eq!(
            alerts.len(),
            1,
            "the new exhausted window may also raise its own alert"
        );
        assert!(tracker
            .observe(
                &AgentKind::Codex,
                vec![limit("corrected", 0, Some(next))],
                corrected + 2
            )
            .2
            .is_empty());
    }

    #[test]
    fn a_new_window_reported_just_before_the_deadline_is_confirmed_after_it() {
        let mut tracker = UsageTracker::default();
        let now = 1_800_000_000_000;
        let deadline = now + 60_000;
        let next = deadline + 5 * 60 * 60_000;
        tracker.observe(
            &AgentKind::ClaudeCode,
            vec![limit("5h", 100, Some(deadline))],
            now,
        );
        assert!(tracker
            .observe(
                &AgentKind::ClaudeCode,
                vec![limit("5h", 0, Some(next))],
                deadline - 20_000
            )
            .2
            .is_empty());
        assert_eq!(
            tracker
                .observe(
                    &AgentKind::ClaudeCode,
                    vec![limit("5h", 0, Some(next))],
                    deadline + 1
                )
                .2
                .len(),
            1
        );
    }
}
