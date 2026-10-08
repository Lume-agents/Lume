use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::Path,
    sync::mpsc,
    thread,
    time::Duration,
};

use chrono::DateTime;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sysinfo::{get_current_pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use crate::{
    domain::{
        AccessMode, AgentKind, HookEvent, HookEventKind, InteractiveQuestion, PendingQuestion,
        PermissionAction, PermissionProfile, PermissionRequest, QuestionAnswer, QuestionOption,
        SessionActivity, SessionControlOrigin, SessionSource,
    },
    event_server,
    state::now_millis,
};

const MAX_HOOK_STDIN_BYTES: u64 = 2 * 1024 * 1024;
const HOOK_STDIN_READ_TIMEOUT: Duration = Duration::from_secs(2);

pub fn run_hook(provider: &str) -> i32 {
    let fallback = antigravity_hook_output(provider).or_else(|| gemini_hook_output(provider));
    let output = read_hook_event(provider)
        .or(fallback)
        .and_then(|output| serde_json::to_string(&output).ok());
    if let Some(output) = output {
        println!("{output}");
    }
    0
}

fn read_hook_event(provider: &str) -> Option<Value> {
    let input = read_hook_stdin()?;
    let raw: Value = serde_json::from_str(&input).ok()?;
    // Capture exact CLI identity even while the desktop is closed. Diagnostic
    // records remain opt-in; neither failure can block the provider's tool.
    let codex_context = crate::codex_identity_probe::observe_hook(provider, &raw);
    let event = map_event_with_context(provider, &raw, codex_context)?;
    let wait_for_decision = event.wait_for_decision;
    let is_question = matches!(event.event, HookEventKind::QuestionRequest);
    let payload = serde_json::to_string(&event).ok()?;
    if !wait_for_decision && matches!(event.agent, AgentKind::Antigravity | AgentKind::Gemini) {
        event_server::send_observation_event(&payload).ok()?;
        return None;
    }
    let response = event_server::send_event(&payload).ok()?;

    if provider == "claude" && wait_for_decision {
        if is_question {
            claude_question_output(response.question_answers, &raw)
        } else {
            claude_permission_output(response.action, &raw)
        }
    } else {
        None
    }
}

fn read_hook_stdin() -> Option<String> {
    read_hook_input_with_timeout(std::io::stdin(), HOOK_STDIN_READ_TIMEOUT)
}

fn read_hook_input_with_timeout<R>(reader: R, timeout: Duration) -> Option<String>
where
    R: Read + Send + 'static,
{
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("lume-hook-stdin".into())
        .spawn(move || {
            let _ = sender.send(read_bounded_hook_input(reader));
        })
        .ok()?;
    receiver.recv_timeout(timeout).ok()?.ok()
}

fn read_bounded_hook_input<R: Read>(reader: R) -> std::io::Result<String> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_HOOK_STDIN_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_HOOK_STDIN_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Hook input exceeds the size limit",
        ));
    }
    String::from_utf8(bytes)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

fn antigravity_hook_output(provider: &str) -> Option<Value> {
    let (agent, event) = provider.split_once(':')?;
    if agent != "antigravity" {
        return None;
    }
    let event = if matches!(event, "PreToolUseAllow" | "PreToolUseAsk") {
        "PreToolUse"
    } else {
        event
    };

    Some(match event {
        // Preserve the CLI's native approval policy, including headless soft-denial.
        "PreToolUse" => json!({ "decision": "ask" }),
        "Stop" => json!({ "decision": "allow" }),
        _ => json!({}),
    })
}

fn gemini_hook_output(provider: &str) -> Option<Value> {
    (provider == "gemini").then(|| json!({}))
}

#[cfg(test)]
fn map_event(provider: &str, raw: &Value) -> Option<HookEvent> {
    map_event_with_context(provider, raw, None)
}

fn map_event_with_context(
    provider: &str,
    raw: &Value,
    codex_context: Option<(Option<u32>, SessionSource)>,
) -> Option<HookEvent> {
    let (provider, forced_hook_name) = provider
        .split_once(':')
        .map_or((provider, None), |(provider, event)| {
            (provider, Some(event))
        });
    let agent = match provider {
        "codex" => AgentKind::Codex,
        "claude" => AgentKind::ClaudeCode,
        "antigravity" => AgentKind::Antigravity,
        "gemini" => AgentKind::Gemini,
        _ => return None,
    };
    // Child hooks can reuse the parent's session ID. Drop their explicit
    // lineage before normalization loses it and before ancestry fallback can
    // turn them into root conversation activity. A root tool describing a
    // subagent still passes: tool_input/agent_id are not lineage markers.
    if provider == "codex"
        && (raw
            .get("parent_session_id")
            .is_some_and(|value| !value.is_null())
            || raw
                .get("parent_thread_id")
                .is_some_and(|value| !value.is_null())
            || raw.get("is_subagent").and_then(Value::as_bool) == Some(true)
            || raw
                .get("source")
                .is_some_and(|source| source.is_object() || source.as_str() == Some("subagent")))
    {
        return None;
    }
    let hook_name = forced_hook_name
        .map(str::to_string)
        .or_else(|| string(raw, "hook_event_name"))?;
    let hook_name = if provider == "antigravity"
        && matches!(hook_name.as_str(), "PreToolUseAllow" | "PreToolUseAsk")
    {
        "PreToolUse".to_string()
    } else {
        hook_name
    };
    // The CLI's agent view starts background sessions that nobody has used yet.
    // Their start and idle events say nothing worth a card; the first real activity does.
    if provider == "claude"
        && matches!(
            hook_name.as_str(),
            "SessionStart" | "SessionEnd" | "Notification"
        )
        && string(raw, "session_id").is_some_and(|session_id| {
            crate::integrations::claude_background_session_unused(&session_id)
        })
    {
        return None;
    }
    // Claude's internal forks (recap, prompt suggestion) also fire SubagentStop,
    // but without an agent_type; only real subagents carry one.
    if provider == "claude"
        && matches!(hook_name.as_str(), "SubagentStart" | "SubagentStop")
        && string(raw, "agent_type").is_none_or(|agent_type| agent_type.trim().is_empty())
    {
        return None;
    }
    // Antigravity and legacy Gemini hooks are observation-only. Their payloads
    // already contain a provider-native session identity, while traversing all
    // processes for every tool hook is both expensive and prone to associating
    // another same-workspace CLI process with this conversation.
    let managed_antigravity_stream = is_managed_antigravity_stream(
        provider,
        std::env::var("LUME_ANTIGRAVITY_STREAM").ok().as_deref(),
    );
    let managed_claude_prompt =
        provider == "claude" && std::env::var("LUME_CLAUDE_PROMPT_CAPTURE").as_deref() == Ok("1");
    let (process_id, source, headless_resume) =
        if matches!(&agent, AgentKind::Antigravity | AgentKind::Gemini) {
            (None, SessionSource::Cli, false)
        } else if provider == "codex" && codex_context.is_some() {
            let (pid, source) = codex_context?;
            (pid, source, false)
        } else {
            agent_process_context(provider, string(raw, "session_id").as_deref())
        };
    let event = match (provider, hook_name.as_str()) {
        (_, "SessionStart") => HookEventKind::SessionStarted,
        ("codex", "UserPromptSubmit") | ("claude", "UserPromptSubmit") => HookEventKind::Running,
        ("gemini", "BeforeAgent") => HookEventKind::Running,
        ("antigravity", "PreInvocation" | "PreToolUse" | "PostToolUse" | "PostInvocation") => {
            HookEventKind::Running
        }
        ("claude", "PreToolUse") if is_claude_question(raw) => HookEventKind::QuestionRequest,
        ("codex" | "claude", "PreToolUse") | ("gemini", "BeforeTool") => HookEventKind::Running,
        ("codex", "PostToolUse")
        | ("claude", "PostToolUse" | "PostToolUseFailure")
        | ("gemini", "AfterTool") => HookEventKind::Running,
        ("claude", "PostToolBatch")
        | ("claude", "PermissionDenied")
        | ("claude", "SubagentStart" | "SubagentStop")
        | ("claude", "TaskCreated" | "TaskCompleted") => HookEventKind::Activity,
        (_, "PermissionRequest") => HookEventKind::PermissionRequest,
        ("gemini", "Notification")
            if string(raw, "notification_type").as_deref() == Some("ToolPermission") =>
        {
            HookEventKind::PermissionRequest
        }
        ("claude", "Notification")
            if matches!(
                string(raw, "notification_type").as_deref(),
                Some("idle_prompt" | "agent_needs_input")
            ) =>
        {
            HookEventKind::WaitingForInput
        }
        ("claude", "Notification")
            if string(raw, "notification_type").as_deref() == Some("agent_completed") =>
        {
            if managed_claude_prompt {
                HookEventKind::Activity
            } else if notification_reports_failure(raw) {
                HookEventKind::Failed
            } else {
                HookEventKind::Completed
            }
        }
        ("antigravity", "Stop") => antigravity_stop_event(raw),
        ("claude", "Stop" | "SessionEnd") if managed_claude_prompt => HookEventKind::Activity,
        ("codex", "Stop") | ("claude", "Stop") | ("gemini", "AfterAgent") => {
            HookEventKind::Completed
        }
        (_, "StopFailure") => HookEventKind::Failed,
        ("claude", "SessionEnd") if headless_resume => HookEventKind::Completed,
        (_, "SessionEnd") => HookEventKind::SessionEnded,
        _ => return None,
    };

    let session_id = string(raw, "session_id").or_else(|| string(raw, "conversationId"))?;
    let cwd = if provider == "antigravity" {
        antigravity_working_directory(raw).or_else(|| string(raw, "cwd"))
    } else {
        string(raw, "cwd").or_else(|| antigravity_working_directory(raw))
    };
    let source = if managed_antigravity_stream {
        SessionSource::Desktop
    } else {
        source
    };
    let process_id = (!headless_resume).then_some(process_id).flatten();
    let permission_mode = string(raw, "permission_mode");
    let is_permission = matches!(event, HookEventKind::PermissionRequest);
    let direct_response = provider == "claude"
        && (hook_name == "PermissionRequest" || matches!(event, HookEventKind::QuestionRequest));
    let permission_profile = if is_permission || permission_mode.is_some() {
        Some(permission_profile(
            provider,
            permission_mode.as_deref(),
            raw,
            direct_response,
        ))
    } else {
        None
    };
    let permission = if is_permission {
        Some(permission_request(provider, raw, &session_id))
    } else {
        None
    };
    let question = if matches!(event, HookEventKind::QuestionRequest) {
        claude_question_request(raw, &session_id)
    } else {
        None
    };
    let last_response = matches!(
        &event,
        HookEventKind::Completed | HookEventKind::Failed | HookEventKind::SessionEnded
    )
    .then(|| hook_response(raw))
    .flatten();
    let activity = if matches!(event, HookEventKind::QuestionRequest) {
        None
    } else {
        hook_activity(
            provider,
            hook_name.as_str(),
            raw,
            &session_id,
            last_response.as_deref(),
        )
    };
    let activities = if provider == "claude" {
        claude_transcript_activities(raw, &session_id)
    } else {
        Vec::new()
    };
    let event_status_label = status_label(hook_name.as_str(), &event).map(str::to_string);

    Some(HookEvent {
        event,
        session_id: if managed_antigravity_stream {
            format!("antigravity-stream:{session_id}")
        } else {
            format!("{provider}:{session_id}")
        },
        agent,
        agent_label: None,
        session_name: (provider == "claude")
            .then(|| string(raw, "transcript_path"))
            .flatten()
            .and_then(|path| crate::integrations::claude_transcript_title(Path::new(&path)))
            .or_else(|| {
                ["session_name", "thread_name", "conversation_name", "slug"]
                    .into_iter()
                    .find_map(|key| string(raw, key))
            }),
        project: cwd.as_deref().and_then(project_name),
        source: Some(source),
        source_app: None,
        control_origin: if matches!(std::env::var("LUME_MANAGED_SESSION").as_deref(), Ok("1")) {
            SessionControlOrigin::Lume
        } else {
            SessionControlOrigin::External
        },
        status_label: event_status_label,
        started_at: string(raw, "timestamp"),
        process_id,
        native_session_id: Some(session_id),
        working_directory: cwd,
        permission_profile,
        permission,
        question,
        last_response,
        activity,
        activities,
        wait_for_decision: direct_response,
    })
}

