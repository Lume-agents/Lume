use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::{
    browser_server::BrowserControl,
    codex_bridge::CodexBridge,
    context_builder, control,
    domain::{
        AgentSession, PromptDelivery, SessionStatus, WorkflowAdvanceMode,
        WorkflowConnectionDefinition, WorkflowGroupDefinition, WorkflowHistoryEvent,
        WorkflowHistoryEventKind, WorkflowHistoryRecord, WorkflowQuestion, WorkflowQuestionRoute,
        WorkflowRole, WorkflowRun, WorkflowRunStatus, WorkflowSettings, WorkflowStepDefinition,
        WorkflowStepHistory, WorkflowStepRun, WorkflowStepRunStatus,
    },
    state::{now_millis, AppState},
};

const WORKFLOW_AGENT_LOSS_GRACE_MS: i64 = 5_000;
const WORKFLOW_STARTUP_RECOVERY_GRACE_MS: i64 = 30_000;
const WORKFLOW_ACTIVITY_LIMIT: usize = 200;

#[derive(Clone, Deserialize, Serialize)]
struct ActiveWorkflowRun {
    run: WorkflowRun,
    group: WorkflowGroupDefinition,
    prompts: HashMap<String, String>,
    result_baselines: HashMap<String, HashSet<String>>,
    #[serde(default)]
    unavailable_since: HashMap<String, i64>,
    #[serde(default)]
    paused_from: Option<WorkflowRunStatus>,
    #[serde(default)]
    history_events: Vec<WorkflowHistoryEvent>,
    #[serde(default)]
    step_history: HashMap<String, WorkflowStepHistory>,
    #[serde(skip)]
    recovery_deadline: Option<i64>,
}

#[derive(Clone, Default)]
pub struct WorkflowRuntime {
    runs: Arc<Mutex<HashMap<String, ActiveWorkflowRun>>>,
}

impl WorkflowRuntime {
    pub fn restore(&self, state: &AppState) -> Result<usize, String> {
        let recovery_deadline = now_millis() + WORKFLOW_STARTUP_RECOVERY_GRACE_MS;
        let restored = state
            .workflow_runs()?
            .into_iter()
            .filter_map(|payload| serde_json::from_str::<ActiveWorkflowRun>(&payload).ok())
            .map(|mut active| {
                let recovering = matches!(
                    active.run.status,
                    WorkflowRunStatus::Running | WorkflowRunStatus::Paused
                ) && active
                    .run
                    .current_step_id
                    .as_deref()
                    .and_then(|step_id| {
                        active.run.steps.iter().find(|step| step.step_id == step_id)
                    })
                    .is_some_and(|step| step.status == WorkflowStepRunStatus::Running);
                if recovering {
                    if let Some(step_id) = active.run.current_step_id.as_ref() {
                        active.unavailable_since.remove(step_id);
                    }
                    active.recovery_deadline = Some(recovery_deadline);
                    active.run.recovering = true;
                }
                (active.run.workflow_id.clone(), active)
            })
            .collect::<HashMap<_, _>>();
        let count = restored.len();
        for active in restored.values() {
            persist_public_history(state, active)?;
        }
        *self
            .runs
            .lock()
            .map_err(|_| "Could not restore workflow runs".to_string())? = restored;
        Ok(count)
    }

    pub fn start_monitor(
        &self,
        state: AppState,
        app: AppHandle,
        bridge: CodexBridge,
        browser: BrowserControl,
    ) {
        let runtime = self.clone();
        std::thread::spawn(move || loop {
            let workflow_ids = runtime.monitored_workflow_ids();
            for workflow_id in &workflow_ids {
                let before = runtime
                    .snapshot(workflow_id)
                    .ok()
                    .map(|active| active.run.updated_at);
                if let Ok(Some(run)) = runtime.reconcile(&state, workflow_id) {
                    if before.is_some_and(|updated_at| updated_at != run.updated_at) {
                        emit_changed(&app, &run);
                    }
                    if run.status == WorkflowRunStatus::Ready
                        && runtime.should_advance_automatically(workflow_id)
                    {
                        let _ = runtime.advance(&app, &state, &bridge, &browser, workflow_id);
                    }
                }
            }
            std::thread::sleep(if workflow_ids.is_empty() {
                Duration::from_millis(1_200)
            } else {
                Duration::from_millis(500)
            });
        });
    }

    pub fn get(&self, state: &AppState, workflow_id: &str) -> Result<Option<WorkflowRun>, String> {
        self.reconcile(state, workflow_id)
    }

    pub fn start(
        &self,
        app: &AppHandle,
        state: &AppState,
        bridge: &CodexBridge,
        browser: &BrowserControl,
        group: WorkflowGroupDefinition,
        objective: &str,
    ) -> Result<WorkflowRun, String> {
        let objective = objective.trim();
        if objective.is_empty() {
            return Err("Add an objective before running this workflow".into());
        }
        if objective.len() > 4_000 {
            return Err("The workflow objective cannot exceed 4000 characters".into());
        }
        let order = validate_manual_workflow(&group)?;
        {
            let runs = self
                .runs
                .lock()
                .map_err(|_| "Could not access workflow runs".to_string())?;
            if runs.get(&group.id).is_some_and(|active| {
                !matches!(
                    active.run.status,
                    WorkflowRunStatus::Completed
                        | WorkflowRunStatus::Failed
                        | WorkflowRunStatus::Cancelled
                )
            }) {
                return Err("This workflow already has an active run".into());
            }
        }

        let sessions = state.bounded_sessions(WORKFLOW_ACTIVITY_LIMIT)?;
        let settings = state.preferences()?.workflow_settings;
        let first_step = workflow_step(&group, &order[0])?;
        let session = available_session_for_step(&sessions, first_step)?;
        ensure_rate_limit_reserve(session, &settings)?;
        let prompt = initial_step_prompt(&group, first_step, objective);
        let baseline = result_ids(session);
        submit_workflow_prompt(app, state, bridge, browser, session, &prompt)?;

        let now = now_millis();
        let steps = order
            .iter()
            .map(|step_id| WorkflowStepRun {
                step_id: step_id.clone(),
                status: if step_id == &first_step.id {
                    WorkflowStepRunStatus::Running
                } else {
                    WorkflowStepRunStatus::Pending
                },
                attempt: u16::from(step_id == &first_step.id),
                started_at: (step_id == &first_step.id).then_some(now),
                completed_at: None,
                result_id: None,
                error: None,
            })
            .collect::<Vec<_>>();
        let run = WorkflowRun {
            id: format!("workflow-run:{}:{now}", group.id),
            workflow_id: group.id.clone(),
            objective: objective.to_string(),
            status: WorkflowRunStatus::Running,
            current_step_id: Some(first_step.id.clone()),
            pending_connection_id: None,
            handoff_approved: false,
            recovering: false,
            transition_count: 0,
            steps,
            error: None,
            question: None,
            created_at: now,
            updated_at: now,
        };
        let mut prompts = HashMap::new();
        prompts.insert(first_step.id.clone(), prompt);
        let mut result_baselines = HashMap::new();
        result_baselines.insert(first_step.id.clone(), baseline);
        let step_history = group
            .steps
            .iter()
            .map(|step| {
                let session = sessions.iter().find(|session| {
                    session.native_session_id.as_deref() == Some(step.session_native_id.as_str())
                });
                (
                    step.id.clone(),
                    WorkflowStepHistory {
                        step_id: step.id.clone(),
                        session_native_id: step.session_native_id.clone(),
                        role_label: role_label(step),
                        agent_label: session
                            .map(|session| session.agent_label.clone())
                            .unwrap_or_default(),
                        session_name: session
                            .map(|session| session.session_name.clone())
                            .unwrap_or_default(),
                        project: session
                            .map(|session| session.project.clone())
                            .unwrap_or_default(),
                        ..WorkflowStepHistory::default()
                    },
                )
            })
            .collect();
        let history_events = vec![
            history_event(
                WorkflowHistoryEventKind::Started,
                None,
                None,
                "Workflow started",
                now,
            ),
            history_event(
                WorkflowHistoryEventKind::StepStarted,
                Some(&first_step.id),
                None,
                &format!("{} started", role_label(first_step)),
                now,
            ),
        ];
        let active = ActiveWorkflowRun {
            run: run.clone(),
            group,
            prompts,
            result_baselines,
            unavailable_since: HashMap::new(),
            paused_from: None,
            history_events,
            step_history,
            recovery_deadline: None,
        };
        persist_active(state, &active)?;
        self.runs
            .lock()
            .map_err(|_| "Could not save the workflow run".to_string())?
            .insert(active.run.workflow_id.clone(), active);
        emit_changed(app, &run);
        Ok(run)
    }

    pub fn approve(
        &self,
        app: &AppHandle,
        state: &AppState,
        workflow_id: &str,
    ) -> Result<WorkflowRun, String> {
        self.reconcile(state, workflow_id)?;
        let mut runs = self
            .runs
            .lock()
            .map_err(|_| "Could not access workflow runs".to_string())?;
        let active = runs
            .get_mut(workflow_id)
            .ok_or_else(|| "Workflow run not found".to_string())?;
        if active.run.status != WorkflowRunStatus::WaitingForApproval {
            return Err("This workflow is not waiting for approval".into());
        }
        active.run.handoff_approved = true;
        active.run.status = WorkflowRunStatus::Ready;
        active.run.error = None;
        active.run.updated_at = now_millis();
        let connection_id = active.run.pending_connection_id.clone();
        record_history_event(
            active,
            WorkflowHistoryEventKind::HandoffApproved,
            active.run.current_step_id.clone().as_deref(),
            connection_id.as_deref(),
            "Handoff approved",
            active.run.updated_at,
        );
        persist_active(state, active)?;
        let run = active.run.clone();
        drop(runs);
        emit_changed(app, &run);
        Ok(run)
    }

