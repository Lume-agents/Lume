use std::{
    collections::{HashSet, VecDeque},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, OnceLock,
    },
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter};

use crate::{
    domain::{
        AgentKind, AgentSession, InternalService, PermissionAction, PromptAttachmentInput,
        PromptDelivery, QuestionAnswer, SessionActivity, SessionControlOrigin, SessionSource,
        SessionStatus, WorkflowGroupDefinition, WorkflowHistoryRecord,
    },
    state::{now_millis, visible_subagent_ids},
};

pub const PROTOCOL_VERSION: u16 = 1;
pub const PROTOCOL_FEATURES: &[&str] = &[
    "sessions",
    "activity",
    "results",
    "files",
    "prompts",
    "image_prompts",
    "rate_limits",
    "permissions",
    "interactive_questions",
    "termination",
    "realtime_stream",
    "coordinated_updates",
    "work_status",
    "prompt_interruption",
    "prompt_delivery",
    "response_files",
    "session_takeover",
    "session_model_settings",
    "workflows",
    "workflow_history",
];
pub const STREAM_HEARTBEAT_INTERVAL_MS: u64 = 15_000;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptUnavailableReason {
    UnsupportedAgent,
    SessionNotConnected,
    WorkingDirectoryMissing,
    AgentBusy,
    ExternalSession,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCapabilities {
    pub can_prompt: bool,
    pub prompt_unavailable_reason: Option<PromptUnavailableReason>,
    pub can_approve: bool,
    pub can_answer_question: bool,
    pub can_terminate: bool,
    pub can_open_source: bool,
    pub can_read_results: bool,
    pub can_attach_images: bool,
    pub can_interrupt: bool,
    pub can_take_control: bool,
    pub can_compact: bool,
    pub prompt_deliveries: Vec<PromptDelivery>,
}

impl SessionCapabilities {
    pub fn for_session(session: &AgentSession) -> Self {
        let monitor_only = is_unbound_codex_cli_monitor(session);
        let prompt_unavailable_reason = if session.source == SessionSource::Web
            && matches!(
                session.status,
                SessionStatus::Running | SessionStatus::PermissionRequired
            ) {
            Some(PromptUnavailableReason::AgentBusy)
        } else if session.source == SessionSource::Web {
            None
        } else if session.agent == AgentKind::Unknown {
            Some(PromptUnavailableReason::UnsupportedAgent)
        } else if session.native_session_id.is_none() {
            Some(PromptUnavailableReason::SessionNotConnected)
        } else if session.control_origin == SessionControlOrigin::External
            && matches!(
                session.agent,
                AgentKind::Codex | AgentKind::ClaudeCode | AgentKind::OpenCode | AgentKind::Omp
            )
        {
            Some(PromptUnavailableReason::ExternalSession)
        } else if session.agent != AgentKind::Codex && session.working_directory.is_none() {
            Some(PromptUnavailableReason::WorkingDirectoryMissing)
        } else {
            None
        };
        Self {
            can_prompt: prompt_unavailable_reason.is_none(),
            prompt_unavailable_reason,
            can_approve: session.pending_permission.is_some()
                && session.permission_profile.can_respond_from_lume
                && !monitor_only,
            can_answer_question: session.pending_question.is_some() && !monitor_only,
            can_terminate: (session.agent == AgentKind::Omp
                && session.control_origin == SessionControlOrigin::Lume
                && has_nonempty_value(session.native_session_id.as_deref()))
                || ((session.source == SessionSource::Cli && session.process_id.is_some())
                    || (matches!(
                        session.agent,
                        AgentKind::Codex | AgentKind::OpenCode | AgentKind::Antigravity
                    ) && session.source == SessionSource::Desktop
                        && session.control_origin == SessionControlOrigin::Lume
                        && has_nonempty_value(session.native_session_id.as_deref()))
                    || (session.agent == AgentKind::ClaudeCode
                        && matches!(session.source, SessionSource::Cli | SessionSource::Desktop)
                        && session.process_id.is_none()
                        && session.control_origin == SessionControlOrigin::Lume
                        && has_nonempty_value(session.native_session_id.as_deref()))),
            can_open_source: matches!(session.source, SessionSource::Web | SessionSource::Vscode),
            can_read_results: !session.results.is_empty() || session.last_response.is_some(),
            can_attach_images: session.source != SessionSource::Web
                && session.agent != AgentKind::Unknown
                && !monitor_only
                && !(session.agent == AgentKind::Antigravity
                    && session.source == SessionSource::Desktop),
            can_interrupt: matches!(
                session.status,
                SessionStatus::Running | SessionStatus::PermissionRequired
            ) && session.control_origin == SessionControlOrigin::Lume
                && can_interrupt_session(session),
            can_take_control: session.control_origin == SessionControlOrigin::External
                && session.source == SessionSource::Cli
                && matches!(session.agent, AgentKind::Codex | AgentKind::Omp)
                && has_nonempty_value(session.native_session_id.as_deref())
                && has_nonempty_value(session.working_directory.as_deref())
                && (session.agent == AgentKind::Omp || session.process_id.is_some()),
            can_compact: session.agent == AgentKind::Omp
                && session.control_origin == SessionControlOrigin::Lume
                && session.status == SessionStatus::Completed,
            prompt_deliveries: if (session.agent == AgentKind::Codex
                && session.source != SessionSource::Web
                && session.control_origin == SessionControlOrigin::Lume)
                || (session.agent == AgentKind::Omp
                    && session.control_origin == SessionControlOrigin::Lume)
            {
                vec![
                    PromptDelivery::NewTurn,
                    PromptDelivery::Steer,
                    PromptDelivery::Queue,
                ]
            } else if claude_queues_in_lume(session) {
                // Each message to a Lume-owned Claude conversation is its own run, so they queue; "steer"
                // means send now: stop the running message and run the queued one.
                vec![
                    PromptDelivery::NewTurn,
                    PromptDelivery::Queue,
                    PromptDelivery::Steer,
                ]
            } else {
                vec![PromptDelivery::NewTurn]
            },
        }
    }
}

/// A native thread observed through hooks is not evidence of a controllable CLI.
pub(crate) fn is_unbound_codex_cli_monitor(session: &AgentSession) -> bool {
    session.agent == AgentKind::Codex
        && session.source == SessionSource::Cli
        && session.control_origin == SessionControlOrigin::External
        && session.process_id.is_none()
}

fn can_interrupt_session(session: &AgentSession) -> bool {
    session.source != SessionSource::Web
        && (matches!(
            session.agent,
            AgentKind::Codex | AgentKind::OpenCode | AgentKind::Antigravity
        ) || claude_queues_in_lume(session))
        && has_nonempty_value(session.native_session_id.as_deref())
}

/// A Claude Code conversation Lume owns runs each message as its own `claude --print` process.
fn claude_queues_in_lume(session: &AgentSession) -> bool {
    session.agent == AgentKind::ClaudeCode
        && session.control_origin == SessionControlOrigin::Lume
        && matches!(session.source, SessionSource::Cli | SessionSource::Desktop)
}

fn has_nonempty_value(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.trim().is_empty())
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkItemStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkItem {
    pub label: String,
    pub status: WorkItemStatus,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoSummary {
    pub items: Vec<WorkItem>,
    pub updated_at: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanSummary {
    pub items: Vec<WorkItem>,
    pub explanation: Option<String>,
    pub content: Option<String>,
    pub updated_at: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Active,
    Complete,
    Blocked,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalSummary {
    pub objective: String,
    pub status: GoalStatus,
    pub started_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentWorkSummary {
    pub plan: Option<PlanSummary>,
    pub todo: Option<TodoSummary>,
    pub goal: Option<GoalSummary>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HubSession {
    #[serde(flatten)]
    pub session: AgentSession,
    pub capabilities: SessionCapabilities,
    pub work_summary: AgentWorkSummary,
    pub activity_total: usize,
}

impl From<AgentSession> for HubSession {
    fn from(session: AgentSession) -> Self {
        let capabilities = SessionCapabilities::for_session(&session);
        let work_summary = work_summary(&session.activities);
        let activity_total = session.activities.len();
        Self {
            session,
            capabilities,
            work_summary,
            activity_total,
        }
    }
}

fn work_summary(activities: &[SessionActivity]) -> AgentWorkSummary {
    AgentWorkSummary {
        plan: plan_summary(activities),
        todo: todo_summary(activities),
        goal: goal_summary(activities),
    }
}

fn plan_summary(activities: &[SessionActivity]) -> Option<PlanSummary> {
    activities
        .iter()
        .rev()
        .find_map(plan_document_summary)
        .or_else(|| activities.iter().rev().find_map(plan_activity_summary))
}

fn plan_document_summary(activity: &SessionActivity) -> Option<PlanSummary> {
    if activity.kind != "plan_document" {
        return None;
    }
    let content = activity.detail.as_deref()?.trim();
    if content.is_empty() {
        return None;
    }
    let mut items = Vec::new();
    for raw_line in content.lines() {
        let line = raw_line.trim();
        let (label, status) = if let Some(rest) = line
            .strip_prefix("- [x] ")
            .or_else(|| line.strip_prefix("- [X] "))
        {
            (rest, WorkItemStatus::Completed)
        } else if let Some(rest) = line.strip_prefix("- [ ] ") {
            (rest, WorkItemStatus::Pending)
        } else if let Some(rest) = line.strip_prefix("## ") {
            (rest, WorkItemStatus::Pending)
        } else {
            continue;
        };
        let label = label.trim();
        if !label.is_empty() && !items.iter().any(|item: &WorkItem| item.label == label) {
            items.push(WorkItem {
                label: label.to_string(),
                status,
            });
        }
        if items.len() == 8 {
            break;
        }
    }
    if items.is_empty() {
        items.push(WorkItem {
            label: content
                .lines()
                .next()?
                .trim_start_matches('#')
                .trim()
                .to_string(),
            status: WorkItemStatus::Pending,
        });
    }
    Some(PlanSummary {
        items,
        explanation: None,
        content: Some(content.to_string()),
        updated_at: activity.created_at,
    })
}

fn plan_activity_summary(activity: &SessionActivity) -> Option<PlanSummary> {
    let detail = activity.detail.as_deref()?;
    let (items, explanation) = if activity.kind == "plan" {
        (plan_items(detail), plan_explanation(detail))
    } else if activity_is_todo_tool(activity) {
        (todo_items(detail)?, None)
    } else {
        return None;
    };
    (!items.is_empty()).then(|| PlanSummary {
        items,
        explanation,
        content: None,
        updated_at: activity.created_at,
    })
}

fn plan_explanation(detail: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(detail).ok()?;
    find_json_value(&value, &["explanation"])
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(crate) fn archived_plan_from_activity(activity: &SessionActivity) -> Option<(String, String)> {
    let summary = plan_activity_summary(activity)?;
    let signature = summary
        .items
        .iter()
        .map(|item| item.label.trim().to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("\n");
    if signature.is_empty() {
        return None;
    }
    let mut body = String::new();
    if let Some(explanation) = summary.explanation.as_deref() {
        body.push_str(explanation);
        body.push_str("\n\n");
    }
    for item in &summary.items {
        let marker = if item.status == WorkItemStatus::Completed {
            "x"
        } else {
            " "
        };
        body.push_str(&format!("- [{marker}] {}\n", item.label));
    }
    Some((signature, body.trim().to_string()))
}

pub(crate) fn looks_like_full_plan_document(content: &str) -> bool {
    let normalized = content.to_ascii_lowercase();
    let has_plan_title = normalized.lines().any(|line| {
        let line = line.trim().trim_start_matches('#').trim();
        line.starts_with("planejamento")
            || line.starts_with("plano ")
            || line == "plano"
            || line.starts_with("implementation plan")
            || line.starts_with("workflow plan")
    });
    let section_count = content
        .lines()
        .filter(|line| line.trim_start().starts_with("## "))
        .count();
    let phase_count = normalized
        .lines()
        .filter(|line| {
            let line = line.trim().trim_start_matches('#').trim();
            line.starts_with("fase ") || line.starts_with("phase ")
        })
        .count();

    has_plan_title && (section_count >= 2 || phase_count >= 2)
}

fn todo_summary(activities: &[SessionActivity]) -> Option<TodoSummary> {
    let structured_plan = activities
        .iter()
        .rev()
        .filter(|activity| activity.kind == "plan")
        .find_map(|activity| {
            let items = plan_items(activity.detail.as_deref()?);
            (!items.is_empty()).then_some(TodoSummary {
                items,
                updated_at: activity.created_at,
            })
        });
    let tool_todo = activities
        .iter()
        .rev()
        .filter(|activity| activity_is_todo_tool(activity))
        .find_map(|activity| {
            let items = activity
                .detail
                .as_deref()
                .and_then(todo_items)
                .or_else(|| todo_items(&activity.title))?;
            (!items.is_empty()).then_some(TodoSummary {
                items,
                updated_at: activity.created_at,
            })
        });
    let message_todo = message_todo_summary(activities);

    [structured_plan, tool_todo, message_todo]
        .into_iter()
        .flatten()
        .max_by_key(|summary| summary.updated_at)
}

fn activity_is_todo_tool(activity: &SessionActivity) -> bool {
    if activity.kind != "tool" {
        return false;
    }
    let name = activity
        .title
        .rsplit(['·', '.', ':', '/'])
        .next()
        .unwrap_or(&activity.title)
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    name == "todowrite"
}

fn message_todo_summary(activities: &[SessionActivity]) -> Option<TodoSummary> {
    let (start_index, mut items) = activities
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, activity)| matches!(activity.kind.as_str(), "message" | "analysis"))
        .find_map(|(index, activity)| {
            explicit_message_todo(activity.detail.as_deref()?).map(|items| (index, items))
        })?;
    let mut updated_at = activities[start_index].created_at;

    for activity in activities.iter().skip(start_index + 1) {
        if !matches!(activity.kind.as_str(), "message" | "analysis") {
            continue;
        }
        let Some(detail) = activity.detail.as_deref() else {
            continue;
        };
        let mut changed = false;
        for line in detail.lines() {
            let Some(update) = message_task_line(line, false) else {
                continue;
            };
            if let Some(item) = items
                .iter_mut()
                .find(|item| item.label.trim().eq_ignore_ascii_case(update.label.trim()))
            {
                if item.status != update.status {
                    item.status = update.status;
                    changed = true;
                }
            }
        }
        for completed_label in completed_section_items(detail) {
            let Some(index) = best_todo_match(&items, &completed_label) else {
                continue;
            };
            if items[index].status != WorkItemStatus::Completed {
                items[index].status = WorkItemStatus::Completed;
                changed = true;
            }
        }
        if changed {
            updated_at = activity.created_at;
        }
    }

    Some(TodoSummary { items, updated_at })
}

fn completed_section_items(detail: &str) -> Vec<String> {
    let mut completed_section = false;
    let mut items = Vec::new();
    for raw_line in detail.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(label) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            if completed_section && !label.trim().is_empty() {
                items.push(label.trim().to_string());
            }
            continue;
        }
        let heading = line
            .trim_start_matches('#')
            .trim()
            .trim_end_matches(':')
            .trim()
            .to_lowercase();
        completed_section = heading.starts_with("conclu")
            || heading.starts_with("completed")
            || heading.starts_with("feito")
            || heading.starts_with("finalizado");
    }
    items
}

fn best_todo_match(items: &[WorkItem], completed_label: &str) -> Option<usize> {
    let completed_tokens = task_tokens(completed_label);
    if completed_tokens.is_empty() {
        return None;
    }
    let mut ranked = items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.status != WorkItemStatus::Completed)
        .map(|(index, item)| {
            let item_tokens = task_tokens(&item.label);
            let overlap = item_tokens.intersection(&completed_tokens).count();
            (index, overlap, item_tokens)
        })
        .filter(|(_, overlap, _)| *overlap > 0)
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.1.cmp(&left.1));
    let (index, overlap, item_tokens) = ranked.first()?;
    if *overlap >= 2 {
        return Some(*index);
    }
    if ranked
        .get(1)
        .is_some_and(|candidate| candidate.1 == *overlap)
    {
        return None;
    }
    item_tokens
        .intersection(&completed_tokens)
        .next()
        .filter(|token| token.chars().count() >= 5)
        .map(|_| *index)
}

fn task_tokens(value: &str) -> HashSet<String> {
    const STOP_WORDS: &[&str] = &[
        "para", "com", "sem", "uma", "uns", "das", "dos", "que", "the", "and", "for", "with",
        "from", "this", "that", "de", "do", "da", "em", "no", "na", "ao", "aos",
    ];
    value
        .to_lowercase()
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| token.chars().count() >= 3 && !STOP_WORDS.contains(token))
        .map(ToOwned::to_owned)
        .collect()
}

pub(crate) fn is_work_tracking_message(detail: &str) -> bool {
    explicit_message_todo(detail).is_some()
        || !completed_section_items(detail).is_empty()
        || detail
            .lines()
            .any(|line| message_task_line(line, false).is_some())
}

fn explicit_message_todo(detail: &str) -> Option<Vec<WorkItem>> {
    let mut after_heading = false;
    let mut items = Vec::new();
    for raw_line in detail.lines() {
        let line = raw_line.trim();
        if !after_heading {
            let heading = line
                .trim_start_matches('#')
                .trim()
                .trim_end_matches(':')
                .to_ascii_lowercase();
            after_heading = heading == "todo"
                || heading == "to do"
                || heading.starts_with("todo ")
                || heading.starts_with("to do ")
                || (line.ends_with(':') && is_short_todo_label(&heading));
            continue;
        }
        if let Some(item) = message_task_line(line, true) {
            if !items
                .iter()
                .any(|existing: &WorkItem| existing.label == item.label)
            {
                items.push(item);
            }
        }
        if items.len() == 12 {
            break;
        }
    }
    (!items.is_empty()).then_some(items)
}

/// A short label such as "Current todo:" or "Next to do:". The words must stand alone: a plain
/// substring match took "bruto do erro … para:" ("bru**to do**") for a to-do heading, and the bullets
/// after any such sentence became tasks.
fn is_short_todo_label(heading: &str) -> bool {
    let words = heading
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    words.len() <= 5
        && (words.contains(&"todo") || words.windows(2).any(|pair| pair == ["to", "do"]))
}

fn message_task_line(line: &str, allow_plain_bullet: bool) -> Option<WorkItem> {
    let line = line.trim();
    let (status, label) = if let Some(label) = line
        .strip_prefix("- [x] ")
        .or_else(|| line.strip_prefix("- [X] "))
    {
        (WorkItemStatus::Completed, label)
    } else if let Some(label) = line.strip_prefix("- [ ] ") {
        (WorkItemStatus::Pending, label)
    } else if let Some(label) = line.strip_prefix('✓') {
        (WorkItemStatus::Completed, label)
    } else if let Some(label) = line.strip_prefix('●') {
        (WorkItemStatus::InProgress, label)
    } else if let Some(label) = line.strip_prefix('○') {
        (WorkItemStatus::Pending, label)
    } else if allow_plain_bullet {
        let label = line
            .strip_prefix("- ")
            .or_else(|| numbered_task_label(line))?;
        (WorkItemStatus::Pending, label)
    } else {
        return None;
    };
    let label = label.trim();
    (!label.is_empty()).then(|| WorkItem {
        label: label.to_string(),
        status,
    })
}

fn numbered_task_label(line: &str) -> Option<&str> {
    let marker_end = line.find(['.', ')'])?;
    let marker = &line[..marker_end];
    (!marker.is_empty() && marker.chars().all(|character| character.is_ascii_digit()))
        .then(|| line[marker_end + 1..].trim_start())
        .filter(|label| !label.is_empty())
}

fn plan_items(detail: &str) -> Vec<WorkItem> {
    if let Ok(value) = serde_json::from_str::<Value>(detail) {
        let mut items = Vec::new();
        collect_plan_items(&value, &mut items);
        if !items.is_empty() {
            return items;
        }
    }

    let javascript_items = javascript_work_items(detail, &["step"]);
    if !javascript_items.is_empty() {
        return javascript_items;
    }

    detail
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let (status, label) = if let Some(label) = line.strip_prefix('✓') {
                (WorkItemStatus::Completed, label)
            } else if let Some(label) = line.strip_prefix('●') {
                (WorkItemStatus::InProgress, label)
            } else if let Some(label) = line.strip_prefix('○') {
                (WorkItemStatus::Pending, label)
            } else {
                return None;
            };
            let label = label.trim();
            (!label.is_empty()).then(|| WorkItem {
                label: label.to_string(),
                status,
            })
        })
        .collect()
}