fn is_managed_antigravity_stream(provider: &str, marker: Option<&str>) -> bool {
    provider == "antigravity" && marker == Some("1")
}

fn hook_activity(
    provider: &str,
    hook_name: &str,
    raw: &Value,
    session_id: &str,
    last_response: Option<&str>,
) -> Option<SessionActivity> {
    if matches!(hook_name, "UserPromptSubmit" | "BeforeAgent") {
        let prompt = ["prompt", "user_prompt", "message"]
            .into_iter()
            .find_map(|key| string(raw, key));
        return prompt.map(|prompt| SessionActivity {
            id: format!("{provider}:{session_id}:prompt:{}", now_millis()),
            kind: "prompt".into(),
            title: "Prompt enviado".into(),
            detail: Some(truncate(prompt.trim(), 16 * 1024)),
            status: "completed".into(),
            created_at: now_millis(),
            files: Vec::new(),
            attachments: Vec::new(),
            append_detail: false,
        });
    }
    if matches!(hook_name, "Stop" | "AfterAgent") {
        return last_response.map(|response| SessionActivity {
            id: format!("{provider}:{session_id}:response:{}", now_millis()),
            kind: "message".into(),
            title: "Resposta do agente".into(),
            detail: Some(truncate(response, 32 * 1024)),
            status: "completed".into(),
            created_at: now_millis(),
            files: Vec::new(),
            attachments: Vec::new(),
            append_detail: false,
        });
    }
    if hook_name == "StopFailure" {
        let error = string(raw, "error_details")
            .or_else(|| string(raw, "last_assistant_message"))
            .or_else(|| string(raw, "error"))
            .unwrap_or_else(|| "Claude could not finish the response".into());
        return Some(SessionActivity {
            id: format!("{provider}:{session_id}:failure:{}", now_millis()),
            kind: "error".into(),
            title: "Agent error".into(),
            detail: Some(truncate(error.trim(), 16 * 1024)),
            status: "failed".into(),
            created_at: now_millis(),
            files: Vec::new(),
            attachments: Vec::new(),
            append_detail: false,
        });
    }
    if hook_name == "PermissionDenied" {
        let tool_name = string(raw, "tool_name").unwrap_or_else(|| "Tool".into());
        let resource = raw
            .get("tool_input")
            .and_then(resource_from_input)
            .unwrap_or_else(|| tool_name.clone());
        return Some(SessionActivity {
            id: format!(
                "{provider}:{session_id}:denied:{}",
                string(raw, "tool_use_id").unwrap_or_else(|| now_millis().to_string())
            ),
            kind: "permission".into(),
            title: format!("{tool_name} denied"),
            detail: string(raw, "reason").or(Some(resource)),
            status: "failed".into(),
            created_at: now_millis(),
            files: Vec::new(),
            attachments: Vec::new(),
            append_detail: false,
        });
    }
    if matches!(hook_name, "SubagentStart" | "SubagentStop") {
        let agent_id = string(raw, "agent_id").unwrap_or_else(|| now_millis().to_string());
        let agent_type = string(raw, "agent_type").unwrap_or_else(|| "Subagent".into());
        return Some(SessionActivity {
            id: format!("{provider}:{session_id}:subagent:{agent_id}"),
            kind: "subagent".into(),
            title: agent_type,
            detail: (hook_name == "SubagentStop")
                .then(|| hook_response(raw))
                .flatten(),
            status: if hook_name == "SubagentStart" {
                "running"
            } else {
                "completed"
            }
            .into(),
            created_at: now_millis(),
            files: Vec::new(),
            attachments: Vec::new(),
            append_detail: false,
        });
    }
    if matches!(hook_name, "TaskCreated" | "TaskCompleted") {
        let task_id = string(raw, "task_id").unwrap_or_else(|| now_millis().to_string());
        return Some(SessionActivity {
            id: format!("{provider}:{session_id}:task:{task_id}"),
            kind: "task".into(),
            title: string(raw, "task_subject").unwrap_or_else(|| "Agent task".into()),
            detail: string(raw, "task_description"),
            status: if hook_name == "TaskCreated" {
                "running"
            } else {
                "completed"
            }
            .into(),
            created_at: now_millis(),
            files: Vec::new(),
            attachments: Vec::new(),
            append_detail: false,
        });
    }
    if !matches!(
        hook_name,
        "PreToolUse" | "BeforeTool" | "PostToolUse" | "PostToolUseFailure" | "AfterTool"
    ) {
        return None;
    }

    // Claude's task list is incremental (create, then update by id), so the whole list is read back
    // from disk once the tool has run and shown as a single todo.
    if provider == "claude"
        && string(raw, "tool_name")
            .is_some_and(|name| crate::integrations::is_claude_task_tool(&name))
    {
        if hook_name != "PostToolUse" {
            return None;
        }
        return Some(SessionActivity {
            id: format!("{provider}:{session_id}:todo:tasks"),
            kind: "tool".into(),
            title: "TodoWrite".into(),
            detail: Some(crate::integrations::claude_task_list_detail(session_id)?),
            status: "completed".into(),
            created_at: now_millis(),
            files: Vec::new(),
            attachments: Vec::new(),
            append_detail: false,
        });
    }

    let tool_call = raw.get("toolCall");
    let tool_name = string(raw, "tool_name")
        .or_else(|| string(raw, "tool"))
        .or_else(|| tool_call.and_then(|call| string(call, "name")))
        .unwrap_or_else(|| "Ferramenta".into());
    let input = raw
        .get("tool_input")
        .or_else(|| raw.get("details"))
        .or_else(|| tool_call.and_then(|call| call.get("args")));
    let resource = input
        .and_then(resource_from_input)
        .unwrap_or_else(|| tool_name.clone());
    let lower_tool = tool_name.to_lowercase();
    let is_todo_tool = lower_tool.contains("todo");
    let lower_resource = resource.to_lowercase();
    let is_command = lower_tool.contains("bash")
        || lower_tool.contains("shell")
        || lower_tool.contains("command");
    let kind = if is_todo_tool {
        "tool"
    } else if is_command && is_test_command(&lower_resource) {
        "test"
    } else if is_command {
        "command"
    } else if ["write", "edit", "patch", "file"]
        .iter()
        .any(|needle| lower_tool.contains(needle))
    {
        "file"
    } else {
        "tool"
    };
    let antigravity_tool_failed = provider == "antigravity"
        && hook_name == "PostToolUse"
        && string(raw, "error").is_some_and(|error| !error.trim().is_empty());
    let status = if antigravity_tool_failed {
        "failed"
    } else {
        match hook_name {
            "PreToolUse" | "BeforeTool" => "running",
            "PostToolUseFailure" => "failed",
            _ => "completed",
        }
    };
    let result = raw
        .get("tool_response")
        .or_else(|| raw.get("tool_result"))
        .or_else(|| raw.get("toolResponse"))
        .or_else(|| raw.get("result"))
        .and_then(|value| serde_json::to_string_pretty(value).ok());
    let input_detail = input.and_then(|value| serde_json::to_string_pretty(value).ok());
    // Claude's edit tools report a structured patch or the old and new strings, never a diff:
    // build one so the review center can show the changed lines.
    let file_diff = (kind == "file")
        .then(|| {
            file_change_diff(
                input,
                raw.get("tool_response")
                    .or_else(|| raw.get("tool_result"))
                    .or_else(|| raw.get("toolResponse")),
            )
        })
        .flatten();
    let detail_limit = if file_diff.is_some() {
        48 * 1024
    } else {
        16 * 1024
    };
    let detail = if is_todo_tool {
        input_detail
    } else if file_diff.is_some() {
        file_diff
    } else {
        result.or(input_detail)
    };
    let tool_id = string(raw, "tool_use_id")
        .or_else(|| string(raw, "tool_call_id"))
        .or_else(|| {
            raw.get("stepIdx")
                .and_then(Value::as_i64)
                .map(|id| id.to_string())
        })
        .unwrap_or_else(|| now_millis().to_string());
    let files = if kind == "file" {
        input
            .and_then(resource_from_input)
            .into_iter()
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    Some(SessionActivity {
        id: format!("{provider}:{session_id}:tool:{tool_id}"),
        kind: kind.into(),
        title: truncate(if is_todo_tool { &tool_name } else { &resource }, 240),
        detail: detail.map(|detail| truncate(&detail, detail_limit)),
        status: status.into(),
        created_at: now_millis(),
        files,
        attachments: Vec::new(),
        append_detail: false,
    })
}

/// A unified diff for a file edit made with an edit tool, when its payload allows one.
fn file_change_diff(input: Option<&Value>, response: Option<&Value>) -> Option<String> {
    let path = response
        .and_then(|value| string(value, "filePath"))
        .or_else(|| input.and_then(|value| string(value, "file_path")))?;
    let header = format!("diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n");

    let patch = response
        .and_then(|value| value.get("structuredPatch"))
        .and_then(Value::as_array)
        .filter(|hunks| !hunks.is_empty());
    if let Some(hunks) = patch {
        let mut diff = header;
        for hunk in hunks {
            let number = |key: &str| hunk.get(key).and_then(Value::as_u64).unwrap_or(0);
            diff.push_str(&format!(
                "@@ -{},{} +{},{} @@\n",
                number("oldStart"),
                number("oldLines"),
                number("newStart"),
                number("newLines")
            ));
            for line in hunk
                .get("lines")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                diff.push_str(line);
                diff.push('\n');
            }
        }
        return Some(diff);
    }

    let input = input?;
    let edits: Vec<(String, String)> = match input.get("edits").and_then(Value::as_array) {
        Some(list) => list
            .iter()
            .filter_map(|edit| Some((string(edit, "old_string")?, string(edit, "new_string")?)))
            .collect(),
        None => string(input, "old_string")
            .zip(string(input, "new_string"))
            .into_iter()
            .collect(),
    };
    if !edits.is_empty() {
        let original = response
            .and_then(|value| string(value, "originalFile"))
            .or_else(|| {
                std::fs::metadata(&path)
                    .ok()
                    .filter(|meta| meta.len() < 2 * 1024 * 1024)
                    .and_then(|_| std::fs::read_to_string(&path).ok())
            });
        let mut diff = header;
        for (old, new) in edits {
            let start = original
                .as_deref()
                .and_then(|content| {
                    content
                        .find(&old)
                        .map(|at| content[..at].matches('\n').count() + 1)
                })
                .unwrap_or(1);
            let old_lines: Vec<&str> = old.lines().collect();
            let new_lines: Vec<&str> = new.lines().collect();
            diff.push_str(&format!(
                "@@ -{start},{} +{start},{} @@\n",
                old_lines.len(),
                new_lines.len()
            ));
            for line in old_lines {
                diff.push_str(&format!("-{line}\n"));
            }
            for line in new_lines {
                diff.push_str(&format!("+{line}\n"));
            }
        }
        return Some(diff);
    }

    // A new file: Write reports its whole content, and nothing existed before.
    let content = string(input, "content")?;
    let created = match response.and_then(|value| string(value, "type")) {
        Some(kind) => kind == "create",
        None => !std::path::Path::new(&path).exists(),
    };
    if !created {
        return None;
    }
    let lines: Vec<&str> = content.lines().collect();
    let mut diff = format!(
        "diff --git a/{path} b/{path}\nnew file mode 100644\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{} @@\n",
        lines.len()
    );
    for line in lines {
        diff.push_str(&format!("+{line}\n"));
    }
    Some(diff)
}

