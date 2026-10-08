import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  HubCommandRequest,
  HubCommandResponse,
  ExternalWriterConflict,
  HubSnapshot,
} from "$lib/hubProtocol";
import type {
  AgentSession,
  CompanionStatus,
  DiscoveredLumeNode,
  DockSide,
  HistoryEntry,
  IntegrationDiagnostic,
  IntegrationStatus,
  MobileGatewayStatus,
  MobilePairingOffer,
  MobileScope,
  PairedDevice,
  RemoteLumeNode,
  RemoteLumeNodeHealth,
  RemoteLumeNodeInventory,
  PermissionAction,
  Preferences,
  PromptAttachmentInput,
  PromptDelivery,
  QuestionAnswer,
  ResumableSession,
  ResultNote,
  ReviewDecision,
  ReviewNote,
  SessionActivity,
  SessionNote,
  RestoredTerminalPlacement,
  ExternalAgentPlugin,
  TerminalWindowState,
  WhiteboardLayout,
  WorkflowContextPackage,
  WorkflowGroupDefinition,
  WorkflowHistoryRecord,
  WorkflowRole,
  WorkflowRoleContract,
  WorkflowRun,
} from "$lib/domain";
import { demoHistory, demoSessions } from "$lib/demo";
import type { AgentSlashCommand } from "$lib/slashCommands";

const inDesktop = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export const defaultPreferences: Preferences = {
  language: "en",
  startupMode: "ask",
  darkMode: undefined,
  appearanceTheme: "lume",
  accentColor: undefined,
  accentOpacity: 100,
  darkBase: "theme",
  lightBase: "theme",
  uiFont: "default",
  codeFont: "default",
  workspaceBackgroundColor: undefined,
  workspaceBackgroundOpacity: 96,
  workspaceLightBackgroundColor: undefined,
  workspaceLightBackgroundOpacity: 96,
  workspaceDarkBackgroundColor: undefined,
  workspaceDarkBackgroundOpacity: 96,
  soundEnabled: true,
  soundVolume: 55,
  popupNotificationsEnabled: true,
  autostart: true,
  mobileGatewayEnabled: false,
  overlayX: undefined,
  overlayY: undefined,
  showOverFullscreen: false,
  historyRetentionDays: 30,
  launchTarget: "auto",
  projectProfiles: {},
  sessionAliases: {},
  whiteboardLayouts: [],
  workflowEnabled: false,
  workflowGroups: [],
  workflowSettings: {
    maxTransitions: 10,
    maxAttemptsPerStep: 2,
    stepTimeoutMinutes: 30,
    maxContextTokens: 20_000,
    requireApprovalForSensitiveContext: true,
    pauseOnRateLimit: true,
    minimumRateLimitRemainingPercent: 10,
  },
  globalShortcut: "Ctrl+Shift+Space",
  openShortcut: "Ctrl+Alt+Shift+L",
  newSessionShortcut: "Ctrl+Alt+Shift+N",
  whiteboardShortcut: "Ctrl+Alt+Shift+B",
  workspaceShortcut: "Ctrl+Alt+Shift+W",
};

export async function loadSessions(): Promise<AgentSession[]> {
  try {
    return await invoke<AgentSession[]>("list_sessions");
  } catch {
    return inDesktop() ? [] : structuredClone(demoSessions);
  }
}

export async function renameSession(sessionId: string, name: string): Promise<string> {
  return invoke<string>("rename_session", { sessionId, name });
}

export interface CodexCliConversationChoices {
  processKey: string;
  linkedNativeSessionId?: string | null;
  candidates: Array<{ nativeSessionId: string; name: string }>;
  hasMore: boolean;
}

export function canLinkCodexCli(session: AgentSession): boolean {
  return session.agent === "codex" && session.controlOrigin === "external"
    && session.source === "cli" && Boolean(session.processId) && !session.nativeSessionId;
}

export function isUnidentifiedCodexCli(session: AgentSession): boolean {
  const name = session.sessionName?.trim() ?? "";
  return canLinkCodexCli(session) && /^CLI não identificada(?: \(\d+\))?$/.test(name);
}