    pub fn advance(
        &self,
        app: &AppHandle,
        state: &AppState,
        bridge: &CodexBridge,
        browser: &BrowserControl,
        workflow_id: &str,
    ) -> Result<WorkflowRun, String> {
        self.reconcile(state, workflow_id)?;
        let snapshot = self.snapshot(workflow_id)?;
        if snapshot.run.status != WorkflowRunStatus::Ready {
            return Err("The current workflow step is not ready to advance".into());
        }
        if snapshot.run.question.as_ref().is_some_and(|question| {
            matches!(
                question.route,
                WorkflowQuestionRoute::ToPlanner | WorkflowQuestionRoute::ToOrigin
            )
        }) {
            return self.route_question(app, state, bridge, browser, &snapshot);
        }
        let settings = state.preferences()?.workflow_settings;
        if snapshot.run.transition_count >= settings.max_transitions {
            return self.pause_for_guardrail(
                app,
                state,
                workflow_id,
                "Workflow paused after reaching the transition limit",
            );
        }
        let connection_id = snapshot
            .run
            .pending_connection_id
            .as_deref()
            .ok_or_else(|| "The workflow has no pending handoff".to_string())?;
        let connection = snapshot
            .group
            .connections
            .iter()
            .find(|connection| connection.id == connection_id)
            .ok_or_else(|| "The pending workflow connection no longer exists".to_string())?;
        if connection.requires_approval && !snapshot.run.handoff_approved {
            return Err("Approve this handoff before running the next step".into());
        }
        let source_run = snapshot
            .run
            .steps
            .iter()
            .find(|step| step.step_id == connection.from_step_id)
            .ok_or_else(|| "The source workflow step is missing".to_string())?;
        let prompt = if source_run.status == WorkflowStepRunStatus::Skipped {
            skipped_step_prompt(&snapshot.group, connection, &snapshot.run.objective)?
        } else {
            context_builder::build_context_package_with_limit(
                &snapshot.group,
                connection_id,
                &snapshot.run.objective,
                source_run.result_id.as_deref(),
                &state.bounded_sessions(WORKFLOW_ACTIVITY_LIMIT)?,
                settings.max_context_tokens as usize,
            )?
            .markdown
        };
        let target = workflow_step(&snapshot.group, &connection.to_step_id)?;
        let prompt = format!("{prompt}{}", role_rules(&snapshot.group, target));
        let sessions = state.bounded_sessions(WORKFLOW_ACTIVITY_LIMIT)?;
        let session = available_session_for_step(&sessions, target)?;
        if let Err(error) = ensure_rate_limit_reserve(session, &settings) {
            return self.pause_for_guardrail(app, state, workflow_id, &error);
        }
        let baseline = result_ids(session);
        submit_workflow_prompt(app, state, bridge, browser, session, &prompt)?;

        let mut runs = self
            .runs
            .lock()
            .map_err(|_| "Could not update the workflow run".to_string())?;
        let active = runs
            .get_mut(workflow_id)
            .filter(|active| active.run.id == snapshot.run.id)
            .ok_or_else(|| "The workflow run changed while advancing".to_string())?;
        let now = now_millis();
        let target_run = active
            .run
            .steps
            .iter_mut()
            .find(|step| step.step_id == target.id)
            .ok_or_else(|| "The target workflow step is missing".to_string())?;
        target_run.status = WorkflowStepRunStatus::Running;
        target_run.attempt = target_run.attempt.saturating_add(1);
        target_run.started_at = Some(now);
        target_run.completed_at = None;
        target_run.result_id = None;
        target_run.error = None;
        active.prompts.insert(target.id.clone(), prompt);
        active.result_baselines.insert(target.id.clone(), baseline);
        active.run.current_step_id = Some(target.id.clone());
        active.run.pending_connection_id = None;
        active.run.handoff_approved = false;
        active.run.transition_count = active.run.transition_count.saturating_add(1);
        active.run.status = WorkflowRunStatus::Running;
        active.run.error = None;
        active.run.updated_at = now;
        record_history_event(
            active,
            WorkflowHistoryEventKind::StepStarted,
            Some(&target.id),
            Some(&connection.id),
            &format!("{} started", role_label(target)),
            now,
        );
        persist_active(state, active)?;
        let run = active.run.clone();
        drop(runs);
        emit_changed(app, &run);
        Ok(run)
    }

    pub fn pause(
        &self,
        app: &AppHandle,
        state: &AppState,
        workflow_id: &str,
    ) -> Result<WorkflowRun, String> {
        let mut runs = self
            .runs
            .lock()
            .map_err(|_| "Could not access workflow runs".to_string())?;
        let active = runs
            .get_mut(workflow_id)
            .ok_or_else(|| "Workflow run not found".to_string())?;
        if !matches!(
            active.run.status,
            WorkflowRunStatus::Running
                | WorkflowRunStatus::Ready
                | WorkflowRunStatus::WaitingForApproval
        ) {
            return Err("This workflow cannot be paused in its current state".into());
        }
        active.paused_from = Some(active.run.status);
        active.run.status = WorkflowRunStatus::Paused;
        active.run.updated_at = now_millis();
        let current_step_id = active.run.current_step_id.clone();
        record_history_event(
            active,
            WorkflowHistoryEventKind::Paused,
            current_step_id.as_deref(),
            None,
            "Workflow paused",
            active.run.updated_at,
        );
        persist_active(state, active)?;
        let run = active.run.clone();
        drop(runs);
        emit_changed(app, &run);
        Ok(run)
    }

    pub fn resume(
        &self,
        app: &AppHandle,
        state: &AppState,
        workflow_id: &str,
    ) -> Result<WorkflowRun, String> {
        self.reconcile(state, workflow_id)?;
        let mut runs = self
            .runs
            .lock()
            .map_err(|_| "Could not access workflow runs".to_string())?;
        let active = runs
            .get_mut(workflow_id)
            .ok_or_else(|| "Workflow run not found".to_string())?;
        if active.run.status != WorkflowRunStatus::Paused {
            return Err("This workflow is not paused".into());
        }
        let resumed_status = status_after_pause(active);
        active.run.status = resumed_status;
        active.paused_from = None;
        active.run.error = None;
        active.run.updated_at = now_millis();
        let current_step_id = active.run.current_step_id.clone();
        record_history_event(
            active,
            WorkflowHistoryEventKind::Resumed,
            current_step_id.as_deref(),
            None,
            "Workflow resumed",
            active.run.updated_at,
        );
        persist_active(state, active)?;
        let run = active.run.clone();
        drop(runs);
        emit_changed(app, &run);
        Ok(run)
    }

    pub fn retry(
        &self,
        app: &AppHandle,
        state: &AppState,
        bridge: &CodexBridge,
        browser: &BrowserControl,
        workflow_id: &str,
    ) -> Result<WorkflowRun, String> {
        self.reconcile(state, workflow_id)?;
        let snapshot = self.snapshot(workflow_id)?;
        if snapshot.run.status != WorkflowRunStatus::Failed {
            return Err("Only a failed workflow step can be retried".into());
        }
        let step_id = snapshot
            .run
            .current_step_id
            .as_deref()
            .ok_or_else(|| "The workflow has no current step".to_string())?;
        let step = workflow_step(&snapshot.group, step_id)?;
        let settings = state.preferences()?.workflow_settings;
        let current_attempt = snapshot
            .run
            .steps
            .iter()
            .find(|candidate| candidate.step_id == step_id)
            .map(|candidate| candidate.attempt)
            .unwrap_or_default();
        if current_attempt >= settings.max_attempts_per_step {
            return Err("This workflow step reached its retry limit".into());
        }
        let prompt = snapshot
            .prompts
            .get(step_id)
            .cloned()
            .ok_or_else(|| "The failed step prompt is no longer available".to_string())?;
        let sessions = state.bounded_sessions(WORKFLOW_ACTIVITY_LIMIT)?;
        let session = available_session_for_step(&sessions, step)?;
        ensure_rate_limit_reserve(session, &settings)?;
        let baseline = result_ids(session);
        submit_workflow_prompt(app, state, bridge, browser, session, &prompt)?;

        let mut runs = self
            .runs
            .lock()
            .map_err(|_| "Could not update the workflow run".to_string())?;
        let active = runs
            .get_mut(workflow_id)
            .filter(|active| active.run.id == snapshot.run.id)
            .ok_or_else(|| "The workflow run changed while retrying".to_string())?;
        let now = now_millis();
        let step_run = active
            .run
            .steps
            .iter_mut()
            .find(|candidate| candidate.step_id == step_id)
            .ok_or_else(|| "The failed workflow step is missing".to_string())?;
        step_run.status = WorkflowStepRunStatus::Running;
        step_run.attempt = step_run.attempt.saturating_add(1);
        step_run.started_at = Some(now);
        step_run.completed_at = None;
        step_run.result_id = None;
        step_run.error = None;
        active
            .result_baselines
            .insert(step_id.to_string(), baseline);
        active.run.status = WorkflowRunStatus::Running;
        active.run.error = None;
        active.run.updated_at = now;
        record_history_event(
            active,
            WorkflowHistoryEventKind::StepRetried,
            Some(step_id),
            None,
            &format!("{} retried", role_label(step)),
            now,
        );
        persist_active(state, active)?;
        let run = active.run.clone();
        drop(runs);
        emit_changed(app, &run);
        Ok(run)
    }