fn is_test_command(command: &str) -> bool {
    [
        "npm test",
        "pnpm test",
        "yarn test",
        "cargo test",
        "dotnet test",
        "go test",
        "flutter test",
        "pytest",
        "vitest",
        "jest",
        "mvn test",
        "gradle test",
        "gradlew test",
    ]
    .iter()
    .any(|pattern| command.contains(pattern))
}

/// The profile of a Claude conversation Lume opens with the given access mode;
/// later hooks of each prompt keep it current.
pub(crate) fn claude_launch_profile(mode: Option<&AccessMode>) -> PermissionProfile {
    let mode = match mode {
        Some(AccessMode::Plan | AccessMode::ReadOnly) => Some("plan"),
        Some(AccessMode::WorkspaceWrite) => Some("acceptEdits"),
        Some(AccessMode::FullAccess) => Some("bypassPermissions"),
        Some(AccessMode::Custom) | None => None,
    };
    permission_profile("claude", mode, &Value::Null, true)
}

fn permission_profile(
    provider: &str,
    mode: Option<&str>,
    raw: &Value,
    direct_response: bool,
) -> PermissionProfile {
    let (access_mode, label, policy) = match mode.unwrap_or("default") {
        "bypassPermissions" | "dontAsk" | "danger-full-access" => (
            AccessMode::FullAccess,
            "Acesso amplo",
            "A sessão normalmente não solicita confirmação",
        ),
        "plan" => (
            AccessMode::Plan,
            "Modo de planejamento",
            "Alterações não são permitidas",
        ),
        "acceptEdits" | "workspace-write" => (
            AccessMode::WorkspaceWrite,
            "Edições permitidas",
            "Outras ações ainda podem pedir confirmação",
        ),
        "read-only" => (
            AccessMode::ReadOnly,
            "Somente leitura",
            "Alterações exigem permissão",
        ),
        _ => (AccessMode::Custom, "Permissões da sessão", ""),
    };

    let mut available_actions = if direct_response {
        vec![PermissionAction::AllowOnce, PermissionAction::Deny]
    } else {
        vec![PermissionAction::OpenSource]
    };
    if provider == "claude"
        && raw
            .get("permission_suggestions")
            .and_then(Value::as_array)
            .is_some_and(|suggestions| !suggestions.is_empty())
    {
        available_actions.insert(1, PermissionAction::AllowSession);
    }

    PermissionProfile {
        mode: access_mode,
        label: label.into(),
        approval_policy: policy.into(),
        approvals_reviewer: string(raw, "approvals_reviewer")
            .or_else(|| string(raw, "approvalsReviewer")),
        can_respond_from_lume: direct_response,
        available_actions,
    }
}

fn is_claude_question(raw: &Value) -> bool {
    string(raw, "tool_name").as_deref() == Some("AskUserQuestion")
}

fn notification_reports_failure(raw: &Value) -> bool {
    ["message", "title"]
        .into_iter()
        .filter_map(|key| string(raw, key))
        .any(|value| {
            let value = value.to_lowercase();
            ["failed", "failure", "error", "falhou", "erro"]
                .iter()
                .any(|needle| value.contains(needle))
        })
}

fn antigravity_stop_event(raw: &Value) -> HookEventKind {
    let reason = string(raw, "terminationReason").unwrap_or_default();
    let fully_idle = raw.get("fullyIdle").and_then(Value::as_bool);
    let has_error = string(raw, "error").is_some_and(|error| !error.trim().is_empty());
    if has_error || matches!(reason.as_str(), "error" | "max_steps_exceeded") {
        HookEventKind::Failed
    } else {
        match (fully_idle, reason.as_str()) {
            (Some(false), _) => HookEventKind::Running,
            (Some(true), "model_stop") => HookEventKind::Completed,
            // An incomplete or future Stop payload must not falsely finish a
            // task. Activity preserves the last known session state.
            _ => HookEventKind::Activity,
        }
    }
}

fn claude_question_request(raw: &Value, session_id: &str) -> Option<PendingQuestion> {
    let tool_input = raw.get("tool_input")?;
    let raw_questions = tool_input.get("questions")?.as_array()?;
    let questions = raw_questions
        .iter()
        .enumerate()
        .filter_map(|(index, question)| {
            let prompt = question.get("question")?.as_str()?.to_string();
            Some(InteractiveQuestion {
                id: claude_question_item_id(session_id, index),
                header: question
                    .get("header")
                    .and_then(Value::as_str)
                    .unwrap_or("Question")
                    .to_string(),
                question: prompt,
                is_other: true,
                is_secret: false,
                options: question
                    .get("options")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|option| {
                        Some(QuestionOption {
                            label: option.get("label")?.as_str()?.to_string(),
                            description: option
                                .get("description")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                        })
                    })
                    .collect(),
            })
        })
        .collect::<Vec<_>>();
    if questions.is_empty() {
        return None;
    }
    let tool_id = string(raw, "tool_use_id")
        .or_else(|| string(raw, "toolUseId"))
        .unwrap_or_else(|| {
            format!(
                "{:x}",
                Sha256::digest(format!("{session_id}\n{}", raw_questions.len()).as_bytes())
            )
        });
    Some(PendingQuestion {
        id: format!("claude-question:{session_id}:{tool_id}"),
        questions,
        requested_at: string(raw, "timestamp").unwrap_or_else(|| now_millis().to_string()),
    })
}

fn claude_question_item_id(session_id: &str, index: usize) -> String {
    format!("claude:{session_id}:question:{index}")
}

fn permission_request(provider: &str, raw: &Value, session_id: &str) -> PermissionRequest {
    let tool_name = string(raw, "tool_name").unwrap_or_else(|| "Ferramenta".into());
    let tool_input = raw.get("tool_input").or_else(|| raw.get("details"));
    let resource = tool_input
        .and_then(resource_from_input)
        .or_else(|| string(raw, "message"))
        .unwrap_or_else(|| tool_name.clone());
    let description = tool_input
        .and_then(|input| input.get("description"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("{tool_name} quer executar uma ação"));
    let timestamp = string(raw, "timestamp").unwrap_or_else(|| now_millis().to_string());
    let request_key = [
        "permission_request_id",
        "permissionRequestId",
        "tool_use_id",
        "toolUseId",
        "tool_call_id",
        "toolCallId",
        "request_id",
        "requestId",
        "id",
    ]
    .into_iter()
    .find_map(|key| string(raw, key))
    .unwrap_or_else(|| {
        let bucket = timestamp
            .parse::<i64>()
            .unwrap_or_else(|_| now_millis())
            .div_euclid(30_000);
        let fingerprint = format!("{provider}\n{session_id}\n{tool_name}\n{resource}\n{bucket}");
        format!("{:x}", Sha256::digest(fingerprint.as_bytes()))
    });
    let kind = if tool_name.to_lowercase().contains("bash")
        || tool_name.to_lowercase().contains("shell")
    {
        "command"
    } else if resource.contains("http://") || resource.contains("https://") {
        "network"
    } else if resource.contains('/') || resource.contains('\\') {
        "file"
    } else {
        "tool"
    };

    PermissionRequest {
        id: format!("{provider}:{session_id}:{request_key}"),
        kind: kind.into(),
        summary: truncate(&description, 180),
        resource: truncate(&resource, 320),
        risk: risk_for(&tool_name, &resource).into(),
        requested_at: timestamp,
    }
}

fn resource_from_input(input: &Value) -> Option<String> {
    for key in ["command", "file_path", "path", "url", "query"] {
        if let Some(value) = input.get(key).and_then(Value::as_str) {
            return Some(value.to_string());
        }
    }
    serde_json::to_string(input).ok()
}

fn risk_for(tool: &str, resource: &str) -> &'static str {
    let content = format!("{tool} {resource}").to_lowercase();
    if [
        "rm -rf",
        "format ",
        "del /",
        "sudo ",
        "reg delete",
        "drop table",
    ]
    .iter()
    .any(|pattern| content.contains(pattern))
    {
        "high"
    } else if ["write", "edit", "bash", "shell", "http", "mcp"]
        .iter()
        .any(|pattern| content.contains(pattern))
    {
        "medium"
    } else {
        "low"
    }
}

fn claude_permission_output(action: Option<PermissionAction>, raw: &Value) -> Option<Value> {
    let decision = match action? {
        PermissionAction::AllowOnce => json!({ "behavior": "allow" }),
        PermissionAction::AllowSession => {
            let suggestions = raw
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
            json!({ "behavior": "allow", "updatedPermissions": suggestions })
        }
        PermissionAction::Deny => json!({
            "behavior": "deny",
            "message": "Permissão recusada no Lume",
            "interrupt": false
        }),
        PermissionAction::OpenSource => return None,
    };
    Some(json!({
        "hookSpecificOutput": {
            "hookEventName": "PermissionRequest",
            "decision": decision
        }
    }))
}

fn claude_question_output(answers: Option<Vec<QuestionAnswer>>, raw: &Value) -> Option<Value> {
    let answers = answers?;
    let session_id = string(raw, "session_id")?;
    let mut updated_input = raw.get("tool_input")?.clone();
    let questions = updated_input.get("questions")?.as_array()?.clone();
    let mapped = questions
        .iter()
        .enumerate()
        .filter_map(|(index, question)| {
            let prompt = question.get("question")?.as_str()?;
            let answer = answers
                .iter()
                .find(|answer| answer.question_id == claude_question_item_id(&session_id, index))?
                .answers
                .join(", ");
            Some((prompt.to_string(), Value::String(answer)))
        })
        .collect::<serde_json::Map<_, _>>();
    if mapped.len() != questions.len() {
        return None;
    }
    updated_input
        .as_object_mut()?
        .insert("answers".into(), Value::Object(mapped));
    Some(json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "allow",
            "updatedInput": updated_input
        }
    }))
}

