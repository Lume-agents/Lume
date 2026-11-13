//! Answers the `can_use_tool` requests of Lume's streaming Claude process with
//! the chat's own permission and question cards: AskUserQuestion becomes a
//! question, ExitPlanMode a plan to approve, anything else a permission.

use std::time::Duration;

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::{
    domain::{HookEvent, HookEventKind, PermissionAction, SessionControlOrigin},
    event_server,
    state::AppState,
};

const WAIT: Duration = Duration::from_secs(15 * 60);
pub const PLAN_KIND: &str = "plan";
const KEEP_PLANNING: &str =
    "The user chose to keep planning. Ask what to change in the plan before leaving plan mode.";
const DENIED: &str = "Permissão recusada no Lume";

/// The hook-shaped payload the adapters already know how to read.
pub fn hook_shape(native_id: &str, request: &Value) -> Value {
    json!({
        "session_id": native_id,
        "tool_name": request.get("tool_name").cloned().unwrap_or(Value::Null),
        "tool_input": request.get("input").cloned().unwrap_or_else(|| json!({})),
        "tool_use_id": request.get("tool_use_id").cloned().unwrap_or(Value::Null),
        "permission_suggestions": request.get("permission_suggestions").cloned().unwrap_or(Value::Null),
        "description": request.get("description").cloned().unwrap_or(Value::Null),
    })
}

/// What Claude gets back for a permission decision. Allowing passes the input back unchanged.
pub fn permission_response(action: Option<PermissionAction>, request: &Value) -> Value {
    let input = request.get("input").cloned().unwrap_or_else(|| json!({}));
    let is_plan = request.get("tool_name").and_then(Value::as_str) == Some("ExitPlanMode");
    match action {
        Some(PermissionAction::AllowOnce) => json!({ "behavior": "allow", "updatedInput": input }),
        Some(PermissionAction::AllowSession) => {
            let suggestions = request
                .get("permission_suggestions")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|mut suggestion| {
                    if let Some(object) = suggestion.as_object_mut() {
                        object.insert("destination".into(), Value::String("session".into()));
                    }
                    suggestion
                })
                .collect::<Vec<_>>();
            json!({ "behavior": "allow", "updatedInput": input, "updatedPermissions": suggestions })
        }
        _ if is_plan => json!({ "behavior": "deny", "message": KEEP_PLANNING }),
        _ => json!({ "behavior": "deny", "message": DENIED }),
    }
}

/// What Claude gets back for answered questions; no answer declines the tool.
pub fn question_response(
    answers: Option<Vec<crate::domain::QuestionAnswer>>,
    raw: &Value,
) -> Value {
    answers
        .and_then(|answers| crate::adapters::claude_question_updated_input(&answers, raw))
        .map(|input| json!({ "behavior": "allow", "updatedInput": input }))
        .unwrap_or_else(
            || json!({ "behavior": "deny", "message": "A pergunta ficou sem resposta" }),
        )
}

fn base_event(session_id: &str, native_id: &str, mode: Option<&str>) -> HookEvent {
    HookEvent {
        event: HookEventKind::PermissionRequest,
        session_id: session_id.into(),
        agent: crate::domain::AgentKind::ClaudeCode,
        agent_label: Some("Claude Code".into()),
        session_name: None,
        project: None,
        source: None,
        source_app: None,
        control_origin: SessionControlOrigin::Lume,
        status_label: None,
        started_at: None,
        process_id: None,
        native_session_id: Some(native_id.into()),
        working_directory: None,
        permission_profile: Some(crate::adapters::claude_direct_profile(mode)),
        permission: None,
        question: None,
        last_response: None,
        activity: None,
        activities: Vec::new(),
        wait_for_decision: true,
    }
}

pub fn answer_can_use_tool(
    app: &AppHandle,
    state: &AppState,
    session_id: &str,
    native_id: &str,
    mode: Option<&str>,
    request: &Value,
) -> Value {
    let raw = hook_shape(native_id, request);
    let tool = request
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut event = base_event(session_id, native_id, mode);
    if tool == "AskUserQuestion" {
        let Some(question) = crate::adapters::claude_question_request(&raw, native_id) else {
            return crate::claude_session::deny("Lume could not read these questions");
        };
        let question_id = question.id.clone();
        event.event = HookEventKind::QuestionRequest;
        event.status_label = Some("Aguardando sua resposta".into());
        event.question = Some(question);
        if event_server::publish_event(state, app, event).is_err() {
            return crate::claude_session::deny("Lume could not show these questions");
        }
        let answers = state
            .wait_for_question_answer(&question_id, WAIT)
            .ok()
            .flatten();
        if answers.is_none() {
            let _ = state.expire_question(&question_id);
        }
        return question_response(answers, &raw);
    }
    let mut permission = crate::adapters::permission_request("claude", &raw, native_id);
    if tool == "ExitPlanMode" {
        permission.kind = PLAN_KIND.into();
        permission.summary = "Aprovar o plano?".into();
        if let Some(plan) = request
            .get("input")
            .and_then(|input| input.get("plan"))
            .and_then(Value::as_str)
        {
            permission.resource = plan.chars().take(4 * 1024).collect();
        }
    }
    let permission_id = permission.id.clone();
    event.status_label = Some("Aguardando sua permissão".into());
    event.permission = Some(permission);
    if event_server::publish_event(state, app, event).is_err() {
        return crate::claude_session::deny(DENIED);
    }
    let action = state.wait_for_decision(&permission_id, WAIT).ok().flatten();
    permission_response(action, request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::QuestionAnswer;

    fn ask() -> Value {
        json!({
            "subtype": "can_use_tool",
            "tool_name": "AskUserQuestion",
            "input": { "questions": [{ "question": "Color?", "header": "Color",
                "options": [{ "label": "Red" }, { "label": "Blue" }], "multiSelect": false }] },
            "tool_use_id": "toolu_1",
        })
    }

    #[test]
    fn answers_go_back_as_updated_input() {
        let raw = hook_shape("native", &ask());
        let question = crate::adapters::claude_question_request(&raw, "native").unwrap();
        let answers = vec![QuestionAnswer {
            question_id: question.questions[0].id.clone(),
            answers: vec!["Blue".into()],
        }];
        let response = question_response(Some(answers), &raw);
        assert_eq!(response["behavior"], "allow");
        assert_eq!(response["updatedInput"]["answers"]["Color?"], "Blue");
        assert_eq!(response["updatedInput"]["questions"][0]["header"], "Color");
        assert_eq!(question_response(None, &raw)["behavior"], "deny");
    }

    #[test]
    fn plan_denial_means_keep_planning_and_allow_passes_the_input() {
        let plan = json!({ "tool_name": "ExitPlanMode", "input": { "plan": "1. Do it" } });
        assert_eq!(permission_response(None, &plan)["message"], KEEP_PLANNING);
        let allowed = permission_response(Some(PermissionAction::AllowOnce), &plan);
        assert_eq!(allowed["behavior"], "allow");
        assert_eq!(allowed["updatedInput"]["plan"], "1. Do it");
        let bash = json!({
            "tool_name": "Bash",
            "input": { "command": "ls" },
            "permission_suggestions": [{ "type": "addRules", "destination": "localSettings" }],
        });
        assert_eq!(
            permission_response(Some(PermissionAction::Deny), &bash)["message"],
            DENIED
        );
        let session = permission_response(Some(PermissionAction::AllowSession), &bash);
        assert_eq!(session["updatedPermissions"][0]["destination"], "session");
    }
}