    pub fn skip(
        &self,
        app: &AppHandle,
        state: &AppState,
        workflow_id: &str,
    ) -> Result<WorkflowRun, String> {
        self.reconcile(state, workflow_id)?;
        let mut runs = self
            .runs
            .lock()
            .map_err(|_| "Could not access workflow runs".to_string())?;
        let active = runs
            .get_mut(workflow_id)
            .ok_or_else(|| "Workflow run not found".to_string())?;
        if active.run.status != WorkflowRunStatus::Failed {
            return Err("Only a failed workflow step can be skipped".into());
        }
        let step_id = active
            .run
            .current_step_id
            .clone()
            .ok_or_else(|| "The workflow has no current step".to_string())?;
        let now = now_millis();
        let step_run = active
            .run
            .steps
            .iter_mut()
            .find(|step| step.step_id == step_id)
            .ok_or_else(|| "The failed workflow step is missing".to_string())?;
        step_run.status = WorkflowStepRunStatus::Skipped;
        step_run.completed_at = Some(now);
        step_run.error = None;
        active.run.error = None;
        record_history_event(
            active,
            WorkflowHistoryEventKind::StepSkipped,
            Some(&step_id),
            None,
            "Workflow step skipped",
            now,
        );
        set_pending_transition(active, &step_id, now)?;
        persist_active(state, active)?;
        let run = active.run.clone();
        drop(runs);
        emit_changed(app, &run);
        Ok(run)
    }

    pub fn cancel(
        &self,
        app: &AppHandle,
        state: &AppState,
        workflow_id: &str,
    ) -> Result<WorkflowRun, String> {
        let mut runs = self
            .runs
            .lock()
            .map_err(|_| "Could not access workflow runs".to_string())?;
        let active = runs
            .get_mut(workflow_id)
            .ok_or_else(|| "Workflow run not found".to_string())?;
        if matches!(
            active.run.status,
            WorkflowRunStatus::Completed | WorkflowRunStatus::Cancelled
        ) {
            return Err("This workflow has already finished".into());
        }
        active.run.status = WorkflowRunStatus::Cancelled;
        active.run.error = None;
        active.run.updated_at = now_millis();
        let current_step_id = active.run.current_step_id.clone();
        record_history_event(
            active,
            WorkflowHistoryEventKind::Cancelled,
            current_step_id.as_deref(),
            None,
            "Workflow cancelled",
            active.run.updated_at,
        );
        persist_active(state, active)?;
        let run = active.run.clone();
        drop(runs);
        emit_changed(app, &run);
        Ok(run)
    }

    pub fn current_session_id(
        &self,
        state: &AppState,
        workflow_id: &str,
    ) -> Result<Option<String>, String> {
        let snapshot = self.snapshot(workflow_id)?;
        let Some(step_id) = snapshot.run.current_step_id.as_deref() else {
            return Ok(None);
        };
        let step = workflow_step(&snapshot.group, step_id)?;
        Ok(state
            .bounded_sessions(WORKFLOW_ACTIVITY_LIMIT)?
            .into_iter()
            .find(|session| {
                session.native_session_id.as_deref() == Some(step.session_native_id.as_str())
            })
            .map(|session| session.id))
    }

    /// A chat can get its real id when its first prompt starts the thread. Steps that
    /// pointed at the old id follow it, in the saved workflows and in any run.
    pub fn rename_session(&self, app: &AppHandle, state: &AppState, old: &str, new: &str) {
        if old == new || old.trim().is_empty() || new.trim().is_empty() {
            return;
        }
        if let Ok(mut preferences) = state.preferences() {
            let mut changed = false;
            for group in &mut preferences.workflow_groups {
                if group.steps.iter().any(|step| step.session_native_id == new) {
                    continue;
                }
                for step in group
                    .steps
                    .iter_mut()
                    .filter(|step| step.session_native_id == old)
                {
                    step.session_native_id = new.to_string();
                    changed = true;
                }
            }
            if changed && state.save_preferences(&preferences).is_ok() {
                let _ = app.emit("lume://preferences-changed", &preferences);
            }
        }
        let Ok(mut runs) = self.runs.lock() else {
            return;
        };
        for active in runs.values_mut() {
            let mut changed = false;
            for step in active
                .group
                .steps
                .iter_mut()
                .filter(|step| step.session_native_id == old)
            {
                step.session_native_id = new.to_string();
                changed = true;
            }
            for history in active
                .step_history
                .values_mut()
                .filter(|history| history.session_native_id == old)
            {
                history.session_native_id = new.to_string();
                changed = true;
            }
            if changed {
                let _ = persist_active(state, active);
            }
        }
    }

    pub fn rebind_session(
        &self,
        app: &AppHandle,
        state: &AppState,
        workflow_id: &str,
        step_id: &str,
        session_native_id: &str,
    ) -> Result<Option<WorkflowRun>, String> {
        let session_native_id = session_native_id.trim();
        let sessions = state.bounded_sessions(WORKFLOW_ACTIVITY_LIMIT)?;
        let replacement_session = sessions
            .iter()
            .find(|session| session.native_session_id.as_deref() == Some(session_native_id));
        if session_native_id.is_empty() || replacement_session.is_none() {
            return Err("Choose an agent session that is currently connected".into());
        }

        let mut preferences = state.preferences()?;
        let group = preferences
            .workflow_groups
            .iter_mut()
            .find(|group| group.id == workflow_id)
            .ok_or_else(|| "Workflow group not found".to_string())?;
        if group
            .steps
            .iter()
            .any(|step| step.id != step_id && step.session_native_id == session_native_id)
        {
            return Err("This agent session is already used by another workflow step".into());
        }
        let step = group
            .steps
            .iter_mut()
            .find(|step| step.id == step_id)
            .ok_or_else(|| "Workflow step not found".to_string())?;
        step.session_native_id = session_native_id.to_string();
        state.save_preferences(&preferences)?;

        let mut runs = self
            .runs
            .lock()
            .map_err(|_| "Could not access workflow runs".to_string())?;
        let Some(active) = runs.get_mut(workflow_id) else {
            return Ok(None);
        };
        if active
            .group
            .steps
            .iter()
            .any(|step| step.id != step_id && step.session_native_id == session_native_id)
        {
            return Err("This agent session is already used by another workflow step".into());
        }
        let active_step = active
            .group
            .steps
            .iter_mut()
            .find(|step| step.id == step_id)
            .ok_or_else(|| "Workflow step not found".to_string())?;
        active_step.session_native_id = session_native_id.to_string();
        if let Some(replacement_session) = replacement_session {
            active.step_history.insert(
                step_id.to_string(),
                WorkflowStepHistory {
                    step_id: step_id.to_string(),
                    session_native_id: session_native_id.to_string(),
                    role_label: role_label(active_step),
                    agent_label: replacement_session.agent_label.clone(),
                    session_name: replacement_session.session_name.clone(),
                    project: replacement_session.project.clone(),
                    ..WorkflowStepHistory::default()
                },
            );
        }
        active.unavailable_since.remove(step_id);
        active.recovery_deadline = None;
        active.run.recovering = false;
        if active.run.current_step_id.as_deref() == Some(step_id)
            && matches!(
                active.run.status,
                WorkflowRunStatus::Running | WorkflowRunStatus::Paused
            )
        {
            fail_step(
                active,
                step_id,
                "The workflow agent was replaced. Retry this step to continue",
            );
        } else {
            active.run.updated_at = now_millis();
        }
        record_history_event(
            active,
            WorkflowHistoryEventKind::SessionReplaced,
            Some(step_id),
            None,
            "Workflow agent replaced",
            active.run.updated_at,
        );
        persist_active(state, active)?;
        let run = active.run.clone();
        drop(runs);
        emit_changed(app, &run);
        Ok(Some(run))
    }

    /// The user answers the doubt the workflow stopped on. The answer goes to the step that asked them:
    /// the orchestrator, or a step that has no orchestrator above it.
    pub fn answer_question(
        &self,
        app: &AppHandle,
        state: &AppState,
        bridge: &CodexBridge,
        browser: &BrowserControl,
        workflow_id: &str,
        answer: &str,
    ) -> Result<WorkflowRun, String> {
        let answer = answer.trim();
        if answer.is_empty() {
            return Err("Write an answer before sending it".into());
        }
        if answer.len() > 8_000 {
            return Err("The answer cannot exceed 8000 characters".into());
        }
        self.reconcile(state, workflow_id)?;
        let snapshot = self.snapshot(workflow_id)?;
        let question = snapshot
            .run
            .question
            .clone()
            .filter(|question| {
                snapshot.run.status == WorkflowRunStatus::WaitingForUser
                    && question.route == WorkflowQuestionRoute::ToUser
            })
            .ok_or_else(|| "This workflow is not waiting for an answer".to_string())?;
        let target = workflow_step(&snapshot.group, &question.step_id)?;
        let prompt = format!(
            "# Lume workflow\n\nThe user answered your question.\n\n## Your question\n\n{}\n\n## The user's answer\n\n{}\n\n## Objective\n\n{}\n\nContinue your step with this answer.{}",
            question.text.trim(),
            answer,
            snapshot.run.objective.trim(),
            role_rules(&snapshot.group, target),
        );
        let sessions = state.bounded_sessions(WORKFLOW_ACTIVITY_LIMIT)?;
        let session = available_session_for_step(&sessions, target)?;
        let baseline = result_ids(session);
        submit_workflow_prompt(app, state, bridge, browser, session, &prompt)?;

        let mut runs = self
            .runs
            .lock()
            .map_err(|_| "Could not update the workflow run".to_string())?;
        let active = runs
            .get_mut(workflow_id)
            .filter(|active| active.run.id == snapshot.run.id)
            .ok_or_else(|| "The workflow run changed while answering".to_string())?;
        let now = now_millis();
        start_step(active, &target.id, prompt, baseline, now)?;
        // The orchestrator still owes the step that asked first an answer; otherwise the doubt is closed.
        active.run.question = (question.origin_step_id != question.step_id).then(|| WorkflowQuestion {
            route: WorkflowQuestionRoute::ToPlanner,
            ..question
        });
        record_history_event(
            active,
            WorkflowHistoryEventKind::QuestionAnswered,
            Some(&target.id),
            None,
            "The user answered the question",
            now,
        );
        persist_active(state, active)?;
        let run = active.run.clone();
        drop(runs);
        emit_changed(app, &run);
        Ok(run)
    }