fn status_label(hook: &str, event: &HookEventKind) -> Option<&'static str> {
    match hook {
        "Stop" if matches!(event, HookEventKind::Failed) => Some("Encerrado com erro"),
        "Stop" if matches!(event, HookEventKind::Running) => Some("Executando"),
        "Stop" if matches!(event, HookEventKind::Completed) => Some("Finalizado"),
        "SessionStart" => Some("Sessão detectada"),
        "UserPromptSubmit" | "BeforeAgent" | "PreInvocation" | "PreToolUse" | "PostToolUse"
        | "PostToolUseFailure" | "PostToolBatch" | "PostInvocation" | "AfterTool" => {
            Some("Executando")
        }
        "PermissionRequest" => Some("Aguardando permissão"),
        "PermissionDenied" => Some("Permissão negada"),
        "Notification" if matches!(event, HookEventKind::PermissionRequest) => {
            Some("Aguardando permissão")
        }
        "Notification" if matches!(event, HookEventKind::Completed) => Some("Finalizado"),
        "Notification" if matches!(event, HookEventKind::Failed) => Some("Encerrado com erro"),
        "Notification" => Some("Aguardando sua resposta"),
        "AfterAgent" | "SessionEnd" => Some("Finalizado"),
        "StopFailure" => Some("Encerrado com erro"),
        _ => None,
    }
}

fn agent_process_context(
    provider: &str,
    session_id: Option<&str>,
) -> (Option<u32>, SessionSource, bool) {
    if provider == "codex" {
        let (pid, source) = crate::codex_identity_probe::hook_process_context();
        return (pid, source, false);
    }
    let Some(current_pid) = get_current_pid().ok() else {
        return (None, SessionSource::Cli, false);
    };
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[current_pid]),
        true,
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::Always)
            .without_tasks(),
    );
    // O processo atual é `lume hook <provider>` e contém o nome do agente nos
    // próprios argumentos. A busca precisa começar no processo pai para não
    // associar o chat ao PID efêmero do hook.
    let Some(mut pid) = system
        .process(current_pid)
        .and_then(|process| process.parent())
    else {
        return (None, SessionSource::Cli, false);
    };
    let mut chain: Vec<(u32, String, Vec<String>)> = Vec::new();
    let mut source = SessionSource::Cli;
    let mut headless_resume = false;
    for _ in 0..10 {
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing()
                .with_cmd(UpdateKind::Always)
                .without_tasks(),
        );
        let Some(process) = system.process(pid) else {
            break;
        };
        let name = process.name().to_string_lossy().to_lowercase();
        let arguments = process
            .cmd()
            .iter()
            .map(|part| part.to_string_lossy().to_lowercase())
            .collect::<Vec<_>>();
        let command = arguments.join(" ");
        chain.push((pid.as_u32(), name.clone(), arguments.clone()));
        if provider == "claude"
            && command.split_whitespace().any(|part| part == "--print")
            && command
                .split_whitespace()
                .any(|part| matches!(part, "--resume" | "--session-id"))
        {
            headless_resume = true;
        }
        if name == "code"
            || name == "code.exe"
            || command.contains("visual studio code")
            || command.contains(".vscode/extensions")
        {
            source = SessionSource::Vscode;
        }
        let Some(parent) = process.parent() else {
            break;
        };
        pid = parent;
    }
    // Claude's own registry says which process holds this conversation.
    let registered_pids = session_id
        .filter(|_| provider == "claude")
        .map(crate::integrations::claude_registered_pids)
        .unwrap_or_default();
    (
        pick_agent_pid(provider, &chain, &registered_pids),
        source,
        headless_resume,
    )
}

/// The process a hook reports as its conversation, from the hook's ancestors
/// (nearest first).
///
/// Hooks of Codex/Gemini can pass through an ephemeral shell whose command also
/// names the provider, so the outermost matching process wins: the stable one,
/// not the wrapper that exits right after sending the event.
///
/// Claude's background sessions break that rule. The daemon's processes sit
/// between the conversation and the CLI that launched the daemon, and that CLI
/// is "the outermost claude" for every conversation. So Claude's registry wins
/// when it names an ancestor, and the search never climbs past the daemon.
fn pick_agent_pid(
    provider: &str,
    chain: &[(u32, String, Vec<String>)],
    registered: &[u32],
) -> Option<u32> {
    let chain = if provider == "claude" {
        let end = chain
            .iter()
            .position(|(_, _, arguments)| crate::discovery::is_claude_daemon_arguments(arguments))
            .unwrap_or(chain.len());
        &chain[..end]
    } else {
        chain
    };
    chain
        .iter()
        .find(|(pid, _, _)| registered.contains(pid))
        .map(|(pid, _, _)| *pid)
        .or_else(|| {
            chain
                .iter()
                .filter(|(_, name, arguments)| hook_parent_is_user_agent(provider, name, arguments))
                .map(|(pid, _, _)| *pid)
                .next_back()
        })
}

fn hook_parent_is_user_agent(provider: &str, name: &str, arguments: &[String]) -> bool {
    if provider == "codex" {
        // A local --no-daemon server has a TUI ancestor; a shared daemon does
        // not. Keep walking past either server but never store its PID on a
        // conversation or offer to terminate it as an external CLI.
        return crate::discovery::detect_agent_arguments(name, arguments) == Some(AgentKind::Codex)
            && !crate::discovery::is_codex_infrastructure_arguments(name, arguments);
    }
    let process_marker = if provider == "antigravity" {
        "agy"
    } else {
        provider
    };
    arguments.iter().any(|part| {
        let executable = part
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(part)
            .trim_matches(['\"', '\'']);
        executable == process_marker || executable.strip_suffix(".exe") == Some(process_marker)
    })
}

#[cfg(test)]
mod process_context_tests {
    use super::{hook_parent_is_user_agent, pick_agent_pid};

    // `argv` as the OS gives it. Claude retitles its daemon processes, so their
    // `argv[0]` is one string such as "claude bg-pty-host".
    fn ancestor(pid: u32, name: &str, argv: &[&str]) -> (u32, String, Vec<String>) {
        (
            pid,
            name.to_string(),
            argv.iter().map(|element| element.to_lowercase()).collect(),
        )
    }

    // The tree `claude` builds for its background sessions: each conversation
    // runs under the daemon's pty host, and the daemon under the CLI panel.
    fn daemon_tree(conversation: (u32, &str, &[&str])) -> Vec<(u32, String, Vec<String>)> {
        vec![
            ancestor(conversation.0, conversation.1, conversation.2),
            ancestor(
                3_329_806,
                "2.1.292",
                &[
                    "claude bg-pty-host",
                    "--bg-pty-host",
                    "/tmp/p.sock",
                    "120",
                    "30",
                    "--",
                    "/v/2.1.292",
                    "--agent",
                    "claude",
                ],
            ),
            ancestor(
                3_329_776,
                "claude",
                &[
                    "/home/user/.local/bin/claude",
                    "daemon",
                    "run",
                    "--origin",
                    "transient",
                ],
            ),
            ancestor(3_329_380, "claude", &["claude"]),
            ancestor(3_329_132, "bash", &["bash"]),
        ]
    }

    #[test]
    fn background_sessions_do_not_borrow_the_pid_of_the_cli_that_started_the_daemon() {
        // With Claude's registry: every conversation reports its own process.
        let claimed_spare: &[&str] = &[
            "claude bg-spare",
            "--bg-spare",
            "/tmp/spare/527fcebf.claim.sock",
        ];
        let background: &[&str] = &[
            "/v/2.1.292",
            "--session-id",
            "929e9dc1",
            "--agent",
            "claude",
            "--inherit-permission-mode",
            "auto",
        ];
        let fork: &[&str] = &[
            "/v/2.1.292",
            "--session-id",
            "b2750109",
            "--fork-session",
            "--resume",
            "x.jsonl",
        ];
        for (pid, name, argv, session) in [
            (3_329_829, "2.1.292", claimed_spare, "analysis-fork"),
            (3_329_828, "2.1.292", background, "bg-user"),
            (3_329_834, "2.1.292", fork, "bg-fork"),
        ] {
            let chain = daemon_tree((pid, name, argv));
            assert_eq!(
                pick_agent_pid("claude", &chain, &[pid]),
                Some(pid),
                "{session}"
            );
        }
        // Without it (the registry file is written a moment after SessionStart)
        // neither the pty host nor the CLI panel is taken for the conversation.
        assert_eq!(
            pick_agent_pid("claude", &daemon_tree((3_329_834, "2.1.292", fork)), &[]),
            None
        );
        assert_eq!(
            pick_agent_pid(
                "claude",
                &daemon_tree((3_329_828, "2.1.292", background)),
                &[]
            ),
            Some(3_329_828)
        );
    }

    // Runs the real choice over this machine's live Claude processes:
    // cargo test live_claude_sessions -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_claude_sessions_report_their_own_process() {
        use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always),
        );
        let home = std::env::var("HOME").expect("HOME");
        let mut checked = 0;
        for entry in std::fs::read_dir(format!("{home}/.claude/sessions"))
            .expect("registro")
            .flatten()
        {
            let Ok(record) = serde_json::from_str::<serde_json::Value>(
                &std::fs::read_to_string(entry.path()).unwrap_or_default(),
            ) else {
                continue;
            };
            let (Some(pid), Some(session)) = (
                record["pid"].as_u64().map(|pid| pid as u32),
                record["sessionId"].as_str(),
            ) else {
                continue;
            };
            if system.process(Pid::from_u32(pid)).is_none()
                || record["cwd"]
                    .as_str()
                    .is_some_and(|cwd| cwd.starts_with("/tmp"))
            {
                continue;
            }
            let mut chain = Vec::new();
            let mut next = Some(Pid::from_u32(pid));
            while let (Some(current), true) = (next, chain.len() < 10) {
                let Some(process) = system.process(current) else {
                    break;
                };
                chain.push((
                    current.as_u32(),
                    process.name().to_string_lossy().to_lowercase(),
                    process
                        .cmd()
                        .iter()
                        .map(|part| part.to_string_lossy().to_lowercase())
                        .collect::<Vec<_>>(),
                ));
                next = process.parent();
            }
            let registered = crate::integrations::claude_registered_pids(session);
            let picked = pick_agent_pid("claude", &chain, &registered);
            let without_registry = pick_agent_pid("claude", &chain, &[]);
            println!(
                "{} {:<11} real={pid:<8} com registro={picked:?} sem registro={without_registry:?}",
                &session[..8],
                record["kind"].as_str().unwrap_or("?"),
            );
            assert_eq!(picked, Some(pid), "{session}");
            assert_ne!(
                without_registry,
                Some(3_329_806),
                "o pty host não é a conversa"
            );
            checked += 1;
        }
        assert!(checked > 0, "nenhuma sessão Claude viva para conferir");
    }

    #[test]
    fn a_plain_claude_cli_keeps_its_outermost_process() {
        // Not under a daemon: the stable CLI beats an ephemeral wrapper.
        let chain = vec![
            ancestor(30, "sh", &["sh", "-c", "claude-hook-wrapper", "claude"]),
            ancestor(20, "claude", &["claude"]),
            ancestor(10, "bash", &["bash"]),
        ];
        assert_eq!(pick_agent_pid("claude", &chain, &[]), Some(20));
        assert_eq!(pick_agent_pid("claude", &chain, &[20]), Some(20));
    }

    #[test]
    fn other_providers_keep_choosing_the_outermost_match() {
        let chain = vec![
            ancestor(30, "sh", &["sh", "-c", "codex", "hook"]),
            ancestor(20, "codex", &["codex"]),
        ];
        assert_eq!(pick_agent_pid("claude", &chain, &[]), None);
        assert_eq!(
            pick_agent_pid("gemini", &[ancestor(5, "gemini", &["gemini"])], &[]),
            Some(5)
        );
    }

    #[test]
    fn codex_hooks_skip_servers_but_recognize_the_standalone_tui() {
        for command in [
            "codex app-server --listen unix:// --managed-daemon",
            "codex app-server daemon pid-update-loop",
            "codex app-server --listen ws://127.0.0.1:50001",
            "codex -c features.code_mode_host=true app-server",
            "bash -lc codex app-server",
        ] {
            let arguments = command
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>();
            assert!(
                !hook_parent_is_user_agent("codex", arguments[0].as_str(), &arguments),
                "{command}"
            );
        }
        let arguments = ["codex", "--no-daemon", "resume", "--all"].map(str::to_string);
        assert!(hook_parent_is_user_agent("codex", "codex", &arguments));
    }

    #[test]
    fn codex_hook_shell_text_is_not_a_cli_identity() {
        let arguments = ["bash", "-lc", "lume hook codex"].map(str::to_string);
        assert!(!hook_parent_is_user_agent("codex", "bash", &arguments));
        let arguments = [
            "node",
            "/opt/node_modules/@openai/codex/bin/codex.js",
            "resume",
            "--all",
        ]
        .map(str::to_string);
        assert!(hook_parent_is_user_agent("codex", "node", &arguments));
    }
}

