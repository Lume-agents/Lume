//! What the Antigravity CLI adds on top of the generic ACP bridge: its tools carry their native
//! name and arguments, so they are presented like the stream and hook rows; `ask_question` waits
//! for the next prompt instead of a client request; and token usage arrives per turn.

use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, OnceLock},
};

use serde_json::{json, Value};

use crate::{
    domain::{
        AgentKind, InteractiveQuestion, PendingQuestion, PermissionAction, PermissionProfile,
        QuestionAnswer, QuestionOption, SessionActivity,
    },
    state::{now_millis, AppState},
};

/// The ACP mode a Lume permission choice runs with. "Allow all" has no ACP mode: it runs as
/// `auto` and Lume approves whatever the agent still asks.
pub(crate) fn acp_mode_for_permission(mode: &str) -> &'static str {
    match mode {
        crate::antigravity_stream::PERMISSION_ACCEPT_EDITS => "accept-edits",
        crate::antigravity_stream::PERMISSION_AUTO
        | crate::antigravity_stream::PERMISSION_ALLOW_ALL => "auto",
        crate::antigravity_stream::PERMISSION_PLAN => "plan",
        _ => "default",
    }
}

pub(crate) fn permission_mode(state: &AppState, native_id: &str) -> String {
    state
        .permission_mode_override_for_native_id(AgentKind::Antigravity, native_id)
        .ok()
        .flatten()
        .filter(|mode| crate::antigravity_stream::is_permission_mode(mode))
        .unwrap_or_else(|| crate::antigravity_stream::PERMISSION_DEFAULT.into())
}

/// What a Lume-driven Antigravity session runs with: Lume answers its permission requests.
pub(crate) fn permission_profile(mode: &str) -> PermissionProfile {
    let mut profile = crate::antigravity_stream::permission_profile_for_mode(mode);
    profile.can_respond_from_lume = true;
    profile.available_actions = vec![PermissionAction::AllowOnce, PermissionAction::Deny];
    profile
}

pub(crate) fn approves_everything(state: &AppState, native_id: &str) -> bool {
    permission_mode(state, native_id) == crate::antigravity_stream::PERMISSION_ALLOW_ALL
}

pub(crate) struct ToolUpdate {
    pub activity: Option<SessionActivity>,
    pub question: Option<PendingQuestion>,
}

fn params_of(update: &Value) -> Value {
    let raw = update
        .get("rawInput")
        .filter(|value| value.is_object())
        .or_else(|| update.get("args").filter(|value| value.is_object()));
    raw.cloned().unwrap_or_else(|| json!({}))
}

fn output_of(update: &Value) -> Option<String> {
    let raw = update.get("rawOutput").filter(|value| !value.is_null());
    let from_raw = raw.and_then(|value| match value {
        Value::String(text) => Some(text.clone()),
        Value::Object(map) if map.is_empty() => None,
        Value::Object(map) => map
            .get("output")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| serde_json::to_string_pretty(value).ok()),
        other => serde_json::to_string_pretty(other).ok(),
    });
    let from_content = || {
        let lines = update["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|content| content["content"]["text"].as_str())
            .collect::<Vec<_>>();
        (!lines.is_empty()).then(|| lines.join("\n"))
    };
    from_raw
        .or_else(from_content)
        .map(|text| crate::integrations::clean_antigravity_generic_output(&text))
        .filter(|text| !text.is_empty())
}

fn file_diffs() -> &'static Mutex<HashMap<String, String>> {
    static DIFFS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    DIFFS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn subagents() -> &'static Mutex<HashMap<(String, String), String>> {
    static CHILDREN: OnceLock<Mutex<HashMap<(String, String), String>>> = OnceLock::new();
    CHILDREN.get_or_init(|| Mutex::new(HashMap::new()))
}

fn waiting_questions() -> &'static Mutex<HashSet<String>> {
    static WAITING: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    WAITING.get_or_init(|| Mutex::new(HashSet::new()))
}

fn context_samples() -> &'static Mutex<HashMap<String, (u64, u64)>> {
    static SAMPLES: OnceLock<Mutex<HashMap<String, (u64, u64)>>> = OnceLock::new();
    SAMPLES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The child conversation a subagent tool call opened, once its result named it.