export async function loadCodexCliConversations(sessionId: string, query = ""): Promise<CodexCliConversationChoices> {
  return invoke("list_codex_cli_conversations", { sessionId, query });
}

export async function linkCodexCliConversation(sessionId: string, processKey: string, nativeSessionId: string): Promise<void> {
  return invoke("link_codex_cli_conversation", { sessionId, processKey, nativeSessionId });
}

export async function forkSessionFromMessage(
  sessionId: string,
  turnId: string | undefined,
  prompt: string,
  response: string,
): Promise<string> {
  return invoke<string>("fork_session_from_message", { sessionId, turnId, prompt, response });
}

export async function loadHubSnapshot(): Promise<HubSnapshot> {
  return invoke<HubSnapshot>("get_hub_snapshot");
}

export async function loadWorkspaceConversationPage(
  sessionId: string,
  beforeCreatedAt: number,
  beforeActivityId: string,
): Promise<{ activities: SessionActivity[]; hasMore: boolean }> {
  return invoke("get_workspace_conversation_page", { sessionId, beforeCreatedAt, beforeActivityId });
}

export interface WorkspacePromptIndexEntry {
  id: string;
  createdAt: number;
  detail: string;
}

export interface PathMention {
  path: string;
  isDirectory: boolean;
}

/** Files and folders under the session folder that match an `@` mention. */
export async function searchSessionPaths(sessionId: string, query: string): Promise<PathMention[]> {
  return invoke<PathMention[]>("search_session_paths", { sessionId, query });
}

export async function loadWorkspacePromptIndexPage(
  sessionId: string,
  beforeCreatedAt?: number,
  beforeActivityId?: string,
  query?: string,
): Promise<{ prompts: WorkspacePromptIndexEntry[]; hasMore: boolean }> {
  return invoke("get_workspace_prompt_index_page", { sessionId, beforeCreatedAt, beforeActivityId, query });
}

export async function loadSubagentTimeline(
  sessionId: string,
  activityId: string,
): Promise<SessionActivity[]> {
  return invoke("get_subagent_timeline", { sessionId, activityId });
}

export async function loadTerminalHubSnapshot(label: string, activityLimit = 60): Promise<HubSnapshot> {
  return invoke<HubSnapshot>("get_terminal_hub_snapshot", { label, activityLimit });
}

export async function loadMobileGatewayStatus(): Promise<MobileGatewayStatus> {
  return invoke<MobileGatewayStatus>("get_mobile_gateway_status");
}

export async function enableMobileGateway(): Promise<MobileGatewayStatus> {
  return invoke<MobileGatewayStatus>("enable_mobile_gateway");
}

export async function disableMobileGateway(): Promise<MobileGatewayStatus> {
  return invoke<MobileGatewayStatus>("disable_mobile_gateway");
}

export async function beginMobilePairing(): Promise<MobilePairingOffer> {
  return invoke<MobilePairingOffer>("begin_mobile_pairing");
}

export async function loadPairedDevices(): Promise<PairedDevice[]> {
  return invoke<PairedDevice[]>("list_paired_devices");
}

export async function revokePairedDevice(id: string): Promise<boolean> {
  return invoke<boolean>("revoke_paired_device", { id });
}

export async function setPairedDeviceScopes(
  id: string,
  scopes: MobileScope[],
): Promise<boolean> {
  return invoke<boolean>("set_paired_device_scopes", { id, scopes });
}

export async function discoverLumeNodes(): Promise<DiscoveredLumeNode[]> {
  return invoke<DiscoveredLumeNode[]>("discover_lume_nodes");
}

export async function pairLumeNode(pairingUri: string): Promise<RemoteLumeNode> {
  return invoke<RemoteLumeNode>("pair_lume_node", { pairingUri });
}

export async function loadRemoteLumeNodes(): Promise<RemoteLumeNode[]> {
  return invoke<RemoteLumeNode[]>("list_remote_lume_nodes");
}