    /// Sends a doubt to the orchestrator, or the orchestrator's answer back to the step that asked.
    fn route_question(
        &self,
        app: &AppHandle,
        state: &AppState,
        bridge: &CodexBridge,
        browser: &BrowserControl,
        snapshot: &ActiveWorkflowRun,
    ) -> Result<WorkflowRun, String> {
        let workflow_id = snapshot.run.workflow_id.as_str();
        let settings = state.preferences()?.workflow_settings;
        if snapshot.run.transition_count >= settings.max_transitions {
            return self.pause_for_guardrail(
                app,
                state,
                workflow_id,
                "Workflow paused after reaching the transition limit",
            );
        }
        let question = snapshot
            .run
            .question
            .clone()
            .ok_or_else(|| "The workflow has no question to route".to_string())?;
        let origin = workflow_step(&snapshot.group, &question.origin_step_id)?;
        let (target, prompt) = match question.route {
            WorkflowQuestionRoute::ToPlanner => {
                let planner = workflow_step(&snapshot.group, &question.step_id)?;
                (
                    planner,
                    format!(
                        "# Lume workflow\n\nThe **{}** step has a question and stopped until it is answered.\n\n## Question\n\n{}\n\n## Objective\n\n{}\n\nAnswer it from your plan and the project. Your whole response goes back to the {} as the answer, so write only the answer and what it should do next. If only the user can decide, end your response with `<lume-question>your question to the user</lume-question>` instead.{}",
                        role_label(origin),
                        question.text.trim(),
                        snapshot.run.objective.trim(),
                        role_label(origin),
                        role_rules(&snapshot.group, planner),
                    ),
                )
            }
            WorkflowQuestionRoute::ToOrigin => {
                let planner = workflow_step(&snapshot.group, &question.step_id)?;
                (
                    origin,
                    format!(
                        "# Lume workflow\n\nThe **{}** answered your question.\n\n## Your question\n\n{}\n\n## Answer\n\n{}\n\nContinue your step with this answer.{}",
                        role_label(planner),
                        question.text.trim(),
                        question.answer.as_deref().unwrap_or_default().trim(),
                        role_rules(&snapshot.group, origin),
                    ),
                )
            }
            WorkflowQuestionRoute::ToUser => {
                return Err("This question is waiting for the user's answer".into())
            }
        };
        let sessions = state.bounded_sessions(WORKFLOW_ACTIVITY_LIMIT)?;
        let session = available_session_for_step(&sessions, target)?;
        if let Err(error) = ensure_rate_limit_reserve(session, &settings) {
            return self.pause_for_guardrail(app, state, workflow_id, &error);
        }
        let baseline = result_ids(session);
        submit_workflow_prompt(app, state, bridge, browser, session, &prompt)?;

        let mut runs = self
            .runs
            .lock()
            .map_err(|_| "Could not update the workflow run".to_string())?;
        let active = runs
            .get_mut(workflow_id)
            .filter(|active| active.run.id == snapshot.run.id)
            .ok_or_else(|| "The workflow run changed while routing a question".to_string())?;
        let now = now_millis();
        start_step(active, &target.id, prompt, baseline, now)?;
        active.run.transition_count = active.run.transition_count.saturating_add(1);
        if question.route == WorkflowQuestionRoute::ToOrigin {
            active.run.question = None;
        }
        record_history_event(
            active,
            WorkflowHistoryEventKind::StepStarted,
            Some(&target.id),
            None,
            &format!("{} started", role_label(target)),
            now,
        );
        persist_active(state, active)?;
        let run = active.run.clone();
        drop(runs);
        emit_changed(app, &run);
        Ok(run)
    }

    fn snapshot(&self, workflow_id: &str) -> Result<ActiveWorkflowRun, String> {
        self.runs
            .lock()
            .map_err(|_| "Could not access workflow runs".to_string())?
            .get(workflow_id)
            .cloned()
            .ok_or_else(|| "Workflow run not found".to_string())
    }