pub(crate) fn subagent_conversation_id(parent_id: &str, call_id: &str) -> Option<String> {
    subagents()
        .lock()
        .ok()?
        .get(&(parent_id.to_string(), call_id.to_string()))
        .cloned()
}

/// A diff from the ACP `diff` content, for edits whose arguments did not give one.
fn content_diff(update: &Value) -> Option<String> {
    let diff = update["content"]
        .as_array()?
        .iter()
        .find(|content| content["type"] == "diff")?;
    let path = diff["path"].as_str()?.replace('\\', "/");
    let new_text = diff["newText"].as_str().unwrap_or_default();
    let old_text = diff["oldText"].as_str();
    let new_lines = new_text.lines().collect::<Vec<_>>();
    let mut text = match old_text {
        None => format!(
            "diff --git a/{path} b/{path}\nnew file mode 100644\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{} @@\n",
            new_lines.len()
        ),
        Some(old) => format!(
            "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1,{} +1,{} @@\n",
            old.lines().count(),
            new_lines.len()
        ),
    };
    for line in old_text.into_iter().flat_map(str::lines) {
        text.push_str(&format!("-{line}\n"));
    }
    for line in new_lines {
        text.push_str(&format!("+{line}\n"));
    }
    Some(text)
}

fn question_from(session_id: &str, call_id: &str, params: &Value) -> Option<PendingQuestion> {
    let questions = params["questions"]
        .as_array()?
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let question = item["question"].as_str()?.trim();
            (!question.is_empty()).then(|| InteractiveQuestion {
                id: format!("q{index}"),
                header: String::new(),
                question: question.chars().take(2_000).collect(),
                is_other: true,
                is_secret: false,
                options: item["options"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|option| {
                        option
                            .as_str()
                            .or_else(|| option["label"].as_str())
                            .map(str::trim)
                            .filter(|label| !label.is_empty())
                    })
                    .take(20)
                    .map(|label| QuestionOption {
                        label: label.chars().take(300).collect(),
                        description: String::new(),
                    })
                    .collect(),
            })
        })
        .take(8)
        .collect::<Vec<_>>();
    (!questions.is_empty()).then(|| PendingQuestion {
        id: format!("agy-question:{session_id}:{call_id}"),
        questions,
        requested_at: chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now()).to_rfc3339(),
    })
}

/// The answer Lume sends back as the next prompt: Antigravity reads it as the user's reply.
pub(crate) fn answer_prompt(question: &PendingQuestion, answers: &[QuestionAnswer]) -> String {
    let lines = question
        .questions
        .iter()
        .filter_map(|item| {
            let answer = answers
                .iter()
                .find(|answer| answer.question_id == item.id)?
                .answers
                .iter()
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>()
                .join(", ");
            (!answer.is_empty()).then_some((item.question.as_str(), answer))
        })
        .collect::<Vec<_>>();
    match lines.as_slice() {
        [(_, answer)] => answer.clone(),
        _ => lines
            .iter()
            .map(|(question, answer)| format!("{question}\n{answer}"))
            .collect::<Vec<_>>()
            .join("\n\n"),
    }
}