export async function loadRemoteLumeNodeHealth(nodeId: string): Promise<RemoteLumeNodeHealth> {
  return invoke<RemoteLumeNodeHealth>("get_remote_lume_node_health", { nodeId });
}

export async function loadRemoteLumeNodeInventory(nodeId: string): Promise<RemoteLumeNodeInventory> {
  return invoke<RemoteLumeNodeInventory>("get_remote_lume_node_inventory", { nodeId });
}

export async function forgetRemoteLumeNode(nodeId: string): Promise<boolean> {
  return invoke<boolean>("forget_remote_lume_node", { nodeId });
}

export async function executeHubCommand(
  request: HubCommandRequest,
): Promise<HubCommandResponse> {
  return invoke<HubCommandResponse>("execute_hub_command", { request });
}

export async function decidePermission(
  sessionId: string,
  permissionId: string,
  action: PermissionAction,
): Promise<void> {
  await invoke("resolve_permission", {
    sessionId,
    permissionId,
    action,
  });
}

export async function answerQuestion(
  sessionId: string,
  questionId: string,
  answers: QuestionAnswer[],
): Promise<void> {
  await invoke("resolve_question", {
    sessionId,
    questionId,
    answers,
  });
}

export async function openSessionSource(sessionId: string): Promise<void> {
  await invoke("open_session_source", { sessionId });
}

export async function moveOverlay(
  x: number,
  y: number,
  persist: boolean,
  monitorId?: string,
): Promise<void> {
  await invoke("move_overlay", { x: Math.round(x), y: Math.round(y), persist, monitorId });
}

/** Asks the window manager for the keyboard (XWayland fallback). False when it is not needed. */
export async function activateOverlayWindow(): Promise<boolean> {
  return invoke<boolean>("activate_overlay_window");
}

/** Logged only when Lume starts with LUME_ORB_DEBUG=1. */
export async function reportOverlayGeometry(report: string): Promise<void> {
  await invoke("report_overlay_geometry", { report });
}

export async function resizeOverlaySurface(width: number, height: number): Promise<void> {
  await invoke("resize_overlay_surface", {
    width: Math.max(1, Math.round(width)),
    height: Math.max(1, Math.round(height)),
  });
}

export async function submitPrompt(
  sessionId: string,
  prompt: string,
  attachments: PromptAttachmentInput[] = [],
  delivery: PromptDelivery = "new_turn",
): Promise<void> {
  await invoke("submit_prompt", { sessionId, prompt, attachments, delivery });
}

export async function readLocalImageDataUrl(path: string): Promise<string> {
  return invoke<string>("read_local_image_data_url", { path });
}

export async function exportLocalFile(sourcePath: string, destinationPath: string): Promise<void> {
  await invoke("export_local_file", { sourcePath, destinationPath });
}

export async function setTerminalFileDialogActive(
  label: string,
  active: boolean,
): Promise<void> {
  await invoke("set_terminal_file_dialog_active", { label, active });
}

export async function setNativeFileDialogActive(active: boolean): Promise<void> {
  await invoke("set_native_file_dialog_active", { active });
}

export async function refreshAgentRateLimits(agent: AgentSession["agent"]): Promise<void> {
  await invoke("refresh_agent_rate_limits", { agent });
}

export async function terminateSession(sessionId: string): Promise<void> {
  await invoke("terminate_session", { sessionId });
}

export async function takeControlSession(
  sessionId: string,
  prompt = "",
  attachments: PromptAttachmentInput[] = [],
): Promise<void> {
  await invoke("take_control_session", { sessionId, prompt, attachments });
}

export async function listExternalWriterConflicts(): Promise<ExternalWriterConflict[]> {
  return invoke("list_external_writer_conflicts");
}

export async function cancelExternalWriterAttempt(conflict: ExternalWriterConflict): Promise<void> {
  await invoke("cancel_external_writer_attempt", {
    sessionId: conflict.sessionId,
    nativeSessionId: conflict.nativeSessionId,
    processId: conflict.processId,
  });
}

export async function forkCodexThread(sessionId: string): Promise<string> {
  return invoke("fork_codex_thread", { sessionId });
}