fn collect_plan_items(value: &Value, items: &mut Vec<WorkItem>) {
    match value {
        Value::Array(values) => {
            for value in values {
                collect_plan_items(value, items);
            }
        }
        Value::Object(object) => {
            if let Some(label) = object.get("step").and_then(Value::as_str) {
                let status = object
                    .get("status")
                    .and_then(Value::as_str)
                    .map(work_item_status)
                    .unwrap_or(WorkItemStatus::Pending);
                items.push(WorkItem {
                    label: label.to_string(),
                    status,
                });
                return;
            }
            if let Some(plan) = object.get("plan") {
                collect_plan_items(plan, items);
            }
        }
        Value::String(value) => {
            if let Ok(nested) = serde_json::from_str::<Value>(value) {
                collect_plan_items(&nested, items);
            }
        }
        _ => {}
    }
}

fn todo_items(detail: &str) -> Option<Vec<WorkItem>> {
    let label_keys = [
        "content",
        "subject",
        "task",
        "title",
        "text",
        "step",
        "description",
    ];
    let items = serde_json::from_str::<Value>(detail)
        .ok()
        .and_then(|value| {
            let entries = find_json_value(&value, &["todos", "tasks", "plan"])?
                .as_array()?
                .clone();
            Some(
                entries
                    .iter()
                    .filter_map(|entry| {
                        let label = find_json_value(entry, &label_keys)
                            .and_then(Value::as_str)?
                            .trim();
                        if label.is_empty() {
                            return None;
                        }
                        let status = find_json_value(entry, &["status"])
                            .and_then(Value::as_str)
                            .map(work_item_status)
                            .unwrap_or(WorkItemStatus::Pending);
                        Some(WorkItem {
                            label: label.to_string(),
                            status,
                        })
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .filter(|items| !items.is_empty())
        .unwrap_or_else(|| javascript_work_items(detail, &label_keys));
    (!items.is_empty()).then_some(items)
}

fn javascript_work_items(detail: &str, label_keys: &[&str]) -> Vec<WorkItem> {
    let mut items = Vec::new();
    for (start, character) in detail.char_indices() {
        if character != '{' {
            continue;
        }
        let Some(end) = javascript_object_end(detail, start) else {
            continue;
        };
        let object = &detail[start..end];
        let Some(label) = label_keys
            .iter()
            .find_map(|key| javascript_string_property(object, key))
        else {
            continue;
        };
        let label = label.trim();
        if label.is_empty() || items.iter().any(|item: &WorkItem| item.label == label) {
            continue;
        }
        let status = javascript_string_property(object, "status")
            .as_deref()
            .map(work_item_status)
            .unwrap_or(WorkItemStatus::Pending);
        items.push(WorkItem {
            label: label.to_string(),
            status,
        });
    }
    items
}

fn javascript_object_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut index = start;
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    while index < bytes.len() {
        let current = bytes[index];
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if current == b'\\' {
                escaped = true;
            } else if current == active_quote {
                quote = None;
            }
        } else {
            match current {
                b'\'' | b'"' | b'`' => quote = Some(current),
                b'{' => depth += 1,
                b'}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(index + 1);
                    }
                }
                _ => {}
            }
        }
        index += 1;
    }
    None
}

fn javascript_string_property(source: &str, key: &str) -> Option<String> {
    let bytes = source.as_bytes();
    let key_bytes = key.as_bytes();
    let mut index = 0usize;
    let mut quote = None;
    let mut escaped = false;
    while index < bytes.len() {
        let current = bytes[index];
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if current == b'\\' {
                escaped = true;
            } else if current == active_quote {
                quote = None;
            }
            index += 1;
            continue;
        }
        if matches!(current, b'\'' | b'"') {
            let key_start = index + 1;
            let key_end = key_start + key_bytes.len();
            if source.as_bytes().get(key_start..key_end) == Some(key_bytes)
                && bytes.get(key_end) == Some(&current)
            {
                index = key_end + 1;
            } else {
                quote = Some(current);
                index += 1;
                continue;
            }
        } else if source.as_bytes().get(index..index + key_bytes.len()) == Some(key_bytes)
            && (index == 0 || !is_javascript_identifier(bytes[index - 1]))
            && bytes
                .get(index + key_bytes.len())
                .is_none_or(|value| !is_javascript_identifier(*value))
        {
            index += key_bytes.len();
        } else {
            index += 1;
            continue;
        }
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        if bytes.get(index) != Some(&b':') {
            continue;
        }
        index += 1;
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        let delimiter = *bytes.get(index)?;
        if !matches!(delimiter, b'\'' | b'"' | b'`') {
            continue;
        }
        index += 1;
        let value_start = index;
        let mut segment_start = value_start;
        let mut value = String::new();
        while index < bytes.len() {
            if bytes[index] == b'\\' {
                value.push_str(&source[segment_start..index]);
                index += 1;
                let escaped = *bytes.get(index)?;
                value.push(match escaped {
                    b'n' => '\n',
                    b'r' => '\r',
                    b't' => '\t',
                    value => value as char,
                });
                index += 1;
                segment_start = index;
                continue;
            }
            if bytes[index] == delimiter {
                if value.is_empty() {
                    return Some(source[value_start..index].to_string());
                }
                value.push_str(&source[segment_start..index]);
                return Some(value);
            }
            index += 1;
        }
    }
    None
}