/// None when the update does not name its tool, so the generic ACP row is used instead.
pub(crate) fn tool_update(session_id: &str, update: &Value) -> Option<ToolUpdate> {
    let name = update["name"].as_str().filter(|name| !name.is_empty())?;
    let call_id = update["toolCallId"].as_str().unwrap_or("tool");
    let params = params_of(update);
    let state_name = update["status"].as_str().unwrap_or_default();
    if name == "ask_question" {
        let question = matches!(state_name, "pending" | "in_progress")
            .then(|| question_from(session_id, call_id, &params))
            .flatten();
        if question.is_some() {
            if let Ok(mut waiting) = waiting_questions().lock() {
                waiting.insert(session_id.to_string());
            }
        }
        return Some(ToolUpdate {
            activity: None,
            question,
        });
    }
    if crate::adapters::is_antigravity_internal_tool(name, Some(&params)) {
        return Some(ToolUpdate {
            activity: None,
            question: None,
        });
    }
    let failed = state_name == "failed";
    let done = failed || state_name == "completed";
    let output = output_of(update);
    let error = failed
        .then(|| {
            update["rawOutput"]["error"]
                .as_str()
                .map(str::to_string)
                .or_else(|| output.clone())
        })
        .flatten();
    let presentation = crate::adapters::antigravity_tool_presentation(
        name,
        Some(&params),
        None,
        output.as_deref(),
        update.get("rawOutput").filter(|value| !value.is_null()),
        error.as_deref(),
        done,
    );
    if presentation.kind == "subagent" && done {
        if let Some(child) = output
            .as_deref()
            .and_then(crate::integrations::extract_uuid_token)
        {
            if let Ok(mut children) = subagents().lock() {
                if children.len() >= 256 {
                    children.clear();
                }
                children.insert((session_id.to_string(), call_id.to_string()), child);
            }
        }
    }
    let id = if presentation.kind == "subagent" {
        format!("antigravity:{session_id}:subagent:{call_id}")
    } else {
        format!("agy-tool:{session_id}:{call_id}")
    };
    let mut detail = presentation.detail;
    if presentation.kind == "file" {
        let is_diff = |text: &Option<String>| {
            text.as_deref()
                .is_some_and(|text| text.starts_with("diff --git "))
        };
        if !is_diff(&detail) {
            detail = content_diff(update).or(detail);
        }
        if let Ok(mut diffs) = file_diffs().lock() {
            if !done {
                if is_diff(&detail) {
                    if diffs.len() >= 64 {
                        diffs.clear();
                    }
                    diffs.insert(id.clone(), detail.clone().unwrap_or_default());
                }
            } else if let Some(earlier) = diffs.remove(&id) {
                // A write that replaced the file can no longer read what was there before.
                let new_file_only = detail
                    .as_deref()
                    .is_some_and(|text| text.contains("\n--- /dev/null\n"));
                if !is_diff(&detail) || new_file_only {
                    detail = Some(earlier);
                }
            }
        }
    }
    Some(ToolUpdate {
        activity: Some(SessionActivity {
            id,
            kind: presentation.kind.into(),
            title: presentation.title.chars().take(180).collect(),
            detail: detail.map(|text| text.chars().take(presentation.detail_limit).collect()),
            status: if failed {
                "failed"
            } else if done {
                "completed"
            } else {
                "running"
            }
            .into(),
            created_at: now_millis(),
            files: presentation.files,
            attachments: Vec::new(),
            append_detail: false,
        }),
        question: None,
    })
}

pub(crate) fn record_context(session_id: &str, update: &Value) {
    let number = |value: &Value| {
        value
            .as_u64()
            .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
    };
    let (Some(used), Some(size)) = (number(&update["used"]), number(&update["size"])) else {
        return;
    };
    if let Ok(mut samples) = context_samples().lock() {
        samples.insert(session_id.to_string(), (used, size));
    }
}

/// True once per question the agent left open when its turn ended.
pub(crate) fn take_waiting_question(session_id: &str) -> bool {
    waiting_questions()
        .lock()
        .is_ok_and(|mut waiting| waiting.remove(session_id))
}