export async function interruptPrompt(sessionId: string): Promise<void> {
  await invoke("interrupt_prompt", { sessionId });
}

export type CollaborationMode = "default" | "plan";

export interface CodexReasoningEffortOption {
  value: string;
  description: string;
}

export interface CodexModelOption {
  /** Claude Code only: whether the model can run in auto permission mode. */
  supportsAutoMode?: boolean;
  model: string;
  displayName: string;
  description: string;
  isDefault: boolean;
  defaultReasoningEffort: string;
  supportedReasoningEfforts: CodexReasoningEffortOption[];
}

export interface CodexThreadModelSettings {
  model: string;
  reasoningEffort?: string;
  serviceTier?: string | null;
  models: CodexModelOption[];
  sessionModes?: {
    currentMode: string;
    options: { value: string; label: string; description: string }[];
  };
}

export interface SessionModelOverride {
  model?: string;
  reasoningEffort?: string;
}

export async function getSessionCollaborationMode(
  sessionId: string,
): Promise<CollaborationMode> {
  return invoke<CollaborationMode>("get_session_collaboration_mode", { sessionId });
}

export async function setSessionCollaborationMode(
  sessionId: string,
  mode: CollaborationMode,
): Promise<CollaborationMode> {
  return invoke<CollaborationMode>("set_session_collaboration_mode", { sessionId, mode });
}

export async function listSessionSlashCommands(sessionId: string): Promise<AgentSlashCommand[]> {
  return invoke<AgentSlashCommand[]>("list_session_slash_commands", { sessionId });
}

export async function getSessionModelSettings(
  sessionId: string,
): Promise<CodexThreadModelSettings> {
  return invoke<CodexThreadModelSettings>("get_session_model_settings", { sessionId });
}

export async function setSessionModelSettings(
  sessionId: string,
  model: string,
  effort: string,
): Promise<CodexThreadModelSettings> {
  return invoke<CodexThreadModelSettings>("set_session_model_settings", {
    sessionId,
    model,
    effort,
  });
}

export async function setSessionAgentMode(sessionId: string, mode: string): Promise<CodexThreadModelSettings> {
  return invoke<CodexThreadModelSettings>("set_session_agent_mode", { sessionId, mode });
}

export async function setSessionFastMode(sessionId: string, enabled: boolean): Promise<boolean> {
  return invoke<boolean>("set_session_fast_mode", { sessionId, enabled });
}

/** The models Claude Code offers and the model and effort this session runs with. */
export interface ClaudeModelSettings {
  model: string;
  reasoningEffort?: string | null;
  models: CodexModelOption[];
}

/** How an agent asks before acting: the modes on offer and the one in effect. */
export interface PermissionSettings {
  mode: string;
  modes: string[];
}

/** Claude Code, Codex, and managed Antigravity sessions; modes differ by agent. */
export async function getSessionPermissionMode(sessionId: string): Promise<PermissionSettings> {
  return invoke<PermissionSettings>("get_session_permission_mode", { sessionId });
}

export async function setSessionPermissionMode(
  sessionId: string,
  mode: string,
): Promise<PermissionSettings> {
  return invoke<PermissionSettings>("set_session_permission_mode", { sessionId, mode });
}

export async function getClaudeSessionModelSettings(
  sessionId: string,
): Promise<ClaudeModelSettings> {
  return invoke<ClaudeModelSettings>("get_claude_session_model_settings", { sessionId });
}

export async function setClaudeSessionModelSettings(
  sessionId: string,
  model?: string,
  effort?: string,
): Promise<ClaudeModelSettings> {
  return invoke<ClaudeModelSettings>("set_claude_session_model_settings", {
    sessionId,
    model: model || null,
    effort: effort || null,
  });
}

export async function steerQueuedPrompt(
  sessionId: string,
  activityId: string,
): Promise<void> {
  await invoke("steer_queued_prompt", { sessionId, activityId });
}

export async function openTerminalWindow(sessionId: string): Promise<string> {
  return invoke<string>("open_terminal_window", { sessionId });
}

export async function openWorkspaceWindow(): Promise<void> {
  await invoke("open_workspace_window");
}