    fn monitored_workflow_ids(&self) -> Vec<String> {
        self.runs
            .lock()
            .map(|runs| {
                runs.iter()
                    .filter(|(_, active)| {
                        active.run.status == WorkflowRunStatus::Ready
                            || (matches!(
                                active.run.status,
                                WorkflowRunStatus::Running | WorkflowRunStatus::Paused
                            ) && active
                                .run
                                .current_step_id
                                .as_deref()
                                .and_then(|step_id| {
                                    active.run.steps.iter().find(|step| step.step_id == step_id)
                                })
                                .is_some_and(|step| step.status == WorkflowStepRunStatus::Running))
                    })
                    .map(|(workflow_id, _)| workflow_id.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn should_advance_automatically(&self, workflow_id: &str) -> bool {
        self.snapshot(workflow_id).ok().is_some_and(|active| {
            // A doubt travels between the orchestrator and the step that asked on its own.
            if active.run.status == WorkflowRunStatus::Ready && active.run.question.is_some() {
                return true;
            }
            active.run.status == WorkflowRunStatus::Ready
                && active
                    .run
                    .pending_connection_id
                    .as_deref()
                    .and_then(|connection_id| {
                        active
                            .group
                            .connections
                            .iter()
                            .find(|connection| connection.id == connection_id)
                    })
                    .is_some_and(|connection| {
                        connection.advance_mode == WorkflowAdvanceMode::Automatic
                            && (!connection.requires_approval || active.run.handoff_approved)
                    })
        })
    }

    fn pause_for_guardrail(
        &self,
        app: &AppHandle,
        state: &AppState,
        workflow_id: &str,
        message: &str,
    ) -> Result<WorkflowRun, String> {
        let mut runs = self
            .runs
            .lock()
            .map_err(|_| "Could not access workflow runs".to_string())?;
        let active = runs
            .get_mut(workflow_id)
            .ok_or_else(|| "Workflow run not found".to_string())?;
        active.paused_from = Some(active.run.status);
        active.run.status = WorkflowRunStatus::Paused;
        active.run.error = Some(message.to_string());
        active.run.updated_at = now_millis();
        let current_step_id = active.run.current_step_id.clone();
        record_history_event(
            active,
            WorkflowHistoryEventKind::GuardrailPaused,
            current_step_id.as_deref(),
            None,
            message,
            active.run.updated_at,
        );
        persist_active(state, active)?;
        let run = active.run.clone();
        drop(runs);
        emit_changed(app, &run);
        Ok(run)
    }

    fn reconcile(
        &self,
        state: &AppState,
        workflow_id: &str,
    ) -> Result<Option<WorkflowRun>, String> {
        let sessions = state.bounded_sessions(WORKFLOW_ACTIVITY_LIMIT)?;
        let settings = state.preferences()?.workflow_settings;
        let mut runs = self
            .runs
            .lock()
            .map_err(|_| "Could not access workflow runs".to_string())?;
        let Some(active) = runs.get_mut(workflow_id) else {
            return Ok(None);
        };
        let previous_updated_at = active.run.updated_at;
        if !matches!(
            active.run.status,
            WorkflowRunStatus::Running | WorkflowRunStatus::Paused
        ) {
            return Ok(Some(active.run.clone()));
        }
        let Some(step_id) = active.run.current_step_id.clone() else {
            return Ok(Some(active.run.clone()));
        };
        let Some(step_index) = active
            .run
            .steps
            .iter()
            .position(|step| step.step_id == step_id)
        else {
            return Err("The active workflow step is missing".into());
        };
        if active.run.steps[step_index].status != WorkflowStepRunStatus::Running {
            return Ok(Some(active.run.clone()));
        }
        let step = workflow_step(&active.group, &step_id)?;
        let step_label = role_label(step);
        let session = sessions.iter().find(|session| {
            session.native_session_id.as_deref() == Some(step.session_native_id.as_str())
        });
        let now = now_millis();
        let started_at = active.run.steps[step_index].started_at.unwrap_or_default();
        if !active.run.recovering
            && active.run.status == WorkflowRunStatus::Running
            && settings.step_timeout_minutes > 0
            && now.saturating_sub(started_at)
                >= i64::from(settings.step_timeout_minutes) * 60 * 1_000
        {
            fail_step(
                active,
                &step_id,
                "The workflow step exceeded its configured timeout",
            );
            persist_active(state, active)?;
            return Ok(Some(active.run.clone()));
        }
        let Some(session) = session else {
            if active
                .recovery_deadline
                .is_some_and(|deadline| now < deadline)
            {
                return Ok(Some(active.run.clone()));
            }
            active.recovery_deadline = None;
            let unavailable_since = active
                .unavailable_since
                .entry(step_id.clone())
                .or_insert(now);
            if now.saturating_sub(*unavailable_since) >= WORKFLOW_AGENT_LOSS_GRACE_MS {
                fail_step(
                    active,
                    &step_id,
                    "The agent for this workflow step disconnected",
                );
                persist_active(state, active)?;
            }
            return Ok(Some(active.run.clone()));
        };
        if active.run.recovering {
            active.run.recovering = false;
            active.run.updated_at = now;
        }
        active.recovery_deadline = None;
        let baseline = active
            .result_baselines
            .get(&step_id)
            .cloned()
            .unwrap_or_default();
        let agent_is_busy = matches!(
            session.status,
            SessionStatus::Running | SessionStatus::PermissionRequired
        );
        let result = session
            .results
            .iter()
            .filter(|result| !baseline.contains(&result.id))
            .max_by_key(|result| result.created_at);
        if session.status == SessionStatus::Failed && session.updated_at >= started_at {
            active.unavailable_since.remove(&step_id);
            fail_step(
                active,
                &step_id,
                "The agent reported that this workflow step failed",
            );
        } else if let Some(result) = result.filter(|_| !agent_is_busy) {
            active.unavailable_since.remove(&step_id);
            let result_id = result.id.clone();
            {
                let step_run = &mut active.run.steps[step_index];
                step_run.status = WorkflowStepRunStatus::Completed;
                step_run.completed_at = Some(result.created_at);
                step_run.result_id = Some(result_id.clone());
                step_run.error = None;
            }
            if let Some(step_history) = active.step_history.get_mut(&step_id) {
                let (response, files, tests) =
                    crate::context_builder::sanitize_workflow_history_result(
                        result,
                        session.working_directory.as_deref(),
                    );
                step_history.response = (!response.is_empty()).then_some(response);
                step_history.files = files;
                step_history.tests = tests;
                step_history.captured_at = Some(result.created_at);
            }
            active.run.error = None;
            record_history_event(
                active,
                WorkflowHistoryEventKind::StepCompleted,
                Some(&step_id),
                None,
                &format!("{step_label} completed"),
                result.created_at,
            );
            let response = result.response.clone();
            if route_after_completion(active, &step_id, &response, now) {
                persist_active(state, active)?;
                return Ok(Some(active.run.clone()));
            }
            set_pending_transition(active, &step_id, now)?;
            if settings.require_approval_for_sensitive_context
                && active
                    .run
                    .pending_connection_id
                    .as_deref()
                    .is_some_and(|connection_id| {
                        context_builder::build_context_package_with_limit(
                            &active.group,
                            connection_id,
                            &active.run.objective,
                            Some(&result_id),
                            &sessions,
                            settings.max_context_tokens as usize,
                        )
                        .ok()
                        .is_some_and(|package| {
                            package
                                .redactions
                                .iter()
                                .any(|entry| matches!(entry.kind.as_str(), "secret" | "file"))
                        })
                    })
            {
                active.run.status = WorkflowRunStatus::WaitingForApproval;
                active.run.error =
                    Some("Sensitive context was redacted. Review and approve this handoff".into());
            }
        } else if !agent_is_busy
            && session.updated_at >= started_at
            && prompt_ended_without_result(session, started_at)
        {
            let unavailable_since = active
                .unavailable_since
                .entry(step_id.clone())
                .or_insert(now);
            if now.saturating_sub(*unavailable_since) >= WORKFLOW_AGENT_LOSS_GRACE_MS {
                fail_step(
                    active,
                    &step_id,
                    "The workflow prompt ended without a completed result",
                );
            }
        } else {
            active.unavailable_since.remove(&step_id);
        }
        if active.run.updated_at != previous_updated_at {
            persist_active(state, active)?;
        }
        Ok(Some(active.run.clone()))
    }
}

fn persist_active(state: &AppState, active: &ActiveWorkflowRun) -> Result<(), String> {
    let payload = serde_json::to_string(active).map_err(|error| error.to_string())?;
    state.save_workflow_run(&active.run.workflow_id, &payload, active.run.updated_at)?;
    persist_public_history(state, active)
}

fn persist_public_history(state: &AppState, active: &ActiveWorkflowRun) -> Result<(), String> {
    let steps = active
        .group
        .steps
        .iter()
        .filter_map(|step| active.step_history.get(&step.id).cloned())
        .collect();
    state.save_workflow_history(&WorkflowHistoryRecord {
        run: active.run.clone(),
        group: active.group.clone(),
        events: active.history_events.clone(),
        steps,
    })
}

fn history_event(
    kind: WorkflowHistoryEventKind,
    step_id: Option<&str>,
    connection_id: Option<&str>,
    summary: &str,
    created_at: i64,
) -> WorkflowHistoryEvent {
    WorkflowHistoryEvent {
        id: format!(
            "workflow-event:{created_at}:{kind:?}:{}:{}",
            step_id.unwrap_or("workflow"),
            connection_id.unwrap_or("none")
        ),
        kind,
        step_id: step_id.map(str::to_string),
        connection_id: connection_id.map(str::to_string),
        summary: summary.to_string(),
        created_at,
    }
}

fn record_history_event(
    active: &mut ActiveWorkflowRun,
    kind: WorkflowHistoryEventKind,
    step_id: Option<&str>,
    connection_id: Option<&str>,
    summary: &str,
    created_at: i64,
) {
    active.history_events.push(history_event(
        kind,
        step_id,
        connection_id,
        summary,
        created_at,
    ));
}

fn prompt_ended_without_result(session: &AgentSession, started_at: i64) -> bool {
    prompt_status_indicates_interruption(&session.status_label)
        || session.activities.iter().any(|activity| {
            activity.created_at >= started_at
                && (activity.status.eq_ignore_ascii_case("interrupted")
                    || activity.title.eq_ignore_ascii_case("Prompt interrupted")
                    || activity.title.eq_ignore_ascii_case("Prompt interrompido"))
        })
}

fn prompt_status_indicates_interruption(label: &str) -> bool {
    let label = label.trim().to_ascii_lowercase();
    label.contains("prompt interrupted")
        || label.contains("prompt interrompido")
        || label.contains("prompt connection ended")
        || label.contains("conexão do prompt encerrada")
}

fn validate_manual_workflow(group: &WorkflowGroupDefinition) -> Result<Vec<String>, String> {
    if group.id.trim().is_empty() || group.steps.is_empty() {
        return Err("Save at least one workflow step before running it".into());
    }
    for step in &group.steps {
        if step.id.trim().is_empty() || step.session_native_id.trim().is_empty() {
            return Err("Every workflow step must reference a connected session".into());
        }
        if step.role == WorkflowRole::Custom && step.custom_role_label.trim().is_empty() {
            return Err("Custom workflow roles need a label".into());
        }
        if step.instruction.trim().is_empty()
            || step.expected_input.trim().is_empty()
            || step.produced_output.trim().is_empty()
            || step.completion_condition.trim().is_empty()
        {
            return Err("Complete every workflow role contract before running it".into());
        }
    }
    let known = group
        .steps
        .iter()
        .map(|step| step.id.as_str())
        .collect::<HashSet<_>>();
    if known.len() != group.steps.len() {
        return Err("Workflow step identifiers must be unique".into());
    }
    let connection_ids = group
        .connections
        .iter()
        .map(|connection| connection.id.as_str())
        .collect::<HashSet<_>>();
    if connection_ids.len() != group.connections.len() {
        return Err("Workflow connection identifiers must be unique".into());
    }
    let mut incoming: HashMap<&str, usize> = HashMap::new();
    let mut outgoing: HashMap<&str, &WorkflowConnectionDefinition> = HashMap::new();
    for connection in &group.connections {
        if !known.contains(connection.from_step_id.as_str())
            || !known.contains(connection.to_step_id.as_str())
        {
            return Err("Workflow connections must reference existing steps".into());
        }
        if outgoing
            .insert(connection.from_step_id.as_str(), connection)
            .is_some()
        {
            return Err(
                "Manual execution currently supports one outgoing connection per step".into(),
            );
        }
        let count = incoming.entry(connection.to_step_id.as_str()).or_default();
        *count += 1;
        if *count > 1 {
            return Err(
                "Manual execution currently supports one incoming connection per step".into(),
            );
        }
    }
    let roots = group
        .steps
        .iter()
        .filter(|step| incoming.get(step.id.as_str()).copied().unwrap_or_default() == 0)
        .collect::<Vec<_>>();
    if roots.len() != 1 {
        return Err("The workflow needs exactly one first step".into());
    }
    let mut order = Vec::with_capacity(group.steps.len());
    let mut current = roots[0].id.as_str();
    let mut visited = HashSet::new();
    loop {
        if !visited.insert(current) {
            return Err("Circular workflow connections cannot be executed".into());
        }
        order.push(current.to_string());
        let Some(connection) = outgoing.get(current) else {
            break;
        };
        current = connection.to_step_id.as_str();
    }
    if order.len() != group.steps.len() {
        return Err("All workflow steps must belong to one connected route".into());
    }
    Ok(order)
}

fn workflow_step<'a>(
    group: &'a WorkflowGroupDefinition,
    step_id: &str,
) -> Result<&'a WorkflowStepDefinition, String> {
    group
        .steps
        .iter()
        .find(|step| step.id == step_id)
        .ok_or_else(|| "Workflow step not found".to_string())
}

fn available_session_for_step<'a>(
    sessions: &'a [AgentSession],
    step: &WorkflowStepDefinition,
) -> Result<&'a AgentSession, String> {
    let session = sessions
        .iter()
        .find(|session| {
            session.native_session_id.as_deref() == Some(step.session_native_id.as_str())
        })
        .ok_or_else(|| "The workflow agent is not connected".to_string())?;
    if matches!(
        session.status,
        SessionStatus::Running | SessionStatus::PermissionRequired
    ) {
        return Err("Wait for this workflow agent to become available before continuing".into());
    }
    Ok(session)
}

fn ensure_rate_limit_reserve(
    session: &AgentSession,
    settings: &WorkflowSettings,
) -> Result<(), String> {
    if !settings.pause_on_rate_limit || settings.minimum_rate_limit_remaining_percent == 0 {
        return Ok(());
    }
    let maximum_used = session
        .rate_limits
        .iter()
        .map(|limit| limit.used_percent)
        .max();
    let blocked_at = 100_u8.saturating_sub(settings.minimum_rate_limit_remaining_percent);
    if maximum_used.is_some_and(|used| used >= blocked_at) {
        return Err(format!(
            "Workflow paused to preserve at least {}% of this agent's rate limit",
            settings.minimum_rate_limit_remaining_percent
        ));
    }
    Ok(())
}