fn is_javascript_identifier(value: u8) -> bool {
    value.is_ascii_alphanumeric() || matches!(value, b'_' | b'$')
}

fn work_item_status(status: &str) -> WorkItemStatus {
    match status
        .trim()
        .to_ascii_lowercase()
        .replace(['-', ' '], "_")
        .as_str()
    {
        "completed" | "complete" | "done" => WorkItemStatus::Completed,
        "inprogress" | "in_progress" | "running" | "active" => WorkItemStatus::InProgress,
        _ => WorkItemStatus::Pending,
    }
}

fn goal_summary(activities: &[SessionActivity]) -> Option<GoalSummary> {
    let mut ordered = activities.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|activity| activity.created_at);
    let mut goal: Option<GoalSummary> = None;

    for activity in ordered {
        if activity.kind != "tool" {
            continue;
        }
        let title = activity.title.to_ascii_lowercase();
        let tool = if title.contains("create_goal") {
            "create"
        } else if title.contains("update_goal") {
            "update"
        } else if title.contains("get_goal") {
            "get"
        } else {
            continue;
        };
        let fields = activity
            .detail
            .as_deref()
            .and_then(goal_fields)
            .unwrap_or_default();
        let inferred_started_at = fields.started_at.or_else(|| {
            fields
                .elapsed_ms
                .map(|elapsed| activity.created_at - elapsed)
        });

        match tool {
            "create" => {
                let Some(objective) = fields.objective else {
                    continue;
                };
                goal = Some(GoalSummary {
                    objective,
                    status: fields.status.unwrap_or(GoalStatus::Active),
                    started_at: inferred_started_at.unwrap_or(activity.created_at),
                    updated_at: activity.created_at,
                });
            }
            "get" => {
                if goal.is_none() {
                    let Some(objective) = fields.objective.clone() else {
                        continue;
                    };
                    goal = Some(GoalSummary {
                        objective,
                        status: fields.status.clone().unwrap_or(GoalStatus::Active),
                        started_at: inferred_started_at.unwrap_or(activity.created_at),
                        updated_at: activity.created_at,
                    });
                }
                if let Some(goal) = goal.as_mut() {
                    if let Some(objective) = fields.objective {
                        goal.objective = objective;
                    }
                    if let Some(status) = fields.status {
                        goal.status = status;
                    }
                    if let Some(started_at) = fields.started_at {
                        goal.started_at = started_at;
                    }
                    goal.updated_at = activity.created_at;
                }
            }
            "update" => {
                if let Some(goal) = goal.as_mut() {
                    if let Some(status) = fields.status {
                        goal.status = status;
                    }
                    goal.updated_at = activity.created_at;
                }
            }
            _ => {}
        }
    }
    goal
}