function workspaceBootId(): number | undefined {
  const id = (window as Window & { __LUME_WORKSPACE_BOOT_ID__?: number }).__LUME_WORKSPACE_BOOT_ID__;
  return typeof id === "number" && Number.isSafeInteger(id) && id > 0 ? id : undefined;
}

export async function markWorkspaceFrontendReady(): Promise<void> {
  const bootId = workspaceBootId();
  if (bootId !== undefined) await invoke("workspace_frontend_ready", { bootId });
}

export async function reportWorkspaceFrontendFailure(reason: string): Promise<void> {
  const bootId = workspaceBootId();
  if (bootId !== undefined) await invoke("workspace_frontend_failed", { bootId, reason });
}

export async function markTerminalFrontendReady(label: string): Promise<void> {
  await invoke("terminal_frontend_ready", { label });
}

export async function toggleTerminalGroupFullscreen(label: string): Promise<boolean | null> {
  return invoke<boolean | null>("toggle_terminal_group_fullscreen", { label });
}

export async function terminalGroupFullscreenActive(label: string): Promise<boolean> {
  return invoke<boolean>("terminal_group_fullscreen_active", { label });
}

export async function loadTerminalWindows(): Promise<TerminalWindowState[]> {
  if (!inDesktop()) return [];
  return invoke<TerminalWindowState[]>("list_terminal_windows");
}

export async function setTerminalWindowsVisible(visible: boolean): Promise<void> {
  if (!inDesktop()) return;
  await invoke("set_terminal_windows_visible", { visible });
}

export async function loadTerminalWindowState(label: string): Promise<TerminalWindowState> {
  return invoke<TerminalWindowState>("get_terminal_window_state", { label });
}

export async function minimizeTerminalWindow(label: string): Promise<void> {
  await invoke("minimize_terminal_window", { label });
}

export async function closeTerminalWindow(label: string): Promise<void> {
  await invoke("close_terminal_window", { label });
}

export async function moveTerminalWindow(
  label: string,
  x: number,
  y: number,
  finalize: boolean,
): Promise<TerminalWindowState> {
  return invoke<TerminalWindowState>("move_terminal_window", {
    label,
    x: Math.round(x),
    y: Math.round(y),
    finalize,
  });
}

export async function cancelTerminalWindowMove(label: string): Promise<TerminalWindowState> {
  return invoke<TerminalWindowState>("cancel_terminal_window_move", { label });
}

export async function syncTerminalWindowPosition(
  label: string,
  x: number,
  y: number,
  finalize: boolean,
): Promise<TerminalWindowState> {
  return invoke<TerminalWindowState>("sync_terminal_window_position", {
    label,
    x: Math.round(x),
    y: Math.round(y),
    finalize,
  });
}

export async function loadTerminalDragSnapshot(
  label: string,
): Promise<{ pressed: boolean; x: number; y: number }> {
  return invoke("terminal_drag_snapshot", { label });
}

export async function beginTerminalNativeDrag(label: string): Promise<void> {
  await invoke("begin_terminal_native_drag", { label });
}

export async function resizeTerminalWindow(
  label: string,
  x: number,
  y: number,
  width: number,
  height: number,
  fromLeft: boolean,
  fromTop: boolean,
): Promise<TerminalWindowState> {
  return invoke<TerminalWindowState>("resize_terminal_window", {
    label,
    x: Math.round(x),
    y: Math.round(y),
    width: Math.round(width),
    height: Math.round(height),
    fromLeft,
    fromTop,
  });
}

export async function beginLayeredTerminalResize(label: string): Promise<TerminalWindowState> {
  return invoke<TerminalWindowState>("begin_layered_terminal_resize", { label });
}

export async function finishLayeredTerminalResize(label: string): Promise<TerminalWindowState> {
  return invoke<TerminalWindowState>("finish_layered_terminal_resize", { label });
}

export type WorkflowBridgeContext = {
  groupId: string;
  sourceSessionNativeId: string;
  targetSessionNativeId: string;
  side: DockSide;
  nativeConnectors: boolean;
  height: number;
};