fn submit_workflow_prompt(
    app: &AppHandle,
    state: &AppState,
    bridge: &CodexBridge,
    browser: &BrowserControl,
    session: &AgentSession,
    prompt: &str,
) -> Result<(), String> {
    control::submit_prompt(
        app,
        state,
        bridge,
        browser,
        &session.id,
        prompt,
        Vec::new(),
        PromptDelivery::NewTurn,
        false,
    )
}

fn result_ids(session: &AgentSession) -> HashSet<String> {
    session
        .results
        .iter()
        .map(|result| result.id.clone())
        .collect()
}

fn role_label(step: &WorkflowStepDefinition) -> String {
    if step.role == WorkflowRole::Custom && !step.custom_role_label.trim().is_empty() {
        return step.custom_role_label.trim().to_string();
    }
    format!("{:?}", step.role)
}

fn initial_step_prompt(
    group: &WorkflowGroupDefinition,
    step: &WorkflowStepDefinition,
    objective: &str,
) -> String {
    format!(
        "# Lume workflow\n\n- Workflow: `{}`\n- Role: **{}**\n\n## Objective\n\n{}\n\n## Instructions\n\n{}\n\n## Expected input\n\n{}\n\n## Expected output\n\n{}\n\n## Completion condition\n\n{}",
        group.id,
        role_label(step),
        objective.trim(),
        step.instruction.trim(),
        step.expected_input.trim(),
        step.produced_output.trim(),
        step.completion_condition.trim(),
    ) + &role_rules(group, step)
}

fn skipped_step_prompt(
    group: &WorkflowGroupDefinition,
    connection: &WorkflowConnectionDefinition,
    objective: &str,
) -> Result<String, String> {
    let source = workflow_step(group, &connection.from_step_id)?;
    let target = workflow_step(group, &connection.to_step_id)?;
    Ok(format!(
        "# Lume workflow\n\nThe previous **{}** step was skipped by the user. Continue without its result.\n\n## Objective\n\n{}\n\n## Instructions\n\n{}\n\n## Completion condition\n\n{}{}",
        role_label(source),
        objective.trim(),
        target.instruction.trim(),
        target.completion_condition.trim(),
        if connection.additional_instruction.trim().is_empty() {
            String::new()
        } else {
            format!(
                "\n\n## Connection instruction\n\n{}",
                connection.additional_instruction.trim()
            )
        },
    ))
}

fn set_pending_transition(
    active: &mut ActiveWorkflowRun,
    source_step_id: &str,
    now: i64,
) -> Result<(), String> {
    let connection = active
        .group
        .connections
        .iter()
        .find(|connection| connection.from_step_id == source_step_id)
        .cloned();
    if let Some(connection) = connection {
        active.run.pending_connection_id = Some(connection.id.clone());
        active.run.handoff_approved = false;
        if active.run.status != WorkflowRunStatus::Paused {
            active.run.status = if connection.requires_approval {
                WorkflowRunStatus::WaitingForApproval
            } else {
                WorkflowRunStatus::Ready
            };
        }
        record_history_event(
            active,
            WorkflowHistoryEventKind::HandoffReady,
            Some(source_step_id),
            Some(&connection.id),
            if connection.requires_approval {
                "Handoff waiting for approval"
            } else {
                "Handoff ready"
            },
            now,
        );
    } else {
        active.run.pending_connection_id = None;
        active.run.status = WorkflowRunStatus::Completed;
        record_history_event(
            active,
            WorkflowHistoryEventKind::Completed,
            Some(source_step_id),
            None,
            "Workflow completed",
            now,
        );
    }
    active.run.updated_at = now;
    Ok(())
}

const QUESTION_OPEN: &str = "<lume-question>";
const QUESTION_CLOSE: &str = "</lume-question>";

