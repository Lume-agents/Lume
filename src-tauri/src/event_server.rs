use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    thread,
    time::Duration,
};

use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

use crate::{
    domain::{HookEvent, HookEventKind, HookResponse, PermissionAction},
    state::AppState,
};

pub const EVENT_SERVER_ADDRESS: &str = "127.0.0.1:43119";
const HOOK_EVENT_IO_TIMEOUT: Duration = Duration::from_secs(2);
const OBSERVATION_HOOK_IO_TIMEOUT: Duration = Duration::from_millis(300);
const MAX_HOOK_EVENT_BYTES: u64 = 2 * 1024 * 1024;

pub fn start(state: AppState, app: AppHandle) -> Result<(), String> {
    let listener = TcpListener::bind(EVENT_SERVER_ADDRESS)
        .map_err(|error| format!("Não foi possível iniciar a entrada local de eventos: {error}"))?;
    thread::Builder::new()
        .name("lume-event-server".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let state = state.clone();
                let app = app.clone();
                let _ = thread::Builder::new()
                    .name("lume-event-client".into())
                    .spawn(move || handle_connection(stream, state, app));
            }
        })
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn handle_connection(mut stream: TcpStream, state: AppState, app: AppHandle) {
    if stream
        .set_read_timeout(Some(HOOK_EVENT_IO_TIMEOUT))
        .and_then(|()| stream.set_write_timeout(Some(HOOK_EVENT_IO_TIMEOUT)))
        .is_err()
    {
        return;
    }
    let response = read_event(&stream).and_then(|mut event| {
        let automatically_approved = permission_is_automatically_approved(&state, &event)?;
        let wait_for_decision = event.wait_for_decision;
        let question_id = event.question.as_ref().map(|question| question.id.clone());
        if automatically_approved {
            event.event = HookEventKind::Running;
            event.status_label = Some("Executando".into());
            event.permission = None;
            event.wait_for_decision = false;
        }
        let permission_id = publish_event(&state, &app, event)?;

        if automatically_approved {
            return Ok(HookResponse {
                ok: true,
                action: wait_for_decision.then_some(PermissionAction::AllowOnce),
                question_answers: None,
                message: None,
            });
        }
        if wait_for_decision {
            if let Some(question_id) = question_id {
                let answers =
                    state.wait_for_question_answer(&question_id, Duration::from_secs(15 * 60))?;
                if answers.is_none() {
                    state.expire_question(&question_id)?;
                    crate::protocol::emit_sessions_changed(&app);
                }
                return Ok(HookResponse {
                    ok: answers.is_some(),
                    action: None,
                    question_answers: answers,
                    message: None,
                });
            }
            let permission_id = permission_id.ok_or_else(|| {
                "O evento aguardava uma decisão, mas não continha permissão".to_string()
            })?;
            let action = state.wait_for_decision(&permission_id, Duration::from_secs(15 * 60))?;
            return Ok(HookResponse {
                ok: action.is_some(),
                action,
                question_answers: None,
                message: None,
            });
        }

        Ok(HookResponse {
            ok: true,
            action: None,
            question_answers: None,
            message: None,
        })
    });

    let response = response.unwrap_or_else(|message| HookResponse {
        ok: false,
        action: None,
        question_answers: None,
        message: Some(message),
    });
    if let Ok(payload) = serde_json::to_string(&response) {
        let _ = writeln!(stream, "{payload}");
    }
}

fn permission_is_automatically_approved(
    state: &AppState,
    event: &HookEvent,
) -> Result<bool, String> {
    if !matches!(event.event, HookEventKind::PermissionRequest) {
        return Ok(false);
    }
    if event
        .permission_profile
        .as_ref()
        .is_some_and(|profile| profile.automatically_approves())
    {
        return Ok(true);
    }

    state.session_automatically_approves(&event.session_id, event.native_session_id.as_deref())
}

