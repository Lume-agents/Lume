//! CLI display identity is deliberately separate from native session ownership.
//! A user-selected link never supplies a native ID to a control command.
use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::{
    codex_identity_probe::{valid_thread_id, ProcessIdentity},
    domain::{
        AgentKind, AgentSession, HookEvent, HookEventKind, SessionControlOrigin, SessionSource,
    },
};

const MAX_LINKS: usize = 128;
const MAX_OBSERVATIONS: usize = 256;
const OBSERVATION_TTL_MS: i64 = 24 * 60 * 60 * 1000;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DisplayLink {
    pub process: ProcessIdentity,
    pub native_session_id: String,
}

impl DisplayLink {
    pub fn key(&self) -> String {
        serde_json::to_string(&self.process).unwrap_or_default()
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConversationCandidate {
    pub native_session_id: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConversationChoices {
    pub process_key: String,
    pub linked_native_session_id: Option<String>,
    pub candidates: Vec<ConversationCandidate>,
    pub has_more: bool,
}

#[derive(Default)]
pub(crate) struct CliIdentities {
    live: HashMap<u32, ProcessIdentity>,
    links: HashMap<u32, DisplayLink>,
    automatic_links: HashMap<u32, DisplayLink>,
    pending: HashMap<String, i64>,
    editors: HashMap<String, i64>,
    live_hook_at: HashMap<String, i64>,
    root_hook_activity_at: HashMap<String, i64>,
    ended_root_hooks: HashMap<String, i64>,
}

fn external_codex(session: &AgentSession) -> bool {
    session.agent == AgentKind::Codex && session.control_origin == SessionControlOrigin::External
}

pub(crate) fn can_choose_conversation(session: &AgentSession) -> bool {
    external_codex(session)
        && session.source == SessionSource::Cli
        && session.process_id.is_some()
        && session.native_session_id.is_none()
}

fn bounded_observation(map: &mut HashMap<String, i64>, id: &str, now: i64) {
    map.retain(|_, time| now.saturating_sub(*time) <= OBSERVATION_TTL_MS);
    map.insert(id.to_owned(), now);
    while map.len() > MAX_OBSERVATIONS {
        let oldest = map
            .iter()
            .min_by_key(|(_, time)| **time)
            .map(|(id, _)| id.clone());
        if let Some(id) = oldest {
            map.remove(&id);
        }
    }
}

impl CliIdentities {
    pub fn load(links: Vec<DisplayLink>) -> Self {
        let mut state = Self::default();
        for link in links.into_iter().take(MAX_LINKS) {
            if valid_thread_id(&link.native_session_id) && link.process.matches(&link.process) {
                state.links.insert(link.process.pid, link);
            }
        }
        state
    }

    /// Called with actual Codex TUI processes, never a shared App Server.
    pub fn reconcile(&mut self, live: Vec<ProcessIdentity>, now: i64) -> Vec<String> {
        self.root_hook_activity_at
            .retain(|_, time| now.saturating_sub(*time) <= OBSERVATION_TTL_MS);
        self.ended_root_hooks
            .retain(|_, time| now.saturating_sub(*time) <= OBSERVATION_TTL_MS);
        self.live = live
            .into_iter()
            .map(|process| (process.pid, process))
            .collect();
        let mut removed = Vec::new();
        self.links.retain(|pid, link| {
            let keep = self
                .live
                .get(pid)
                .is_some_and(|process| link.process.matches(process));
            if !keep {
                removed.push(link.key());
            }
            keep
        });
        self.automatic_links.retain(|pid, link| {
            self.live
                .get(pid)
                .is_some_and(|current| link.process.matches(current))
        });
        for link in self.links.values() {
            bounded_observation(&mut self.pending, &link.native_session_id, now);
        }
        removed
    }

    pub fn observe(&mut self, event: &HookEvent, sessions: &[AgentSession], now: i64) {
        if event.agent != AgentKind::Codex || event.control_origin != SessionControlOrigin::External
        {
            return;
        }
        let Some(id) = event
            .native_session_id
            .as_deref()
            .filter(|id| valid_thread_id(id))
        else {
            return;
        };
        let hook = event.session_id == format!("codex:{id}");
        self.ended_root_hooks
            .retain(|_, time| now.saturating_sub(*time) <= OBSERVATION_TTL_MS);
        if hook && matches!(event.event, HookEventKind::SessionEnded) {
            bounded_observation(&mut self.ended_root_hooks, id, now);
            self.root_hook_activity_at.remove(id);
            self.live_hook_at.remove(id);
            self.pending.remove(id);
            self.editors.remove(id);
            return;
        }
        if hook {
            let ended_at = self.ended_root_hooks.get(id).copied();
            let fresh_prompt = matches!(event.event, HookEventKind::Running)
                && event
                    .activity
                    .iter()
                    .chain(event.activities.iter())
                    .any(|activity| {
                        activity.kind == "prompt"
                            && ended_at.is_none_or(|ended| activity.created_at > ended)
                            && activity.created_at <= now
                    });
            if fresh_prompt {
                self.ended_root_hooks.remove(id);
            }
        }
        if event.process_id.is_some() {
            return;
        }
        if hook {
            let ended = self.ended_root_hooks.contains_key(id);
            if !ended {
                bounded_observation(&mut self.live_hook_at, id, now);
            }
            // SessionStart also fires after compaction. Only actual root-turn
            // activity establishes a monitor card, never loaded metadata. A
            // late Stop/tool event cannot resurrect an ended conversation.
            if !ended && !matches!(event.event, HookEventKind::SessionStarted) {
                self.restore_root_hook_visibility(id, now, now);
            }
            // Positive editor ancestry is a separate proof, not a CLI guess.
            if event.source == Some(SessionSource::Vscode) {
                bounded_observation(&mut self.editors, id, now);
                self.pending.remove(id);
                return;
            }
        }
        let unresolved_cli = sessions.iter().any(can_choose_conversation);
        if event.source == Some(SessionSource::Cli)
            || (unresolved_cli
                && event.session_id.starts_with("codex-app-server:")
                && !self.editors.contains_key(id))
        {
            bounded_observation(&mut self.pending, id, now);
        }
    }

    /// Restore only independently proven root-hook activity, not cached thread
    /// metadata. After an end, the caller must prove a newer user prompt. This
    /// grants monitoring visibility, never process ownership.
    pub fn restore_root_hook_visibility(&mut self, id: &str, observed_at: i64, now: i64) {
        if !valid_thread_id(id)
            || observed_at <= 0
            || observed_at > now
            || now.saturating_sub(observed_at) > OBSERVATION_TTL_MS
            || self
                .ended_root_hooks
                .get(id)
                .is_some_and(|ended| observed_at <= *ended)
        {
            return;
        }
        self.root_hook_activity_at
            .retain(|_, time| now.saturating_sub(*time) <= OBSERVATION_TTL_MS);
        self.ended_root_hooks
            .retain(|_, time| now.saturating_sub(*time) <= OBSERVATION_TTL_MS);
        self.ended_root_hooks.remove(id);
        let observed_at = self
            .root_hook_activity_at
            .get(id)
            .copied()
            .unwrap_or_default()
            .max(observed_at);
        bounded_observation(&mut self.root_hook_activity_at, id, observed_at);
        let live_hook_at = self
            .live_hook_at
            .get(id)
            .copied()
            .unwrap_or_default()
            .max(observed_at);
        bounded_observation(&mut self.live_hook_at, id, live_hook_at);
    }

    pub fn has_root_cli_monitor(&self, id: &str) -> bool {
        self.root_hook_activity_at.contains_key(id) && !self.editors.contains_key(id)
    }

    pub fn visible(&self, session: &AgentSession) -> bool {
        if !external_codex(session) || session.process_id.is_some() {
            return true;
        }
        let Some(id) = session.native_session_id.as_deref() else {
            return true;
        };
        if self.ended_root_hooks.contains_key(id) && !self.editors.contains_key(id) {
            return false;
        }
        if self
            .links
            .keys()
            .chain(self.automatic_links.keys())
            .any(|pid| {
                self.link_for(*pid)
                    .is_some_and(|link| link.native_session_id == id)
            })
        {
            return false;
        }
        if self.root_hook_activity_at.contains_key(id) {
            return true;
        }
        if session.id.starts_with("codex-app-server:")
            && valid_thread_id(id)
            && !self.editors.contains_key(id)
        {
            // A rollout's creation origin is not proof of a current editor.
            return false;
        }
        // Loaded daemon threads are not proof of an open CLI. Creation-origin
        // VS Code metadata is ignored once this observation was ambiguous.
        session.source != SessionSource::Cli && !self.pending.contains_key(id)
    }

    pub fn current(&self, pid: u32) -> Option<&ProcessIdentity> {
        self.live.get(&pid)
    }

    pub fn observe_snapshot(&mut self, sessions: &[AgentSession], now: i64) {
        if !sessions.iter().any(can_choose_conversation) {
            return;
        }
        for session in sessions.iter().filter(|session| {
            external_codex(session)
                && session.process_id.is_none()
                && session.id.starts_with("codex-app-server:")
        }) {
            if let Some(id) = session
                .native_session_id
                .as_deref()
                .filter(|id| valid_thread_id(id))
            {
                if !self.editors.contains_key(id) {
                    bounded_observation(&mut self.pending, id, now);
                }
            }
        }
    }

    pub fn observed_at(&self, id: &str) -> i64 {
        self.pending.get(id).copied().unwrap_or_default()
    }

    pub fn link_for(&self, pid: u32) -> Option<&DisplayLink> {
        self.automatic_links
            .get(&pid)
            .or_else(|| self.links.get(&pid))
            .filter(|link| {
                self.live
                    .get(&pid)
                    .is_some_and(|process| link.process.matches(process))
            })
    }

    pub fn has_live_links(&self) -> bool {
        self.links
            .keys()
            .chain(self.automatic_links.keys())
            .any(|pid| self.link_for(*pid).is_some())
    }

    pub fn replace_automatic_links(&mut self, links: Vec<DisplayLink>, now: i64) -> bool {
        let old = self
            .automatic_links
            .iter()
            .map(|(pid, link)| (*pid, link.key(), link.native_session_id.clone()))
            .collect::<std::collections::HashSet<_>>();
        let links = links
            .into_iter()
            .filter(|link| {
                valid_thread_id(&link.native_session_id)
                    && self
                        .live
                        .get(&link.process.pid)
                        .is_some_and(|current| link.process.matches(current))
            })
            .collect::<Vec<_>>();
        let mut threads_by_pid = HashMap::<u32, HashSet<String>>::new();
        let mut pids_by_thread = HashMap::<String, HashSet<u32>>::new();
        // Check every valid claim before applying the limit or deduplicating.
        // A valid manual link participates too: automatic discovery must not
        // silently replace it or claim its conversation for another process.
        for link in links.iter().chain(self.links.values().filter(|link| {
            valid_thread_id(&link.native_session_id)
                && self
                    .live
                    .get(&link.process.pid)
                    .is_some_and(|current| link.process.matches(current))
        })) {
            threads_by_pid
                .entry(link.process.pid)
                .or_default()
                .insert(link.native_session_id.clone());
            pids_by_thread
                .entry(link.native_session_id.clone())
                .or_default()
                .insert(link.process.pid);
        }
        self.automatic_links.clear();
        for link in links {
            if threads_by_pid
                .get(&link.process.pid)
                .is_some_and(|threads| threads.len() == 1)
                && pids_by_thread
                    .get(&link.native_session_id)
                    .is_some_and(|pids| pids.len() == 1)
                && self.automatic_links.len() < MAX_LINKS
            {
                self.automatic_links.entry(link.process.pid).or_insert(link);
            }
        }
        for link in self.automatic_links.values() {
            bounded_observation(&mut self.pending, &link.native_session_id, now);
        }
        let next = self
            .automatic_links
            .iter()
            .map(|(pid, link)| (*pid, link.key(), link.native_session_id.clone()))
            .collect::<std::collections::HashSet<_>>();
        old != next
    }

    pub fn available(&self, id: &str, pid: u32, sessions: &[AgentSession]) -> bool {
        valid_thread_id(id)
            && !self
                .links
                .values()
                .chain(self.automatic_links.values())
                .any(|link| link.process.pid != pid && link.native_session_id == id)
            && !sessions.iter().any(|session| {
                session.agent == AgentKind::Codex
                    && session.native_session_id.as_deref() == Some(id)
                    && (session.control_origin == SessionControlOrigin::Lume
                        || session.process_id.is_some_and(|other| other != pid))
            })
    }

    pub fn prepare_link(
        &self,
        process: ProcessIdentity,
        id: &str,
        sessions: &[AgentSession],
    ) -> Result<DisplayLink, String> {
        if !self
            .current(process.pid)
            .is_some_and(|current| process.matches(current))
        {
            return Err("Esta CLI foi fechada ou reiniciada. Atualize a lista.".into());
        }
        if !self.available(id, process.pid, sessions) {
            return Err("Esta conversa já está vinculada ou controlada por outra sessão.".into());
        }
        if self.links.len() >= MAX_LINKS && !self.links.contains_key(&process.pid) {
            return Err("Limite de vínculos de CLI atingido.".into());
        }
        Ok(DisplayLink {
            process,
            native_session_id: id.to_owned(),
        })
    }

    pub fn insert(&mut self, link: DisplayLink, now: i64) {
        bounded_observation(&mut self.pending, &link.native_session_id, now);
        self.links.insert(link.process.pid, link);
    }

    /// Project only read-only information onto the CLI card. The raw sessions
    /// and their control capabilities stay untouched, including native ID.
    pub fn project(
        &self,
        sessions: Vec<AgentSession>,
        names: &HashMap<String, String>,
    ) -> Vec<AgentSession> {
        let native = sessions
            .iter()
            .filter_map(|session| session.native_session_id.as_deref().map(|id| (id, session)))
            .collect::<HashMap<_, _>>();
        let mut projected = sessions
            .iter()
            .filter(|session| can_choose_conversation(session))
            .map(|session| {
                let mut card = session.clone();
                let Some(link) = session.process_id.and_then(|pid| self.link_for(pid)) else {
                    card.session_name = "CLI não identificada".into();
                    return (card.id.clone(), card);
                };
                if let Some(source) = native.get(link.native_session_id.as_str()) {
                    card.activities = source.activities.clone();
                    card.results = source.results.clone();
                    card.prompt_token_usage = source.prompt_token_usage.clone();
                    card.last_response = source.last_response.clone();
                    // Do not turn an old rollout's interrupted/running state into
                    // the status of a newly opened CLI.
                    let born = (link.process.started_at as i64).saturating_mul(1000);
                    let fresh = source
                        .activities
                        .iter()
                        .any(|activity| activity.created_at >= born)
                        || self
                            .live_hook_at
                            .get(&link.native_session_id)
                            .is_some_and(|time| *time >= born);
                    if fresh {
                        card.status = source.status.clone();
                        card.status_label = source.status_label.clone();
                        card.updated_at = source.updated_at;
                    }
                }
                card.session_name = names
                    .get(&link.native_session_id)
                    .cloned()
                    .or_else(|| {
                        native
                            .get(link.native_session_id.as_str())
                            .map(|s| s.session_name.clone())
                            .filter(|s| !s.trim().is_empty())
                    })
                    .unwrap_or_else(|| "Conversa vinculada".into());
                (card.id.clone(), card)
            })
            .collect::<HashMap<_, _>>();
        sessions
            .into_iter()
            .filter(|session| self.visible(session))
            .map(|session| projected.remove(&session.id).unwrap_or(session))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const THREAD_A: &str = "aaaaaaaa-1234-5678-9abc-123456789abc";
    const THREAD_B: &str = "bbbbbbbb-1234-5678-9abc-123456789abc";
    const THREAD_C: &str = "cccccccc-1234-5678-9abc-123456789abc";

    fn process(pid: u32) -> ProcessIdentity {
        ProcessIdentity {
            pid,
            started_at: 1_000,
            start_marker: Some(u64::from(pid)),
            boot_marker: "test-boot".into(),
        }
    }

    fn link(pid: u32, thread: &str) -> DisplayLink {
        DisplayLink {
            process: process(pid),
            native_session_id: thread.into(),
        }
    }

    fn identities(pids: &[u32]) -> CliIdentities {
        let mut identities = CliIdentities::default();
        identities.reconcile(pids.iter().copied().map(process).collect(), 1);
        identities
    }

    fn conversation(id: &str, thread: &str) -> AgentSession {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "agent": "codex",
            "agentLabel": "Codex",
            "sessionName": "A conversa real",
            "project": "project-original",
            "source": "cli",
            "controlOrigin": "external",
            "status": "running",
            "statusLabel": "Trabalhando",
            "startedAt": "2026-10-01T00:00:00Z",
            "updatedAt": 1,
            "nativeSessionId": thread,
            "workingDirectory": "/work/project-original",
            "permissionProfile": {
                "mode": "read_only", "label": "Monitoramento",
                "approvalPolicy": "never", "canRespondFromLume": false,
                "availableActions": []
            }
        }))
        .expect("conversation")
    }

    fn root_hook(thread: &str, kind: HookEventKind) -> HookEvent {
        let mut hook: HookEvent = serde_json::from_value(serde_json::json!({
            "event": "running", "sessionId": format!("codex:{thread}"),
            "agent": "codex", "source": "cli", "controlOrigin": "external",
            "nativeSessionId": thread
        }))
        .expect("root hook");
        hook.event = kind;
        hook
    }

    fn prompt_hook(thread: &str, created_at: i64) -> HookEvent {
        let mut hook = root_hook(thread, HookEventKind::Running);
        hook.activity = Some(
            serde_json::from_value(serde_json::json!({
                "id": format!("codex:{thread}:prompt:{created_at}"), "kind": "prompt",
                "title": "Prompt enviado", "status": "completed", "createdAt": created_at
            }))
            .expect("prompt activity"),
        );
        hook
    }

    #[test]
    fn root_prompt_restores_monitoring_without_claiming_a_cli_or_changing_ownership() {
        let mut identities = identities(&[101]);
        let conversation = conversation(&format!("codex-app-server:{THREAD_A}"), THREAD_A);
        assert!(!identities.visible(&conversation));
        let mut cli = conversation.clone();
        cli.id = "process:codex:101".into();
        cli.process_id = Some(101);
        cli.native_session_id = None;
        let sessions = vec![cli, conversation.clone()];
        identities.observe(&root_hook(THREAD_A, HookEventKind::Running), &sessions, 2);

        let projected = identities.project(sessions, &HashMap::new());
        let monitor = projected.iter().find(|s| s.id == conversation.id).unwrap();
        assert_eq!(
            serde_json::to_value(monitor).unwrap(),
            serde_json::to_value(conversation).unwrap()
        );
        let cli = projected
            .iter()
            .find(|s| s.process_id == Some(101))
            .unwrap();
        assert_eq!(cli.session_name, "CLI não identificada");
        assert!(cli.native_session_id.is_none());
        assert!(identities.link_for(101).is_none());
        assert_eq!(projected.len(), 2);
    }

    #[test]
    fn loaded_daemon_metadata_and_standalone_session_starts_do_not_restore_monitoring() {
        let mut identities = identities(&[101]);
        let conversation = conversation(&format!("codex-app-server:{THREAD_A}"), THREAD_A);
        let mut cli = conversation.clone();
        cli.process_id = Some(101);
        cli.native_session_id = None;
        identities.observe_snapshot(&[cli, conversation.clone()], 2);
        let mut loaded = root_hook(THREAD_A, HookEventKind::Running);
        loaded.session_id = conversation.id.clone();
        identities.observe(&loaded, &[], 3);
        identities.observe(&root_hook(THREAD_A, HookEventKind::SessionStarted), &[], 4);
        assert!(!identities.visible(&conversation));
    }

    #[test]
    fn session_end_revokes_monitoring_and_compaction_cannot_resurrect_it() {
        let mut identities = CliIdentities::default();
        let conversation = conversation(&format!("codex:{THREAD_A}"), THREAD_A);
        identities.observe(&root_hook(THREAD_A, HookEventKind::Running), &[], 1);
        assert!(identities.visible(&conversation));
        let mut ended = root_hook(THREAD_A, HookEventKind::SessionEnded);
        // A later hook may have acquired a proven PID; it must still close the
        // old monitor observation without changing the physical CLI identity.
        ended.process_id = Some(101);
        identities.observe(&ended, &[], 2);
        assert!(!identities.visible(&conversation));
        identities.observe(&root_hook(THREAD_A, HookEventKind::SessionStarted), &[], 3);
        assert!(!identities.visible(&conversation));
        identities.observe(&root_hook(THREAD_A, HookEventKind::Completed), &[], 4);
        identities.observe(&root_hook(THREAD_A, HookEventKind::Activity), &[], 5);
        identities.observe(&root_hook(THREAD_A, HookEventKind::Running), &[], 6);
        assert!(!identities.visible(&conversation));
        let mut changed_source = conversation.clone();
        changed_source.source = SessionSource::Web;
        assert!(
            !identities.visible(&changed_source),
            "source metadata cannot evade an end"
        );
        identities.observe(&prompt_hook(THREAD_A, 1), &[], 7);
        assert!(!identities.visible(&conversation));
        let mut prompt = prompt_hook(THREAD_A, 8);
        prompt.activities = prompt.activity.take().into_iter().collect();
        identities.observe(&prompt, &[], 8);
        assert!(identities.visible(&conversation));
    }

    #[test]
    fn trusted_persisted_prompt_must_be_newer_than_the_root_end() {
        let mut identities = CliIdentities::default();
        let conversation = conversation(&format!("codex-app-server:{THREAD_A}"), THREAD_A);
        identities.observe(&root_hook(THREAD_A, HookEventKind::SessionEnded), &[], 2);
        identities.restore_root_hook_visibility(THREAD_A, 1, 3);
        assert!(!identities.visible(&conversation));
        identities.restore_root_hook_visibility(THREAD_A, 2, 3);
        assert!(!identities.visible(&conversation));
        identities.restore_root_hook_visibility(THREAD_A, 3, 3);
        assert!(identities.has_root_cli_monitor(THREAD_A));
        assert!(identities.visible(&conversation));
    }

    #[test]
    fn positive_editor_hooks_do_not_become_root_cli_monitor_proof() {
        let mut identities = CliIdentities::default();
        let mut hook = root_hook(THREAD_A, HookEventKind::Running);
        hook.source = Some(SessionSource::Vscode);
        identities.observe(&hook, &[], 1);
        assert!(!identities.has_root_cli_monitor(THREAD_A));
        let mut conversation = conversation(&format!("codex-app-server:{THREAD_A}"), THREAD_A);
        conversation.source = SessionSource::Vscode;
        assert!(identities.visible(&conversation));
    }

    #[test]
    fn newly_identified_root_prompt_clears_the_old_end_without_claiming_a_monitor_pid() {
        let mut identities = CliIdentities::default();
        identities.observe(&root_hook(THREAD_A, HookEventKind::SessionEnded), &[], 1);
        let mut prompt = prompt_hook(THREAD_A, 2);
        prompt.process_id = Some(101);
        identities.observe(&prompt, &[], 2);
        assert!(!identities.ended_root_hooks.contains_key(THREAD_A));
        assert!(!identities.has_root_cli_monitor(THREAD_A));
        assert!(identities.current(101).is_none());
        identities.observe(&root_hook(THREAD_A, HookEventKind::Completed), &[], 3);
        assert!(identities.has_root_cli_monitor(THREAD_A));
    }

    #[test]
    fn confirmed_link_deduplicates_a_visible_root_conversation_without_granting_ownership() {
        let mut identities = identities(&[101]);
        let conversation = conversation(&format!("codex-app-server:{THREAD_A}"), THREAD_A);
        let mut cli = conversation.clone();
        cli.id = "process:codex:101".into();
        cli.process_id = Some(101);
        cli.native_session_id = None;
        identities.observe(&root_hook(THREAD_A, HookEventKind::Running), &[], 2);
        identities.insert(link(101, THREAD_A), 3);
        let projected = identities.project(vec![cli, conversation], &HashMap::new());
        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0].session_name, "A conversa real");
        assert_eq!(projected[0].process_id, Some(101));
        assert_eq!(projected[0].control_origin, SessionControlOrigin::External);
        assert!(projected[0].native_session_id.is_none());
        assert!(!projected[0].permission_profile.can_respond_from_lume);
    }

    #[test]
    fn persisted_root_hook_visibility_is_bounded_by_age_and_thread_identity() {
        let mut identities = CliIdentities::default();
        let conversation = conversation(&format!("codex-app-server:{THREAD_A}"), THREAD_A);
        let now = OBSERVATION_TTL_MS + 10;
        identities.restore_root_hook_visibility(THREAD_A, 1, now);
        assert!(!identities.visible(&conversation));
        identities.restore_root_hook_visibility("not-a-thread", now, now);
        assert!(identities.root_hook_activity_at.is_empty());
        identities.restore_root_hook_visibility(THREAD_A, now + 1, now);
        assert!(!identities.visible(&conversation));
        identities.restore_root_hook_visibility(THREAD_A, now, now);
        assert!(identities.visible(&conversation));
        identities.reconcile(Vec::new(), now + OBSERVATION_TTL_MS + 1);
        assert!(!identities.visible(&conversation));
    }

    #[test]
    fn automatic_links_reject_every_conflicting_claim_regardless_of_order() {
        for claims in [
            vec![link(101, THREAD_A), link(101, THREAD_B)],
            vec![link(101, THREAD_A), link(102, THREAD_A)],
            vec![
                link(101, THREAD_A),
                link(101, THREAD_B),
                link(102, THREAD_B),
            ],
        ] {
            for reverse in [false, true] {
                let mut identities = identities(&[101, 102, 103]);
                let mut claims = claims.clone();
                if reverse {
                    claims.reverse();
                }
                claims.push(link(103, THREAD_C));
                assert!(identities.replace_automatic_links(claims, 2));
                assert_eq!(identities.automatic_links.len(), 1);
                assert!(identities.link_for(101).is_none());
                assert!(identities.link_for(102).is_none());
                assert_eq!(
                    identities.link_for(103).unwrap().native_session_id,
                    THREAD_C
                );
            }
        }
    }

    #[test]
    fn automatic_conflict_removes_the_previous_automatic_link() {
        let mut identities = identities(&[101, 102]);
        assert!(identities.replace_automatic_links(vec![link(101, THREAD_A)], 2));
        assert!(
            identities.replace_automatic_links(vec![link(101, THREAD_A), link(102, THREAD_A)], 3,)
        );
        assert!(identities.automatic_links.is_empty());
    }

    #[test]
    fn duplicate_identical_automatic_claims_do_not_conflict_or_consume_the_limit() {
        let mut identities = identities(&[101, 102]);
        let mut claims = vec![link(101, THREAD_A); MAX_LINKS + 1];
        claims.push(link(102, THREAD_B));
        assert!(identities.replace_automatic_links(claims.clone(), 2));
        assert_eq!(identities.automatic_links.len(), 2);
        claims.reverse();
        assert!(!identities.replace_automatic_links(claims, 3));
    }

    #[test]
    fn automatic_claims_cannot_override_or_steal_a_valid_manual_link() {
        for claims in [
            vec![link(101, THREAD_B)],
            vec![link(102, THREAD_A)],
            vec![link(101, THREAD_A), link(102, THREAD_A)],
        ] {
            let mut identities = identities(&[101, 102, 103]);
            identities.insert(link(101, THREAD_A), 1);
            let mut claims = claims;
            claims.push(link(103, THREAD_C));
            assert!(identities.replace_automatic_links(claims, 2));
            assert_eq!(identities.automatic_links.len(), 1);
            assert_eq!(
                identities.link_for(101).unwrap().native_session_id,
                THREAD_A
            );
            assert!(identities.link_for(102).is_none());
            assert_eq!(identities.links.len(), 1);
        }
    }

    #[test]
    fn an_identical_manual_and_automatic_link_is_not_a_conflict() {
        let mut identities = identities(&[101]);
        identities.insert(link(101, THREAD_A), 1);
        assert!(identities.replace_automatic_links(vec![link(101, THREAD_A)], 2));
        assert_eq!(identities.automatic_links.len(), 1);
        assert_eq!(identities.links.len(), 1);
        assert_eq!(
            identities.link_for(101).unwrap().native_session_id,
            THREAD_A
        );
    }

    #[test]
    fn invalid_and_stale_claims_do_not_block_a_current_automatic_link() {
        let mut identities = identities(&[101, 102]);
        let mut stale_manual = link(101, THREAD_B);
        stale_manual.process.start_marker = Some(999);
        identities.insert(stale_manual, 1);
        let mut stale_automatic = link(102, THREAD_A);
        stale_automatic.process.boot_marker = "previous-boot".into();
        assert!(identities.replace_automatic_links(
            vec![
                link(101, THREAD_A),
                link(101, "invalid-thread"),
                stale_automatic,
            ],
            2,
        ));
        assert_eq!(identities.automatic_links.len(), 1);
        assert_eq!(
            identities.link_for(101).unwrap().native_session_id,
            THREAD_A
        );
        assert!(identities.link_for(102).is_none());
    }

    #[test]
    fn automatic_conflicts_after_the_limit_are_still_rejected() {
        let mut identities = identities(&[101, 102]);
        let mut claims = vec![link(101, THREAD_A); MAX_LINKS];
        claims.push(link(102, THREAD_A));
        assert!(!identities.replace_automatic_links(claims, 2));
        assert!(identities.automatic_links.is_empty());
    }
}