/// The doubt a step ended its response with, inside `<lume-question>…</lume-question>`.
fn extract_question(response: &str) -> Option<String> {
    let lower = response.to_ascii_lowercase();
    let start = lower.rfind(QUESTION_OPEN)? + QUESTION_OPEN.len();
    let rest = &response[start..];
    let end = rest.to_ascii_lowercase().find(QUESTION_CLOSE).unwrap_or(rest.len());
    let text = rest[..end].trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// The orchestrator a step answers to: the closest planner step before it in the route.
fn orchestrator_for(active: &ActiveWorkflowRun, step_id: &str) -> Option<String> {
    let index = active.run.steps.iter().position(|step| step.step_id == step_id)?;
    active.run.steps[..index]
        .iter()
        .rev()
        .find(|step| {
            workflow_step(&active.group, &step.step_id)
                .is_ok_and(|definition| definition.role == WorkflowRole::Planner)
        })
        .map(|step| step.step_id.clone())
}

fn has_orchestrator_before(group: &WorkflowGroupDefinition, step: &WorkflowStepDefinition) -> bool {
    let mut current = step.id.as_str();
    for _ in 0..group.steps.len() {
        let Some(connection) = group.connections.iter().find(|item| item.to_step_id == current) else {
            return false;
        };
        current = connection.from_step_id.as_str();
        if workflow_step(group, current).is_ok_and(|candidate| candidate.role == WorkflowRole::Planner) {
            return true;
        }
    }
    false
}

/// The rules Lume adds to every workflow prompt: an orchestrator only plans, and a doubt is asked in a
/// `<lume-question>` block so the workflow can route it instead of the step guessing.
fn role_rules(group: &WorkflowGroupDefinition, step: &WorkflowStepDefinition) -> String {
    if step.role == WorkflowRole::Planner {
        return "\n\n## Workflow rules\n\n- You are the orchestrator of this workflow. Do not implement anything: do not edit, create or delete files, and do not run commands that change the project. Read, analyze and plan only.\n- Your final response is handed to the next step as its instructions, so write everything it needs to do the work.\n- If something needs a decision or information only the user can give, do not guess: end your response with `<lume-question>your question</lume-question>`. The workflow then stops and asks the user instead of handing off.".into();
    }
    let who = if has_orchestrator_before(group, step) {
        "The orchestrator answers it, or asks the user when it cannot."
    } else {
        "The user answers it."
    };
    format!(
        "\n\n## Workflow rules\n\n- If you are unsure about something you cannot resolve from the plan or the project, do not guess: stop and end your response with `<lume-question>your question</lume-question>`. {who} Then you continue with the answer."
    )
}

/// After a step finishes: a doubt goes to the orchestrator or the user, and an orchestrator's answer goes back
/// to the step that asked. Returns false when the run follows its route as usual.
fn route_after_completion(
    active: &mut ActiveWorkflowRun,
    step_id: &str,
    response: &str,
    now: i64,
) -> bool {
    let existing = active.run.question.clone();
    if let Some(text) = extract_question(response) {
        let is_planner = workflow_step(&active.group, step_id)
            .is_ok_and(|step| step.role == WorkflowRole::Planner);
        let planner = (!is_planner).then(|| orchestrator_for(active, step_id)).flatten();
        set_step_status(active, step_id, WorkflowStepRunStatus::Waiting);
        active.run.pending_connection_id = None;
        active.run.handoff_approved = false;
        let paused = active.run.status == WorkflowRunStatus::Paused;
        match planner {
            Some(planner_id) => {
                active.run.question = Some(WorkflowQuestion {
                    origin_step_id: step_id.to_string(),
                    step_id: planner_id,
                    route: WorkflowQuestionRoute::ToPlanner,
                    text,
                    answer: None,
                    created_at: now,
                });
                if !paused {
                    active.run.status = WorkflowRunStatus::Ready;
                }
                record_history_event(
                    active,
                    WorkflowHistoryEventKind::QuestionForPlanner,
                    Some(step_id),
                    None,
                    "Question sent to the orchestrator",
                    now,
                );
            }
            None => {
                let origin = existing
                    .filter(|question| question.step_id == step_id)
                    .map(|question| question.origin_step_id)
                    .unwrap_or_else(|| step_id.to_string());
                active.run.question = Some(WorkflowQuestion {
                    origin_step_id: origin,
                    step_id: step_id.to_string(),
                    route: WorkflowQuestionRoute::ToUser,
                    text,
                    answer: None,
                    created_at: now,
                });
                if !paused {
                    active.run.status = WorkflowRunStatus::WaitingForUser;
                }
                record_history_event(
                    active,
                    WorkflowHistoryEventKind::QuestionForUser,
                    Some(step_id),
                    None,
                    "Waiting for the user's answer",
                    now,
                );
            }
        }
        active.run.updated_at = now;
        return true;
    }
    if let Some(question) = existing {
        if question.step_id == step_id && question.origin_step_id != step_id {
            active.run.question = Some(WorkflowQuestion {
                route: WorkflowQuestionRoute::ToOrigin,
                answer: Some(response.trim().to_string()),
                ..question
            });
            if active.run.status != WorkflowRunStatus::Paused {
                active.run.status = WorkflowRunStatus::Ready;
            }
            record_history_event(
                active,
                WorkflowHistoryEventKind::QuestionAnswered,
                Some(step_id),
                None,
                "The orchestrator answered the question",
                now,
            );
            active.run.updated_at = now;
            return true;
        }
        active.run.question = None;
    }
    false
}

fn set_step_status(active: &mut ActiveWorkflowRun, step_id: &str, status: WorkflowStepRunStatus) {
    if let Some(step) = active.run.steps.iter_mut().find(|step| step.step_id == step_id) {
        step.status = status;
    }
}

/// Marks a step as running a prompt Lume just sent it.
fn start_step(
    active: &mut ActiveWorkflowRun,
    step_id: &str,
    prompt: String,
    baseline: HashSet<String>,
    now: i64,
) -> Result<(), String> {
    let step_run = active
        .run
        .steps
        .iter_mut()
        .find(|step| step.step_id == step_id)
        .ok_or_else(|| "The workflow step is missing".to_string())?;
    step_run.status = WorkflowStepRunStatus::Running;
    step_run.attempt = step_run.attempt.saturating_add(1);
    step_run.started_at = Some(now);
    step_run.completed_at = None;
    step_run.result_id = None;
    step_run.error = None;
    active.prompts.insert(step_id.to_string(), prompt);
    active.result_baselines.insert(step_id.to_string(), baseline);
    active.unavailable_since.remove(step_id);
    active.run.current_step_id = Some(step_id.to_string());
    active.run.status = WorkflowRunStatus::Running;
    active.run.error = None;
    active.run.updated_at = now;
    Ok(())
}

fn fail_step(active: &mut ActiveWorkflowRun, step_id: &str, message: &str) {
    let now = now_millis();
    let was_failed = active
        .run
        .steps
        .iter()
        .find(|step| step.step_id == step_id)
        .is_some_and(|step| step.status == WorkflowStepRunStatus::Failed);
    if let Some(step) = active
        .run
        .steps
        .iter_mut()
        .find(|step| step.step_id == step_id)
    {
        step.status = WorkflowStepRunStatus::Failed;
        step.completed_at = Some(now);
        step.error = Some(message.to_string());
    }
    active.run.status = WorkflowRunStatus::Failed;
    active.run.recovering = false;
    active.run.error = Some(message.to_string());
    active.run.updated_at = now;
    if !was_failed {
        record_history_event(
            active,
            WorkflowHistoryEventKind::StepFailed,
            Some(step_id),
            None,
            message,
            now,
        );
    }
}

fn status_after_pause(active: &ActiveWorkflowRun) -> WorkflowRunStatus {
    if let Some(question) = active.run.question.as_ref() {
        let current_is_running = active
            .run
            .current_step_id
            .as_deref()
            .and_then(|id| active.run.steps.iter().find(|step| step.step_id == id))
            .is_some_and(|step| step.status == WorkflowStepRunStatus::Running);
        if current_is_running {
            return WorkflowRunStatus::Running;
        }
        return if question.route == WorkflowQuestionRoute::ToUser {
            WorkflowRunStatus::WaitingForUser
        } else {
            WorkflowRunStatus::Ready
        };
    }
    if active.paused_from == Some(WorkflowRunStatus::WaitingForApproval)
        && !active.run.handoff_approved
    {
        return WorkflowRunStatus::WaitingForApproval;
    }
    let Some(step_id) = active.run.current_step_id.as_deref() else {
        return WorkflowRunStatus::Draft;
    };
    let status = active
        .run
        .steps
        .iter()
        .find(|step| step.step_id == step_id)
        .map(|step| step.status)
        .unwrap_or_default();
    match status {
        WorkflowStepRunStatus::Running => WorkflowRunStatus::Running,
        WorkflowStepRunStatus::Completed | WorkflowStepRunStatus::Skipped => active
            .run
            .pending_connection_id
            .as_deref()
            .and_then(|id| active.group.connections.iter().find(|item| item.id == id))
            .map(|connection| {
                if connection.requires_approval && !active.run.handoff_approved {
                    WorkflowRunStatus::WaitingForApproval
                } else {
                    WorkflowRunStatus::Ready
                }
            })
            .unwrap_or(WorkflowRunStatus::Completed),
        WorkflowStepRunStatus::Failed => WorkflowRunStatus::Failed,
        WorkflowStepRunStatus::Waiting => WorkflowRunStatus::Ready,
        WorkflowStepRunStatus::Pending => active.paused_from.unwrap_or(WorkflowRunStatus::Ready),
    }
}

fn emit_changed(app: &AppHandle, run: &WorkflowRun) {
    let _ = app.emit("lume://workflow-run-changed", run);
    crate::protocol::emit_workflows_changed(app);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        WorkflowAdvanceMode, WorkflowConnectionDefinition, WorkflowContextPolicy,
        WorkflowContextSelection, WorkflowRoleContract,
    };

    fn step(id: &str) -> WorkflowStepDefinition {
        let WorkflowRoleContract {
            instruction,
            expected_input,
            produced_output,
            completion_condition,
        } = WorkflowRole::Implementer.default_contract();
        WorkflowStepDefinition {
            id: id.into(),
            session_native_id: format!("session-{id}"),
            role: WorkflowRole::Implementer,
            custom_role_label: String::new(),
            instruction,
            expected_input,
            produced_output,
            completion_condition,
            attempt: 0,
        }
    }

    fn connection(from: &str, to: &str) -> WorkflowConnectionDefinition {
        WorkflowConnectionDefinition {
            id: format!("{from}-{to}"),
            from_step_id: from.into(),
            to_step_id: to.into(),
            include_response: true,
            include_files: true,
            include_tests: true,
            context_policy: WorkflowContextPolicy::Standard,
            context_selection: WorkflowContextSelection::default(),
            additional_instruction: String::new(),
            requires_approval: true,
            advance_mode: WorkflowAdvanceMode::Manual,
        }
    }

    fn active_run(group: WorkflowGroupDefinition, current_step_id: &str) -> ActiveWorkflowRun {
        ActiveWorkflowRun {
            run: WorkflowRun {
                id: "run".into(),
                workflow_id: group.id.clone(),
                objective: "Ship the feature".into(),
                status: WorkflowRunStatus::Running,
                current_step_id: Some(current_step_id.into()),
                steps: group
                    .steps
                    .iter()
                    .map(|step| WorkflowStepRun {
                        step_id: step.id.clone(),
                        status: if step.id == current_step_id {
                            WorkflowStepRunStatus::Completed
                        } else {
                            WorkflowStepRunStatus::Pending
                        },
                        ..WorkflowStepRun::default()
                    })
                    .collect(),
                ..WorkflowRun::default()
            },
            group,
            prompts: HashMap::new(),
            result_baselines: HashMap::new(),
            unavailable_since: HashMap::new(),
            paused_from: None,
            history_events: Vec::new(),
            step_history: HashMap::new(),
            recovery_deadline: None,
        }
    }

    fn planner_workflow() -> WorkflowGroupDefinition {
        let mut planner = step("plan");
        planner.role = WorkflowRole::Planner;
        WorkflowGroupDefinition {
            id: "workflow".into(),
            terminal_group_id: "terminals".into(),
            steps: vec![planner, step("build")],
            connections: vec![connection("plan", "build")],
        }
    }

    #[test]
    fn a_question_is_read_from_the_end_of_a_response() {
        assert_eq!(
            extract_question("Plan...\n\n<lume-question>\nShould the cache be shared?\n</lume-question>").as_deref(),
            Some("Should the cache be shared?")
        );
        assert_eq!(extract_question("No doubts here."), None);
        assert_eq!(extract_question("<lume-question>  </lume-question>"), None);
    }

    #[test]
    fn the_orchestrator_is_told_not_to_implement_and_how_to_ask() {
        let group = planner_workflow();
        let planner = initial_step_prompt(&group, &group.steps[0], "Ship it");
        assert!(planner.contains("Do not implement anything"));
        assert!(planner.contains("<lume-question>"));
        let builder = role_rules(&group, &group.steps[1]);
        assert!(builder.contains("The orchestrator answers it"));
    }

    #[test]
    fn an_orchestrator_question_stops_the_handoff_and_waits_for_the_user() {
        let mut active = active_run(planner_workflow(), "plan");
        assert!(route_after_completion(&mut active, "plan", "Plan\n<lume-question>Which database?</lume-question>", 10));
        assert_eq!(active.run.status, WorkflowRunStatus::WaitingForUser);
        assert_eq!(active.run.pending_connection_id, None);
        let question = active.run.question.as_ref().expect("question");
        assert_eq!(question.route, WorkflowQuestionRoute::ToUser);
        assert_eq!(question.step_id, "plan");
        assert_eq!(question.text, "Which database?");
        assert_eq!(active.run.steps[0].status, WorkflowStepRunStatus::Waiting);
    }

    #[test]
    fn an_implementer_question_goes_to_the_orchestrator_and_its_answer_comes_back() {
        let mut active = active_run(planner_workflow(), "build");
        assert!(route_after_completion(&mut active, "build", "<lume-question>Keep the old API?</lume-question>", 10));
        assert_eq!(active.run.status, WorkflowRunStatus::Ready);
        let question = active.run.question.clone().expect("question");
        assert_eq!((question.route, question.step_id.as_str(), question.origin_step_id.as_str()), (WorkflowQuestionRoute::ToPlanner, "plan", "build"));
        assert_eq!(active.run.steps[1].status, WorkflowStepRunStatus::Waiting);

        // The orchestrator answers without a question of its own: the answer goes back to the implementer.
        active.run.status = WorkflowRunStatus::Running;
        assert!(route_after_completion(&mut active, "plan", "Yes, keep it.", 20));
        let question = active.run.question.clone().expect("question");
        assert_eq!(question.route, WorkflowQuestionRoute::ToOrigin);
        assert_eq!(question.answer.as_deref(), Some("Yes, keep it."));
        assert_eq!(active.run.status, WorkflowRunStatus::Ready);
    }

    #[test]
    fn an_orchestrator_that_cannot_answer_asks_the_user_for_the_implementer() {
        let mut active = active_run(planner_workflow(), "build");
        route_after_completion(&mut active, "build", "<lume-question>Keep the old API?</lume-question>", 10);
        active.run.status = WorkflowRunStatus::Running;
        assert!(route_after_completion(&mut active, "plan", "<lume-question>Do clients still call v1?</lume-question>", 20));
        assert_eq!(active.run.status, WorkflowRunStatus::WaitingForUser);
        let question = active.run.question.clone().expect("question");
        assert_eq!((question.route, question.step_id.as_str(), question.origin_step_id.as_str()), (WorkflowQuestionRoute::ToUser, "plan", "build"));
        assert_eq!(status_after_pause(&active), WorkflowRunStatus::WaitingForUser);
    }

    #[test]
    fn a_step_without_an_orchestrator_asks_the_user_and_a_closed_question_follows_the_route() {
        let group = WorkflowGroupDefinition {
            id: "workflow".into(),
            terminal_group_id: "terminals".into(),
            steps: vec![step("one"), step("two")],
            connections: vec![connection("one", "two")],
        };
        assert!(role_rules(&group, &group.steps[1]).contains("The user answers it"));
        let mut active = active_run(group, "one");
        assert!(route_after_completion(&mut active, "one", "<lume-question>Which port?</lume-question>", 10));
        assert_eq!(active.run.status, WorkflowRunStatus::WaitingForUser);
        // After the user's answer the question is closed, so the next result hands off as usual.
        active.run.question = None;
        assert!(!route_after_completion(&mut active, "one", "Done on port 80.", 20));
    }

    #[test]
    fn interrupted_prompt_status_is_recognized_without_a_result() {
        assert!(prompt_status_indicates_interruption(
            "Conexão do prompt encerrada"
        ));
        assert!(prompt_status_indicates_interruption("Prompt interrupted"));
    }

    #[test]
    fn idle_session_is_not_mistaken_for_an_interrupted_prompt() {
        assert!(!prompt_status_indicates_interruption("Waiting for action"));
    }

    #[test]
    fn manual_workflow_requires_one_connected_route() {
        let group = WorkflowGroupDefinition {
            id: "workflow".into(),
            terminal_group_id: "terminals".into(),
            steps: vec![step("one"), step("two"), step("three")],
            connections: vec![connection("one", "two"), connection("two", "three")],
        };

        assert_eq!(
            validate_manual_workflow(&group).expect("route"),
            vec!["one", "two", "three"]
        );
    }

    #[test]
    fn manual_workflow_rejects_branches() {
        let group = WorkflowGroupDefinition {
            id: "workflow".into(),
            terminal_group_id: "terminals".into(),
            steps: vec![step("one"), step("two"), step("three")],
            connections: vec![connection("one", "two"), connection("one", "three")],
        };

        assert!(validate_manual_workflow(&group).is_err());
    }

    #[test]
    fn initial_prompt_contains_the_contract_and_objective() {
        let step = step("one");
        let group = WorkflowGroupDefinition {
            id: "workflow".into(),
            terminal_group_id: "terminals".into(),
            steps: vec![step.clone()],
            connections: Vec::new(),
        };

        let prompt = initial_step_prompt(&group, &step, "Ship the feature");
        assert!(prompt.contains("Ship the feature"));
        assert!(prompt.contains("Completion condition"));
        assert!(prompt.contains("Implementer"));
    }

    #[test]
    fn completed_step_waits_for_required_handoff_approval() {
        let group = WorkflowGroupDefinition {
            id: "workflow".into(),
            terminal_group_id: "terminals".into(),
            steps: vec![step("one"), step("two")],
            connections: vec![connection("one", "two")],
        };
        let mut active = active_run(group, "one");

        set_pending_transition(&mut active, "one", 42).expect("transition");

        assert_eq!(active.run.status, WorkflowRunStatus::WaitingForApproval);
        assert_eq!(active.run.pending_connection_id.as_deref(), Some("one-two"));
        assert!(!active.run.handoff_approved);
    }

    #[test]
    fn sensitive_approval_is_not_bypassed_by_pause_and_resume() {
        let group = WorkflowGroupDefinition {
            id: "workflow".into(),
            terminal_group_id: "terminals".into(),
            steps: vec![step("one"), step("two")],
            connections: vec![connection("one", "two")],
        };
        let mut active = active_run(group, "one");
        set_pending_transition(&mut active, "one", 42).expect("transition");
        active.paused_from = Some(WorkflowRunStatus::WaitingForApproval);
        active.run.status = WorkflowRunStatus::Paused;

        assert_eq!(
            status_after_pause(&active),
            WorkflowRunStatus::WaitingForApproval
        );
    }

    #[test]
    fn final_step_completes_the_run() {
        let group = WorkflowGroupDefinition {
            id: "workflow".into(),
            terminal_group_id: "terminals".into(),
            steps: vec![step("one")],
            connections: Vec::new(),
        };
        let mut active = active_run(group, "one");

        set_pending_transition(&mut active, "one", 42).expect("transition");

        assert_eq!(active.run.status, WorkflowRunStatus::Completed);
        assert!(active.run.pending_connection_id.is_none());
        assert!(active
            .history_events
            .iter()
            .any(|event| event.kind == WorkflowHistoryEventKind::Completed));
    }

    #[test]
    fn public_workflow_history_does_not_persist_internal_prompts() {
        let group = WorkflowGroupDefinition {
            id: "workflow-history".into(),
            terminal_group_id: "terminals".into(),
            steps: vec![step("one")],
            connections: Vec::new(),
        };
        let mut active = active_run(group, "one");
        active.prompts.insert("one".into(), "private prompt".into());
        active.step_history.insert(
            "one".into(),
            WorkflowStepHistory {
                step_id: "one".into(),
                session_native_id: "session-one".into(),
                role_label: "Implementer".into(),
                agent_label: "Codex".into(),
                ..WorkflowStepHistory::default()
            },
        );
        let state = AppState::new(std::path::Path::new(":memory:")).expect("state");
        persist_active(&state, &active).expect("persist workflow");

        let history = state.workflow_history(10).expect("history");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].steps[0].agent_label, "Codex");
        let public_payload = serde_json::to_string(&history[0]).expect("public payload");
        assert!(!public_payload.contains("private prompt"));
    }

    #[test]
    fn automatic_connection_advances_when_ready() {
        let mut next = connection("one", "two");
        next.requires_approval = false;
        next.advance_mode = WorkflowAdvanceMode::Automatic;
        let group = WorkflowGroupDefinition {
            id: "workflow".into(),
            terminal_group_id: "terminals".into(),
            steps: vec![step("one"), step("two")],
            connections: vec![next],
        };
        let mut active = active_run(group, "one");
        set_pending_transition(&mut active, "one", 42).expect("transition");
        let runtime = WorkflowRuntime::default();
        runtime
            .runs
            .lock()
            .expect("runs")
            .insert("workflow".into(), active);

        assert!(runtime.should_advance_automatically("workflow"));
    }

    #[test]
    fn automatic_connection_still_waits_for_required_approval() {
        let mut next = connection("one", "two");
        next.advance_mode = WorkflowAdvanceMode::Automatic;
        let group = WorkflowGroupDefinition {
            id: "workflow".into(),
            terminal_group_id: "terminals".into(),
            steps: vec![step("one"), step("two")],
            connections: vec![next],
        };
        let mut active = active_run(group, "one");
        set_pending_transition(&mut active, "one", 42).expect("transition");
        let runtime = WorkflowRuntime::default();
        runtime
            .runs
            .lock()
            .expect("runs")
            .insert("workflow".into(), active);

        assert!(!runtime.should_advance_automatically("workflow"));
    }

    #[test]
    fn persisted_workflow_is_restored_with_its_pending_handoff() {
        let mut next = connection("one", "two");
        next.requires_approval = false;
        next.advance_mode = WorkflowAdvanceMode::Automatic;
        let group = WorkflowGroupDefinition {
            id: "workflow".into(),
            terminal_group_id: "terminals".into(),
            steps: vec![step("one"), step("two")],
            connections: vec![next],
        };
        let mut active = active_run(group, "one");
        set_pending_transition(&mut active, "one", 42).expect("transition");
        let state = AppState::new(std::path::Path::new(":memory:")).expect("state");
        persist_active(&state, &active).expect("persist workflow");

        let restored = WorkflowRuntime::default();
        assert_eq!(restored.restore(&state).expect("restore workflow"), 1);
        let snapshot = restored.snapshot("workflow").expect("restored run");
        assert_eq!(snapshot.run.status, WorkflowRunStatus::Ready);
        assert_eq!(
            snapshot.run.pending_connection_id.as_deref(),
            Some("one-two")
        );
        assert!(restored.should_advance_automatically("workflow"));
    }

    #[test]
    fn restored_running_workflow_waits_for_agent_rediscovery() {
        let group = WorkflowGroupDefinition {
            id: "workflow-recovery".into(),
            terminal_group_id: "terminals".into(),
            steps: vec![step("one")],
            connections: Vec::new(),
        };
        let mut active = active_run(group, "one");
        active.run.steps[0].status = WorkflowStepRunStatus::Running;
        active
            .unavailable_since
            .insert("one".into(), now_millis() - WORKFLOW_AGENT_LOSS_GRACE_MS);
        let state = AppState::new(std::path::Path::new(":memory:")).expect("state");
        persist_active(&state, &active).expect("persist workflow");

        let restored = WorkflowRuntime::default();
        assert_eq!(restored.restore(&state).expect("restore workflow"), 1);
        let run = restored
            .get(&state, "workflow-recovery")
            .expect("reconcile workflow")
            .expect("restored run");

        assert_eq!(run.status, WorkflowRunStatus::Running);
        assert!(run.recovering);
        assert_eq!(run.steps[0].status, WorkflowStepRunStatus::Running);
        assert!(run.error.is_none());
        let active = restored
            .snapshot("workflow-recovery")
            .expect("active recovery");
        assert!(active.recovery_deadline.is_some());
        assert!(!active.unavailable_since.contains_key("one"));
    }
}