export async function openWorkflowBridgeWindow(label: string, side: DockSide): Promise<string> {
  return invoke<string>("open_workflow_bridge_window", { label, side });
}

export async function setWorkflowConnectionHover(
  label: string,
  side: DockSide,
  hovered: boolean,
): Promise<void> {
  await invoke("set_workflow_connection_hover", { label, side, hovered });
}

export async function prepareWorkflowBridgeWindow(label: string, side: DockSide): Promise<string> {
  return invoke<string>("prepare_workflow_bridge_window", { label, side });
}

export async function discardPreparedWorkflowBridgeWindow(label: string): Promise<void> {
  await invoke("discard_prepared_workflow_bridge_window", { label });
}

export async function loadWorkflowBridgeContext(label: string): Promise<WorkflowBridgeContext> {
  return invoke<WorkflowBridgeContext>("get_workflow_bridge_context", { label });
}

export async function setWorkflowBridgeExpanded(
  label: string,
  expanded: boolean,
  contentHeight?: number,
): Promise<void> {
  await invoke("set_workflow_bridge_expanded", { label, expanded, contentHeight });
}

export async function undockTerminalWindow(label: string): Promise<TerminalWindowState> {
  return invoke<TerminalWindowState>("undock_terminal_window", { label });
}

export async function setTerminalWorkflowEnabled(
  enabled: boolean,
): Promise<TerminalWindowState[]> {
  return invoke<TerminalWindowState[]>("set_terminal_workflow_enabled", { enabled });
}

export async function restoreTerminalLayout(
  entries: RestoredTerminalPlacement[],
): Promise<TerminalWindowState[]> {
  return invoke<TerminalWindowState[]>("restore_terminal_layout", { entries });
}

export async function loadHistory(): Promise<HistoryEntry[]> {
  try {
    return await invoke<HistoryEntry[]>("list_history", { limit: 100 });
  } catch {
    return inDesktop() ? [] : structuredClone(demoHistory);
  }
}

export async function loadWorkflowHistory(limit = 100): Promise<WorkflowHistoryRecord[]> {
  if (!inDesktop()) return [];
  return invoke<WorkflowHistoryRecord[]>("list_workflow_history", { limit });
}

export async function loadResultNotes(): Promise<ResultNote[]> {
  if (!inDesktop()) return [];
  return invoke<ResultNote[]>("list_result_notes", { limit: 100 });
}

export async function saveResultNote(
  sessionId: string,
  resultId: string,
  title: string,
): Promise<ResultNote> {
  return invoke<ResultNote>("save_result_note", { sessionId, resultId, title });
}

export async function deleteResultNote(id: string): Promise<void> {
  await invoke("delete_result_note", { id });
}

export async function loadReviewNotes(sessionId: string): Promise<ReviewNote[]> {
  if (!inDesktop()) return [];
  return invoke<ReviewNote[]>("list_review_notes", { sessionId });
}

export async function saveReviewNote(
  sessionId: string,
  resultId: string,
  body: string,
): Promise<ReviewNote> {
  return invoke<ReviewNote>("save_review_note", { sessionId, resultId, body });
}

export async function deleteReviewNote(sessionId: string, resultId: string): Promise<void> {
  await invoke("delete_review_note", { sessionId, resultId });
}

export async function loadReviewDecisions(sessionId: string): Promise<ReviewDecision[]> {
  if (!inDesktop()) return [];
  return invoke<ReviewDecision[]>("list_review_decisions", { sessionId });
}

export async function setReviewDecision(
  sessionId: string,
  resultId: string,
  decision: ReviewDecision["decision"],
  note?: string,
): Promise<ReviewDecision> {
  return invoke<ReviewDecision>("set_review_decision", {
    sessionId,
    resultId,
    decision,
    note,
  });
}

export async function loadSessionNotes(sessionId: string): Promise<SessionNote[]> {
  if (!inDesktop()) return [];
  return invoke<SessionNote[]>("list_session_notes", { sessionId });
}