pub fn publish_event(
    state: &AppState,
    app: &AppHandle,
    event: HookEvent,
) -> Result<Option<String>, String> {
    let session_id = event.session_id.clone();
    let native_session_id = event.native_session_id.clone();
    let previous_status = state.session_status(&session_id, native_session_id.as_deref())?;
    let notification = notification_for(&event, previous_status.as_ref());
    let permission_id = state.ingest(event)?;
    crate::protocol::emit_session_changed(app, &session_id, native_session_id.as_deref());
    if state.session_is_visible(&session_id, native_session_id.as_deref())
        && state.preferences()?.popup_notifications_enabled
    {
        if let Some((title, body)) = notification {
            let _ = app.notification().builder().title(title).body(body).show();
        }
    }
    Ok(permission_id)
}

fn notification_for(
    event: &HookEvent,
    previous_status: Option<&crate::domain::SessionStatus>,
) -> Option<(String, String)> {
    if !crate::domain::should_notify(&event.event, previous_status) {
        return None;
    }
    let agent = event
        .agent_label
        .clone()
        .unwrap_or_else(|| match event.agent {
            crate::domain::AgentKind::Codex => "Codex".into(),
            crate::domain::AgentKind::ChatGpt => "ChatGPT".into(),
            crate::domain::AgentKind::Claude => "Claude".into(),
            crate::domain::AgentKind::ClaudeCode => "Claude Code".into(),
            crate::domain::AgentKind::Antigravity => "Antigravity".into(),
            crate::domain::AgentKind::OpenCode => "OpenCode".into(),
            crate::domain::AgentKind::DeepSeek => "DeepSeek".into(),
            crate::domain::AgentKind::Gemini => "Gemini".into(),
            crate::domain::AgentKind::Unknown => "Agente".into(),
        });
    let project = event.project.as_deref().unwrap_or("sessão local");
    let title = match event.event {
        crate::domain::HookEventKind::PermissionRequest => "Lume · Permissão necessária",
        crate::domain::HookEventKind::QuestionRequest => "Lume · Resposta necessária",
        crate::domain::HookEventKind::Completed => "Lume · Tarefa finalizada",
        crate::domain::HookEventKind::Failed => "Lume · Erro na sessão",
        _ => return None,
    };
    Some((title.into(), format!("{agent} · {project}")))
}

fn read_event<R: Read>(reader: R) -> Result<HookEvent, String> {
    let mut line = String::new();
    let bytes_read = BufReader::new(reader.take(MAX_HOOK_EVENT_BYTES + 1))
        .read_line(&mut line)
        .map_err(|error| error.to_string())?;
    if bytes_read as u64 > MAX_HOOK_EVENT_BYTES {
        return Err("Evento local excede o limite de tamanho".into());
    }
    serde_json::from_str(&line).map_err(|error| format!("Evento local inválido: {error}"))
}

pub fn send_event(event_json: &str) -> Result<HookResponse, String> {
    let event: HookEvent = serde_json::from_str(event_json)
        .map_err(|error| format!("Evento local inválido: {error}"))?;
    let mut stream = TcpStream::connect_timeout(
        &EVENT_SERVER_ADDRESS
            .parse()
            .map_err(|error| format!("Endereço local inválido: {error}"))?,
        Duration::from_secs(2),
    )
    .map_err(|_| "O Lume não está em execução".to_string())?;
    if !event.wait_for_decision {
        stream
            .set_write_timeout(Some(HOOK_EVENT_IO_TIMEOUT))
            .and_then(|_| stream.set_read_timeout(Some(HOOK_EVENT_IO_TIMEOUT)))
            .map_err(|error| format!("Não foi possível limitar a espera do evento: {error}"))?;
    }
    stream
        .write_all(event_json.trim().as_bytes())
        .and_then(|_| stream.write_all(b"\n"))
        .map_err(|error| error.to_string())?;
    let _ = stream.shutdown(Shutdown::Write);

    let mut response = String::new();
    BufReader::new(stream)
        .read_line(&mut response)
        .map_err(|error| error.to_string())?;
    serde_json::from_str(&response).map_err(|error| error.to_string())
}