#[derive(Default)]
struct GoalFields {
    objective: Option<String>,
    status: Option<GoalStatus>,
    started_at: Option<i64>,
    elapsed_ms: Option<i64>,
}

fn goal_fields(detail: &str) -> Option<GoalFields> {
    let value = serde_json::from_str::<Value>(detail).ok()?;
    Some(GoalFields {
        objective: find_json_value(&value, &["objective"])
            .and_then(Value::as_str)
            .or_else(|| find_json_value(&value, &["goal"]).and_then(Value::as_str))
            .map(str::to_string),
        status: find_json_value(&value, &["status"])
            .and_then(Value::as_str)
            .and_then(goal_status),
        started_at: find_json_value(
            &value,
            &["startedAt", "started_at", "createdAt", "created_at"],
        )
        .and_then(Value::as_i64)
        .map(timestamp_millis),
        elapsed_ms: find_json_value(
            &value,
            &[
                "elapsedMs",
                "elapsed_ms",
                "elapsedTimeMs",
                "elapsed_time_ms",
            ],
        )
        .and_then(Value::as_i64)
        .or_else(|| {
            find_json_value(
                &value,
                &[
                    "elapsedSeconds",
                    "elapsed_seconds",
                    "elapsedTimeSeconds",
                    "elapsed_time_seconds",
                ],
            )
            .and_then(Value::as_i64)
            .map(|seconds| seconds.saturating_mul(1_000))
        }),
    })
}

fn timestamp_millis(timestamp: i64) -> i64 {
    if timestamp.abs() < 100_000_000_000 {
        timestamp.saturating_mul(1_000)
    } else {
        timestamp
    }
}

fn find_json_value<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    match value {
        Value::Object(object) => keys.iter().find_map(|key| object.get(*key)).or_else(|| {
            object
                .values()
                .find_map(|value| find_json_value(value, keys))
        }),
        Value::Array(values) => values.iter().find_map(|value| find_json_value(value, keys)),
        Value::String(_) => None,
        _ => None,
    }
}