export async function saveSessionNote(
  sessionId: string,
  note: Pick<SessionNote, "title" | "body" | "kind" | "pinned"> & { id?: string },
): Promise<SessionNote> {
  return invoke<SessionNote>("save_session_note", {
    sessionId,
    noteId: note.id,
    title: note.title,
    body: note.body,
    kind: note.kind,
    pinned: note.pinned,
  });
}

export async function deleteSessionNote(id: string): Promise<void> {
  await invoke("delete_session_note", { id });
}

export async function loadPreferences(): Promise<Preferences> {
  try {
    return await invoke<Preferences>("get_preferences");
  } catch {
    return { ...defaultPreferences };
  }
}

export async function loadWorkflowRoleContract(
  role: WorkflowRole,
): Promise<WorkflowRoleContract> {
  return invoke<WorkflowRoleContract>("get_workflow_role_contract", { role });
}

export async function previewWorkflowContext(
  group: WorkflowGroupDefinition,
  connectionId: string,
  objective: string,
  sourceResultId?: string,
): Promise<WorkflowContextPackage> {
  return invoke<WorkflowContextPackage>("preview_workflow_context", {
    group,
    connectionId,
    objective,
    sourceResultId,
  });
}

export async function loadWorkflowRun(workflowId: string): Promise<WorkflowRun | null> {
  return invoke<WorkflowRun | null>("get_workflow_run", { workflowId });
}

export async function startWorkflowRun(
  group: WorkflowGroupDefinition,
  objective: string,
): Promise<WorkflowRun> {
  return invoke<WorkflowRun>("start_workflow_run", { group, objective });
}

export async function approveWorkflowHandoff(workflowId: string): Promise<WorkflowRun> {
  return invoke<WorkflowRun>("approve_workflow_handoff", { workflowId });
}

export async function advanceWorkflowRun(workflowId: string): Promise<WorkflowRun> {
  return invoke<WorkflowRun>("advance_workflow_run", { workflowId });
}

export async function pauseWorkflowRun(workflowId: string): Promise<WorkflowRun> {
  return invoke<WorkflowRun>("pause_workflow_run", { workflowId });
}

export async function resumeWorkflowRun(workflowId: string): Promise<WorkflowRun> {
  return invoke<WorkflowRun>("resume_workflow_run", { workflowId });
}

export async function retryWorkflowStep(workflowId: string): Promise<WorkflowRun> {
  return invoke<WorkflowRun>("retry_workflow_step", { workflowId });
}

export async function skipWorkflowStep(workflowId: string): Promise<WorkflowRun> {
  return invoke<WorkflowRun>("skip_workflow_step", { workflowId });
}

export async function cancelWorkflowRun(workflowId: string): Promise<WorkflowRun> {
  return invoke<WorkflowRun>("cancel_workflow_run", { workflowId });
}

export async function rebindWorkflowSession(
  workflowId: string,
  stepId: string,
  sessionNativeId: string,
): Promise<WorkflowRun | null> {
  return invoke<WorkflowRun | null>("rebind_workflow_session", {
    workflowId,
    stepId,
    sessionNativeId,
  });
}

export type DisplayBackend =
  | "native"
  | "native-gnome"
  | "xwayland-fallback"
  | "gnome-wayland-limited";

export async function loadDisplayBackend(): Promise<DisplayBackend> {
  if (!inDesktop()) return "native";
  try {
    return await invoke("display_backend");
  } catch {
    return "native";
  }
}

export async function loadOverlayPosition(): Promise<{ x: number; y: number }> {
  return invoke("get_overlay_position");
}

export async function savePreferences(preferences: Preferences): Promise<void> {
  if (!("__TAURI_INTERNALS__" in window)) return;
  await invoke("set_preferences", { preferences });
}

export async function watchShortcutRegistrationError(
  onChange: (error: string | null) => void,
): Promise<() => void> {
  if (!inDesktop()) return () => {};
  let revision = 0;
  let active = true;
  const stop = await listen<string | null>(
    "lume://shortcut-registration-error",
    ({ payload }) => {
      revision += 1;
      if (active) onChange(payload);
    },
  );
  const initialRevision = revision;
  try {
    const error = await invoke<string | null>("get_shortcut_registration_error");
    if (active && revision === initialRevision) onChange(error);
  } catch {
    // A development webview may still be connected to an older app process.
  }
  return () => {
    active = false;
    stop();
  };
}