/// Send telemetry-only hooks without waiting for the UI/event handler response.
/// This keeps Antigravity and Gemini tool execution independent of Lume latency.
pub fn send_observation_event(event_json: &str) -> Result<(), String> {
    let event: HookEvent = serde_json::from_str(event_json)
        .map_err(|error| format!("Evento local inválido: {error}"))?;
    if event.wait_for_decision
        || !matches!(
            event.agent,
            crate::domain::AgentKind::Antigravity | crate::domain::AgentKind::Gemini
        )
    {
        return Err("Only non-interactive Google agent hooks can be sent asynchronously".into());
    }
    let address = EVENT_SERVER_ADDRESS
        .parse()
        .map_err(|error| format!("Endereço local inválido: {error}"))?;
    send_observation_event_to(address, event_json)
}

fn send_observation_event_to(address: SocketAddr, event_json: &str) -> Result<(), String> {
    let mut stream = TcpStream::connect_timeout(&address, OBSERVATION_HOOK_IO_TIMEOUT)
        .map_err(|_| "O Lume não está em execução".to_string())?;
    stream
        .set_write_timeout(Some(OBSERVATION_HOOK_IO_TIMEOUT))
        .map_err(|error| format!("Não foi possível limitar a espera do evento: {error}"))?;
    stream
        .write_all(event_json.trim().as_bytes())
        .and_then(|_| stream.write_all(b"\n"))
        .map_err(|error| error.to_string())?;
    let _ = stream.shutdown(Shutdown::Write);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::domain::{
        AccessMode, AgentKind, PermissionProfile, PermissionRequest, SessionSource,
    };

    fn event(kind: HookEventKind) -> HookEvent {
        HookEvent {
            event: kind,
            session_id: "codex:thread-1".into(),
            agent: AgentKind::Codex,
            agent_label: Some("Codex".into()),
            session_name: None,
            project: Some("Lume".into()),
            source: Some(SessionSource::Cli),
            source_app: None,
            control_origin: crate::domain::SessionControlOrigin::External,
            status_label: None,
            started_at: None,
            process_id: Some(4242),
            native_session_id: Some("thread-1".into()),
            working_directory: Some("/work/lume".into()),
            permission_profile: None,
            permission: None,
            question: None,
            last_response: None,
            activity: None,
            activities: Vec::new(),
            wait_for_decision: false,
        }
    }

    #[test]
    fn observation_hook_does_not_wait_for_the_event_server_response() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener local");
        let address = listener.local_addr().expect("endereço local");
        let received = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("aceita hook");
            let mut line = String::new();
            BufReader::new(stream)
                .read_line(&mut line)
                .expect("lê evento");
            line
        });
        let payload = r#"{"event":"Running","session_id":"ag:1"}"#;

        send_observation_event_to(address, payload).expect("envia sem resposta");
        assert_eq!(
            received.join().expect("evento recebido"),
            format!("{payload}\n")
        );
    }

    #[test]
    fn hook_event_reader_rejects_payloads_over_the_size_limit() {
        let oversized = " ".repeat(MAX_HOOK_EVENT_BYTES as usize + 1);
        let error = read_event(std::io::Cursor::new(oversized)).expect_err("limite do payload");

        assert!(error.contains("excede o limite"));
    }

    #[test]
    fn permission_uses_the_stored_automatic_profile_when_the_hook_omits_it() {
        let state = AppState::new(Path::new(":memory:")).expect("state");
        let mut started = event(HookEventKind::SessionStarted);
        started.permission_profile = Some(PermissionProfile {
            mode: AccessMode::WorkspaceWrite,
            label: "Approve for me".into(),
            approval_policy: "on-request".into(),
            approvals_reviewer: Some("auto_review".into()),
            can_respond_from_lume: false,
            available_actions: vec![PermissionAction::OpenSource],
        });
        state.ingest(started).expect("stored profile");

        let mut permission = event(HookEventKind::PermissionRequest);
        permission.permission = Some(PermissionRequest {
            id: "permission-1".into(),
            kind: "command".into(),
            summary: "Run command".into(),
            resource: "cargo test".into(),
            risk: "medium".into(),
            requested_at: "1".into(),
        });

        assert!(
            permission_is_automatically_approved(&state, &permission).expect("automatic decision")
        );
    }
}