fn goal_status(status: &str) -> Option<GoalStatus> {
    match status.trim().to_ascii_lowercase().as_str() {
        "active" | "running" | "in_progress" | "inprogress" => Some(GoalStatus::Active),
        "complete" | "completed" | "done" => Some(GoalStatus::Complete),
        "blocked" | "failed" => Some(GoalStatus::Blocked),
        _ => None,
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HubSnapshot {
    pub protocol_version: u16,
    pub desktop_version: String,
    pub generated_at: i64,
    pub features: Vec<String>,
    pub sessions: Vec<HubSession>,
    pub internal_services: Vec<InternalService>,
    pub workflow_groups: Vec<WorkflowGroupDefinition>,
    pub workflow_history: Vec<WorkflowHistoryRecord>,
}

impl HubSnapshot {
    pub fn new(sessions: Vec<AgentSession>) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            desktop_version: env!("CARGO_PKG_VERSION").to_string(),
            generated_at: now_millis(),
            features: PROTOCOL_FEATURES
                .iter()
                .map(|feature| (*feature).to_string())
                .collect(),
            sessions: sessions.into_iter().map(HubSession::from).collect(),
            internal_services: Vec::new(),
            workflow_groups: Vec::new(),
            workflow_history: Vec::new(),
        }
    }

    pub fn with_activity_limit(sessions: Vec<AgentSession>, activity_limit: usize) -> Self {
        let mut snapshot = Self::new(sessions);
        let limit = activity_limit.max(1);
        for session in &mut snapshot.sessions {
            let recent_subagents = visible_subagent_ids(&session.session.activities);
            session.session.activities.retain(|activity| {
                activity.kind != "subagent" || recent_subagents.contains(&activity.id)
            });
            let activity_start = session.session.activities.len().saturating_sub(limit);
            if activity_start > 0 {
                let recent_children = session.session.activities[..activity_start]
                    .iter()
                    .filter(|activity| recent_subagents.contains(&activity.id))
                    .cloned()
                    .collect::<Vec<_>>();
                session.session.activities.drain(..activity_start);
                session.session.activities.splice(0..0, recent_children);
            }
            let result_start = session.session.results.len().saturating_sub(limit);
            if result_start > 0 {
                session.session.results.drain(..result_start);
            }
        }
        snapshot
    }

    pub fn with_workflows(
        mut self,
        groups: Vec<WorkflowGroupDefinition>,
        history: Vec<WorkflowHistoryRecord>,
    ) -> Self {
        self.workflow_groups = groups;
        self.workflow_history = history;
        self
    }

    pub fn with_internal_services(mut self, services: Vec<InternalService>) -> Self {
        self.internal_services = services;
        self
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum HubCommand {
    SubmitPrompt {
        session_id: String,
        prompt: String,
        #[serde(default)]
        attachments: Vec<PromptAttachmentInput>,
        #[serde(default)]
        delivery: PromptDelivery,
    },
    TakeControlSession {
        session_id: String,
        prompt: String,
        #[serde(default)]
        attachments: Vec<PromptAttachmentInput>,
    },
    ResolvePermission {
        session_id: String,
        permission_id: String,
        action: PermissionAction,
    },
    ResolveQuestion {
        session_id: String,
        question_id: String,
        answers: Vec<QuestionAnswer>,
    },
    TerminateSession {
        session_id: String,
    },
    InterruptPrompt {
        session_id: String,
    },
    DownloadResponseFile {
        session_id: String,
        attachment_id: String,
    },
    OpenSessionSource {
        session_id: String,
    },
    RefreshRateLimits {
        agent: AgentKind,
    },
    StartWorkflow {
        workflow_id: String,
        objective: String,
    },
    ApproveWorkflowHandoff {
        workflow_id: String,
    },
    AdvanceWorkflow {
        workflow_id: String,
    },
    PauseWorkflow {
        workflow_id: String,
    },
    ResumeWorkflow {
        workflow_id: String,
    },
    RetryWorkflowStep {
        workflow_id: String,
    },
    SkipWorkflowStep {
        workflow_id: String,
    },
    CancelWorkflow {
        workflow_id: String,
    },
    ReportMobileVersion {
        version: String,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HubCommandRequest {
    pub request_id: String,
    #[serde(flatten)]
    pub command: HubCommand,
}

impl HubCommandRequest {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_identifier("request_id", &self.request_id, 128)?;
        match &self.command {
            HubCommand::SubmitPrompt {
                session_id,
                prompt,
                attachments,
                ..
            } => {
                validate_identifier("session_id", session_id, 512)?;
                if prompt.trim().is_empty() && attachments.is_empty() {
                    return Err(ProtocolError::new(
                        "prompt_empty",
                        "O prompt e os anexos estão vazios",
                    ));
                }
                validate_prompt_payload(prompt, attachments)?;
            }
            HubCommand::TakeControlSession {
                session_id,
                prompt,
                attachments,
            } => {
                validate_identifier("session_id", session_id, 512)?;
                validate_prompt_payload(prompt, attachments)?;
            }
            HubCommand::ResolvePermission {
                session_id,
                permission_id,
                ..
            } => {
                validate_identifier("session_id", session_id, 512)?;
                validate_identifier("permission_id", permission_id, 512)?;
            }
            HubCommand::ResolveQuestion {
                session_id,
                question_id,
                answers,
            } => {
                validate_identifier("session_id", session_id, 512)?;
                validate_identifier("question_id", question_id, 512)?;
                if answers.is_empty() {
                    return Err(ProtocolError::new(
                        "answers_empty",
                        "A resposta da pergunta está vazia",
                    ));
                }
                if answers.len() > 4 {
                    return Err(ProtocolError::new(
                        "too_many_answers",
                        "A solicitação aceita no máximo 4 perguntas",
                    ));
                }
                for answer in answers {
                    validate_identifier("answer.question_id", &answer.question_id, 256)?;
                    if answer.answers.is_empty()
                        || answer.answers.iter().all(|value| value.trim().is_empty())
                    {
                        return Err(ProtocolError::new(
                            "answer_empty",
                            "Uma das respostas está vazia",
                        ));
                    }
                    if answer.answers.len() > 8
                        || answer.answers.iter().any(|value| value.len() > 16 * 1024)
                    {
                        return Err(ProtocolError::new(
                            "answer_too_large",
                            "Uma das respostas excede o limite permitido",
                        ));
                    }
                }
            }
            HubCommand::TerminateSession { session_id }
            | HubCommand::InterruptPrompt { session_id }
            | HubCommand::OpenSessionSource { session_id } => {
                validate_identifier("session_id", session_id, 512)?;
            }
            HubCommand::DownloadResponseFile {
                session_id,
                attachment_id,
            } => {
                validate_identifier("session_id", session_id, 512)?;
                validate_identifier("attachment_id", attachment_id, 512)?;
            }
            HubCommand::RefreshRateLimits { .. } => {}
            HubCommand::StartWorkflow {
                workflow_id,
                objective,
            } => {
                validate_identifier("workflow_id", workflow_id, 256)?;
                if objective.trim().is_empty() {
                    return Err(ProtocolError::new(
                        "workflow_objective_empty",
                        "Add an objective before running this workflow",
                    ));
                }
                if objective.len() > 4_000 {
                    return Err(ProtocolError::new(
                        "workflow_objective_too_large",
                        "The workflow objective exceeds 4000 characters",
                    ));
                }
            }
            HubCommand::ApproveWorkflowHandoff { workflow_id }
            | HubCommand::AdvanceWorkflow { workflow_id }
            | HubCommand::PauseWorkflow { workflow_id }
            | HubCommand::ResumeWorkflow { workflow_id }
            | HubCommand::RetryWorkflowStep { workflow_id }
            | HubCommand::SkipWorkflowStep { workflow_id }
            | HubCommand::CancelWorkflow { workflow_id } => {
                validate_identifier("workflow_id", workflow_id, 256)?;
            }
            HubCommand::ReportMobileVersion { version } => {
                validate_identifier("version", version, 32)?;
                if !version
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || ".-+".contains(character))
                {
                    return Err(ProtocolError::new(
                        "version_invalid",
                        "A versão informada é inválida",
                    ));
                }
            }
        }
        Ok(())
    }
}

fn validate_prompt_payload(
    prompt: &str,
    attachments: &[PromptAttachmentInput],
) -> Result<(), ProtocolError> {
    if prompt.len() > 16 * 1024 {
        return Err(ProtocolError::new(
            "prompt_too_large",
            "O prompt excede 16 KB",
        ));
    }
    if attachments.len() > 4 {
        return Err(ProtocolError::new(
            "too_many_attachments",
            "O prompt aceita no máximo 4 imagens",
        ));
    }
    Ok(())
}

pub fn is_version_newer(candidate: &str, installed: &str) -> bool {
    let version_parts = |value: &str| {
        let stable = value.split('-').next().unwrap_or(value);
        let mut parsed = [0_u64; 4];
        for (index, part) in stable.split('.').take(parsed.len()).enumerate() {
            parsed[index] = part
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse()
                .unwrap_or(0);
        }
        parsed
    };
    version_parts(candidate) > version_parts(installed)
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolError {
    pub code: String,
    pub message: String,
}

impl ProtocolError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn from_control(message: String) -> Self {
        let code = if message.contains("Sessão não encontrada") {
            "session_not_found"
        } else if message.contains("Aguarde o agente") {
            "session_busy"
        } else if message.contains("não informou") || message.contains("não possui") {
            "session_not_ready"
        } else if message.contains("não oferece") || message.contains("não permite") {
            "unsupported_action"
        } else if message.contains("processo isolado") {
            "unsafe_termination"
        } else {
            "command_failed"
        };
        Self::new(code, message)
    }
}

fn validate_identifier(field: &str, value: &str, max_len: usize) -> Result<(), ProtocolError> {
    if value.trim().is_empty() {
        return Err(ProtocolError::new(
            format!("{field}_empty"),
            format!("{field} está vazio"),
        ));
    }
    if value.len() > max_len {
        return Err(ProtocolError::new(
            format!("{field}_too_large"),
            format!("{field} excede o limite"),
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HubCommandResponse {
    pub protocol_version: u16,
    pub request_id: String,
    pub ok: bool,
    pub error: Option<ProtocolError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl HubCommandResponse {
    pub fn success(request_id: String) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            request_id,
            ok: true,
            error: None,
            data: None,
        }
    }

    pub fn success_with_data(request_id: String, data: Value) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            request_id,
            ok: true,
            error: None,
            data: Some(data),
        }
    }

    pub fn failure(request_id: String, error: ProtocolError) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            request_id,
            ok: false,
            error: Some(error),
            data: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum HubEvent {
    SessionsChanged,
    WorkflowsChanged,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HubEventEnvelope {
    pub protocol_version: u16,
    pub event_id: String,
    pub sequence: u64,
    pub occurred_at: i64,
    #[serde(flatten)]
    pub event: HubEvent,
}

#[derive(Clone, Debug, Serialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum HubStreamMessage {
    Hello {
        protocol_version: u16,
        features: Vec<String>,
        heartbeat_interval_ms: u64,
    },
    Snapshot {
        sequence: u64,
        snapshot: HubSnapshot,
    },
    Update {
        events: Vec<HubEventEnvelope>,
        snapshot: HubSnapshot,
    },
    Error {
        code: String,
        message: String,
        retryable: bool,
    },
}

impl HubStreamMessage {
    pub fn hello() -> Self {
        Self::Hello {
            protocol_version: PROTOCOL_VERSION,
            features: PROTOCOL_FEATURES
                .iter()
                .map(|feature| (*feature).to_string())
                .collect(),
            heartbeat_interval_ms: STREAM_HEARTBEAT_INTERVAL_MS,
        }
    }
}

static EVENT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn event_journal() -> &'static Mutex<VecDeque<HubEventEnvelope>> {
    static JOURNAL: OnceLock<Mutex<VecDeque<HubEventEnvelope>>> = OnceLock::new();
    JOURNAL.get_or_init(|| Mutex::new(VecDeque::with_capacity(256)))
}

pub fn emit_sessions_changed(app: &AppHandle) {
    emit_sessions_changed_for(app, None, None);
}

pub fn emit_session_changed(app: &AppHandle, session_id: &str, native_session_id: Option<&str>) {
    emit_sessions_changed_for(app, Some(session_id), native_session_id);
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionChangePayload<'a> {
    session_id: Option<&'a str>,
    native_session_id: Option<&'a str>,
}

fn emit_sessions_changed_for(
    app: &AppHandle,
    session_id: Option<&str>,
    native_session_id: Option<&str>,
) {
    let event = record_hub_event(HubEvent::SessionsChanged);
    let _ = app.emit(
        "lume://sessions-changed",
        SessionChangePayload {
            session_id,
            native_session_id,
        },
    );
    let _ = app.emit("lume://hub-event", event);
}

pub fn emit_workflows_changed(app: &AppHandle) {
    let event = record_hub_event(HubEvent::WorkflowsChanged);
    let _ = app.emit("lume://hub-event", event);
}

fn record_hub_event(event: HubEvent) -> HubEventEnvelope {
    let sequence = EVENT_SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
    let occurred_at = now_millis();
    let envelope = HubEventEnvelope {
        protocol_version: PROTOCOL_VERSION,
        event_id: format!("{occurred_at}-{sequence}"),
        sequence,
        occurred_at,
        event,
    };
    if let Ok(mut journal) = event_journal().lock() {
        if journal.len() == 256 {
            journal.pop_front();
        }
        journal.push_back(envelope.clone());
    }
    envelope
}

pub fn events_since(sequence: u64) -> Vec<HubEventEnvelope> {
    event_journal()
        .lock()
        .map(|journal| {
            journal
                .iter()
                .filter(|event| event.sequence > sequence)
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

pub fn latest_event_sequence() -> u64 {
    EVENT_SEQUENCE.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AccessMode, PermissionProfile, SessionStatus};

    fn session() -> AgentSession {
        AgentSession {
            id: "codex:thread-1".into(),
            agent: AgentKind::Codex,
            agent_label: "Codex".into(),
            session_name: "Codex · Lume".into(),
            project: "Lume".into(),
            source: SessionSource::Cli,
            source_app: None,
            control_origin: SessionControlOrigin::Lume,
            status: SessionStatus::WaitingForInput,
            status_label: "Esperando ação".into(),
            started_at: "1".into(),
            updated_at: 1,
            process_id: Some(42),
            native_session_id: Some("thread-1".into()),
            working_directory: Some("/work/lume".into()),
            permission_profile: PermissionProfile {
                mode: AccessMode::WorkspaceWrite,
                label: "Workspace".into(),
                approval_policy: "on-request".into(),
                approvals_reviewer: None,
                can_respond_from_lume: true,
                available_actions: Vec::new(),
            },
            pending_permission: None,
            pending_question: None,
            last_response: None,
            results: Vec::new(),
            activities: Vec::new(),
            rate_limits: Vec::new(),
            rate_limits_error: None,
            prompt_token_usage: Vec::new(),
            forked_from: None,
        }
    }

    #[test]
    fn snapshot_keeps_internal_services_separate_from_user_sessions() {
        let snapshot =
            HubSnapshot::new(vec![session()]).with_internal_services(vec![InternalService {
                id: "codex:internal:7".into(),
                agent: AgentKind::Codex,
                label: "Memories".into(),
                process_id: 7,
            }]);
        assert_eq!(snapshot.sessions.len(), 1);
        assert_eq!(snapshot.internal_services.len(), 1);
        let serialized = serde_json::to_value(snapshot).expect("snapshot JSON");
        assert_eq!(serialized["internalServices"][0]["label"], "Memories");
    }

    #[test]
    fn snapshot_keeps_a_completed_subagent_after_more_prompts() {
        let mut parent = session();
        parent.activities.push(SessionActivity {
            id: "child-1".into(),
            kind: "subagent".into(),
            title: "Reviewer".into(),
            detail: None,
            status: "completed".into(),
            created_at: 1,
            files: Vec::new(),
            attachments: Vec::new(),
            append_detail: false,
        });
        for index in 0..70 {
            let mut activity = parent.activities[0].clone();
            activity.id = format!("tool-{index}");
            activity.kind = "tool".into();
            activity.created_at = index + 2;
            parent.activities.push(activity);
        }
        let snapshot = HubSnapshot::with_activity_limit(vec![parent.clone()], 60);
        assert!(snapshot.sessions[0]
            .session
            .activities
            .iter()
            .any(|activity| activity.id == "child-1"));
        for index in 0..3 {
            let mut prompt = parent.activities[0].clone();
            prompt.id = format!("prompt-{index}");
            prompt.kind = "prompt".into();
            prompt.created_at = index + 72;
            parent.activities.push(prompt);
        }
        let snapshot = HubSnapshot::with_activity_limit(vec![parent], 60);
        assert!(snapshot.sessions[0]
            .session
            .activities
            .iter()
            .any(|activity| activity.id == "child-1"));
    }

    #[test]
    fn snapshot_is_versioned_and_contains_capabilities() {
        let snapshot = HubSnapshot::new(vec![session()]);
        assert_eq!(snapshot.protocol_version, PROTOCOL_VERSION);
        assert_eq!(snapshot.desktop_version, env!("CARGO_PKG_VERSION"));
        assert!(snapshot.features.contains(&"prompts".to_string()));
        assert!(snapshot
            .features
            .contains(&"coordinated_updates".to_string()));
        assert!(snapshot.features.contains(&"work_status".to_string()));
        assert!(snapshot.sessions[0].capabilities.can_prompt);
        assert!(snapshot.sessions[0].capabilities.can_terminate);
        let json = serde_json::to_value(snapshot).expect("snapshot");
        assert_eq!(json["sessions"][0]["nativeSessionId"], "thread-1");
        assert_eq!(json["sessions"][0]["capabilities"]["canPrompt"], true);
    }

    #[test]
    fn terminal_snapshot_limits_old_activities_but_preserves_total() {
        let mut session = session();
        for index in 0..5 {
            session.activities.push(SessionActivity {
                id: format!("message-{index}"),
                kind: "message".into(),
                title: "Codex".into(),
                detail: Some(format!("Message {index}")),
                status: "completed".into(),
                created_at: index,
                files: Vec::new(),
                attachments: Vec::new(),
                append_detail: false,
            });
        }

        let snapshot = HubSnapshot::with_activity_limit(vec![session], 2);
        let session = &snapshot.sessions[0];
        assert_eq!(session.activity_total, 5);
        assert_eq!(session.session.activities.len(), 2);
        assert_eq!(session.session.activities[0].id, "message-3");
    }

    #[test]
    fn web_chatgpt_does_not_inherit_codex_runtime_capabilities() {
        let mut web = session();
        web.id = "web:chatgpt:tab-1".into();
        web.agent = AgentKind::ChatGpt;
        web.agent_label = "ChatGPT".into();
        web.source = SessionSource::Web;
        web.status = SessionStatus::Running;
        web.process_id = None;
        web.native_session_id = Some("tab-1".into());
        web.working_directory = None;

        let capabilities = SessionCapabilities::for_session(&web);
        assert!(!capabilities.can_prompt);
        assert_eq!(
            capabilities.prompt_unavailable_reason,
            Some(PromptUnavailableReason::AgentBusy)
        );
        assert!(!capabilities.can_interrupt);
        assert_eq!(
            capabilities.prompt_deliveries,
            vec![PromptDelivery::NewTurn]
        );
    }

    #[test]
    fn web_claude_is_not_treated_as_claude_code() {
        let mut web = session();
        web.id = "web:claude:tab-2".into();
        web.agent = AgentKind::Claude;
        web.agent_label = "Claude".into();
        web.source = SessionSource::Web;
        web.process_id = None;
        web.native_session_id = Some("tab-2".into());
        web.working_directory = None;

        let capabilities = SessionCapabilities::for_session(&web);
        assert!(capabilities.can_prompt);
        assert!(!capabilities.can_interrupt);
        assert_eq!(
            capabilities.prompt_deliveries,
            vec![PromptDelivery::NewTurn]
        );
    }

    #[test]
    fn cli_codex_keeps_queue_and_steer_capabilities() {
        let capabilities = SessionCapabilities::for_session(&session());
        assert_eq!(
            capabilities.prompt_deliveries,
            vec![
                PromptDelivery::NewTurn,
                PromptDelivery::Steer,
                PromptDelivery::Queue,
            ]
        );
    }

    #[test]
    fn managed_headless_codex_can_be_terminated_without_a_process() {
        let mut managed = session();
        managed.source = SessionSource::Desktop;
        managed.process_id = None;

        assert!(SessionCapabilities::for_session(&managed).can_terminate);
    }

    #[test]
    fn managed_opencode_uses_direct_chat_capabilities_without_a_terminal() {
        let mut managed = session();
        managed.agent = AgentKind::OpenCode;
        managed.source = SessionSource::Desktop;
        managed.process_id = None;
        managed.status = SessionStatus::Running;
        let capabilities = SessionCapabilities::for_session(&managed);
        assert!(capabilities.can_prompt);
        assert!(capabilities.can_attach_images);
        assert!(capabilities.can_terminate);
        assert!(capabilities.can_interrupt);
        assert!(!capabilities.can_take_control);
        assert_eq!(
            capabilities.prompt_deliveries,
            vec![PromptDelivery::NewTurn]
        );
        managed.control_origin = SessionControlOrigin::External;
        assert!(!SessionCapabilities::for_session(&managed).can_prompt);
    }

    #[test]
    fn claude_opened_by_lume_without_a_terminal_can_be_ended() {
        let mut headless = session();
        headless.agent = AgentKind::ClaudeCode;
        headless.source = SessionSource::Cli;
        headless.process_id = None;
        headless.control_origin = SessionControlOrigin::Lume;
        headless.native_session_id = Some("claude-session".into());
        assert!(SessionCapabilities::for_session(&headless).can_terminate);
        headless.control_origin = SessionControlOrigin::External;
        assert!(!SessionCapabilities::for_session(&headless).can_terminate);
    }

    #[test]
    fn claude_opened_by_lume_can_queue_and_be_interrupted_while_running() {
        let mut claude = session();
        claude.agent = AgentKind::ClaudeCode;
        claude.source = SessionSource::Cli;
        claude.process_id = None;
        claude.control_origin = SessionControlOrigin::Lume;
        claude.native_session_id = Some("claude-session".into());
        claude.status = SessionStatus::Running;
        let capabilities = SessionCapabilities::for_session(&claude);
        assert!(capabilities.can_interrupt);
        assert_eq!(
            capabilities.prompt_deliveries,
            vec![
                PromptDelivery::NewTurn,
                PromptDelivery::Queue,
                PromptDelivery::Steer
            ]
        );
        claude.control_origin = SessionControlOrigin::External;
        let external = SessionCapabilities::for_session(&claude);
        assert!(!external.can_interrupt);
        assert_eq!(external.prompt_deliveries, vec![PromptDelivery::NewTurn]);
    }

    #[test]
    fn external_codex_cli_is_read_only_until_control_is_transferred() {
        let mut external = session();
        external.control_origin = SessionControlOrigin::External;
        let capabilities = SessionCapabilities::for_session(&external);

        assert!(!capabilities.can_prompt);
        assert!(capabilities.can_take_control);
        assert!(!capabilities.can_interrupt);
        assert_eq!(
            capabilities.prompt_unavailable_reason,
            Some(PromptUnavailableReason::ExternalSession)
        );
        assert_eq!(
            capabilities.prompt_deliveries,
            vec![PromptDelivery::NewTurn]
        );
        external.source = SessionSource::Vscode;
        assert!(!SessionCapabilities::for_session(&external).can_take_control);

        external.source = SessionSource::Cli;
        external.working_directory = None;
        assert!(!SessionCapabilities::for_session(&external).can_take_control);

        external.working_directory = Some("/work/lume".into());
        external.agent = AgentKind::ClaudeCode;
        assert!(!SessionCapabilities::for_session(&external).can_take_control);
    }

    #[test]
    fn unbound_codex_cli_monitor_keeps_results_without_authorizing_control() {
        let mut external = session();
        external.control_origin = SessionControlOrigin::External;
        external.process_id = None;
        external.last_response = Some("Observed response".into());
        external.pending_permission = Some(crate::domain::PermissionRequest {
            id: "approval".into(),
            kind: "command".into(),
            summary: "Observed approval".into(),
            resource: "command".into(),
            risk: "low".into(),
            requested_at: "1".into(),
        });
        external.pending_question = Some(crate::domain::PendingQuestion {
            id: "question".into(),
            questions: Vec::new(),
            requested_at: "1".into(),
        });

        let snapshot = HubSnapshot::new(vec![external.clone()]);
        assert_eq!(snapshot.sessions.len(), 1);
        let capabilities = &snapshot.sessions[0].capabilities;
        assert!(capabilities.can_read_results);
        assert!(!capabilities.can_prompt);
        assert!(!capabilities.can_approve);
        assert!(!capabilities.can_answer_question);
        assert!(!capabilities.can_terminate);
        assert!(!capabilities.can_interrupt);
        assert!(!capabilities.can_take_control);
        assert!(!capabilities.can_attach_images);

        external.process_id = Some(42);
        let bound = SessionCapabilities::for_session(&external);
        assert!(bound.can_approve);
        assert!(bound.can_answer_question);
        assert!(bound.can_take_control);
    }

    #[test]
    fn unbound_monitor_guard_does_not_change_other_session_sources() {
        let mut candidate = session();
        candidate.process_id = None;
        assert!(!is_unbound_codex_cli_monitor(&candidate));
        candidate.control_origin = SessionControlOrigin::External;
        assert!(is_unbound_codex_cli_monitor(&candidate));
        for source in [
            SessionSource::Desktop,
            SessionSource::Vscode,
            SessionSource::Web,
        ] {
            candidate.source = source;
            assert!(!is_unbound_codex_cli_monitor(&candidate));
        }
        candidate.source = SessionSource::Cli;
        candidate.agent = AgentKind::ClaudeCode;
        assert!(!is_unbound_codex_cli_monitor(&candidate));
    }

    #[test]
    fn snapshot_uses_the_latest_structured_plan_as_the_current_plan() {
        let mut session = session();
        session.activities.push(SessionActivity {
            id: "plan-1".into(),
            kind: "plan".into(),
            title: "Plano atualizado".into(),
            detail: Some(
                "Preparando a entrega\n✓ Mapear eventos\n● Criar a bandeja\n○ Validar builds"
                    .into(),
            ),
            status: "running".into(),
            created_at: 12,
            files: Vec::new(),
            attachments: Vec::new(),
            append_detail: false,
        });

        let snapshot = HubSnapshot::new(vec![session]);
        let plan = snapshot.sessions[0]
            .work_summary
            .plan
            .as_ref()
            .expect("plan");
        assert_eq!(plan.items.len(), 3);
        assert_eq!(plan.items[0].status, WorkItemStatus::Completed);
        assert_eq!(plan.items[1].status, WorkItemStatus::InProgress);
        assert_eq!(plan.updated_at, 12);
    }

    #[test]
    fn snapshot_exposes_saved_plan_separately_from_the_current_todo() {
        let mut session = session();
        session.activities.extend([
            SessionActivity {
                id: "plan-1".into(),
                kind: "plan".into(),
                title: "Plan updated".into(),
                detail: Some("○ Implement handoff\n○ Test handoff".into()),
                status: "completed".into(),
                created_at: 10,
                files: Vec::new(),
                attachments: Vec::new(),
                append_detail: false,
            },
            SessionActivity {
                id: "message-1".into(),
                kind: "plan_document".into(),
                title: "Codex".into(),
                detail: Some(
                    "# Planejamento — Workflow\n\n## Fase 1\nHandoff manual\n\n## Fase 2\nModo Workflow"
                        .into(),
                ),
                status: "completed".into(),
                created_at: 20,
                files: Vec::new(),
                attachments: Vec::new(),
                append_detail: false,
            },
        ]);
        let snapshot = HubSnapshot::new(vec![session]);
        let plan = snapshot.sessions[0]
            .work_summary
            .plan
            .as_ref()
            .expect("plan");
        assert_eq!(plan.items.len(), 2);
        assert_eq!(plan.items[0].label, "Fase 1");
        assert!(plan.content.is_some());
        assert_eq!(plan.updated_at, 20);
        let todo = snapshot.sessions[0]
            .work_summary
            .todo
            .as_ref()
            .expect("todo");
        assert_eq!(todo.items.len(), 2);
        assert_eq!(todo.updated_at, 10);
    }

    #[test]
    fn snapshot_summarizes_todo_tool_items() {
        let mut session = session();
        session.activities.push(SessionActivity {
            id: "todo-1".into(),
            kind: "tool".into(),
            title: "TodoWrite".into(),
            detail: Some(
                r#"{"todos":[{"content":"Inspect hooks","status":"completed"},{"content":"Build tray","status":"in_progress"}]}"#
                    .into(),
            ),
            status: "completed".into(),
            created_at: 20,
            files: Vec::new(),
            attachments: Vec::new(),
            append_detail: false,
        });

        let snapshot = HubSnapshot::new(vec![session]);
        let todo = snapshot.sessions[0]
            .work_summary
            .todo
            .as_ref()
            .expect("todo");
        assert_eq!(todo.items.len(), 2);
        assert_eq!(todo.items[1].label, "Build tray");
        assert_eq!(todo.items[1].status, WorkItemStatus::InProgress);
    }

    #[test]
    fn snapshot_ignores_tool_catalog_descriptions_as_todo_items() {
        let mut session = session();
        session.activities.push(SessionActivity {
            id: "tool-catalog".into(),
            kind: "tool".into(),
            title: "Tool catalog".into(),
            detail: Some(
                r#"{"tools":[{"name":"github_update_issue","description":"Access repositories, issues, and pull requests."},{"name":"web_run","description":"Tools in the web namespace."}]}"#
                    .into(),
            ),
            status: "completed".into(),
            created_at: 21,
            files: Vec::new(),
            attachments: Vec::new(),
            append_detail: false,
        });

        let snapshot = HubSnapshot::new(vec![session]);
        assert!(snapshot.sessions[0].work_summary.todo.is_none());
        assert!(snapshot.sessions[0].work_summary.plan.is_none());
    }

    #[test]
    fn a_sentence_that_merely_contains_to_do_is_not_a_todo_heading() {
        let sentence = "Erros mais claros: o painel mostrava o código bruto do erro. Agora aparecem mensagens para:\n\n- identidade do Git não configurada;\n- nada para commitar;";
        assert!(explicit_message_todo(sentence).is_none());
        assert!(!is_work_tracking_message(sentence));
        let heading = "Próximo to do:\n\n- Conferir estados\n- Rodar checks";
        assert_eq!(
            explicit_message_todo(heading).map(|items| items.len()),
            Some(2)
        );
    }

    #[test]
    fn snapshot_tracks_explicit_todo_messages_when_the_agent_has_no_plan_tool() {
        let mut session = session();
        session.activities.extend([
            SessionActivity {
                id: "todo-message".into(),
                kind: "message".into(),
                title: "Resposta do agente".into(),
                detail: Some(
                    "TODO de teste:\n\n- Conferir estados\n- Validar reordenação\n- Rodar checks"
                        .into(),
                ),
                status: "running".into(),
                created_at: 20,
                files: Vec::new(),
                attachments: Vec::new(),
                append_detail: false,
            },
            SessionActivity {
                id: "todo-progress-1".into(),
                kind: "message".into(),
                title: "Resposta do agente".into(),
                detail: Some("✓ Conferir estados\n\nSeguindo para a próxima etapa.".into()),
                status: "running".into(),
                created_at: 21,
                files: Vec::new(),
                attachments: Vec::new(),
                append_detail: false,
            },
            SessionActivity {
                id: "todo-progress-2".into(),
                kind: "message".into(),
                title: "Resposta do agente".into(),
                detail: Some("● Validar reordenação".into()),
                status: "running".into(),
                created_at: 22,
                files: Vec::new(),
                attachments: Vec::new(),
                append_detail: false,
            },
        ]);

        let snapshot = HubSnapshot::new(vec![session]);
        let todo = snapshot.sessions[0]
            .work_summary
            .todo
            .as_ref()
            .expect("todo from explicit agent message");
        assert_eq!(todo.items.len(), 3);
        assert_eq!(todo.items[0].status, WorkItemStatus::Completed);
        assert_eq!(todo.items[1].status, WorkItemStatus::InProgress);
        assert_eq!(todo.items[2].status, WorkItemStatus::Pending);
        assert_eq!(todo.updated_at, 22);
    }

    #[test]
    fn snapshot_tracks_numbered_todo_after_a_goal_announcement() {
        let mut session = session();
        session.activities.push(SessionActivity {
            id: "numbered-todo-message".into(),
            kind: "message".into(),
            title: "Resposta do agente".into(),
            detail: Some(
                "Goal criada. TO DO ativo:\n\n1. Proteger o bridge local.\n2. Persistir ownership e filas."
                    .into(),
            ),
            status: "running".into(),
            created_at: 24,
            files: Vec::new(),
            attachments: Vec::new(),
            append_detail: false,
        });

        let snapshot = HubSnapshot::new(vec![session]);
        let todo = snapshot.sessions[0]
            .work_summary
            .todo
            .as_ref()
            .expect("numbered todo");
        assert_eq!(todo.items.len(), 2);
        assert_eq!(todo.items[0].label, "Proteger o bridge local.");
        assert_eq!(todo.items[1].label, "Persistir ownership e filas.");
    }

    #[test]
    fn work_summaries_parse_nested_javascript_tool_arguments() {
        let plan = plan_items(
            r#"{plan:[{step:"Inspect",status:"in_progress"},{step:'Fix',status:'pending'}]}"#,
        );
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0].label, "Inspect");
        assert_eq!(plan[0].status, WorkItemStatus::InProgress);
        assert_eq!(plan[1].label, "Fix");

        let todo = todo_items(
            r#"{todos:[{content:"Inspect",status:"completed"},{content:'Ship',status:'pending'}]}"#,
        )
        .expect("todo items");
        assert_eq!(todo.len(), 2);
        assert_eq!(todo[0].label, "Inspect");
        assert_eq!(todo[0].status, WorkItemStatus::Completed);
        assert_eq!(todo[1].label, "Ship");
    }

    #[test]
    fn snapshot_tracks_explicit_todo_from_intermediate_agent_updates() {
        let mut session = session();
        session.activities.push(SessionActivity {
            id: "todo-analysis".into(),
            kind: "analysis".into(),
            title: "Atualização".into(),
            detail: Some("TODO desta rodada:\n\n- Corrigir bookmark\n- Revisar SVGs".into()),
            status: "running".into(),
            created_at: 30,
            files: Vec::new(),
            attachments: Vec::new(),
            append_detail: false,
        });

        let snapshot = HubSnapshot::new(vec![session]);
        let todo = snapshot.sessions[0]
            .work_summary
            .todo
            .as_ref()
            .expect("todo from intermediate agent update");
        assert_eq!(todo.items.len(), 2);
        assert_eq!(todo.items[0].label, "Corrigir bookmark");
        assert_eq!(todo.updated_at, 30);
    }

    #[test]
    fn snapshot_reconciles_completed_summary_bullets_with_todo_items() {
        let mut session = session();
        session.activities.extend([
            SessionActivity {
                id: "todo-analysis".into(),
                kind: "analysis".into(),
                title: "Atualização".into(),
                detail: Some(
                    "TODO desta rodada:\n\n- Refinar o bookmark e a task-list responsiva aos temas.\n- Criar ícones próprios para PLAN, GOAL e TODO.\n- Adicionar progresso percentual ao GOAL.\n- Garantir duração em todas as respostas finais.\n- Animar Ver mais e Ver menos.\n- Explicar o gráfico do Inspector.\n- Abrir o TODO automaticamente ao criar e concluir tarefas."
                        .into(),
                ),
                status: "running".into(),
                created_at: 40,
                files: Vec::new(),
                attachments: Vec::new(),
                append_detail: false,
            },
            SessionActivity {
                id: "todo-final".into(),
                kind: "message".into(),
                title: "Resposta do agente".into(),
                detail: Some(
                    "Concluído:\n\n- Bookmark agora revela o menu por trás do trigger.\n- TODO menor e abrindo automaticamente ao criar e concluir tarefas.\n- SVGs próprios para TODO, PLAN e GOAL.\n- GOAL ganhou progresso percentual.\n- Respostas finais mostram duração.\n- Ver mais e Ver menos ganharam transição.\n- Gráfico do Inspector agora está explicado."
                        .into(),
                ),
                status: "completed".into(),
                created_at: 41,
                files: Vec::new(),
                attachments: Vec::new(),
                append_detail: false,
            },
        ]);

        let snapshot = HubSnapshot::new(vec![session]);
        let todo = snapshot.sessions[0]
            .work_summary
            .todo
            .as_ref()
            .expect("todo reconciled from completion summary");
        assert!(todo
            .items
            .iter()
            .all(|item| item.status == WorkItemStatus::Completed));
        assert_eq!(todo.updated_at, 41);
    }

    #[test]
    fn snapshot_tracks_goal_lifecycle_and_start_time() {
        let mut session = session();
        session.activities.extend([
            SessionActivity {
                id: "goal-create".into(),
                kind: "tool".into(),
                title: "functions · create_goal".into(),
                detail: Some(r#"{"objective":"Ship Lume mobile"}"#.into()),
                status: "completed".into(),
                created_at: 100,
                files: Vec::new(),
                attachments: Vec::new(),
                append_detail: false,
            },
            SessionActivity {
                id: "goal-update".into(),
                kind: "tool".into(),
                title: "functions · update_goal".into(),
                detail: Some(r#"{"status":"complete"}"#.into()),
                status: "completed".into(),
                created_at: 250,
                files: Vec::new(),
                attachments: Vec::new(),
                append_detail: false,
            },
        ]);

        let snapshot = HubSnapshot::new(vec![session]);
        let goal = snapshot.sessions[0]
            .work_summary
            .goal
            .as_ref()
            .expect("goal");
        assert_eq!(goal.objective, "Ship Lume mobile");
        assert_eq!(goal.status, GoalStatus::Complete);
        assert_eq!(goal.started_at, 100);
        assert_eq!(goal.updated_at, 250);
    }

    #[test]
    fn snapshot_recovers_elapsed_goal_from_get_goal() {
        let mut session = session();
        session.activities.push(SessionActivity {
            id: "goal-read".into(),
            kind: "tool".into(),
            title: "functions · get_goal".into(),
            detail: Some(
                r#"{"objective":"Review agents","status":"active","elapsed_seconds":90}"#.into(),
            ),
            status: "completed".into(),
            created_at: 100_000,
            files: Vec::new(),
            attachments: Vec::new(),
            append_detail: false,
        });

        let snapshot = HubSnapshot::new(vec![session]);
        let goal = snapshot.sessions[0]
            .work_summary
            .goal
            .as_ref()
            .expect("goal");
        assert_eq!(goal.started_at, 10_000);
    }

    #[test]
    fn snapshot_normalizes_goal_start_time_from_seconds() {
        let mut session = session();
        session.activities.push(SessionActivity {
            id: "goal-read-seconds".into(),
            kind: "tool".into(),
            title: "functions · get_goal".into(),
            detail: Some(
                r#"{"goal":{"objective":"Test goal","status":"active","createdAt":1785190621}}"#
                    .into(),
            ),
            status: "completed".into(),
            created_at: 1_785_190_942_000,
            files: Vec::new(),
            attachments: Vec::new(),
            append_detail: false,
        });

        let snapshot = HubSnapshot::new(vec![session]);
        let goal = snapshot.sessions[0]
            .work_summary
            .goal
            .as_ref()
            .expect("goal");
        assert_eq!(goal.started_at, 1_785_190_621_000);
    }

    #[test]
    fn realtime_stream_contract_is_versioned_and_additive() {
        let hello = serde_json::to_value(HubStreamMessage::hello()).expect("hello");
        assert_eq!(hello["type"], "hello");
        assert_eq!(hello["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(hello["heartbeatIntervalMs"], STREAM_HEARTBEAT_INTERVAL_MS);
        assert!(hello["features"]
            .as_array()
            .is_some_and(|features| features.iter().any(|value| value == "realtime_stream")));

        let snapshot = serde_json::to_value(HubStreamMessage::Snapshot {
            sequence: 9,
            snapshot: HubSnapshot::new(vec![session()]),
        })
        .expect("snapshot");
        assert_eq!(snapshot["type"], "snapshot");
        assert_eq!(snapshot["sequence"], 9);
        assert_eq!(snapshot["snapshot"]["sessions"][0]["id"], "codex:thread-1");
        assert!(snapshot["snapshot"]["workflowGroups"].is_array());
        assert!(snapshot["snapshot"]["workflowHistory"].is_array());
    }

    #[test]
    fn process_only_session_explains_why_prompt_is_unavailable() {
        let mut session = session();
        session.native_session_id = None;
        let capabilities = SessionCapabilities::for_session(&session);
        assert!(!capabilities.can_prompt);
        assert_eq!(
            capabilities.prompt_unavailable_reason,
            Some(PromptUnavailableReason::SessionNotConnected)
        );
    }

    #[test]
    fn command_json_is_stable_and_validated() {
        let json = r#"{
            "requestId":"mobile-1",
            "type":"submit_prompt",
            "sessionId":"codex:thread-1",
            "prompt":"Continue"
        }"#;
        let request: HubCommandRequest = serde_json::from_str(json).expect("comando");
        request.validate().expect("válido");
        assert_eq!(
            request.command,
            HubCommand::SubmitPrompt {
                session_id: "codex:thread-1".into(),
                prompt: "Continue".into(),
                attachments: Vec::new(),
                delivery: PromptDelivery::NewTurn,
            }
        );
        let serialized = serde_json::to_value(request).expect("json");
        assert_eq!(serialized["type"], "submit_prompt");
        assert_eq!(serialized["sessionId"], "codex:thread-1");
    }

    #[test]
    fn empty_prompt_is_rejected_at_the_protocol_boundary() {
        let request = HubCommandRequest {
            request_id: "mobile-1".into(),
            command: HubCommand::SubmitPrompt {
                session_id: "codex:thread-1".into(),
                prompt: "  ".into(),
                attachments: Vec::new(),
                delivery: PromptDelivery::NewTurn,
            },
        };
        assert_eq!(
            request.validate().expect_err("inválido").code,
            "prompt_empty"
        );
    }

    #[test]
    fn mobile_takeover_preserves_the_pending_prompt() {
        let request: HubCommandRequest = serde_json::from_str(
            r#"{"requestId":"mobile-takeover","type":"take_control_session","sessionId":"codex:thread-1","prompt":"Continue from my phone"}"#,
        )
        .expect("takeover command");
        request.validate().expect("valid takeover command");
        assert_eq!(
            request.command,
            HubCommand::TakeControlSession {
                session_id: "codex:thread-1".into(),
                prompt: "Continue from my phone".into(),
                attachments: Vec::new(),
            }
        );
    }

    #[test]
    fn mobile_can_transfer_control_without_sending_a_prompt() {
        let request: HubCommandRequest = serde_json::from_str(
            r#"{"requestId":"mobile-control-only","type":"take_control_session","sessionId":"codex:thread-1","prompt":""}"#,
        )
        .expect("control-only command");
        request.validate().expect("valid control-only takeover");
    }

    #[test]
    fn workflow_commands_are_versioned_and_validate_their_objective() {
        let request: HubCommandRequest = serde_json::from_str(
            r#"{"requestId":"workflow-mobile","type":"start_workflow","workflowId":"workflow-1","objective":"Review the change"}"#,
        )
        .expect("workflow command");
        request.validate().expect("valid workflow command");
        assert_eq!(
            request.command,
            HubCommand::StartWorkflow {
                workflow_id: "workflow-1".into(),
                objective: "Review the change".into(),
            }
        );

        let invalid = HubCommandRequest {
            request_id: "workflow-mobile-empty".into(),
            command: HubCommand::StartWorkflow {
                workflow_id: "workflow-1".into(),
                objective: "  ".into(),
            },
        };
        assert_eq!(
            invalid.validate().expect_err("empty objective").code,
            "workflow_objective_empty"
        );
    }

    #[test]
    fn companion_versions_are_compared_numerically() {
        assert!(is_version_newer("0.10.0", "0.9.9"));
        assert!(is_version_newer("0.9.1", "0.9.0"));
        assert!(!is_version_newer("0.9.0", "0.9.0"));
        assert!(!is_version_newer("0.8.9", "0.9.0"));

        let invalid = HubCommandRequest {
            request_id: "mobile-version".into(),
            command: HubCommand::ReportMobileVersion {
                version: "../../update".into(),
            },
        };
        assert_eq!(
            invalid.validate().expect_err("inválido").code,
            "version_invalid"
        );
    }
}