export async function takePendingShortcutAction(): Promise<
  "open" | "palette" | "new-session" | "whiteboard" | null
> {
  if (!inDesktop()) return null;
  return invoke("take_pending_shortcut_action");
}

export async function loadIntegrationStatuses(): Promise<IntegrationStatus[]> {
  if (!("__TAURI_INTERNALS__" in window)) {
    return [
      { kind: "codex", label: "Codex", installed: true, configured: false, canConfigure: true, canLaunch: true, directPermissions: true, detail: "Ready to connect" },
      { kind: "claude", label: "Claude Code", installed: true, configured: true, canConfigure: true, canLaunch: true, directPermissions: true, detail: "Monitoring and decisions connected" },
      { kind: "antigravity", label: "Antigravity CLI", installed: true, configured: false, canConfigure: true, canLaunch: true, directPermissions: false, detail: "CLI only; VS Code Gemini Code Assist is separate" },
      { kind: "deepseek", label: "DeepSeek Harness", installed: false, configured: false, canConfigure: false, canLaunch: true, directPermissions: false, detail: "CLI not found" },
      { kind: "gemini", label: "Gemini CLI (legacy)", installed: true, configured: false, canConfigure: false, canLaunch: false, directPermissions: false, detail: "Process monitoring only; shared Gemini settings are untouched" },
    ];
  }
  return invoke<IntegrationStatus[]>("integration_statuses");
}

export async function configureIntegration(
  kind: IntegrationStatus["kind"],
  enabled: boolean,
): Promise<void> {
  await invoke("configure_integration", { kind, enabled });
}

export async function loadResumableSessions(
  kind: IntegrationStatus["kind"],
): Promise<ResumableSession[]> {
  if (!("__TAURI_INTERNALS__" in window)) return [];
  return invoke<ResumableSession[]>("list_resumable_sessions", { kind });
}

export async function diagnoseIntegration(
  kind: IntegrationStatus["kind"],
): Promise<IntegrationDiagnostic> {
  return invoke<IntegrationDiagnostic>("diagnose_integration", { kind });
}

export async function launchAgentSession(
  agent: IntegrationStatus["kind"],
  workingDirectory: string,
  resume: boolean,
  resumeId: string | undefined,
  target: Preferences["launchTarget"],
  permissionMode?: Preferences["projectProfiles"][string]["permissionMode"],
  approvalPolicy?: Preferences["projectProfiles"][string]["approvalPolicy"],
): Promise<void> {
  await invoke("launch_session", {
    request: {
      agent,
      workingDirectory,
      resume,
      resumeId,
      target,
      initialPrompt: undefined,
      permissionMode,
      approvalPolicy,
    },
  });
}

export async function loadVscodeStatus(): Promise<CompanionStatus> {
  if (!("__TAURI_INTERNALS__" in window)) {
    return { installed: true, configured: false, detail: "Necessário para abrir sessões no editor" };
  }
  return invoke<CompanionStatus>("vscode_status");
}

export async function configureVscode(enabled: boolean): Promise<void> {
  await invoke("configure_vscode", { enabled });
}

export async function revealBrowserCompanion(): Promise<string> {
  return invoke<string>("reveal_browser_companion");
}

export async function loadExternalPlugins(): Promise<ExternalAgentPlugin[]> {
  if (!inDesktop()) return [];
  return invoke<ExternalAgentPlugin[]>("list_external_plugins");
}

export async function installExternalPlugin(path: string): Promise<ExternalAgentPlugin> {
  return invoke<ExternalAgentPlugin>("install_external_plugin", { path });
}

export async function removeExternalPlugin(id: string): Promise<void> {
  await invoke("remove_external_plugin", { id });
}

export async function revealPluginDirectory(): Promise<string> {
  return invoke<string>("reveal_plugin_directory");
}