pub(crate) fn turn_usage_activity(
    session_id: &str,
    result: &Value,
    prompt_at: Option<i64>,
) -> Option<SessionActivity> {
    let usage = result.get("usage")?;
    let field = |key: &str| usage.get(key).and_then(Value::as_u64);
    let input = field("inputTokens").unwrap_or(0);
    let output = field("outputTokens").unwrap_or(0);
    if input + output == 0 {
        return None;
    }
    let cache_read = field("cachedReadTokens");
    let (context, window) = context_samples()
        .lock()
        .ok()
        .and_then(|samples| samples.get(session_id).copied())
        .map(|(used, size)| (used, Some(size)))
        .unwrap_or((input + cache_read.unwrap_or(0) + output, None));
    let created_at = now_millis();
    let mut detail = json!({
        "turnId": format!("agy-acp-turn-{created_at}"),
        "totalTokens": input + output,
        "inputTokens": input,
        "outputTokens": output,
        "contextTokens": context,
        "createdAt": created_at,
    });
    if let Some(cache_read) = cache_read {
        detail["cacheReadTokens"] = json!(cache_read);
    }
    if let Some(window) = window.filter(|window| *window > 0) {
        detail["contextWindow"] = json!(window);
    }
    if let Some(prompt_at) = prompt_at {
        detail["promptAt"] = json!(prompt_at);
    }
    Some(SessionActivity {
        id: format!("agy-token:{session_id}:acp:{created_at}"),
        kind: "token_usage".into(),
        title: "Uso de tokens".into(),
        detail: Some(detail.to_string()),
        status: "completed".into(),
        created_at,
        files: Vec::new(),
        attachments: Vec::new(),
        append_detail: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acp_tool_updates_read_like_the_stream_rows() {
        let command = tool_update(
            "conv",
            &json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": "call_1",
                "name": "run_command",
                "status": "completed",
                "kind": "execute",
                "title": "ls -la",
                "rawInput": {"CommandLine": "ls -la", "toolSummary": "List directory contents"},
                "rawOutput": {"output": "Created At: t\nCompleted At: t\nCommand exited with code 0.\n\ntotal 1"}
            }),
        )
        .unwrap()
        .activity
        .unwrap();
        assert_eq!(command.id, "agy-tool:conv:call_1");
        assert_eq!(command.kind, "command");
        assert_eq!(command.title, "ls -la");
        assert_eq!(command.status, "completed");
        assert!(command.detail.unwrap().starts_with("Command exited with code 0."));

        let write = tool_update(
            "conv",
            &json!({
                "toolCallId": "call_2",
                "name": "write_file",
                "status": "completed",
                "kind": "edit",
                "rawInput": {"Path": "/nowhere/lume-acp-test/hello.txt", "Content": "hi\n", "Overwrite": true},
                "content": [{"type": "diff", "path": "/nowhere/lume-acp-test/hello.txt", "newText": "hi\n"}]
            }),
        )
        .unwrap()
        .activity
        .unwrap();
        assert_eq!(write.kind, "file");
        assert_eq!(write.files, vec!["/nowhere/lume-acp-test/hello.txt"]);
        assert!(write.detail.unwrap().contains("+hi\n"));

        let internal = tool_update(
            "conv",
            &json!({"toolCallId": "call_3", "name": "schedule", "status": "completed", "rawInput": {}}),
        )
        .unwrap();
        assert!(internal.activity.is_none() && internal.question.is_none());
        assert!(tool_update("conv", &json!({"toolCallId": "call_4", "status": "completed"})).is_none());
    }

    #[test]
    fn an_open_question_becomes_a_question_card_and_its_answer_the_next_prompt() {
        let update = tool_update(
            "conv-q",
            &json!({
                "toolCallId": "call_q",
                "name": "ask_question",
                "status": "pending",
                "rawInput": {"questions": [{"is_multi_select": false, "options": ["Red", "Blue"], "question": "Which color do you prefer?"}]}
            }),
        )
        .unwrap();
        assert!(update.activity.is_none());
        let question = update.question.unwrap();
        assert_eq!(question.id, "agy-question:conv-q:call_q");
        assert_eq!(question.questions[0].question, "Which color do you prefer?");
        assert_eq!(question.questions[0].options.len(), 2);
        assert!(take_waiting_question("conv-q"));
        assert!(!take_waiting_question("conv-q"));
        let answers = [QuestionAnswer {
            question_id: "q0".into(),
            answers: vec!["Blue".into()],
        }];
        assert_eq!(answer_prompt(&question, &answers), "Blue");
    }

    #[test]
    fn turn_usage_reports_the_context_the_agent_measured() {
        record_context("conv-u", &json!({"used": 9457, "size": 1048576}));
        let activity = turn_usage_activity(
            "conv-u",
            &json!({"usage": {"inputTokens": 13414, "outputTokens": 446, "cachedReadTokens": 13720}}),
            Some(5),
        )
        .unwrap();
        let detail: Value = serde_json::from_str(activity.detail.as_deref().unwrap()).unwrap();
        assert_eq!(detail["contextTokens"], 9457);
        assert_eq!(detail["contextWindow"], 1048576);
        assert_eq!(detail["totalTokens"], 13860);
        assert_eq!(detail["cacheReadTokens"], 13720);
        assert!(turn_usage_activity("conv-u", &json!({"usage": {}}), None).is_none());
    }

    #[test]
    fn allow_all_runs_as_auto_with_lume_answering_the_rest() {
        assert_eq!(acp_mode_for_permission("agy_allow_all"), "auto");
        assert_eq!(acp_mode_for_permission("agy_auto"), "auto");
        assert_eq!(acp_mode_for_permission("agy_accept_edits"), "accept-edits");
        assert_eq!(acp_mode_for_permission("agy_plan"), "plan");
        assert_eq!(acp_mode_for_permission("agy_default"), "default");
    }
}