fn string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

fn antigravity_working_directory(raw: &Value) -> Option<String> {
    let workspaces = raw
        .get("workspacePaths")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    let args = raw.get("toolCall").and_then(|call| call.get("args"));
    let target_paths = [
        "Cwd",
        "cwd",
        "DirectoryPath",
        "AbsolutePath",
        "TargetFile",
        "path",
    ]
    .into_iter()
    .filter_map(|key| args.and_then(|args| string(args, key)))
    .collect::<Vec<_>>();

    let matching_tool_workspace = workspaces
        .iter()
        .filter(|workspace| {
            target_paths
                .iter()
                .any(|target| path_is_within(target, workspace))
        })
        .max_by_key(|workspace| workspace.len())
        .copied();
    let matching_cwd_workspace = string(raw, "cwd").and_then(|cwd| {
        workspaces
            .iter()
            .filter(|workspace| path_is_within(&cwd, workspace))
            .max_by_key(|workspace| workspace.len())
            .copied()
    });

    matching_tool_workspace
        .or(matching_cwd_workspace)
        .or_else(|| workspaces.first().copied())
        .map(str::to_string)
}

fn path_is_within(path: &str, root: &str) -> bool {
    let normalize = |value: &str| {
        let value = value.replace('\\', "/");
        let value = value.trim_end_matches('/');
        let bytes = value.as_bytes();
        let looks_windows = bytes.len() > 2
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && bytes[2] == b'/';
        if cfg!(windows) || looks_windows {
            value.to_ascii_lowercase()
        } else {
            value.to_string()
        }
    };
    let path = normalize(path);
    let root = normalize(root);
    path == root
        || path
            .strip_prefix(&root)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn hook_response(value: &Value) -> Option<String> {
    ["last_assistant_message", "prompt_response", "response"]
        .into_iter()
        .find_map(|key| string(value, key))
        .map(|response| truncate(response.trim(), 32 * 1024))
        .filter(|response| !response.is_empty())
}

const CLAUDE_TRANSCRIPT_TAIL_BYTES: u64 = 4 * 1024 * 1024;
const CLAUDE_TRANSCRIPT_TRANSIENT_LIMIT: usize = 160;
const CLAUDE_TRANSCRIPT_LINE_LIMIT: usize = 512 * 1024;

fn claude_transcript_activities(raw: &Value, session_id: &str) -> Vec<SessionActivity> {
    let mut activities = Vec::new();
    for path in ["transcript_path", "agent_transcript_path"]
        .into_iter()
        .filter_map(|key| string(raw, key))
    {
        read_claude_transcript(&path, session_id, &mut activities);
    }
    activities.sort_by_key(|activity| activity.created_at);
    let mut seen = HashSet::new();
    activities.retain(|activity| seen.insert(activity.id.clone()));
    prune_claude_transcript_activities(&mut activities);
    activities
}

fn prune_claude_transcript_activities(activities: &mut Vec<SessionActivity>) {
    let transient_count = activities
        .iter()
        .filter(|activity| !matches!(activity.kind.as_str(), "prompt" | "message"))
        .count();
    let mut remove = transient_count.saturating_sub(CLAUDE_TRANSCRIPT_TRANSIENT_LIMIT);
    activities.retain(|activity| {
        let transient = !matches!(activity.kind.as_str(), "prompt" | "message");
        if transient && remove > 0 {
            remove -= 1;
            false
        } else {
            true
        }
    });
}

fn read_claude_transcript(path: &str, session_id: &str, activities: &mut Vec<SessionActivity>) {
    let Ok(mut file) = File::open(path) else {
        return;
    };
    let Ok(metadata) = file.metadata() else {
        return;
    };
    if !metadata.is_file() {
        return;
    }

    let start = metadata.len().saturating_sub(CLAUDE_TRANSCRIPT_TAIL_BYTES);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return;
    }
    let mut reader = BufReader::new(file);
    if start > 0 {
        let mut partial = Vec::new();
        if reader.read_until(b'\n', &mut partial).is_err() {
            return;
        }
    }

    let mut turn: Option<ClaudeTurnTokens> = None;
    let mut fork_origin: Option<String> = None;
    for line in reader.lines().map_while(Result::ok) {
        if line.len() > CLAUDE_TRANSCRIPT_LINE_LIMIT {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let role = entry
            .get("message")
            .and_then(|message| message.get("role"))
            .and_then(Value::as_str);
        if !matches!(role, Some("assistant" | "user")) {
            continue;
        }
        track_claude_turn_tokens(&entry, session_id, &mut turn, activities);
        if fork_origin.is_none() && entry.get("isSidechain").and_then(Value::as_bool) != Some(true)
        {
            fork_origin = entry
                .pointer("/forkedFrom/sessionId")
                .and_then(Value::as_str)
                .filter(|origin| !origin.is_empty())
                .map(str::to_string);
        }
        if role == Some("user")
            && (entry.get("isMeta").and_then(Value::as_bool) == Some(true)
                || entry.get("isSidechain").and_then(Value::as_bool) == Some(true))
        {
            continue;
        }
        let entry_id = string(&entry, "uuid")
            .unwrap_or_else(|| format!("{:x}", Sha256::digest(line.as_bytes())));
        let created_at = string(&entry, "timestamp")
            .and_then(|timestamp| DateTime::parse_from_rfc3339(&timestamp).ok())
            .map(|timestamp| timestamp.timestamp_millis())
            .unwrap_or_else(now_millis);
        let Some(content) = entry
            .get("message")
            .and_then(|message| message.get("content"))
        else {
            continue;
        };
        match content {
            Value::String(text) => {
                if role == Some("user") {
                    push_claude_user_text(activities, session_id, &entry_id, 0, text, created_at);
                } else {
                    push_claude_transcript_activity(
                        activities, session_id, &entry_id, 0, "message", "Claude", text, created_at,
                    );
                }
            }
            Value::Array(blocks) => {
                for (index, block) in blocks.iter().enumerate() {
                    match string(block, "type").as_deref() {
                        Some("text") => {
                            if let Some(text) = string(block, "text") {
                                if role == Some("user") {
                                    push_claude_user_text(
                                        activities, session_id, &entry_id, index, &text, created_at,
                                    );
                                } else {
                                    push_claude_transcript_activity(
                                        activities, session_id, &entry_id, index, "message",
                                        "Claude", &text, created_at,
                                    );
                                }
                            }
                        }
                        // The CLI shows this text between tool calls as a message, not as a tool event.
                        Some("thinking") if role == Some("assistant") => {
                            if let Some(thinking) = string(block, "thinking") {
                                push_claude_transcript_activity(
                                    activities, session_id, &entry_id, index, "message",
                                    "Thinking", &thinking, created_at,
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(turn) = turn {
        activities.extend(turn.finish(session_id));
    }
    activities.extend(fork_origin.map(|origin| fork_origin_activity(session_id, origin)));
}

/// Carries a conversation's fork origin to the session (see `remember_activity`).
pub(crate) fn fork_origin_activity(session_id: &str, origin: String) -> SessionActivity {
    SessionActivity {
        id: format!("{session_id}:fork-origin"),
        kind: "fork_origin".into(),
        title: "Fork".into(),
        detail: Some(origin),
        status: "completed".into(),
        created_at: now_millis(),
        files: Vec::new(),
        attachments: Vec::new(),
        append_detail: false,
    }
}

/// Token usage of one prompt: every API response between a user prompt and the next.
struct ClaudeTurnTokens {
    turn_id: String,
    // A response is split into one entry per content block, all repeating the
    // same usage, so each message id is counted once.
    responses: HashMap<String, (u64, u64)>,
    last_at: i64,
}

impl ClaudeTurnTokens {
    fn finish(self, session_id: &str) -> Option<SessionActivity> {
        let input = self.responses.values().map(|(input, _)| input).sum::<u64>();
        let output = self
            .responses
            .values()
            .map(|(_, output)| output)
            .sum::<u64>();
        if input + output == 0 {
            return None;
        }
        let detail = json!({
            "turnId": self.turn_id,
            "totalTokens": input + output,
            "inputTokens": input,
            "outputTokens": output,
            "createdAt": self.last_at,
        });
        Some(SessionActivity {
            id: format!("claude:{session_id}:token-usage:{}", self.turn_id),
            kind: "token_usage".into(),
            title: "Uso de tokens".into(),
            detail: Some(detail.to_string()),
            status: "completed".into(),
            created_at: self.last_at,
            files: Vec::new(),
            attachments: Vec::new(),
            append_detail: false,
        })
    }
}

fn track_claude_turn_tokens(
    entry: &Value,
    session_id: &str,
    turn: &mut Option<ClaudeTurnTokens>,
    activities: &mut Vec<SessionActivity>,
) {
    if entry.get("isSidechain").and_then(Value::as_bool) == Some(true) {
        return;
    }
    let created_at = string(entry, "timestamp")
        .and_then(|timestamp| DateTime::parse_from_rfc3339(&timestamp).ok())
        .map(|timestamp| timestamp.timestamp_millis());
    let message = &entry["message"];
    if message["role"] == "user" {
        if !is_claude_user_prompt(entry) {
            return;
        }
        if let Some(previous) = turn.take() {
            activities.extend(previous.finish(session_id));
        }
        *turn = string(entry, "uuid").map(|turn_id| ClaudeTurnTokens {
            turn_id,
            responses: HashMap::new(),
            last_at: created_at.unwrap_or_else(now_millis),
        });
        return;
    }
    // Responses before the first prompt in the tail belong to a turn that
    // started outside it; counting them would understate that turn.
    let (Some(turn), Some(usage), Some(id)) = (
        turn.as_mut(),
        message.get("usage"),
        message.get("id").and_then(Value::as_str),
    ) else {
        return;
    };
    let tokens = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
    let input = tokens("input_tokens")
        + tokens("cache_creation_input_tokens")
        + tokens("cache_read_input_tokens");
    turn.responses
        .insert(id.to_string(), (input, tokens("output_tokens")));
    if let Some(created_at) = created_at {
        turn.last_at = turn.last_at.max(created_at);
    }
}

fn is_claude_user_prompt(entry: &Value) -> bool {
    if entry.get("isMeta").and_then(Value::as_bool) == Some(true) {
        return false;
    }
    match entry.pointer("/message/content") {
        Some(Value::String(text)) => visible_claude_user_text(text),
        Some(Value::Array(blocks)) => blocks.iter().any(|block| {
            string(block, "type").as_deref() == Some("text")
                && string(block, "text").is_some_and(|text| visible_claude_user_text(&text))
        }),
        _ => false,
    }
}

/// The CLI records a cancelled request as a user message that starts with this marker,
/// followed by whatever was typed next. It is a notice, not something the user said.
fn claude_interruption(text: &str) -> Option<(&'static str, &str)> {
    let rest = text
        .trim_start()
        .strip_prefix("[Request interrupted by user")?;
    if let Some(rest) = rest.strip_prefix(" for tool use]") {
        Some(("tool_use", rest.trim()))
    } else {
        rest.strip_prefix(']').map(|rest| ("user", rest.trim()))
    }
}

fn push_claude_user_text(
    activities: &mut Vec<SessionActivity>,
    session_id: &str,
    entry_id: &str,
    block_index: usize,
    text: &str,
    created_at: i64,
) {
    let text = match claude_interruption(text) {
        Some((reason, rest)) => {
            activities.push(SessionActivity {
                id: format!("claude:{session_id}:transcript:{entry_id}:{block_index}:interrupt"),
                kind: "interrupt".into(),
                title: "Prompt interrupted".into(),
                detail: Some(reason.into()),
                status: "interrupted".into(),
                created_at,
                files: Vec::new(),
                attachments: Vec::new(),
                append_detail: false,
            });
            rest
        }
        None => text,
    };
    if visible_claude_user_text(text) {
        push_claude_transcript_activity(
            activities,
            session_id,
            entry_id,
            block_index,
            "prompt",
            "You",
            text,
            created_at,
        );
    }
}

fn visible_claude_user_text(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && ![
            "<command-",
            "<local-command-",
            "<system-reminder>",
            "<available-deferred-tools>",
        ]
        .iter()
        .any(|prefix| value.starts_with(prefix))
}

#[allow(clippy::too_many_arguments)]
fn push_claude_transcript_activity(
    activities: &mut Vec<SessionActivity>,
    session_id: &str,
    entry_id: &str,
    block_index: usize,
    kind: &str,
    title: &str,
    detail: &str,
    created_at: i64,
) {
    let detail = detail.trim();
    if detail.is_empty() {
        return;
    }
    activities.push(SessionActivity {
        id: format!("claude:{session_id}:transcript:{entry_id}:{block_index}"),
        kind: kind.into(),
        title: title.into(),
        detail: Some(truncate(detail, 32 * 1024)),
        status: "completed".into(),
        created_at,
        files: Vec::new(),
        attachments: Vec::new(),
        append_detail: false,
    });
}

fn project_name(path: &str) -> Option<String> {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let shortened = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{shortened}…")
    } else {
        shortened
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_stdin_reader_is_bounded_and_times_out() {
        let oversized = vec![b'x'; MAX_HOOK_STDIN_BYTES as usize + 1];
        assert!(read_bounded_hook_input(std::io::Cursor::new(oversized)).is_err());
        assert_eq!(
            read_bounded_hook_input(std::io::Cursor::new(b"{}".to_vec())).unwrap(),
            "{}"
        );

        struct SlowReader;
        impl Read for SlowReader {
            fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
                thread::sleep(Duration::from_millis(40));
                Ok(0)
            }
        }

        assert_eq!(
            read_hook_input_with_timeout(SlowReader, Duration::from_millis(2)),
            None,
            "a stalled hook pipe must use the provider fallback"
        );
    }

    #[test]
    fn antigravity_hooks_return_protocol_safe_defaults() {
        assert_eq!(
            antigravity_hook_output("antigravity:PreToolUse"),
            Some(json!({ "decision": "ask" }))
        );
        assert_eq!(
            antigravity_hook_output("antigravity:PreToolUseAllow"),
            Some(json!({ "decision": "ask" }))
        );
        assert_eq!(
            antigravity_hook_output("antigravity:PreToolUseAsk"),
            Some(json!({ "decision": "ask" }))
        );
        assert_eq!(
            antigravity_hook_output("antigravity:PostToolUse"),
            Some(json!({}))
        );
        assert_eq!(
            antigravity_hook_output("antigravity:Stop"),
            Some(json!({ "decision": "allow" }))
        );
        assert_eq!(antigravity_hook_output("claude:PreToolUse"), None);
        assert_eq!(gemini_hook_output("gemini"), Some(json!({})));
        assert_eq!(gemini_hook_output("claude"), None);
    }

    #[test]
    fn antigravity_stream_marker_never_changes_external_or_other_agent_identity() {
        assert!(is_managed_antigravity_stream("antigravity", Some("1")));
        assert!(!is_managed_antigravity_stream("antigravity", None));
        assert!(!is_managed_antigravity_stream("gemini", Some("1")));
    }

    #[test]
    fn antigravity_stop_status_does_not_report_errors_or_background_work_as_complete() {
        assert!(matches!(
            antigravity_stop_event(&json!({
                "terminationReason": "error",
                "error": "tool failed",
                "fullyIdle": true
            })),
            HookEventKind::Failed
        ));
        assert!(matches!(
            antigravity_stop_event(&json!({
                "terminationReason": "model_stop",
                "fullyIdle": false
            })),
            HookEventKind::Running
        ));
        assert!(matches!(
            antigravity_stop_event(&json!({
                "terminationReason": "model_stop",
                "fullyIdle": true
            })),
            HookEventKind::Completed
        ));
        assert!(matches!(
            antigravity_stop_event(&json!({
                "terminationReason": "future_reason",
                "fullyIdle": true
            })),
            HookEventKind::Activity
        ));
        assert!(matches!(
            antigravity_stop_event(&json!({
                "terminationReason": "model_stop"
            })),
            HookEventKind::Activity
        ));
    }

    #[test]
    fn antigravity_stop_label_matches_the_normalized_status() {
        assert_eq!(
            status_label("Stop", &HookEventKind::Failed),
            Some("Encerrado com erro")
        );
        assert_eq!(
            status_label("Stop", &HookEventKind::Running),
            Some("Executando")
        );
        assert_eq!(
            status_label("Stop", &HookEventKind::Completed),
            Some("Finalizado")
        );
        assert_eq!(status_label("Stop", &HookEventKind::Activity), None);
    }

    #[test]
    fn antigravity_post_tool_error_is_visible_as_failed_activity() {
        let activity = hook_activity(
            "antigravity",
            "PostToolUse",
            &json!({
                "toolCall": { "name": "run_command", "args": { "CommandLine": "npm test" } },
                "error": "exit status 1",
                "stepIdx": 4
            }),
            "conversation-1",
            None,
        )
        .expect("atividade de ferramenta");

        assert_eq!(activity.status, "failed");
    }

    #[test]
    fn claude_permission_uses_session_only_suggestion() {
        let raw = json!({
            "permission_suggestions": [{
                "type": "addRules",
                "rules": [{ "toolName": "Bash", "ruleContent": "npm test" }],
                "behavior": "allow",
                "destination": "localSettings"
            }]
        });
        let output = claude_permission_output(Some(PermissionAction::AllowSession), &raw)
            .expect("resposta Claude");
        assert_eq!(
            output["hookSpecificOutput"]["decision"]["updatedPermissions"][0]["destination"],
            "session"
        );
        assert_eq!(
            output["hookSpecificOutput"]["decision"]["behavior"],
            "allow"
        );
    }

    #[test]
    fn codex_child_lineage_cannot_be_normalized_into_root_activity() {
        let parent = "aaaaaaaa-1234-5678-9abc-123456789abc";
        for hook in [
            "SessionStart",
            "UserPromptSubmit",
            "PreToolUse",
            "Stop",
            "SessionEnd",
        ] {
            for (key, marker) in [
                ("parent_session_id", json!(parent)),
                ("parent_thread_id", json!(parent)),
                ("is_subagent", json!(true)),
                ("source", json!({ "subagent": { "thread_spawn": {} } })),
                ("source", json!("subagent")),
            ] {
                let mut raw = json!({
                    "session_id": parent, "hook_event_name": hook,
                    "tool_name": "spawn_agent", "tool_input": { "message": "Inspect" }
                });
                raw[key] = marker;
                assert!(
                    map_event_with_context("codex", &raw, Some((None, SessionSource::Web)))
                        .is_none(),
                    "{hook}: {key}"
                );
                assert!(map_event("codex", &raw).is_none(), "fallback {hook}: {key}");
            }
        }
    }

    #[test]
    fn codex_root_subagent_tools_keep_their_main_thread_timeline() {
        let parent = "aaaaaaaa-1234-5678-9abc-123456789abc";
        for hook in ["PreToolUse", "PostToolUse"] {
            let raw = json!({
                "session_id": parent, "hook_event_name": hook,
                "parent_session_id": null, "parent_thread_id": null,
                "is_subagent": false, "source": "resume",
                "agent_id": "child-1", "tool_name": "spawn_agent",
                "tool_input": { "agent_id": "child-1", "message": "Inspect" }
            });
            let event = map_event_with_context("codex", &raw, Some((None, SessionSource::Web)))
                .expect("root subagent tool");
            assert!(matches!(event.event, HookEventKind::Running));
            assert_eq!(event.session_id, format!("codex:{parent}"));
            assert_eq!(event.native_session_id.as_deref(), Some(parent));
            assert_eq!(event.activity.unwrap().kind, "tool");
        }
    }

    #[test]
    fn codex_root_prompt_and_compaction_start_still_normalize() {
        for (hook, source) in [("UserPromptSubmit", "resume"), ("SessionStart", "compact")] {
            let raw = json!({
                "session_id": "aaaaaaaa-1234-5678-9abc-123456789abc",
                "hook_event_name": hook, "source": source
            });
            let event = map_event_with_context("codex", &raw, Some((None, SessionSource::Web)))
                .expect("root event");
            assert!(matches!(
                event.event,
                HookEventKind::Running | HookEventKind::SessionStarted
            ));
        }
    }

    #[test]
    fn claude_questions_are_not_mapped_as_permissions() {
        let raw = json!({
            "session_id": "session-1",
            "hook_event_name": "PreToolUse",
            "tool_name": "AskUserQuestion",
            "tool_use_id": "tool-1",
            "cwd": "/work/lume",
            "tool_input": {
                "questions": [{
                    "header": "Approach",
                    "question": "Which approach should I use?",
                    "options": [
                        { "label": "A", "description": "First approach" },
                        { "label": "B", "description": "Second approach" }
                    ],
                    "multiSelect": false
                }]
            }
        });
        let event = map_event("claude", &raw).expect("pergunta Claude");
        assert!(matches!(event.event, HookEventKind::QuestionRequest));
        assert!(event.permission.is_none());
        assert_eq!(
            event.question.as_ref().unwrap().questions[0].options.len(),
            2
        );
    }

    #[test]
    fn claude_question_answers_use_updated_tool_input() {
        let raw = json!({
            "session_id": "session-1",
            "tool_input": {
                "questions": [{
                    "header": "Approach",
                    "question": "Which approach should I use?",
                    "options": [{ "label": "A" }, { "label": "B" }],
                    "multiSelect": false
                }]
            }
        });
        let output = claude_question_output(
            Some(vec![QuestionAnswer {
                question_id: claude_question_item_id("session-1", 0),
                answers: vec!["B".into()],
            }]),
            &raw,
        )
        .expect("resposta Claude");
        assert_eq!(output["hookSpecificOutput"]["permissionDecision"], "allow");
        assert_eq!(
            output["hookSpecificOutput"]["updatedInput"]["answers"]["Which approach should I use?"],
            "B"
        );
    }

    #[test]
    fn gemini_tool_permission_is_observation_only() {
        let raw = json!({
            "session_id": "gemini-session",
            "cwd": "/work/project",
            "hook_event_name": "Notification",
            "notification_type": "ToolPermission",
            "message": "Permitir ferramenta?",
            "details": { "file_path": "/work/project/file.txt" }
        });
        let event = map_event("gemini", &raw).expect("evento Gemini");
        let profile = event.permission_profile.expect("perfil");
        assert!(!profile.can_respond_from_lume);
        assert_eq!(
            profile.available_actions,
            vec![PermissionAction::OpenSource]
        );
        assert!(!event.wait_for_decision);
    }

    #[test]
    fn claude_permission_profile_follows_each_session_mode() {
        let raw = json!({
            "session_id": "claude-session",
            "cwd": "/work/project",
            "hook_event_name": "PermissionRequest",
            "permission_mode": "plan",
            "tool_name": "Bash",
            "tool_input": { "command": "npm test" }
        });
        let event = map_event("claude", &raw).expect("evento Claude");
        let profile = event.permission_profile.expect("perfil");
        assert_eq!(profile.mode, AccessMode::Plan);
        assert!(profile.can_respond_from_lume);
        assert_eq!(
            profile.available_actions,
            vec![PermissionAction::AllowOnce, PermissionAction::Deny]
        );
    }

    #[test]
    fn tool_completion_returns_the_session_to_running() {
        for (provider, hook) in [
            ("codex", "PostToolUse"),
            ("claude", "PostToolUse"),
            ("claude", "PostToolUseFailure"),
            ("gemini", "AfterTool"),
        ] {
            let raw = json!({
                "session_id": format!("{provider}-session"),
                "cwd": "/work/project",
                "hook_event_name": hook,
                "tool_name": "Bash"
            });
            let event = map_event(provider, &raw).expect("evento pós-ferramenta");
            assert!(matches!(event.event, HookEventKind::Running));
            assert_eq!(event.status_label.as_deref(), Some("Executando"));
        }
    }

    #[test]
    fn non_permission_hooks_keep_the_active_full_access_mode() {
        let raw = json!({
            "session_id": "claude-session",
            "cwd": "/work/project",
            "hook_event_name": "UserPromptSubmit",
            "permission_mode": "bypassPermissions"
        });
        let event = map_event("claude", &raw).expect("evento Claude");
        let profile = event.permission_profile.expect("perfil");
        assert_eq!(profile.mode, AccessMode::FullAccess);
        assert!(!profile.can_respond_from_lume);
    }

    #[test]
    fn completed_hook_carries_the_final_agent_response() {
        let raw = json!({
            "session_id": "claude-session",
            "cwd": "/work/project",
            "hook_event_name": "Stop",
            "last_assistant_message": "Resposta final do agente"
        });
        let event = map_event("claude", &raw).expect("evento final do Claude");
        assert!(matches!(event.event, HookEventKind::Completed));
        assert_eq!(
            event.last_response.as_deref(),
            Some("Resposta final do agente")
        );
        assert_eq!(
            event
                .activity
                .as_ref()
                .map(|activity| activity.kind.as_str()),
            Some("message")
        );
    }

    #[test]
    fn claude_newer_lifecycle_events_are_preserved_as_activity() {
        for hook in [
            "PermissionDenied",
            "PostToolBatch",
            "SubagentStart",
            "SubagentStop",
            "TaskCreated",
            "TaskCompleted",
        ] {
            let raw = json!({
                "session_id": "claude-session",
                "cwd": "/work/project",
                "hook_event_name": hook,
                "tool_name": "Bash",
                "tool_input": { "command": "npm test" },
                "tool_use_id": "tool-1",
                "reason": "Blocked by classifier",
                "agent_id": "agent-1",
                "agent_type": "Explore",
                "task_id": "task-1",
                "task_subject": "Inspect hooks"
            });
            let event = map_event("claude", &raw).expect("evento novo do Claude");
            assert!(matches!(event.event, HookEventKind::Activity));
            if hook != "PostToolBatch" {
                assert!(event.activity.is_some(), "{hook} deve produzir atividade");
            }
        }
    }

    #[test]
    fn a_cancelled_request_is_a_notice_and_keeps_what_was_typed_next() {
        assert_eq!(
            claude_interruption("[Request interrupted by user]"),
            Some(("user", ""))
        );
        assert_eq!(
            claude_interruption("[Request interrupted by user for tool use]"),
            Some(("tool_use", ""))
        );
        assert_eq!(
            claude_interruption("[Request interrupted by user]\n\nfaça outra coisa"),
            Some(("user", "faça outra coisa"))
        );
        assert_eq!(
            claude_interruption("faça isso [Request interrupted by user]"),
            None
        );

        let mut activities = Vec::new();
        push_claude_user_text(
            &mut activities,
            "s",
            "e1",
            0,
            "[Request interrupted by user]\n\nfaça outra coisa",
            5,
        );
        let kinds = activities
            .iter()
            .map(|a| (a.kind.as_str(), a.status.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            [("interrupt", "interrupted"), ("prompt", "completed")]
        );
        assert_eq!(activities[1].detail.as_deref(), Some("faça outra coisa"));
        // Alone, the marker is only a notice: nothing the user said.
        let mut alone = Vec::new();
        push_claude_user_text(&mut alone, "s", "e2", 0, "[Request interrupted by user]", 6);
        assert_eq!(alone.len(), 1);
        assert_eq!(alone[0].kind, "interrupt");
    }

    #[test]
    fn claude_internal_fork_recap_is_not_a_subagent_activity() {
        for agent_type in [json!(""), Value::Null] {
            let mut raw = json!({
                "session_id": "claude-session",
                "cwd": "/work/project",
                "hook_event_name": "SubagentStop",
                "agent_id": "fork-1",
                "last_assistant_message": "Você pediu uma análise do projeto."
            });
            if !agent_type.is_null() {
                raw["agent_type"] = agent_type;
            }
            assert!(map_event("claude", &raw).is_none());
        }
    }

    #[test]
    fn claude_edit_hook_activity_carries_the_diff() {
        let activity = hook_activity(
            "claude",
            "PostToolUse",
            &json!({
                "tool_name": "Edit",
                "tool_use_id": "toolu_1",
                "tool_input": { "file_path": "/work/app/src/a.rs", "old_string": "x", "new_string": "y" },
                "tool_response": {
                    "filePath": "/work/app/src/a.rs", "oldString": "x", "newString": "y",
                    "originalFile": "a\nb\nx\n", "userModified": false, "replaceAll": false,
                    "structuredPatch": [{ "oldStart": 1, "oldLines": 3, "newStart": 1, "newLines": 3, "lines": [" a", " b", "-x", "+y"] }]
                }
            }),
            "session-1",
            None,
        )
        .expect("activity");
        assert_eq!(activity.kind, "file");
        let detail = activity.detail.expect("detail");
        assert!(
            detail.starts_with("diff --git a//work/app/src/a.rs"),
            "{detail}"
        );
        assert!(detail.contains("-x\n+y"));
        assert_eq!(activity.files, vec!["/work/app/src/a.rs".to_string()]);
    }

    #[test]
    fn claude_edit_tools_become_reviewable_diffs() {
        let patched = file_change_diff(
            Some(&json!({ "file_path": "/work/a.ts", "old_string": "x", "new_string": "y" })),
            Some(&json!({ "filePath": "/work/a.ts", "structuredPatch": [
                { "oldStart": 4, "oldLines": 2, "newStart": 4, "newLines": 2, "lines": [" keep", "-x", "+y"] }
            ] })),
        )
        .expect("structured patch");
        assert!(patched.starts_with("diff --git a//work/a.ts b//work/a.ts\n"));
        assert!(patched.contains("@@ -4,2 +4,2 @@\n keep\n-x\n+y\n"));

        let edited = file_change_diff(
            Some(&json!({ "file_path": "/nonexistent/b.ts", "old_string": "a\nb", "new_string": "c" })),
            Some(&json!({ "originalFile": "one\na\nb\ntwo" })),
        )
        .expect("old and new strings");
        assert!(edited.contains("@@ -2,2 +2,1 @@\n-a\n-b\n+c\n"));

        let created = file_change_diff(
            Some(&json!({ "file_path": "/nonexistent/c.ts", "content": "l1\nl2" })),
            Some(&json!({ "type": "create" })),
        )
        .expect("a new file");
        assert!(
            created.contains("--- /dev/null") && created.contains("@@ -0,0 +1,2 @@\n+l1\n+l2\n")
        );

        assert!(file_change_diff(Some(&json!({ "command": "ls" })), None).is_none());
    }

    #[test]
    fn claude_agent_completed_notification_finishes_or_fails_the_session() {
        let completed = map_event(
            "claude",
            &json!({
                "session_id": "claude-session",
                "hook_event_name": "Notification",
                "notification_type": "agent_completed",
                "message": "Background agent completed"
            }),
        )
        .expect("notificação concluída");
        assert!(matches!(completed.event, HookEventKind::Completed));

        let failed = map_event(
            "claude",
            &json!({
                "session_id": "claude-session",
                "hook_event_name": "Notification",
                "notification_type": "agent_completed",
                "message": "Background agent failed"
            }),
        )
        .expect("notificação de falha");
        assert!(matches!(failed.event, HookEventKind::Failed));
    }

    #[test]
    fn claude_fork_origin_reaches_the_session() {
        let path = std::env::temp_dir().join(format!("lume-claude-fork-{}.jsonl", now_millis()));
        let entry = json!({
            "type": "user",
            "uuid": "copied-prompt",
            "sessionId": "forked-session",
            "forkedFrom": { "sessionId": "original-session", "messageUuid": "copied-prompt" },
            "message": { "role": "user", "content": "Original question" }
        });
        std::fs::write(&path, entry.to_string()).expect("transcript");
        let raw = json!({
            "session_id": "forked-session",
            "cwd": "/work/project",
            "hook_event_name": "Stop",
            "transcript_path": path.to_string_lossy()
        });
        let event = map_event("claude", &raw).expect("evento");
        let _ = std::fs::remove_file(&path);
        let state = crate::state::AppState::new(std::path::Path::new(":memory:")).expect("estado");
        state.ingest(event).expect("ingest");
        let sessions = state.sessions().expect("sessões");
        assert_eq!(sessions[0].forked_from.as_deref(), Some("original-session"));
        assert!(sessions[0]
            .activities
            .iter()
            .all(|activity| activity.kind != "fork_origin"));
    }

    #[test]
    fn claude_transcript_reports_token_usage_per_prompt() {
        let path = std::env::temp_dir().join(format!("lume-claude-tokens-{}.jsonl", now_millis()));
        let assistant = |id: &str, at: &str, input: u64, cached: u64, output: u64| {
            json!({
                "type": "assistant",
                "uuid": format!("{id}-{at}"),
                "timestamp": at,
                "message": {
                    "id": id,
                    "role": "assistant",
                    "content": [{ "type": "text", "text": "ok" }],
                    "usage": {
                        "input_tokens": input,
                        "cache_creation_input_tokens": 0,
                        "cache_read_input_tokens": cached,
                        "output_tokens": output
                    }
                }
            })
        };
        let user = |uuid: &str, at: &str, content: Value| json!({ "type": "user", "uuid": uuid, "timestamp": at, "message": { "role": "user", "content": content } });
        let transcript = [
            // Before the first prompt in view: its turn is incomplete, so it is skipped.
            assistant("msg-0", "2026-07-28T11:59:00.000Z", 50, 0, 5),
            user("prompt-1", "2026-07-28T12:00:00.000Z", json!("First task")),
            assistant("msg-1", "2026-07-28T12:00:01.000Z", 10, 100, 20),
            // Same response, next content block: counted once.
            assistant("msg-1", "2026-07-28T12:00:02.000Z", 10, 100, 20),
            user(
                "tool-result",
                "2026-07-28T12:00:03.000Z",
                json!([{ "type": "tool_result", "content": "done" }]),
            ),
            assistant("msg-2", "2026-07-28T12:00:04.000Z", 5, 130, 7),
            user(
                "prompt-2",
                "2026-07-28T12:01:00.000Z",
                json!([{ "type": "text", "text": "Second task" }]),
            ),
            assistant("msg-3", "2026-07-28T12:01:05.000Z", 3, 0, 4),
        ];
        std::fs::write(
            &path,
            transcript
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .expect("transcript");
        let activities = claude_transcript_activities(
            &json!({ "transcript_path": path.to_string_lossy() }),
            "claude-session",
        );
        let _ = std::fs::remove_file(&path);
        let usage = activities
            .iter()
            .filter(|activity| activity.kind == "token_usage")
            .map(|activity| {
                serde_json::from_str::<crate::domain::PromptTokenUsage>(
                    activity.detail.as_deref().unwrap_or_default(),
                )
                .expect("uso")
            })
            .map(|usage| {
                (
                    usage.turn_id,
                    usage.input_tokens,
                    usage.output_tokens,
                    usage.total_tokens,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            usage,
            [
                ("prompt-1".to_string(), 245, 27, 272),
                ("prompt-2".to_string(), 3, 4, 7),
            ]
        );
    }

    #[test]
    fn claude_transcript_exposes_intermediate_text_and_thinking() {
        let path =
            std::env::temp_dir().join(format!("lume-claude-transcript-{}.jsonl", now_millis()));
        let transcript = [
            json!({
                "type": "user",
                "uuid": "user-prompt",
                "timestamp": "2026-07-28T11:59:59.000Z",
                "isMeta": false,
                "message": {
                    "role": "user",
                    "content": "Check the Claude hook."
                }
            }),
            json!({
                "type": "assistant",
                "uuid": "assistant-thinking",
                "timestamp": "2026-07-28T12:00:00.000Z",
                "message": {
                    "role": "assistant",
                    "content": [{ "type": "thinking", "thinking": "Inspect the hook contract." }]
                }
            }),
            json!({
                "type": "assistant",
                "uuid": "assistant-message",
                "timestamp": "2026-07-28T12:00:01.000Z",
                "message": {
                    "role": "assistant",
                    "content": [{ "type": "text", "text": "The hook is connected." }]
                }
            }),
            json!({
                "type": "user",
                "uuid": "user-tool-result",
                "timestamp": "2026-07-28T12:00:02.000Z",
                "message": {
                    "role": "user",
                    "content": [{ "type": "tool_result", "content": "ignored" }]
                }
            }),
        ]
        .into_iter()
        .map(|entry| serde_json::to_string(&entry).expect("json"))
        .collect::<Vec<_>>()
        .join("\n");
        std::fs::write(&path, transcript).expect("transcript");

        let event = map_event(
            "claude",
            &json!({
                "session_id": "claude-session",
                "hook_event_name": "PostToolBatch",
                "transcript_path": path
            }),
        )
        .expect("evento com transcript");
        let _ = std::fs::remove_file(path);

        assert_eq!(event.activities.len(), 3);
        assert_eq!(event.activities[0].kind, "prompt");
        // Reasoning text is part of the conversation, outside the tool events.
        assert_eq!(event.activities[1].kind, "message");
        assert_eq!(event.activities[1].title, "Thinking");
        assert_eq!(
            event.activities[2].detail.as_deref(),
            Some("The hook is connected.")
        );
        assert!(event.activities[0].created_at < event.activities[2].created_at);
    }

    #[test]
    fn claude_transcript_limit_preserves_conversation() {
        let mut activities = Vec::new();
        for index in 0..190 {
            push_claude_transcript_activity(
                &mut activities,
                "long-session",
                &format!("thinking-{index}"),
                0,
                "thinking",
                "Thinking",
                "internal activity",
                index,
            );
        }
        for index in 0..180 {
            push_claude_transcript_activity(
                &mut activities,
                "long-session",
                &format!("message-{index}"),
                0,
                if index % 2 == 0 { "prompt" } else { "message" },
                "Conversation",
                "visible message",
                1_000 + index,
            );
        }

        prune_claude_transcript_activities(&mut activities);

        assert_eq!(
            activities
                .iter()
                .filter(|activity| matches!(activity.kind.as_str(), "prompt" | "message"))
                .count(),
            180
        );
        assert_eq!(
            activities
                .iter()
                .filter(|activity| activity.kind == "thinking")
                .count(),
            CLAUDE_TRANSCRIPT_TRANSIENT_LIMIT
        );
    }

    #[test]
    fn tool_hooks_expose_commands_and_files_as_activity() {
        let command = map_event(
            "claude",
            &json!({
                "session_id": "claude-session",
                "cwd": "/work/project",
                "hook_event_name": "PostToolUse",
                "tool_use_id": "tool-1",
                "tool_name": "Bash",
                "tool_input": { "command": "npm test" },
                "tool_response": { "output": "12 tests passed" }
            }),
        )
        .expect("evento de comando")
        .activity
        .expect("atividade de comando");
        assert_eq!(command.kind, "test");
        assert_eq!(command.title, "npm test");
        assert!(command
            .detail
            .as_deref()
            .is_some_and(|value| value.contains("12 tests passed")));

        let file = map_event(
            "claude",
            &json!({
                "session_id": "claude-session",
                "cwd": "/work/project",
                "hook_event_name": "PreToolUse",
                "tool_use_id": "tool-2",
                "tool_name": "Edit",
                "tool_input": { "file_path": "/work/project/src/app.ts" }
            }),
        )
        .expect("evento de arquivo")
        .activity
        .expect("atividade de arquivo");
        assert_eq!(file.kind, "file");
        assert_eq!(file.files, vec!["/work/project/src/app.ts"]);
    }

    #[test]
    fn todo_tool_keeps_its_items_after_completion() {
        let todo = map_event(
            "claude",
            &json!({
                "session_id": "claude-session",
                "cwd": "/work/project",
                "hook_event_name": "PostToolUse",
                "tool_use_id": "todo-1",
                "tool_name": "TodoWrite",
                "tool_input": {
                    "todos": [
                        { "content": "Inspect hooks", "status": "completed" },
                        { "content": "Validate tray", "status": "in_progress" }
                    ]
                },
                "tool_response": { "ok": true }
            }),
        )
        .expect("evento de todo")
        .activity
        .expect("atividade de todo");

        assert_eq!(todo.kind, "tool");
        assert_eq!(todo.title, "TodoWrite");
        assert!(todo
            .detail
            .as_deref()
            .is_some_and(|value| value.contains("Validate tray")));
    }

    #[test]
    fn antigravity_hooks_use_conversation_and_workspace_fields() {
        let raw = json!({
            "conversationId": "agy-conversation",
            "workspacePaths": ["/work/antigravity"],
            "stepIdx": 2
        });
        let running =
            map_event("antigravity:PreInvocation", &raw).expect("evento Antigravity em execução");
        assert_eq!(running.agent, AgentKind::Antigravity);
        assert_eq!(running.process_id, None);
        assert_eq!(running.source, Some(SessionSource::Cli));
        assert!(matches!(running.event, HookEventKind::Running));
        assert_eq!(
            running.native_session_id.as_deref(),
            Some("agy-conversation")
        );
        assert_eq!(
            running.working_directory.as_deref(),
            Some("/work/antigravity")
        );

        let tool = map_event(
            "antigravity:PreToolUseAllow",
            &json!({
                "conversationId": "agy-conversation",
                "workspacePaths": ["/work/antigravity"],
                "stepIdx": 3,
                "toolCall": {
                    "name": "run_command",
                    "args": { "command": "cargo test" }
                }
            }),
        )
        .expect("evento de ferramenta Antigravity")
        .activity
        .expect("atividade Antigravity");
        assert_eq!(tool.kind, "test");
        assert_eq!(tool.title, "cargo test");

        let completed = map_event(
            "antigravity:Stop",
            &json!({
                "conversationId": "agy-conversation",
                "workspacePaths": ["/work/antigravity"],
                "terminationReason": "model_stop",
                "fullyIdle": true
            }),
        )
        .expect("evento Antigravity finalizado");
        assert!(matches!(completed.event, HookEventKind::Completed));
    }

    #[test]
    fn antigravity_multi_root_events_use_the_workspace_from_tool_arguments() {
        let event = map_event(
            "antigravity:PostToolUse",
            &json!({
            "conversationId": "agy-conversation",
            "workspacePaths": ["/work/first", "/work/second"],
            "cwd": "/work/first",
            "toolCall": {
                "name": "run_command",
                "args": { "Cwd": "/work/second/packages/app", "CommandLine": "npm test" }
                }
            }),
        )
        .expect("evento Antigravity multi-root");

        assert_eq!(event.working_directory.as_deref(), Some("/work/second"));
        assert!(path_is_within(r"C:\Work\App\src", r"C:\Work\App"));
        assert!(path_is_within(r"c:\work\app\src", r"C:\Work\App"));
        assert!(!path_is_within("/work/application", "/work/app"));

        let file_event = map_event(
            "antigravity:PostToolUse",
            &json!({
                "conversationId": "agy-conversation",
                "workspacePaths": ["/work", "/work/project"],
                "toolCall": {
                    "name": "replace_file_content",
                    "args": { "TargetFile": "/work/project/src/lib.rs" }
                }
            }),
        )
        .expect("evento Antigravity de alteração de arquivo");
        assert_eq!(
            file_event.working_directory.as_deref(),
            Some("/work/project"),
            "choose the most specific root containing the changed file"
        );

        let cwd_event = map_event(
            "antigravity:PreInvocation",
            &json!({
                "conversationId": "agy-conversation",
                "workspacePaths": ["/work/first", "/work/second"],
                "cwd": "/work/second/packages/app"
            }),
        )
        .expect("evento que informa só o cwd");
        assert_eq!(cwd_event.working_directory.as_deref(), Some("/work/second"));
    }
}
