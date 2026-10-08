<script lang="ts">
  import { onMount, tick } from "svelte";
  import { slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { openPath } from "@tauri-apps/plugin-opener";
  import type { InteractiveQuestion, PendingQuestion, PermissionAction, QuestionAnswer, SessionActivity, SessionResult } from "$lib/domain";
  import type { PromptAttachmentInput } from "$lib/domain";
  import type { ExternalWriterConflict, HubSession } from "$lib/hubProtocol";
  import type { Language } from "$lib/i18n";
  import ActivityTraceGroup from "$lib/ActivityTraceGroup.svelte";
  import StreamedMessage from "$lib/StreamedMessage.svelte";
  import { MAX_HOLD_MS } from "$lib/streamPacing";
  import { interruptNoticeText } from "$lib/interruptNotice";
import { controlsDiffer, isFastServiceTier, type ControlsSnapshot } from "$lib/agentControls";
  import ThinkingOrb from "$lib/ThinkingOrb.svelte";
  import SessionEnvironmentMenu from "$lib/SessionEnvironmentMenu.svelte";
  import { sessionEnvironments } from "$lib/sessionEnvironments";
  import ThreadAvatar from "$lib/ThreadAvatar.svelte";
  import SubagentPortals from "$lib/SubagentPortals.svelte";
  import WorkspaceWorkBookmarks from "$lib/WorkspaceWorkBookmarks.svelte";
  import CollapsibleUserMessage from "$lib/CollapsibleUserMessage.svelte";
  import BrandIcon from "$lib/BrandIcon.svelte";
  import LumeMascot from "$lib/LumeMascot.svelte";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import UsageBanner from "$lib/UsageBanner.svelte";
  import WorkspaceChatIcon from "$lib/WorkspaceChatIcon.svelte";
  import SendPlaneIcon from "$lib/SendPlaneIcon.svelte";
  import LumeSelect from "$lib/LumeSelect.svelte";
  import { agentSlashCommands, filterSlashCommands, findSlashCommand, loadAgentSlashCommands, slashCommandQuery, slashCommandText, type AgentSlashCommand, type SlashCommand } from "$lib/slashCommands";
  import { caretOnEdgeLine, emptyPromptHistory, historyEntries, stepPromptHistory } from "$lib/promptHistory";
  import { applyMention, mentionAtCaret, type MentionQuery } from "$lib/promptMentions";
  import { claudeEffortForModel, claudeEffortValues, claudeModelOptions } from "$lib/claudeModels";
  import { permissionDescription, permissionLabel, permissionTone } from "$lib/sessionPermissions";
  import AgentConnectionDialog from "$lib/AgentConnectionDialog.svelte";
  import { agentConnectionMessage, type ConnectableAgent } from "$lib/agentConnection";
  import SessionRepositoryBadge from "$lib/SessionRepositoryBadge.svelte";
  import SystemBannerStack, { type SystemBannerItem } from "$lib/SystemBannerStack.svelte";
  import FileTypeIcon from "$lib/FileTypeIcon.svelte";
  import { renderFileTypeIconHtml } from "$lib/fileTypeIcons";
  import ResponseAttachments from "$lib/ResponseAttachments.svelte";
  import { displayText } from "$lib/i18n";
  import { displayFileChangePath, summarizeFileChanges } from "$lib/fileChanges";
  import { activityThinkingLabel, activityThinkingState, formatAgentDuration, isGenericAnalysisPlaceholder } from "$lib/activityPresentation";
  import { collectAgentAlerts } from "$lib/agentAlerts";
  import { usageAlertDismissals } from "$lib/usageAlertDismissals";
  import { renderWorkspaceMarkdownWithFileBadges } from "$lib/workspaceFileReferences.js";
  import { parentWaitingForSubagents, subagentsForSession } from "$lib/workspaceAgents";
  import { BoundedRenderCache } from "$lib/boundedRenderCache";
  import { buildConversationEntries, buildConversationFeed, fileChangesForFinalResponses } from "$lib/sessionConversation";
  import { cleanPromptTransport } from "$lib/chatAttachments";
  import {
    clipboardHasFile,
    clipboardHasImage,
    clipboardMayContainImage,
    collectClipboardFiles,
    collectClipboardImages,
    createImagePreview,
    isImageAttachmentFile,
    isImageAttachmentPath,
    prepareClipboardFile,
    prepareClipboardImage,
  } from "$lib/imageAttachments";
  import {
    getClaudeSessionModelSettings,
    getSessionFastMode,
    getSessionCollaborationMode,
    getSessionModelSettings,
    listSessionSlashCommands,
    forkSessionFromMessage,
    interruptPrompt,
    loadWorkspaceConversationPage,
    answerQuestion,
    decidePermission,
    getSessionPermissionMode,
    setSessionPermissionMode,
    loadWorkspacePromptIndexPage,
    openSessionSource,
    searchSessionPaths,
    readLocalImageDataUrl,
    setNativeFileDialogActive,
    setClaudeSessionModelSettings,
    setSessionCollaborationMode,
    setSessionFastMode,
    setSessionModelSettings,
    setSessionAgentMode,
    steerQueuedPrompt,
    submitPrompt,
    takeControlSession,
    type PermissionSettings,
    type CodexModelOption,
    type CodexThreadModelSettings,
    type CollaborationMode,
    type PathMention,
    type WorkspacePromptIndexEntry,
  } from "$lib/lume";

  let {
    session,
    externalWriterConflict = null,
    language = "en",
    closable = false,
    focused = false,
    maximized = false,
    streamMessages = true,
    visible = true,
    onClose,
    onFocus,
    onOpenReview,
    onOpenRepository,
    onFork,
    onToggleMaximize,
    onDismissExternalWriterConflict,
    onResolveExternalWriterConflict,
  } = $props<{
    session: HubSession;
    externalWriterConflict?: ExternalWriterConflict | null;
    language?: Language;
    closable?: boolean;
    focused?: boolean;
    maximized?: boolean;
    streamMessages?: boolean;
    visible?: boolean;
    onClose?: () => void;
    onFocus?: () => void;
    onOpenReview?: (path: string) => void;
    onOpenRepository?: () => void;
    onFork?: (threadId: string) => void | Promise<void>;
    onToggleMaximize?: () => void;
    onDismissExternalWriterConflict?: (conflict: ExternalWriterConflict) => void;
    onResolveExternalWriterConflict?: (action: "keep_lume" | "open_branch") => void | Promise<void>;
  }>();

  const subagents = $derived(subagentsForSession(session));
  const hasEnvironments = $derived($sessionEnvironments.environments.some((environment) => environment.sessionId === session.id));
  const waitingForSubagents = $derived(parentWaitingForSubagents(session, subagents));
  const presentedStatus = $derived(waitingForSubagents
    ? (language === "pt-BR" ? "Aguardando subagentes" : "Waiting for subagents")
    : displayText(language, session.statusLabel));

  type ResponseSource = {
    id: string;
    kind: "web" | "file" | "search";
    label: string;
    detail: string;
    href?: string;
  };

  type OutgoingPrompt = {
    id: number;
    text: string;
    createdAt: number;
    knownPromptIds: string[];
    flightComplete: boolean;
    accepted: boolean;
  };

  const markdownCache = new BoundedRenderCache(64, 5 * 1024 * 1024);
  const paneOpenedAt = Date.now();
  let prompt = $state("");
  let promptInput = $state<HTMLTextAreaElement | null>(null);
  let slashCommandIndex = $state(0);
  let slashMenuDismissed = $state(false);
  let slashCommandMenu = $state<HTMLDivElement | null>(null);
  let permissionBusy = $state(false);
  let sessionPermission = $state<PermissionSettings | null>(null);
  let permissionMenuOpen = $state(false);
  let permissionMenuLoading = $state(false);
  let permissionMenuSaving = $state(false);
  let permissionMenuRoot = $state<HTMLDivElement | null>(null);
  let loadedPermissionSessionId = "";
  const pendingPermission = $derived(session.pendingPermission ?? null);
  let questionSelections = $state<Record<string, string>>({});
  let questionSending = $state(false);
  const pendingQuestion = $derived<PendingQuestion | null>(session.pendingQuestion ?? null);
  // A new question starts without the previous one's choices.
  $effect(() => {
    void pendingQuestion?.id;
    questionSelections = {};
  });
  let promptHistory = $state(emptyPromptHistory());
  let promptHistoryLoaded = false;
  let promptHistoryHasMore = $state(false);
  let activeMention = $state<MentionQuery | null>(null);
  let mentionResults = $state<PathMention[]>([]);
  let mentionIndex = $state(0);
  let mentionMenu = $state<HTMLDivElement | null>(null);
  let mentionRequest = 0;
  let sending = $state(false);
  let connectionRequired = $state<string | null>(null);
  let dismissedConnectionError = $state("");
  let outgoingPrompts = $state<OutgoingPrompt[]>([]);
  let outgoingSequence = 0;
  const outgoingFlights = new Map<number, { animation: Animation; element: HTMLElement }>();
  let planeLaunching = $state(false);
  let planeLaunchId = $state(0);
  let planeLaunchTimer: number | null = null;
  let takingControl = $state(false);
  let takeoverConfirm = $state(false);
  let takeoverAcceptButton = $state<HTMLButtonElement | null>(null);
  let writerConflictBusy = $state<"keep_lume" | "open_branch" | null>(null);
  let writerConflictError = $state("");
  let writerConflictPrimaryButton = $state<HTMLButtonElement | null>(null);
  let sendError = $state("");
  let promptAttachments = $state<PromptAttachmentInput[]>([]);
  let paneElement = $state<HTMLElement | null>(null);
  let conversationElement = $state<HTMLDivElement | null>(null);
  let conversationContentElement = $state<HTMLDivElement | null>(null);
  let composerElement = $state<HTMLFormElement | null>(null);
  let composerHeight = $state(0);
  let introDismissed = $state(false);
  let composerTransition = $state<"idle" | "out" | "in">("idle");
  let controlsRoot = $state<HTMLDivElement | null>(null);
  let controlsOpen = $state(false);
  let alertsRoot = $state<HTMLDivElement | null>(null);
  let alertMenuOpen = $state(false);
  let dismissedAgentAlertIds = $state<string[]>([]);
  let zoomRoot = $state<HTMLDivElement | null>(null);
  let zoomOpen = $state(false);
  const textZoomMin = 0.8;
  const textZoomMax = 1.8;
  let textZoom = $state(1);
  let controlsLoading = $state(false);
  let controlsSaving = $state(false);
  let controlsError = $state("");
  let collaborationMode = $state<CollaborationMode>("default");
  let fastMode = $state(false);
  let fastSaving = $state(false);
  let modeSaving = $state(false);
  let modelSettings = $state<CodexThreadModelSettings | null>(null);
  let selectedModel = $state("");
  let selectedEffort = $state("");
  let claudeModel = $state("");
  let claudeEffort = $state("");
  let claudeModels = $state<CodexModelOption[]>([]);
  let interrupting = $state(false);
  let steeringQueued = $state(false);
  let followingTail = $state(true);
  let activeSessionId: string | null = null;
  let expandedFileSummaries = $state<string[]>([]);
  let userScrolledAway = false;
  let reattachOnScroll = false;
  let touchStartY = 0;
  let draggingScrollbar = false;
  let olderActivities = $state<SessionActivity[]>([]);
  let olderCursor = $state<{ createdAt: number; id: string } | null>(null);
  let historyHasMore = $state<boolean | null>(null);
  let historyLoading = $state(false);
  let historyError = $state("");
  let visibleFeedLimit = $state(120);
  let promptIndexOpen = $state(false);
  let indexedPrompts = $state<WorkspacePromptIndexEntry[]>([]);
  let promptIndexCursor = $state<{ createdAt: number; id: string } | null>(null);
  let promptIndexInitialized = $state(false);
  let promptIndexHasMore = $state(false);
  let promptIndexLoading = $state(false);
  let promptIndexError = $state("");
  let promptSearchQuery = $state("");
  let promptSearchResults = $state<WorkspacePromptIndexEntry[]>([]);
  let promptSearchCursor = $state<{ createdAt: number; id: string } | null>(null);
  let promptSearchHasMore = $state(false);
  let promptSearchLoading = $state(false);
  let promptSearchInputElement = $state<HTMLInputElement | null>(null);
  let promptSearchTimer: number | null = null;
  let promptSearchGeneration = 0;
  let jumpingToPromptId = $state<string | null>(null);
  let activePromptId = $state<string | null>(null);
  const conversationActivities = $derived.by<SessionActivity[]>(() => {
    if (!olderActivities.length) return session.activities;
    const byId = new Map(olderActivities.map((activity) => [activity.id, activity]));
    for (const activity of session.activities) byId.set(activity.id, activity);
    return [...byId.values()].sort((left, right) => left.createdAt - right.createdAt);
  });
  const chatActivities = $derived(conversationActivities.filter((activity: SessionActivity) =>
    activity.kind !== "warning"
    &&
    !/^functions\s*[·:]\s*(?:create_goal|get_goal|update_goal)$/i.test(activity.title.trim())
    && !isGenericAnalysisPlaceholder(activity)
  ));
  const entries = $derived.by(() => buildConversationEntries(
    session,
    chatActivities,
    (activity) => summarizeFileChanges(
      activity.detail ?? "",
      activityReportedFiles(activity),
      session.workingDirectory,
    ),
  ));
  const outgoingMatches = $derived.by(() => {
    const matches = new Map<number, string>();
    const usedPromptIds = new Set<string>();
    const promptEntries = entries.filter((entry) => entry.activity.kind === "prompt");
    for (const outgoing of outgoingPrompts) {
      const match = promptEntries.find((entry) =>
        !usedPromptIds.has(entry.activity.id)
        && !outgoing.knownPromptIds.includes(entry.activity.id)
        && cleanPromptTransport(entry.activity.detail) === outgoing.text
      );
      if (match) {
        matches.set(outgoing.id, match.activity.id);
        usedPromptIds.add(match.activity.id);
      }
    }
    return matches;
  });
  $effect(() => {
    const matchedIds = outgoingPrompts
      .filter((outgoing) => outgoing.flightComplete && outgoingMatches.has(outgoing.id)
        && (outgoing.accepted || session.source === "web"))
      .map((outgoing) => outgoing.id);
    if (!matchedIds.length) return;
    queueMicrotask(() => {
      outgoingPrompts = outgoingPrompts.filter((outgoing) => !matchedIds.includes(outgoing.id));
    });
  });
  const feed = $derived(buildConversationFeed(entries, { includeAnalysisInTrace: true, workingDirectory: session.workingDirectory }));
  const filesByFinalResponse = $derived(fileChangesForFinalResponses(feed, session.workingDirectory));
  const hasConversationMessage = $derived(entries.some((entry) =>
    entry.activity.kind === "prompt" || entry.activity.kind === "message"
  ));
  const hiddenCount = $derived(Math.max(0, feed.length - visibleFeedLimit));
  const visibleFeed = $derived(hiddenCount ? feed.slice(-visibleFeedLimit) : feed);
  // Only the newest agent message is written out; older ones in a burst just appear.
  const newestMessageEntryId = $derived(
    visibleFeed.findLast((item) => item.kind === "entry" && item.entry.activity.kind === "message")
      ?.id ?? null,
  );
  // What follows a message that is still being written waits for it to finish, but never
  // for long: after MAX_HOLD_MS the message is shown whole and the chat moves on.
  let writingFeedItemId = $state<string | null>(null);
  let holdTimer: number | null = null;
  const finishedWritingIds = new Set<string>();
  function messageWriting(feedItemId: string, active: boolean) {
    if (!active) {
      // Another message mounting must not cancel this one's hold.
      if (writingFeedItemId !== feedItemId) return;
      writingFeedItemId = null;
      if (holdTimer !== null) window.clearTimeout(holdTimer);
      holdTimer = null;
      return;
    }
    if (finishedWritingIds.has(feedItemId)) return;
    writingFeedItemId = feedItemId;
    if (holdTimer !== null) window.clearTimeout(holdTimer);
    holdTimer = window.setTimeout(() => {
      finishedWritingIds.add(feedItemId);
      if (writingFeedItemId === feedItemId) writingFeedItemId = null;
      holdTimer = null;
    }, MAX_HOLD_MS);
  }
  const displayedFeed = $derived.by(() => {
    if (!writingFeedItemId) return visibleFeed;
    const index = visibleFeed.findIndex((item) => item.id === writingFeedItemId);
    return index < 0 ? visibleFeed : visibleFeed.slice(0, index + 1);
  });
  const promptIndexItems = $derived.by(() => {
    const byId = new Map(indexedPrompts.map((item) => [item.id, item]));
    for (const entry of entries) {
      if (entry.activity.kind !== "prompt") continue;
      byId.set(entry.activity.id, {
        id: entry.activity.id,
        createdAt: entry.activity.createdAt,
        detail: entry.activity.detail ?? "",
      });
    }
    return [...byId.values()].sort((left, right) =>
      right.createdAt - left.createdAt || right.id.localeCompare(left.id)
    );
  });
  const normalizedPromptSearch = $derived(promptSearchQuery.trim());
  const visiblePromptIndexItems = $derived.by(() => {
    if (!normalizedPromptSearch) return promptIndexItems;
    const query = normalizedPromptSearch.toLocaleLowerCase(language);
    const byId = new Map(promptSearchResults.map((item) => [item.id, item]));
    for (const item of promptIndexItems) {
      if (cleanPromptTransport(item.detail).toLocaleLowerCase(language).includes(query)) {
        byId.set(item.id, item);
      }
    }
    return [...byId.values()].sort((left, right) =>
      right.createdAt - left.createdAt || right.id.localeCompare(left.id)
    );
  });
  const canLoadEarlier = $derived(Boolean(
    hiddenCount > 0 || (historyHasMore ?? (session.nativeSessionId && session.activities.length >= 60))
  ));
  const activeTraceId = $derived.by(() => {
    if (waitingForSubagents) return null;
    if (!["running", "permission_required"].includes(session.status)) return null;
    const traceIndex = feed.findLastIndex((item) => item.kind === "trace");
    const promptIndex = feed.findLastIndex((item) =>
      item.kind === "entry" && item.entry.activity.kind === "prompt"
    );
    return traceIndex >= promptIndex && traceIndex >= 0 ? feed[traceIndex].id : null;
  });
  const activeThinkingState = $derived.by(() => {
    const activity = chatActivities.findLast((item) => item.kind !== "subagent");
    return activity ? activityThinkingState(activity) : "breathing";
  });
  const promptIsRunning = $derived(session.status === "running");
  const activePromptStartedAt = $derived.by(() => {
    const latestResultAt = session.results.reduce((latest: number, result: SessionResult) => Math.max(latest, result.createdAt), 0);
    const latestFinalAt = entries.reduce((latest, entry) => entry.isFinalResponse ? Math.max(latest, entry.activity.createdAt) : latest, latestResultAt);
    return chatActivities.findLast((activity) => activity.kind === "prompt" && activity.createdAt > latestFinalAt)?.createdAt ?? null;
  });
  const freshChat = $derived(!hasConversationMessage && !promptIsRunning && !introDismissed);
  const composerInIntroPosition = $derived(freshChat || composerTransition === "out");
  const canQueue = $derived(session.capabilities.promptDeliveries.includes("queue"));
  const canCompose = $derived(Boolean(
    session.capabilities.canPrompt || session.capabilities.canTakeControl
  ));
  const canAttach = $derived(Boolean(canCompose && session.capabilities.canAttachImages));
  const canSend = $derived(Boolean(
    canCompose
    && (session.capabilities.canTakeControl || !promptIsRunning || canQueue)
  ));
  const queuedPrompts = $derived(
    session.activities
      .filter((activity: SessionActivity) => ["queued_prompt", "codex_queued_prompt"].includes(activity.kind) && activity.status === "waiting")
      .sort((left: SessionActivity, right: SessionActivity) => left.createdAt - right.createdAt),
  );
  const nextQueuedPrompt = $derived(queuedPrompts[0] ?? null);
  const canSteer = $derived(Boolean(
    promptIsRunning
    && nextQueuedPrompt
    && nextQueuedPrompt.kind === "queued_prompt"
    && session.capabilities.promptDeliveries.includes("steer")
  ));
  // After a cancel Claude's queue waits for you: nothing runs, but you can send it now.
  const canRunQueued = $derived(Boolean(
    !promptIsRunning
    && session.agent === "claude_code"
    && nextQueuedPrompt
    && nextQueuedPrompt.kind === "queued_prompt"
    && session.capabilities.promptDeliveries.includes("steer")
  ));
  // Claude Code and Codex can change how they ask before acting, from Lume.
  const supportsPermissionPicker = $derived(["claude_code", "codex", "antigravity"].includes(session.agent));
  const supportsAgentControls = $derived(["codex", "claude_code", "opencode", "antigravity"].includes(session.agent));
  let sourceEntryId = $state<string | null>(null);
  let actionNotice = $state("");
  let forkingEntryId = $state<string | null>(null);
  let typingClock = $state(Date.now());
  let observedRunningSince = $state<number | null>(null);
  let observedRunningSessionId = "";
  const promptBlocksSettings = $derived(["running", "permission_required"].includes(session.status));
  const modelControlsDisabled = $derived(
    session.controlOrigin !== "lume"
      || controlsLoading
      || controlsSaving
      || (session.agent === "claude_code" && promptBlocksSettings)
  );
  const runtimeControlsDisabled = $derived(
    session.controlOrigin !== "lume"
      || controlsLoading
      || controlsSaving
      || fastSaving
      || modeSaving
      || promptBlocksSettings
  );
  const claudeFastModeAvailable = $derived(claudeModels.some((option) => option.supportsFastMode));
  const sourceEntry = $derived(sourceEntryId
    ? entries.find((entry) => entry.id === sourceEntryId) ?? null
    : null);
  const responseSources = $derived.by(() => sourceEntry ? sourcesForEntry(sourceEntry) : []);

  $effect(() => {
    const currentSessionId = session.id;
    // Snapshot updates replace the session object while a response streams.
    // Reset only when this pane actually switches to another session.
    if (activeSessionId === currentSessionId) return;
    activeSessionId = currentSessionId;
    followingTail = true;
    expandedFileSummaries = [];
    userScrolledAway = false;
    reattachOnScroll = false;
    olderActivities = [];
    olderCursor = null;
    historyHasMore = null;
    historyLoading = false;
    historyError = "";
    visibleFeedLimit = 120;
    promptIndexOpen = false;
    indexedPrompts = [];
    promptIndexCursor = null;
    promptIndexInitialized = false;
    promptIndexHasMore = false;
    promptIndexLoading = false;
    promptIndexError = "";
    promptSearchQuery = "";
    promptSearchResults = [];
    promptSearchCursor = null;
    promptSearchHasMore = false;
    promptSearchLoading = false;
    promptSearchGeneration += 1;
    if (promptSearchTimer) window.clearTimeout(promptSearchTimer);
    promptSearchTimer = null;
    jumpingToPromptId = null;
    activePromptId = null;
    controlsOpen = false;
    alertMenuOpen = false;
    dismissedAgentAlertIds = [];
    zoomOpen = false;
    controlsError = "";
    modelSettings = null;
    claudeModels = [];
    claudeModel = "";
    claudeEffort = "";
    selectedModel = "";
    selectedEffort = "";
    originalControls = null;
    fastMode = false;
    sourceEntryId = null;
    actionNotice = "";
    forkingEntryId = null;
    void tick().then(() => {
      if (session.id === currentSessionId) pinToTail();
    });
  });

  $effect(() => {
    if (!promptIsRunning || waitingForSubagents) {
      observedRunningSince = null;
      observedRunningSessionId = "";
      return;
    }
    if (observedRunningSessionId !== session.id || observedRunningSince === null) {
      observedRunningSessionId = session.id;
      observedRunningSince = Date.now();
    }
    typingClock = Date.now();
    const timer = window.setInterval(() => (typingClock = Date.now()), 1_000);
    return () => window.clearInterval(timer);
  });

  onMount(() => {
    try {
      const savedZoom = Number(localStorage.getItem(`lume-workspace-text-zoom-v1:${session.id}`));
      if (savedZoom >= textZoomMin && savedZoom <= textZoomMax) textZoom = savedZoom;
    } catch {
      // Keep the default size when storage is unavailable.
    }
    const closeControls = (event: PointerEvent) => {
      if (controlsOpen && controlsRoot && !controlsRoot.contains(event.target as Node)) {
        controlsOpen = false;
      }
      if (permissionMenuOpen && permissionMenuRoot && !permissionMenuRoot.contains(event.target as Node)) {
        permissionMenuOpen = false;
      }
      if (zoomOpen && zoomRoot && !zoomRoot.contains(event.target as Node)) {
        zoomOpen = false;
      }
      if (alertMenuOpen && alertsRoot && !alertsRoot.contains(event.target as Node)) {
        alertMenuOpen = false;
      }
    };
    const closeSidebars = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      sourceEntryId = null;
      promptIndexOpen = false;
      alertMenuOpen = false;
    };
    const paneRoot = paneElement;
    const openInlineFile = (event: MouseEvent) => {
      const badge = event.target instanceof Element
        ? event.target.closest<HTMLButtonElement>("button.inline-file-badge[data-local-file]")
        : null;
      if (!badge || !paneRoot?.contains(badge)) return;
      const path = badge.dataset.localFile;
      if (path) void openPath(path).catch((error) => (sendError = String(error).replace(/^Error:\s*/, "")));
    };
    document.addEventListener("pointerdown", closeControls);
    window.addEventListener("keydown", closeSidebars);
    paneRoot?.addEventListener("click", openInlineFile);
    const chat = conversationElement;
    const content = conversationContentElement;
    let lastContentHeight = content?.getBoundingClientRect().height ?? 0;
    const stopScrollbarDrag = () => (draggingScrollbar = false);
    // The reversed scroll container keeps its bottom at scrollTop 0 without
    // writes during streaming. Compensate only while reading older content.
    const resizeObserver = new ResizeObserver(() => {
      if (!chat || !content) return;
      const nextHeight = content.getBoundingClientRect().height;
      const growth = nextHeight - lastContentHeight;
      lastContentHeight = nextHeight;
      if (userScrolledAway && Math.abs(growth) > 0.5) chat.scrollTop -= growth;
    });
    if (content) resizeObserver.observe(content);
    chat?.addEventListener("wheel", handleConversationWheel, { passive: true });
    chat?.addEventListener("touchstart", handleConversationTouchStart, { passive: true });
    chat?.addEventListener("touchmove", handleConversationTouchMove, { passive: true });
    chat?.addEventListener("keydown", handleConversationKeydown);
    chat?.addEventListener("pointerdown", handleConversationPointerDown);
    chat?.addEventListener("pointermove", handleConversationPointerMove);
    window.addEventListener("pointerup", stopScrollbarDrag);
    window.addEventListener("pointercancel", stopScrollbarDrag);
    return () => {
      if (planeLaunchTimer) window.clearTimeout(planeLaunchTimer);
      if (promptSearchTimer) window.clearTimeout(promptSearchTimer);
      for (const flight of outgoingFlights.values()) {
        flight.animation.cancel();
        flight.element.remove();
      }
      outgoingFlights.clear();
      document.removeEventListener("pointerdown", closeControls);
      window.removeEventListener("keydown", closeSidebars);
      paneRoot?.removeEventListener("click", openInlineFile);
      chat?.removeEventListener("wheel", handleConversationWheel);
      chat?.removeEventListener("touchstart", handleConversationTouchStart);
      chat?.removeEventListener("touchmove", handleConversationTouchMove);
      chat?.removeEventListener("keydown", handleConversationKeydown);
      chat?.removeEventListener("pointerdown", handleConversationPointerDown);
      chat?.removeEventListener("pointermove", handleConversationPointerMove);
      window.removeEventListener("pointerup", stopScrollbarDrag);
      window.removeEventListener("pointercancel", stopScrollbarDrag);
      resizeObserver.disconnect();
    };
  });

  function tr(english: string, portuguese: string) {
    return language === "pt-BR" ? portuguese : english;
  }

  async function withNativeDialog<T>(open: () => Promise<T>): Promise<T> {
    let overlaysLowered = false;
    try {
      await setNativeFileDialogActive(true);
      overlaysLowered = true;
    } catch {
      // The picker remains usable on platforms without native z-order control.
    }
    try {
      await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
      return await open();
    } finally {
      if (overlaysLowered) await setNativeFileDialogActive(false).catch(() => undefined);
    }
  }

  const agentAlerts = $derived(collectAgentAlerts([session], language));
  const archivedAgentAlerts = $derived(
    agentAlerts.filter((alert) => dismissedAgentAlertIds.includes(alert.id) || $usageAlertDismissals.includes(alert.id)),
  );
  const systemBanners = $derived.by<SystemBannerItem[]>(() => {
    const items: SystemBannerItem[] = [];
    if (sendError) items.push({ id: "send-error", message: sendError, tone: "error", onDismiss: () => { sendError = ""; } });
    if (controlsError) items.push({ id: "controls-error", message: controlsError, tone: "error", onDismiss: () => { controlsError = ""; } });
    if (historyError) items.push({ id: "history-error", message: historyError, tone: "error", onDismiss: () => { historyError = ""; } });
    if (actionNotice) items.push({ id: "message-action", message: actionNotice, tone: "success", onDismiss: () => { actionNotice = ""; } });
    for (const alert of agentAlerts) {
      if (alert.usage || dismissedAgentAlertIds.includes(alert.id) || $usageAlertDismissals.includes(alert.id)) continue;
      items.push({
        id: alert.id,
        message: alert.message,
        tone: alert.tone,
        duration: alert.tone === "error" ? 7_600 : 6_000,
        onDismiss: () => archiveAgentAlert(alert.id),
      });
    }
    return items;
  });

  // Dismissal is shared by all views of the account's current usage window.
  const usageNotices = $derived(
    agentAlerts.filter((alert) => alert.usage && !dismissedAgentAlertIds.includes(alert.id) && !$usageAlertDismissals.includes(alert.id)),
  );

  function archiveAgentAlert(id: string) {
    usageAlertDismissals.dismiss(id);
    if (dismissedAgentAlertIds.includes(id)) return;
    dismissedAgentAlertIds = [...dismissedAgentAlertIds, id].slice(-120);
  }

  function setTextZoom(value: number) {
    textZoom = Math.round(Math.max(textZoomMin, Math.min(textZoomMax, value)) * 10) / 10;
    try {
      localStorage.setItem(`lume-workspace-text-zoom-v1:${session.id}`, String(textZoom));
    } catch {
      // Zoom still works for this window when storage is unavailable.
    }
  }

  function sessionName() {
    return session.sessionName?.trim() || session.project || session.agentLabel;
  }

  function sourceLabel() {
    if (session.source === "vscode") return tr("Extension", "Extensão");
    if (session.source === "web") return session.sourceApp ? session.sourceApp[0].toUpperCase() + session.sourceApp.slice(1) : "Web";
    return "CLI";
  }

  function sourceIcon() {
    if (session.source === "vscode") return "vscode" as const;
    if (session.source === "web") return session.sourceApp ?? ("browsers" as const);
    return "terminal" as const;
  }

  function activityReportedFiles(activity: SessionActivity): string[] {
    const files = [...activity.files];
    const title = activity.title.trim();
    const titleLooksLikePath = activity.kind === "file"
      && !/^(?:arquivos alterados|alterações da tarefa|\d+\s+arquivos alterados)$/i.test(title)
      && (title.includes("/") || title.includes("\\") || /\.[a-z0-9]{1,8}$/i.test(title));
    if (titleLooksLikePath && !files.includes(title)) files.push(title);
    return files;
  }

  function time(value: number) {
    return new Intl.DateTimeFormat(language, { hour: "2-digit", minute: "2-digit" }).format(new Date(value));
  }

  function promptTime(value: number) {
    return new Intl.DateTimeFormat(language, { dateStyle: "medium", timeStyle: "short" }).format(new Date(value));
  }

  function promptExcerpt(value: string) {
    return cleanPromptTransport(value).replace(/\s+/g, " ").trim().slice(0, 180)
      || tr("Prompt with attachments", "Prompt com anexos");
  }

  function renderMarkdown(key: string, value: string) {
    return markdownCache.render(key, value, (source) => renderWorkspaceMarkdownWithFileBadges(source, renderFileTypeIconHtml, session.workingDirectory));
  }

  function promptForEntry(target: (typeof entries)[number]): SessionActivity | null {
    const targetIndex = entries.findIndex((entry) => entry.id === target.id);
    for (let index = targetIndex; index >= 0; index -= 1) {
      if (entries[index].activity.kind === "prompt") return entries[index].activity;
    }
    return null;
  }

  function turnIdForEntry(target: (typeof entries)[number]): string | undefined {
    const marker = ":turn:";
    const itemMarker = ":item:";
    const start = target.activity.id.indexOf(marker);
    if (start < 0) return undefined;
    const valueStart = start + marker.length;
    const end = target.activity.id.indexOf(itemMarker, valueStart);
    return end > valueStart ? target.activity.id.slice(valueStart, end) : undefined;
  }

  function sourcesForEntry(target: (typeof entries)[number]): ResponseSource[] {
    const targetIndex = entries.findIndex((entry) => entry.id === target.id);
    let startIndex = 0;
    for (let index = targetIndex; index >= 0; index -= 1) {
      if (entries[index].activity.kind === "prompt") {
        startIndex = index;
        break;
      }
    }
    const found = new Map<string, ResponseSource>();
    const add = (source: Omit<ResponseSource, "id">) => {
      const key = `${source.kind}:${source.href ?? source.detail}`.toLocaleLowerCase();
      if (!found.has(key)) found.set(key, { ...source, id: key });
    };
    const collectReferences = (value = "") => {
      for (const match of value.matchAll(/\[([^\]\n]+)\]\(<?((?:https?:\/\/|\/)[^)>\n]+)>?\)/gi)) {
        const href = match[2].trim();
        add({
          kind: href.startsWith("http") ? "web" : "file",
          label: match[1].trim() || href,
          detail: href,
          href: href.startsWith("http") ? href : undefined,
        });
      }
      for (const match of value.matchAll(/https?:\/\/[^\s<>()\]]+/gi)) {
        const href = match[0].replace(/[.,;:!?]+$/, "");
        let label = href;
        try { label = new URL(href).hostname.replace(/^www\./, ""); }
        catch { /* Keep the URL as its label when a streamed citation is incomplete. */ }
        add({ kind: "web", label, detail: href, href });
      }
    };
    collectReferences(target.activity.detail);
    for (const entry of entries.slice(startIndex, targetIndex + 1)) {
      const activity = entry.activity;
      collectReferences(activity.detail);
      if (activity.kind === "tool" && /(?:web|pesquisa|search|browser|browse)/i.test(`${activity.title} ${activity.detail ?? ""}`)) {
        const detail = activity.detail?.trim() || activity.title.trim();
        add({ kind: "search", label: displayText(language, activity.title), detail });
      }
    }
    return [...found.values()];
  }

  async function copyResponse(value: string) {
    try {
      await navigator.clipboard.writeText(value);
      actionNotice = tr("Response copied", "Resposta copiada");
    } catch (error) {
      sendError = String(error).replace(/^Error:\s*/, "");
    }
  }

  async function forkFromEntry(entry: (typeof entries)[number]) {
    const promptActivity = promptForEntry(entry);
    if (!promptActivity?.detail || !entry.activity.detail || forkingEntryId) return;
    forkingEntryId = entry.id;
    try {
      const threadId = await forkSessionFromMessage(
        session.id,
        turnIdForEntry(entry),
        promptActivity.detail,
        entry.activity.detail,
      );
      await onFork?.(threadId);
      actionNotice = tr("Fork created from this response", "Fork criado a partir desta resposta");
    } catch (error) {
      sendError = String(error).replace(/^Error:\s*/, "");
    } finally {
      forkingEntryId = null;
    }
  }

  function composerPlaceholder() {
    if (!session.capabilities.canPrompt) {
      if (session.capabilities.promptUnavailableReason === "monitoring_only") {
        return tr(
          "Legacy Gemini CLI is monitoring-only in Lume",
          "A CLI legada do Gemini é somente monitorada pelo Lume",
        );
      }
      if (session.capabilities.canTakeControl) {
        return tr(
          "Write a prompt to take control of this CLI…",
          "Escreva um prompt para assumir o controle desta CLI…",
        );
      }
      return tr("Open the source to respond", "Abra a origem para responder");
    }
    if (promptIsRunning && canQueue) return tr("Queue the next prompt…", "Coloque o próximo prompt na fila…");
    if (promptIsRunning) return tr("Agent is working…", "O agente está trabalhando…");
    return tr(`Message ${sessionName()}…`, `Mensagem para ${sessionName()}…`);
  }

  function updateOutgoingPrompt(id: number, changes: Partial<OutgoingPrompt>) {
    outgoingPrompts = outgoingPrompts.map((outgoing) => outgoing.id === id ? { ...outgoing, ...changes } : outgoing);
  }

  function cancelOutgoingFlight(id: number) {
    const flight = outgoingFlights.get(id);
    flight?.animation.cancel();
    flight?.element.remove();
    outgoingFlights.delete(id);
  }

  async function animateOutgoingPrompt(outgoing: OutgoingPrompt, sourceRect: DOMRect | null) {
    if (!outgoingPrompts.some((item) => item.id === outgoing.id)) return;
    if (conversationElement) conversationElement.scrollTop = 0;
    const target = conversationContentElement?.querySelector<HTMLElement>(`[data-outgoing-id="${outgoing.id}"] .user-message`);
    const targetRect = target?.getBoundingClientRect();
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (!target || !targetRect || !sourceRect || reducedMotion
      || targetRect.height > Math.max(260, (conversationElement?.clientHeight ?? 0) * .65)) {
      updateOutgoingPrompt(outgoing.id, { flightComplete: true });
      return;
    }

    const appearance = getComputedStyle(target);
    const ghost = target.cloneNode(true) as HTMLElement;
    ghost.style.position = "fixed";
    ghost.style.left = `${targetRect.left}px`;
    ghost.style.top = `${targetRect.top}px`;
    ghost.style.width = `${targetRect.width}px`;
    ghost.style.height = `${targetRect.height}px`;
    ghost.style.maxWidth = "none";
    ghost.style.margin = "0";
    ghost.style.boxSizing = "border-box";
    ghost.style.pointerEvents = "none";
    ghost.style.zIndex = "2147483646";
    ghost.style.visibility = "visible";
    ghost.style.color = appearance.color;
    ghost.style.backgroundColor = appearance.backgroundColor;
    ghost.style.border = appearance.border;
    ghost.style.borderRadius = appearance.borderRadius;
    ghost.style.fontFamily = appearance.fontFamily;
    ghost.style.fontSize = appearance.fontSize;
    ghost.style.lineHeight = appearance.lineHeight;
    ghost.setAttribute("aria-hidden", "true");
    document.body.append(ghost);
    const dx = sourceRect.left - targetRect.left;
    const dy = sourceRect.top - targetRect.top;
    const duration = Math.min(420, Math.max(290, Math.hypot(dx, dy) * .85));
    let animation: Animation;
    try {
      animation = ghost.animate([
        { transform: `translate3d(${dx}px, ${dy}px, 0)`, backgroundColor: "transparent", borderColor: "transparent", opacity: .94 },
        { transform: "translate3d(0, 0, 0)", backgroundColor: appearance.backgroundColor, borderColor: appearance.borderColor, opacity: 1 },
      ], { duration, easing: "cubic-bezier(.16, 1, .3, 1)", fill: "forwards" });
    } catch {
      ghost.remove();
      updateOutgoingPrompt(outgoing.id, { flightComplete: true });
      return;
    }
    outgoingFlights.set(outgoing.id, { animation, element: ghost });
    try {
      await animation.finished;
    } catch {
      // A failed send or a closed pane cancels the flight.
    } finally {
      outgoingFlights.delete(outgoing.id);
      ghost.remove();
      updateOutgoingPrompt(outgoing.id, { flightComplete: true });
    }
  }

  async function sendPrompt() {
    const value = prompt.trim();
    const attachments = promptAttachments;
    if (attachments.length === 0 && /^[/$]/.test(value) && await runSlashCommand(value)) return;
    if ((!value && attachments.length === 0) || !canSend || sending || takingControl) return;
    resetPromptHistory();
    closeMention();
    if (!session.capabilities.canPrompt && session.capabilities.canTakeControl) {
      takeoverConfirm = true;
      await tick();
      takeoverAcceptButton?.focus();
      return;
    }
    const delivery = promptIsRunning ? "queue" : "new_turn";
    const transitionFromFresh = freshChat;
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const sourceRect = composerElement?.querySelector("textarea")?.getBoundingClientRect() ?? null;
    sending = true;
    sendError = "";
    if (transitionFromFresh) composerTransition = reducedMotion ? "in" : "out";
    let outgoing: OutgoingPrompt | null = null;
    if (delivery === "new_turn") {
      outgoing = {
        id: ++outgoingSequence,
        text: value,
        createdAt: Date.now(),
        knownPromptIds: entries.filter((entry) => entry.activity.kind === "prompt").map((entry) => entry.activity.id),
        flightComplete: false,
        accepted: false,
      };
      outgoingPrompts = [...outgoingPrompts, outgoing];
      prompt = "";
      promptAttachments = [];
      userScrolledAway = false;
      reattachOnScroll = false;
      followingTail = true;
    }
    await tick();
    if (transitionFromFresh) {
      if (!reducedMotion) {
        await new Promise<void>((resolve) => window.setTimeout(resolve, 110));
      }
      introDismissed = true;
      composerTransition = "in";
      await tick();
      if (reducedMotion) {
        composerTransition = "idle";
      } else {
        await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
        composerTransition = "idle";
      }
    }
    if (outgoing) void animateOutgoingPrompt(outgoing, sourceRect);
    if (planeLaunchTimer) window.clearTimeout(planeLaunchTimer);
    planeLaunching = !window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (planeLaunching) {
      planeLaunchId += 1;
      planeLaunchTimer = window.setTimeout(() => {
        planeLaunching = false;
        planeLaunchTimer = null;
      }, 450);
    }
    try {
      await submitPrompt(session.id, value, attachments, delivery);
      if (outgoing && session.source !== "web") updateOutgoingPrompt(outgoing.id, { accepted: true });
      else {
        prompt = "";
        promptAttachments = [];
      }
    } catch (error) {
      if (planeLaunchTimer) window.clearTimeout(planeLaunchTimer);
      planeLaunchTimer = null;
      planeLaunching = false;
      if (outgoing) {
        cancelOutgoingFlight(outgoing.id);
        outgoingPrompts = outgoingPrompts.filter((item) => item.id !== outgoing.id);
        if (!prompt.trim()) prompt = value;
        if (!promptAttachments.length) promptAttachments = attachments;
      }
      const connection = agentConnectionMessage(error);
      connectionRequired = connection;
      sendError = connection ? "" : String(error).replace(/^Error:\s*/, "");
    } finally {
      sending = false;
    }
  }

  async function interruptAgentPrompt() {
    if (!session.capabilities.canInterrupt || interrupting) return;
    interrupting = true;
    sendError = "";
    try {
      await interruptPrompt(session.id);
    } catch (error) {
      sendError = String(error).replace(/^Error:\s*/, "");
    } finally {
      interrupting = false;
    }
  }

  async function steerNextPrompt() {
    if (!nextQueuedPrompt || !(canSteer || canRunQueued) || steeringQueued) return;
    steeringQueued = true;
    sendError = "";
    try {
      await steerQueuedPrompt(session.id, nextQueuedPrompt.id);
    } catch (error) {
      sendError = String(error).replace(/^Error:\s*/, "");
    } finally {
      steeringQueued = false;
    }
  }

  function currentModelOption() {
    return modelSettings?.models.find((option) => option.model === selectedModel) ?? null;
  }

  function effortValues() {
    if (session.agent === "claude_code") return claudeEffortValues(claudeModels, claudeModel);
    return currentModelOption()?.supportedReasoningEfforts.map((effort) => effort.value) ?? [];
  }

  function currentEffort() {
    return session.agent === "claude_code" ? claudeEffort : selectedEffort;
  }

  function currentEffortIndex() {
    return Math.max(0, effortValues().indexOf(currentEffort()));
  }

  function effortLabel(value = currentEffort()) {
    return value || tr("Default", "Padrão");
  }

  function effortTone(value: string) {
    switch (value.toLowerCase()) {
      case "ultra": return "#9a70e8";
      case "max": return "#d85c64";
      case "xhigh": return "#e6813f";
      case "high": return "#ed9a48";
      case "medium": return "#eeb561";
      default: return "#4e98ca";
    }
  }

  function effortProgress() {
    const values = effortValues();
    if (values.length < 2) return 0;
    return currentEffortIndex() / (values.length - 1) * 100;
  }

  function chooseEffortIndex(event: Event) {
    const values = effortValues();
    const index = Number((event.currentTarget as HTMLInputElement).value);
    const effort = values[Math.max(0, Math.min(values.length - 1, index))] ?? "";
    if (session.agent === "claude_code") claudeEffort = effort;
    else selectedEffort = effort;
  }

  function chooseModel(model: string) {
    if (modelControlsDisabled) return;
    selectedModel = model;
    const option = modelSettings?.models.find((candidate) => candidate.model === model);
    if (session.agent === "opencode" && model !== modelSettings?.model) {
      selectedEffort = "";
    } else if (option && !option.supportedReasoningEfforts.some((effort) => effort.value === selectedEffort)) {
      selectedEffort = option.defaultReasoningEffort;
    }
    void saveAgentControls();
  }

  function chooseClaudeModel(model: string) {
    if (modelControlsDisabled) return;
    claudeModel = model;
    claudeEffort = claudeEffortForModel(claudeModels, model, claudeEffort);
    void saveAgentControls();
  }

  async function fetchAgentControls() {
      if (session.agent === "codex" || session.agent === "opencode" || session.agent === "antigravity") {
        const [mode, settings] = await Promise.all([
          session.agent === "codex" ? getSessionCollaborationMode(session.id) : Promise.resolve("default" as CollaborationMode),
          getSessionModelSettings(session.id),
        ]);
        collaborationMode = mode;
        modelSettings = settings;
        if (!promptIsRunning) fastMode = isFastServiceTier(settings.serviceTier);
        selectedModel = settings.model;
        const option = settings.models.find((candidate) => candidate.model === settings.model);
        selectedEffort = settings.reasoningEffort
          ?? option?.defaultReasoningEffort
          ?? option?.supportedReasoningEfforts[0]?.value
          ?? "";
      } else if (session.agent === "claude_code") {
        const [settings, fast] = await Promise.all([
          getClaudeSessionModelSettings(session.id),
          getSessionFastMode(session.id),
        ]);
        claudeModels = settings.models;
        claudeModel = settings.model;
        claudeEffort = settings.reasoningEffort ?? "";
        fastMode = fast;
      }
      originalControls ??= session.agent === "claude_code"
        ? { model: claudeModel, effort: claudeEffort, fast: fastMode }
        : { model: selectedModel, effort: selectedEffort, fast: fastMode };
  }

  // The settings the conversation had when this pane first read them: what "reset" returns to.
  let originalControls = $state<ControlsSnapshot | null>(null);

  const controlsChanged = $derived(
    controlsDiffer(
      originalControls,
      session.agent === "claude_code"
        ? { model: claudeModel, effort: claudeEffort, fast: fastMode }
        : { model: selectedModel, effort: selectedEffort, fast: fastMode },
      session.agent === "codex" || session.agent === "claude_code",
    ),
  );

  async function resetAgentControls() {
    const original = originalControls;
    if (!original || modelControlsDisabled || controlsSaving) return;
    if (session.agent === "claude_code") {
      claudeModel = original.model;
      claudeEffort = original.effort;
      await saveAgentControls();
      if (fastMode !== original.fast) await toggleFastMode();
      return;
    }
    selectedModel = original.model;
    selectedEffort = original.effort;
    await saveAgentControls();
    if (session.agent === "codex" && fastMode !== original.fast) await toggleFastMode();
  }

  async function loadAgentControls() {
    controlsLoading = true;
    controlsError = "";
    try {
      await fetchAgentControls();
    } catch (error) {
      controlsError = String(error).replace(/^Error:\s*/, "");
    } finally {
      controlsLoading = false;
    }
  }

  // The picker names the model from the start. Reading is quiet: no spinner, and a
  // failure only leaves the label generic until the picker is opened.
  let preloadedControlsSessionId = "";
  $effect(() => {
    if (!visible || !supportsAgentControls || session.id === preloadedControlsSessionId) return;
    // Codex and OpenCode only answer for conversations Lume controls; Claude's read works for any.
    if (session.agent !== "claude_code" && session.controlOrigin !== "lume") return;
    const sessionId = session.id;
    preloadedControlsSessionId = sessionId;
    void fetchAgentControls().catch(() => {
      if (preloadedControlsSessionId === sessionId) preloadedControlsSessionId = "";
    });
  });

  async function toggleAgentControls() {
    controlsOpen = !controlsOpen;
    if (!controlsOpen || session.controlOrigin !== "lume") return;
    await loadAgentControls();
  }

  async function toggleFastMode() {
    if ((session.agent !== "codex" && session.agent !== "claude_code") || runtimeControlsDisabled || fastSaving) return;
    fastSaving = true;
    sendError = "";
    try {
      fastMode = await setSessionFastMode(session.id, !fastMode);
      if (session.agent === "claude_code") {
        const settings = await getClaudeSessionModelSettings(session.id);
        claudeModels = settings.models;
        claudeModel = settings.model;
        claudeEffort = settings.reasoningEffort ?? "";
      } else if (modelSettings) {
        modelSettings = { ...modelSettings, serviceTier: fastMode ? "priority" : "default" };
      }
    } catch (error) {
      const message = String(error).replace(/^Error:\s*/, "");
      sendError = /(?:command|comando).*set_session_fast_mode.*(?:not found|não encontrado)/i.test(message)
        ? tr("Command not found.", "Comando não encontrado.")
        : message;
    } finally {
      fastSaving = false;
    }
  }

  async function toggleCollaborationMode() {
    if (session.agent !== "codex" || runtimeControlsDisabled || modeSaving) return;
    modeSaving = true;
    sendError = "";
    try {
      collaborationMode = await setSessionCollaborationMode(
        session.id,
        collaborationMode === "plan" ? "default" : "plan",
      );
    } catch (error) {
      sendError = String(error).replace(/^Error:\s*/, "");
    } finally {
      modeSaving = false;
    }
  }

  async function saveAgentControls() {
    if (modelControlsDisabled) return;
    controlsSaving = true;
    controlsError = "";
    try {
      if (session.agent === "codex" || session.agent === "opencode" || session.agent === "antigravity") {
        if ((session.agent !== "antigravity" && !selectedModel) || (session.agent === "codex" && !selectedEffort)) return;
        const savedSettings = await setSessionModelSettings(session.id, selectedModel, selectedEffort);
        modelSettings = savedSettings;
        selectedModel = savedSettings.model;
        selectedEffort = savedSettings.reasoningEffort
          ?? savedSettings.models.find((option) => option.model === savedSettings.model)?.defaultReasoningEffort
          ?? "";
        if (!promptIsRunning) fastMode = isFastServiceTier(modelSettings.serviceTier);
      } else if (session.agent === "claude_code") {
        const settings = await setClaudeSessionModelSettings(
          session.id,
          claudeModel.trim() || undefined,
          claudeEffort || undefined,
        );
        claudeModels = settings.models;
        claudeModel = settings.model;
        claudeEffort = settings.reasoningEffort ?? "";
        fastMode = await getSessionFastMode(session.id);
      }
    } catch (error) {
      controlsError = String(error).replace(/^Error:\s*/, "");
      if (session.agent === "opencode") {
        try {
          modelSettings = await getSessionModelSettings(session.id);
          selectedModel = modelSettings.model;
          selectedEffort = modelSettings.reasoningEffort ?? "";
        } catch { /* Keep the original provider error visible. */ }
      }
    } finally {
      controlsSaving = false;
    }
  }

  async function changeAgentMode(mode: string) {
    if (modelControlsDisabled) return;
    controlsSaving = true;
    controlsError = "";
    try {
      modelSettings = await setSessionAgentMode(session.id, mode);
      selectedModel = modelSettings.model;
      selectedEffort = modelSettings.reasoningEffort ?? "";
    } catch (error) {
      controlsError = String(error).replace(/^Error:\s*/, "");
    } finally {
      controlsSaving = false;
    }
  }

  async function confirmTakeover() {
    const value = prompt.trim();
    if (
      (!value && promptAttachments.length === 0)
      || !session.capabilities.canTakeControl
      || takingControl
    ) return;
    takingControl = true;
    sendError = "";
    try {
      await takeControlSession(session.id, value, promptAttachments);
      prompt = "";
      promptAttachments = [];
      takeoverConfirm = false;
    } catch (error) {
      sendError = String(error).replace(/^Error:\s*/, "");
    } finally {
      takingControl = false;
    }
  }

  async function resolveWriterConflict(action: "keep_lume" | "open_branch") {
    if (!externalWriterConflict || writerConflictBusy) return;
    writerConflictBusy = action;
    writerConflictError = "";
    try {
      await onResolveExternalWriterConflict?.(action);
    } catch (reason) {
      const detail = String(reason).replace(/^Error:\s*/, "");
      writerConflictError = detail;
      if (!externalWriterConflict) sendError = detail;
    } finally {
      writerConflictBusy = null;
    }
  }

  $effect(() => {
    if (!externalWriterConflict) {
      writerConflictError = "";
      return;
    }
    void tick().then(() => writerConflictPrimaryButton?.focus());
  });

  let agentCommands = $state<AgentSlashCommand[]>([]);
  let agentCommandsLoading = $state(false);
  let agentCommandsSessionId = "";
  let agentCommandsFailedAt = 0;

  $effect(() => {
    const sessionId = session.id;
    if (slashCommandQuery(prompt) === null) return;
    const retrying = agentCommandsFailedAt > 0 && Date.now() - agentCommandsFailedAt >= 3000;
    if (sessionId === agentCommandsSessionId && !retrying) return;
    agentCommandsSessionId = sessionId;
    agentCommandsFailedAt = 0;
    agentCommandsLoading = true;
    agentCommands = [];
    loadAgentSlashCommands(sessionId, listSessionSlashCommands)
      .then((commands) => {
        if (session.id === sessionId) agentCommands = commands;
      })
      .catch(() => {
        agentCommandsFailedAt = Date.now();
      })
      .finally(() => {
        if (session.id === sessionId) agentCommandsLoading = false;
      });
  });

  function availableSlashCommands(): SlashCommand[] {
    const commands = agentSlashCommands(agentCommands, session.agent);
    const lumeCommands: SlashCommand[] = [];
    if (session.agent === "codex" && !promptIsRunning) {
      lumeCommands.push(
        { name: "lume-default", description: "Switch Codex to Default mode", source: "lume", prefix: "/", action: "default" },
      );
    }
    if (promptIsRunning && session.capabilities.canInterrupt) {
      lumeCommands.push({ name: "lume-interrupt", description: "Interrupt the current prompt", source: "lume", prefix: "/", action: "interrupt" });
    }
    if (canSteer) {
      lumeCommands.push({ name: "lume-steer", description: "Steer the next queued prompt now", source: "lume", prefix: "/", action: "steer" });
    }
    lumeCommands.push(
      { name: "lume-zoom-in", description: "Increase chat text size", source: "lume", prefix: "/", action: "zoom-in" },
      { name: "lume-zoom-out", description: "Decrease chat text size", source: "lume", prefix: "/", action: "zoom-out" },
    );
    if (onClose) {
      lumeCommands.push({ name: "lume-close", description: "Close this pane", source: "lume", prefix: "/", action: "close" });
    }
    return [...commands, ...lumeCommands];
  }

  const filteredSlashCommands = $derived(
    slashMenuDismissed ? [] : filterSlashCommands(availableSlashCommands(), slashCommandQuery(prompt)),
  );
  const slashMenuVisible = $derived(
    !slashMenuDismissed
    && slashCommandQuery(prompt) !== null
    && (agentCommandsLoading || filteredSlashCommands.length > 0),
  );

  async function selectSlashCommand(command: SlashCommand) {
    prompt = slashCommandText(command);
    slashCommandIndex = 0;
    slashMenuDismissed = true;
    await tick();
    promptInput?.focus();
    promptInput?.setSelectionRange(prompt.length, prompt.length);
  }

  async function revealSelectedSlashCommand() {
    await tick();
    slashCommandMenu
      ?.querySelector<HTMLElement>(`[data-slash-index="${slashCommandIndex}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }

  async function setCollaborationMode(mode: CollaborationMode) {
    if (collaborationMode !== mode) await toggleCollaborationMode();
  }

  async function runSlashCommand(value: string) {
    const action = findSlashCommand(availableSlashCommands(), value)?.action;
    if (!action) return false;
    switch (action) {
      case "model":
        if (!supportsAgentControls) return false;
        if (!controlsOpen) await toggleAgentControls();
        break;
      case "plan":
        // The button is gone, so the one command both enters Plan mode and leaves it.
        await toggleCollaborationMode();
        break;
      case "default":
        await setCollaborationMode("default");
        break;
      case "interrupt":
        await interruptAgentPrompt();
        break;
      case "steer":
        await steerNextPrompt();
        break;
      case "zoom-in":
        setTextZoom(textZoom + 0.1);
        break;
      case "zoom-out":
        setTextZoom(textZoom - 0.1);
        break;
      case "close":
        onClose?.();
        break;
      default:
        return false;
    }
    prompt = "";
    slashMenuDismissed = false;
    return true;
  }

  async function loadSessionPermission() {
    permissionMenuLoading = true;
    try {
      sessionPermission = await getSessionPermissionMode(session.id);
    } catch {
      // The selector stays hidden until the CLI answers; a prompt still runs.
    } finally {
      permissionMenuLoading = false;
    }
  }

  async function toggleSessionPermissionMenu() {
    permissionMenuOpen = !permissionMenuOpen;
    if (permissionMenuOpen) await loadSessionPermission();
  }

  async function chooseSessionPermission(mode: string) {
    if (permissionMenuSaving || mode === sessionPermission?.mode) {
      permissionMenuOpen = false;
      return;
    }
    permissionMenuSaving = true;
    sendError = "";
    try {
      sessionPermission = await setSessionPermissionMode(session.id, mode);
      permissionMenuOpen = false;
    } catch (error) {
      sendError = String(error).replace(/^Error:\s*/, "");
    } finally {
      permissionMenuSaving = false;
    }
  }

  // Plan mode no longer has a button, so its state is read when the conversation opens.
  let loadedCollaborationSessionId = "";
  $effect(() => {
    if (session.agent !== "codex" || session.controlOrigin !== "lume" || session.id === loadedCollaborationSessionId) return;
    loadedCollaborationSessionId = session.id;
    void getSessionCollaborationMode(session.id).then((mode) => { collaborationMode = mode; }).catch(() => undefined);
  });

  // Each conversation reads its mode once; opening the menu refreshes it.
  $effect(() => {
    if (!supportsPermissionPicker || session.id === loadedPermissionSessionId) return;
    loadedPermissionSessionId = session.id;
    sessionPermission = null;
    permissionMenuOpen = false;
    void loadSessionPermission();
  });

  function permissionActionLabel(action: PermissionAction) {
    return {
      allow_once: tr("Allow", "Permitir"),
      allow_session: tr("For this session", "Nesta sessão"),
      deny: tr("Deny", "Recusar"),
      open_source: tr("Open source", "Abrir origem"),
    }[action];
  }

  async function resolvePermission(action: PermissionAction) {
    const request = pendingPermission;
    if (!request || permissionBusy) return;
    permissionBusy = true;
    sendError = "";
    try {
      if (action === "open_source") await openSessionSource(session.id);
      else await decidePermission(session.id, request.id, action);
    } catch (error) {
      sendError = String(error).replace(/^Error:\s*/, "");
    } finally {
      permissionBusy = false;
    }
  }

  async function submitQuestionAnswers(answers: QuestionAnswer[]) {
    if (!pendingQuestion || questionSending) return;
    questionSending = true;
    sendError = "";
    try {
      await answerQuestion(session.id, pendingQuestion.id, answers);
      questionSelections = {};
    } catch (error) {
      sendError = String(error).replace(/^Error:\s*/, "");
    } finally {
      questionSending = false;
    }
  }

  async function chooseQuestionOption(questionId: string, label: string) {
    if (!pendingQuestion) return;
    questionSelections = { ...questionSelections, [questionId]: label };
    if (pendingQuestion.questions.length === 1) {
      await submitQuestionAnswers([{ questionId, answers: [label] }]);
    }
  }

  async function submitSelectedQuestionAnswers() {
    if (!pendingQuestion) return;
    const answers = pendingQuestion.questions
      .filter((question: InteractiveQuestion) => questionSelections[question.id])
      .map((question: InteractiveQuestion) => ({ questionId: question.id, answers: [questionSelections[question.id]] }));
    if (answers.length !== pendingQuestion.questions.length) {
      sendError = tr("Choose one option for each question.", "Escolha uma opção para cada pergunta.");
      return;
    }
    await submitQuestionAnswers(answers);
  }

  async function loadPromptHistory() {
    if (promptHistoryLoaded || !session.nativeSessionId) return;
    promptHistoryLoaded = true;
    const requestedSessionId = session.id;
    const prompts: WorkspacePromptIndexEntry[] = [];
    let cursor: WorkspacePromptIndexEntry | undefined;
    let hasMore = true;
    try {
      // Ten pages (300 prompts) reach back further than anyone scrolls with the arrows.
      for (let page = 0; page < 10 && hasMore; page += 1) {
        const result = await loadWorkspacePromptIndexPage(requestedSessionId, cursor?.createdAt, cursor?.id);
        prompts.push(...result.prompts);
        cursor = result.prompts.at(-1);
        hasMore = result.hasMore && Boolean(cursor);
      }
    } catch {
      promptHistoryLoaded = false;
      return;
    }
    if (session.id !== requestedSessionId) return;
    promptHistory = { ...promptHistory, entries: historyEntries(prompts.map((entry) => entry.detail)) };
    promptHistoryHasMore = hasMore;
  }

  function resetPromptHistory() {
    promptHistory = emptyPromptHistory();
    promptHistoryLoaded = false;
    promptHistoryHasMore = false;
  }

  async function browsePromptHistory(direction: 1 | -1) {
    if (direction === 1) await loadPromptHistory();
    const step = stepPromptHistory(promptHistory, direction, prompt);
    if (!step) return;
    promptHistory = step.history;
    prompt = step.text;
    closeMention();
    slashMenuDismissed = true;
    await tick();
    promptInput?.setSelectionRange(prompt.length, prompt.length);
  }

  function closeMention() {
    mentionRequest += 1;
    activeMention = null;
    mentionResults = [];
    mentionIndex = 0;
  }

  async function refreshMention() {
    const mention = promptInput ? mentionAtCaret(prompt, promptInput.selectionStart ?? prompt.length) : null;
    if (!mention) {
      if (activeMention) closeMention();
      return;
    }
    if (activeMention?.start === mention.start && activeMention.query === mention.query) return;
    activeMention = mention;
    const request = ++mentionRequest;
    try {
      const results = await searchSessionPaths(session.id, mention.query);
      if (request !== mentionRequest) return;
      mentionResults = results;
      mentionIndex = 0;
    } catch {
      if (request === mentionRequest) mentionResults = [];
    }
  }

  async function selectMention(result: PathMention) {
    if (!activeMention || !promptInput) return;
    const applied = applyMention(prompt, promptInput.selectionStart ?? prompt.length, activeMention, result.path, result.isDirectory);
    prompt = applied.text;
    closeMention();
    await tick();
    promptInput?.focus();
    promptInput?.setSelectionRange(applied.caret, applied.caret);
    // A folder keeps the menu open on its contents, like the CLI.
    if (result.isDirectory) void refreshMention();
  }

  async function revealSelectedMention() {
    await tick();
    mentionMenu?.querySelector<HTMLElement>(`[data-mention-index="${mentionIndex}"]`)?.scrollIntoView({ block: "nearest" });
  }

  function handleComposerKeydown(event: KeyboardEvent) {
    if (activeMention && mentionResults.length) {
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        const direction = event.key === "ArrowDown" ? 1 : -1;
        mentionIndex = (mentionIndex + direction + mentionResults.length) % mentionResults.length;
        void revealSelectedMention();
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        closeMention();
        return;
      }
      if ((event.key === "Enter" || event.key === "Tab") && !event.shiftKey && !event.isComposing) {
        event.preventDefault();
        void selectMention(mentionResults[Math.min(mentionIndex, mentionResults.length - 1)]);
        return;
      }
    }
    const slashCommands = filteredSlashCommands;
    if (slashCommands.length) {
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        const direction = event.key === "ArrowDown" ? 1 : -1;
        slashCommandIndex = (slashCommandIndex + direction + slashCommands.length) % slashCommands.length;
        void revealSelectedSlashCommand();
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        slashMenuDismissed = true;
        return;
      }
      if ((event.key === "Enter" || event.key === "Tab") && !event.shiftKey && !event.isComposing) {
        event.preventDefault();
        void selectSlashCommand(slashCommands[Math.min(slashCommandIndex, slashCommands.length - 1)]);
        return;
      }
    }
    if ((event.key === "ArrowUp" || event.key === "ArrowDown") && !event.shiftKey && !event.altKey && !event.metaKey && !event.ctrlKey && !event.isComposing) {
      const direction = event.key === "ArrowUp" ? 1 : -1;
      const target = event.currentTarget as HTMLTextAreaElement;
      const collapsed = target.selectionStart === target.selectionEnd;
      const browsing = promptHistory.index >= 0;
      if (collapsed && (direction === 1 || browsing) && caretOnEdgeLine(prompt, target.selectionStart, direction)) {
        event.preventDefault();
        void browsePromptHistory(direction);
        return;
      }
    }
    if (event.key !== "Enter" || event.shiftKey || event.isComposing) return;
    event.preventDefault();
    void sendPrompt();
  }

  function handlePromptInput() {
    slashCommandIndex = 0;
    slashMenuDismissed = false;
    if (promptHistory.index >= 0) promptHistory = { ...promptHistory, index: -1, draft: "" };
    void refreshMention();
  }

  async function previewLocalImage(path: string) {
    return createImagePreview(await readLocalImageDataUrl(path), language);
  }

  async function chooseAttachments() {
    if (!canAttach || sending || takingControl || promptAttachments.length >= 4) return;
    sendError = "";
    try {
      const selected = await withNativeDialog(() => openDialog({ multiple: true, directory: false }));
      const paths = (Array.isArray(selected) ? selected : selected ? [selected] : [])
        .filter((path): path is string => typeof path === "string")
        .slice(0, 4 - promptAttachments.length);
      const prepared = await Promise.all(paths.map(async (path) => ({
        name: path.split(/[\\/]/).pop() || "file",
        mimeType: "",
        path,
        previewDataUrl: isImageAttachmentPath(path) ? await previewLocalImage(path) : "",
      })));
      promptAttachments = [...promptAttachments, ...prepared];
    } catch (reason) {
      sendError = String(reason).replace(/^Error:\s*/, "");
    }
  }

  async function pasteAttachments(event: ClipboardEvent) {
    if (!clipboardHasFile(event) && !clipboardHasImage(event) && !clipboardMayContainImage(event)) return;
    event.preventDefault();
    if (!canAttach || sending || takingControl) {
      sendError = tr(
        "Files can only be attached when this session can receive a prompt.",
        "Arquivos só podem ser anexados quando esta sessão puder receber um prompt.",
      );
      return;
    }
    sendError = "";
    try {
      let { files, paths } = collectClipboardFiles(event);
      if (!files.length && !paths.length) ({ files, paths } = await collectClipboardImages(event, language));
      const available = 4 - promptAttachments.length;
      const prepared: PromptAttachmentInput[] = [];
      for (const [index, file] of files.slice(0, available).entries()) {
        prepared.push(isImageAttachmentFile(file)
          ? await prepareClipboardImage(file, index, language)
          : await prepareClipboardFile(file, index, language));
      }
      for (const path of paths.slice(0, available - prepared.length)) {
        prepared.push({
          name: path.split(/[\\/]/).pop() || "file",
          mimeType: "",
          path,
          previewDataUrl: isImageAttachmentPath(path) ? await previewLocalImage(path) : "",
        });
      }
      promptAttachments = [...promptAttachments, ...prepared];
    } catch (reason) {
      sendError = String(reason).replace(/^Error:\s*/, "");
    }
  }

  function removeAttachment(index: number) {
    promptAttachments = promptAttachments.filter((_, current) => current !== index);
  }

  function pinToTail() {
    if (!followingTail || !conversationElement) return;
    conversationElement.scrollTop = 0;
  }

  function trackConversationScroll() {
    if (!conversationElement) return;
    if (!userScrolledAway) return;
    if (reattachOnScroll && conversationElement.scrollTop >= -2) {
      userScrolledAway = false;
      reattachOnScroll = false;
      followingTail = true;
    }
  }

  function stopFollowingTail() {
    userScrolledAway = true;
    reattachOnScroll = false;
    followingTail = false;
  }

  function handleConversationWheel(event: WheelEvent) {
    if (event.deltaY < 0) stopFollowingTail();
    else if (event.deltaY > 0 && userScrolledAway) reattachOnScroll = true;
  }

  function handleConversationTouchStart(event: TouchEvent) {
    touchStartY = event.touches[0]?.clientY ?? 0;
  }

  function handleConversationTouchMove(event: TouchEvent) {
    const currentY = event.touches[0]?.clientY ?? touchStartY;
    if (currentY > touchStartY + 4) stopFollowingTail();
    else if (currentY < touchStartY - 4 && userScrolledAway) reattachOnScroll = true;
    touchStartY = currentY;
  }

  function handleConversationKeydown(event: KeyboardEvent) {
    if (event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement) return;
    if (["ArrowUp", "PageUp", "Home"].includes(event.key) || (event.key === " " && event.shiftKey)) stopFollowingTail();
    if (["ArrowDown", "PageDown"].includes(event.key) || event.key === " ") reattachOnScroll = true;
    if (event.key === "End") {
      userScrolledAway = false;
      reattachOnScroll = false;
      followingTail = true;
      pinToTail();
    }
  }

  function handleConversationPointerDown(event: PointerEvent) {
    if (!conversationElement) return;
    const bounds = conversationElement.getBoundingClientRect();
    draggingScrollbar = event.clientX >= bounds.right - 20 && event.clientX <= bounds.right
      && event.clientY >= bounds.top && event.clientY <= bounds.bottom;
    if (draggingScrollbar) {
      stopFollowingTail();
      reattachOnScroll = true;
    }
  }

  function handleConversationPointerMove(event: PointerEvent) {
    if (draggingScrollbar && event.buttons === 1) trackConversationScroll();
  }

  async function revealEarlierMessages() {
    if (historyLoading) return;
    stopFollowingTail();
    historyError = "";
    if (hiddenCount > 0) {
      visibleFeedLimit += 60;
    } else {
      const earliest = olderCursor ?? (
        conversationActivities
          .filter((activity) => activity.kind === "prompt" || (activity.kind === "message" && activity.status !== "running"))
          .at(0) ?? conversationActivities.at(0)
      );
      if (!earliest) {
        historyHasMore = false;
        return;
      }
      const requestedSessionId = session.id;
      historyLoading = true;
      try {
        const page = await loadWorkspaceConversationPage(requestedSessionId, earliest.createdAt, earliest.id);
        if (session.id !== requestedSessionId) return;
        if (page.activities.length) {
          olderActivities = [...page.activities, ...olderActivities];
          olderCursor = { createdAt: page.activities[0].createdAt, id: page.activities[0].id };
          visibleFeedLimit += 60;
        }
        historyHasMore = page.hasMore;
      } catch (error) {
        if (session.id === requestedSessionId) historyError = String(error).replace(/^Error:\s*/, "");
      } finally {
        historyLoading = false;
      }
    }
  }

  async function loadOlderPromptIndex() {
    if (promptIndexLoading || !session.nativeSessionId || (promptIndexInitialized && !promptIndexHasMore)) return;
    const requestedSessionId = session.id;
    promptIndexLoading = true;
    promptIndexError = "";
    try {
      const page = await loadWorkspacePromptIndexPage(
        requestedSessionId,
        promptIndexCursor?.createdAt,
        promptIndexCursor?.id,
      );
      if (session.id !== requestedSessionId) return;
      indexedPrompts = [...indexedPrompts, ...page.prompts];
      const oldest = page.prompts.at(-1);
      if (oldest) promptIndexCursor = { createdAt: oldest.createdAt, id: oldest.id };
      promptIndexHasMore = page.hasMore && Boolean(oldest);
      promptIndexInitialized = true;
    } catch (error) {
      if (session.id === requestedSessionId) promptIndexError = String(error).replace(/^Error:\s*/, "");
    } finally {
      if (session.id === requestedSessionId) promptIndexLoading = false;
    }
  }

  async function loadPromptSearch(reset = false, generation = promptSearchGeneration) {
    const query = normalizedPromptSearch;
    if (!query || (!reset && (promptSearchLoading || !promptSearchHasMore))) return;
    const requestedSessionId = session.id;
    const requestedQuery = query;
    const cursor = reset ? null : promptSearchCursor;
    promptSearchLoading = true;
    promptIndexError = "";
    try {
      const page = await loadWorkspacePromptIndexPage(
        requestedSessionId,
        cursor?.createdAt,
        cursor?.id,
        requestedQuery,
      );
      if (session.id !== requestedSessionId || normalizedPromptSearch !== requestedQuery || generation !== promptSearchGeneration) return;
      promptSearchResults = reset ? page.prompts : [...promptSearchResults, ...page.prompts];
      const oldest = page.prompts.at(-1);
      promptSearchCursor = oldest ? { createdAt: oldest.createdAt, id: oldest.id } : cursor;
      promptSearchHasMore = page.hasMore && Boolean(oldest);
    } catch (error) {
      if (session.id === requestedSessionId && normalizedPromptSearch === requestedQuery && generation === promptSearchGeneration) {
        promptIndexError = String(error).replace(/^Error:\s*/, "");
      }
    } finally {
      if (session.id === requestedSessionId && normalizedPromptSearch === requestedQuery && generation === promptSearchGeneration) {
        promptSearchLoading = false;
      }
    }
  }

  function schedulePromptSearch() {
    if (promptSearchTimer) window.clearTimeout(promptSearchTimer);
    const generation = ++promptSearchGeneration;
    promptSearchResults = [];
    promptSearchCursor = null;
    promptSearchHasMore = false;
    promptIndexError = "";
    if (!normalizedPromptSearch) {
      promptSearchLoading = false;
      return;
    }
    promptSearchTimer = window.setTimeout(() => {
      promptSearchTimer = null;
      void loadPromptSearch(true, generation);
    }, 240);
  }

  function clearPromptSearch() {
    promptSearchQuery = "";
    schedulePromptSearch();
    promptSearchInputElement?.focus();
  }

  function togglePromptIndex() {
    promptIndexOpen = !promptIndexOpen;
    if (!promptIndexOpen) return;
    sourceEntryId = null;
    if (!promptIndexInitialized) void loadOlderPromptIndex();
    void tick().then(() => promptSearchInputElement?.focus());
  }

  function matchesIndexedPrompt(activity: SessionActivity, target: WorkspacePromptIndexEntry) {
    if (activity.kind !== "prompt") return false;
    if (activity.id === target.id) return true;
    const preview = cleanPromptTransport(target.detail);
    return Boolean(preview)
      && Math.abs(activity.createdAt - target.createdAt) < 60_000
      && cleanPromptTransport(activity.detail).startsWith(preview);
  }

  async function loadHistoryThroughPrompt(target: WorkspacePromptIndexEntry, requestedSessionId: string) {
    const earliest = olderCursor ?? (
      conversationActivities
        .filter((activity) => activity.kind === "prompt" || (activity.kind === "message" && activity.status !== "running"))
        .at(0) ?? conversationActivities.at(0)
    );
    if (!earliest || !session.nativeSessionId || historyHasMore === false) return;
    let cursor = { createdAt: earliest.createdAt, id: earliest.id };
    let collected: SessionActivity[] = [];
    let more = true;
    historyLoading = true;
    try {
      while (more && session.id === requestedSessionId) {
        const page = await loadWorkspaceConversationPage(requestedSessionId, cursor.createdAt, cursor.id);
        if (session.id !== requestedSessionId) return;
        more = page.hasMore && page.activities.length > 0;
        if (!page.activities.length) break;
        collected = [...page.activities, ...collected];
        cursor = { createdAt: page.activities[0].createdAt, id: page.activities[0].id };
        if (page.activities.some((activity) => matchesIndexedPrompt(activity, target))) break;
      }
      if (session.id !== requestedSessionId) return;
      if (collected.length) {
        olderActivities = [...collected, ...olderActivities];
        olderCursor = cursor;
      }
      historyHasMore = more;
    } finally {
      if (session.id === requestedSessionId) historyLoading = false;
    }
  }

  async function jumpToPrompt(target: WorkspacePromptIndexEntry) {
    if (jumpingToPromptId || historyLoading) return;
    const requestedSessionId = session.id;
    jumpingToPromptId = target.id;
    promptIndexError = "";
    stopFollowingTail();
    try {
      let match = feed.findIndex((item) => item.kind === "entry" && matchesIndexedPrompt(item.entry.activity, target));
      if (match < 0) {
        await loadHistoryThroughPrompt(target, requestedSessionId);
        await tick();
        match = feed.findIndex((item) => item.kind === "entry" && matchesIndexedPrompt(item.entry.activity, target));
      }
      if (session.id !== requestedSessionId) return;
      if (match >= 0) {
        visibleFeedLimit = Math.max(visibleFeedLimit, feed.length - match);
        await tick();
        const matchingItem = feed[match];
        const activityId = matchingItem.kind === "entry" ? matchingItem.entry.activity.id : target.id;
        const row = [...(conversationContentElement?.querySelectorAll<HTMLElement>("[data-prompt-id]") ?? [])]
          .find((element) => element.dataset.promptId === activityId);
        if (row && conversationElement) {
          const top = row.getBoundingClientRect().top - conversationElement.getBoundingClientRect().top;
          conversationElement.scrollTop += top - 24;
          activePromptId = activityId;
          row.focus({ preventScroll: true });
          promptIndexOpen = false;
          return;
        }
      }
      if (session.id === requestedSessionId) {
        promptIndexError = tr("This prompt could not be found in the available history.", "Este prompt não foi encontrado no histórico disponível.");
      }
    } catch (error) {
      if (session.id === requestedSessionId) promptIndexError = String(error).replace(/^Error:\s*/, "");
    } finally {
      if (session.id === requestedSessionId) jumpingToPromptId = null;
    }
  }

  function scrollToLatest() {
    userScrolledAway = false;
    reattachOnScroll = false;
    followingTail = true;
    pinToTail();
  }

  function toggleFileSummary(id: string) {
    expandedFileSummaries = expandedFileSummaries.includes(id)
      ? expandedFileSummaries.filter((value) => value !== id)
      : [...expandedFileSummaries, id];
  }

  function motionDuration(duration: number) {
    return typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches ? 0 : duration;
  }
</script>

<article
  bind:this={paneElement}
  class:focused
  class:has-environments={hasEnvironments}
  class="session-pane status-{session.status}"
  data-workspace-pane={session.id}
  style:--workspace-chat-font-adjust={`${(textZoom - 1) * 9}px`}
  style:--workspace-chat-small-adjust={`${(textZoom - 1) * 8}px`}
  style:--workspace-chat-tiny-adjust={`${(textZoom - 1) * 7}px`}
  tabindex="-1"
  onpointerdown={() => onFocus?.()}
  onfocusin={() => onFocus?.()}
>
  <SystemBannerStack items={systemBanners} contained {language} dismissLabel={tr("Dismiss", "Fechar")} />
  <header
    class="pane-header"
    role="group"
    aria-label={tr("Session header", "Cabeçalho da sessão")}
  >
    <span class="agent-mark"><ThreadAvatar seed={session.nativeSessionId || session.sessionName || session.id} label={sessionName()} size={38} /></span>
    <span class="pane-identity">
      <strong>{sessionName()}</strong>
      <small title={session.workingDirectory}><BrandIcon name={session.agent} size={10} />{session.agentLabel} · {session.project}{#if session.forkedFrom}<span class="fork-mark" title={tr(`Forked from conversation ${session.forkedFrom.slice(0, 8)}`, `Fork da conversa ${session.forkedFrom.slice(0, 8)}`)}><LumeIcon name="fork" size={10} />{tr("Fork", "Fork")}</span>{/if}</small>
      {#if visible && onOpenRepository}<SessionRepositoryBadge {session} {language} onOpen={onOpenRepository} />{/if}
    </span>
    {#if session.controlOrigin === "external"}
      <span class="source-badge" title={sourceLabel()} aria-label={sourceLabel()}>
        <BrandIcon name={sourceIcon()} size={11} />
        <span class="badge-label">{sourceLabel()}</span>
      </span>
    {/if}
    <span class="status-badge status-{waitingForSubagents ? 'subagents' : session.status}" title={presentedStatus} aria-label={presentedStatus}><i></i><span class="badge-label">{presentedStatus}</span></span>
      <div class="pane-actions">
        {#if archivedAgentAlerts.length}
          <div class="agent-alerts" bind:this={alertsRoot}>
            <button
              class:active={alertMenuOpen}
              class="agent-alert-trigger"
              type="button"
              title={tr("Session alerts", "Alertas da sessão")}
              aria-label={tr(`${agentAlerts.length} session alerts`, `${agentAlerts.length} alertas da sessão`)}
              aria-expanded={alertMenuOpen}
              onclick={() => (alertMenuOpen = !alertMenuOpen)}
            >
              <LumeIcon name="warning" size={17} strokeWidth={1.8} />
              <span>{agentAlerts.length}</span>
            </button>
            {#if alertMenuOpen}
              <section class="agent-alert-menu" aria-label={tr("Session alerts", "Alertas da sessão")}>
                <header><strong>{tr("Session alerts", "Alertas da sessão")}</strong><small>{agentAlerts.length}</small></header>
                <div>
                  {#each agentAlerts as alert (alert.id)}
                    <article class="tone-{alert.tone}">
                      <span><LumeIcon name="warning" size={13} strokeWidth={1.9} /></span>
                      <p>{alert.message}</p>
                    </article>
                  {/each}
                </div>
              </section>
            {/if}
          </div>
        {/if}
        <button
          class:active={promptIndexOpen}
          type="button"
          title={tr("Browse prompts", "Navegar pelos prompts")}
          aria-label={tr("Browse prompts", "Navegar pelos prompts")}
          aria-expanded={promptIndexOpen}
          aria-controls={`workspace-prompt-index-${session.id}`}
          onclick={togglePromptIndex}
        ><WorkspaceChatIcon name="prompts" size={18} active={promptIndexOpen} /></button>
        <div class="text-zoom" bind:this={zoomRoot}>
          <button
            class:active={zoomOpen}
            type="button"
            title={tr("Chat text size", "Tamanho do texto do chat")}
            aria-label={tr("Chat text size", "Tamanho do texto do chat")}
            aria-expanded={zoomOpen}
            onclick={() => (zoomOpen = !zoomOpen)}
          ><WorkspaceChatIcon name="text-size" size={18} active={zoomOpen} /></button>
          {#if zoomOpen}
            <div class="text-zoom-popover" role="group" aria-label={tr("Chat text size", "Tamanho do texto do chat")}>
              <button type="button" disabled={textZoom <= textZoomMin} aria-label={tr("Decrease text size", "Diminuir textos")} onclick={() => setTextZoom(textZoom - 0.1)}>−</button>
              <output>{Math.round(textZoom * 100)}%</output>
              <button type="button" disabled={textZoom >= textZoomMax} aria-label={tr("Increase text size", "Aumentar textos")} onclick={() => setTextZoom(textZoom + 0.1)}>+</button>
            </div>
          {/if}
        </div>
        {#if onToggleMaximize}
          <button
            type="button"
            title={maximized ? tr("Restore pane", "Restaurar painel") : tr("Maximize pane", "Maximizar painel")}
            aria-label={maximized ? tr("Restore pane", "Restaurar painel") : tr("Maximize pane", "Maximizar painel")}
            aria-pressed={maximized}
            onclick={onToggleMaximize}
          >
            {#if maximized}
              <WorkspaceChatIcon name="restore" size={18} active />
            {:else}
              <WorkspaceChatIcon name="maximize" size={18} />
            {/if}
          </button>
        {/if}
        {#if closable}
        <button type="button" title={tr("Close pane", "Fechar painel")} aria-label={tr("Close pane", "Fechar painel")} onclick={onClose}>
          <WorkspaceChatIcon name="close" size={18} />
        </button>
        {/if}
      </div>
  </header>
  <UsageBanner alerts={usageNotices} {language} onDismiss={archiveAgentAlert} />

  {#if subagents.length}
    <SubagentPortals {session} children={subagents} {language} />
  {/if}

  <WorkspaceWorkBookmarks {session} {language} hasSubagents={subagents.length > 0} />

  <div class="conversation-shell" style:--composer-overlap={composerInIntroPosition ? "0px" : `${composerHeight}px`}>
  <div class="conversation" role="region" aria-label={tr("Conversation", "Conversa")} bind:this={conversationElement} onscroll={trackConversationScroll}>
    <div class="conversation-content" bind:this={conversationContentElement}>
    {#if canLoadEarlier}
      <button class="load-earlier-chat" type="button" disabled={historyLoading} onclick={() => void revealEarlierMessages()}>
        <span class:loading={historyLoading} class="load-earlier-icon"><LumeIcon name="chevron-down" size={14} /></span>
        {historyLoading ? tr("Loading earlier messages…", "Carregando mensagens anteriores…") : tr("Load earlier messages", "Carregar mensagens anteriores")}
      </button>
    {/if}
    {#each displayedFeed as feedItem (feedItem.id)}
      {#if feedItem.kind === "trace"}
        <div
          class="workspace-event-trace"
          in:slide={{ duration: motionDuration(175), easing: cubicOut }}
          out:slide={{ duration: motionDuration(120), easing: cubicOut }}
        >
          <ActivityTraceGroup
            activities={feedItem.entries.map((entry) => entry.activity)}
            active={feedItem.id === activeTraceId}
            plain
            {language}
          />
        </div>
      {:else}
        {@const entry = feedItem.entry}
        {@const item = entry.activity}
        {@const changedFiles = filesByFinalResponse.get(entry.id) ?? []}
        {#if item.kind === "prompt" && (item.detail || item.attachments?.length)}
          <div class:targeted={activePromptId === item.id} class:outgoing-replacement={outgoingPrompts.some((outgoing) => !outgoing.flightComplete && outgoingMatches.get(outgoing.id) === item.id)} class="conversation-row user-row" data-prompt-id={item.id} tabindex="-1">
            <span class="time-gutter"><time>{time(item.createdAt)}</time></span>
            <div class="conversation-entry">
              <section class="message user-message">
                {#if item.detail}<CollapsibleUserMessage text={item.detail} {language} />{/if}
                {#if item.attachments?.length}
                  <div class="attachments">
                    {#each item.attachments as attachment}
                      {#if attachment.previewDataUrl}
                        <img src={attachment.previewDataUrl} alt={attachment.name} />
                      {:else}
                        <span title={attachment.name}>{attachment.name}</span>
                      {/if}
                    {/each}
                  </div>
                {/if}
              </section>
            </div>
          </div>
        {:else if item.kind === "message" && item.detail}
          <div class="conversation-row agent-row">
            <span class="time-gutter"><time>{time(item.createdAt)}</time></span>
            <div class="conversation-entry">
              <section class="message agent-message">
                <div class="markdown-content"><StreamedMessage text={item.detail} animate={streamMessages && feedItem.id === newestMessageEntryId && (item.createdAt > paneOpenedAt || item.status === "running")} live={item.status === "running"} startFromBeginning={item.createdAt > paneOpenedAt} onstreaming={(active) => messageWriting(feedItem.id, active)} render={(value) => renderMarkdown(entry.id, value)} /></div>
                <ResponseAttachments
                  text={item.detail}
                  attachments={item.attachments ?? []}
                  workingDirectory={session.workingDirectory}
                  {language}
                  onError={(error) => (sendError = error)}
                />
                {#if entry.isFinalResponse}
                  <footer class="response-footer">
                    <span class="response-duration">{entry.durationMs !== undefined
                      ? `${tr("Worked for", "Trabalhou por")} ${formatAgentDuration(entry.durationMs)}`
                      : tr("Work time unavailable", "Tempo de trabalho indisponível")}</span>
                    <div class="final-actions" aria-label={tr("Response actions", "Ações da resposta")}>
                      <button type="button" title={tr("Copy response", "Copiar resposta")} aria-label={tr("Copy response", "Copiar resposta")} onclick={() => void copyResponse(item.detail ?? "")}><LumeIcon name="copy" size={14} /></button>
                      <button class:active={sourceEntryId === entry.id} type="button" title={tr("View sources", "Visualizar fontes")} aria-label={tr("View sources", "Visualizar fontes")} aria-expanded={sourceEntryId === entry.id} onclick={() => { promptIndexOpen = false; sourceEntryId = sourceEntryId === entry.id ? null : entry.id; }}><LumeIcon name="sources" size={14} /></button>
                      {#if session.agent === "codex" && session.nativeSessionId && promptForEntry(entry)}
                        <button class:loading={forkingEntryId === entry.id} type="button" disabled={Boolean(forkingEntryId)} title={tr("Fork from this prompt", "Criar fork a partir deste prompt")} aria-label={tr("Fork from this prompt", "Criar fork a partir deste prompt")} onclick={() => void forkFromEntry(entry)}><LumeIcon name="fork" size={14} /></button>
                      {/if}
                    </div>
                  </footer>
                {/if}
              </section>
              {#if entry.isFinalResponse && changedFiles.length}
                {@const totalAdded = changedFiles.reduce((total, file) => total + file.added, 0)}
                {@const totalRemoved = changedFiles.reduce((total, file) => total + file.removed, 0)}
                <div class:open={expandedFileSummaries.includes(entry.id)} class="changed-files">
                  <button class="changed-files-toggle" type="button" aria-expanded={expandedFileSummaries.includes(entry.id)} onclick={() => toggleFileSummary(entry.id)}>
                    <LumeIcon name="file" size={14} />
                    <span>{tr(`${changedFiles.length} file${changedFiles.length === 1 ? "" : "s"} changed`, `${changedFiles.length} arquivo${changedFiles.length === 1 ? "" : "s"} alterado${changedFiles.length === 1 ? "" : "s"}`)}</span>
                    {#if !expandedFileSummaries.includes(entry.id) && (totalAdded > 0 || totalRemoved > 0)}
                      <b class="added">+{totalAdded}</b><b class="removed">-{totalRemoved}</b>
                    {/if}
                    <LumeIcon name="chevron-down" size={13} />
                  </button>
                  {#if expandedFileSummaries.includes(entry.id)}
                    <div class="changed-files-list" in:slide={{ duration: motionDuration(170), easing: cubicOut }} out:slide={{ duration: motionDuration(120), easing: cubicOut }}>
                      {#each changedFiles as file}
                        <button class="changed-file" type="button" title={tr(`Review ${file.path}`, `Revisar ${file.path}`)} onclick={() => onOpenReview?.(file.path)}>
                          <FileTypeIcon path={file.path} />
                          <span>{displayFileChangePath(file.path)}</span>
                          {#if file.added > 0 || file.removed > 0}<b class="added">+{file.added}</b><b class="removed">-{file.removed}</b>{/if}
                          <LumeIcon name="chevron-down" size={12} />
                        </button>
                      {/each}
                    </div>
                  {/if}
                </div>
              {/if}
            </div>
          </div>
        {:else if item.kind === "interrupt"}
          <div class="conversation-row agent-row">
            <span class="time-gutter"><time>{time(item.createdAt)}</time></span>
            <div class="conversation-entry">
              <p class="interrupt-notice" role="status"><LumeIcon name="stop" size={11} />{interruptNoticeText(item.detail, tr)}</p>
            </div>
          </div>
        {:else if item.kind === "analysis" && item.detail}
          <div class="conversation-row agent-row">
            <span class="time-gutter"><time>{time(item.createdAt)}</time></span>
            <div class="conversation-entry">
              <section class="analysis-message">
                <header><strong>{displayText(language, item.title)}</strong></header>
                <div class="markdown-content">{@html renderMarkdown(entry.id, item.detail)}</div>
              </section>
            </div>
          </div>
        {/if}
      {/if}
    {:else}
      {#if !freshChat && !outgoingPrompts.length}
        <div class="empty-chat">
          <BrandIcon name={session.agent} size={26} />
          <strong>{tr("No conversation yet", "Nenhuma conversa ainda")}</strong>
          <span>{tr("Messages and activity will appear here in real time.", "Mensagens e atividades aparecerão aqui em tempo real.")}</span>
        </div>
      {/if}
    {/each}
    {#each outgoingPrompts as outgoing (outgoing.id)}
      {#if !outgoingMatches.has(outgoing.id) || !outgoing.flightComplete}
        <div class:in-flight={!outgoing.flightComplete} class:delivery-pending={session.source === "web" && !outgoingMatches.has(outgoing.id)} class="conversation-row user-row outgoing-row" data-outgoing-id={outgoing.id} aria-label={session.source === "web" && !outgoingMatches.has(outgoing.id) ? tr("Waiting for browser confirmation", "Aguardando confirmação do navegador") : undefined} aria-hidden={!outgoing.flightComplete}>
          <span class="time-gutter"><time>{time(outgoing.createdAt)}</time></span>
          <div class="conversation-entry"><section class="message user-message"><CollapsibleUserMessage text={outgoing.text} {language} /></section></div>
        </div>
      {/if}
    {/each}
    {#if session.status === "running" && !waitingForSubagents}
      <div class="agent-typing">
        <ThinkingOrb
          state={activeThinkingState}
          size={42}
          speed={0.92}
          label={tr(`${sessionName()} is working`, `${sessionName()} está trabalhando`)}
        />
        <span class="typing-label">{activityThinkingLabel(activeThinkingState, language)}</span>
        <span class="typing-elapsed" aria-label={tr("Elapsed time", "Tempo decorrido")}>{formatAgentDuration(Math.max(0, typingClock - (activePromptStartedAt ?? observedRunningSince ?? typingClock)))}</span>
      </div>
    {/if}
    </div>
  </div>

  {#if !followingTail}
    <button class="latest-button" type="button" onclick={scrollToLatest} aria-label={tr("Scroll to latest", "Ir para o final")} title={tr("Scroll to latest", "Ir para o final")}>
      <LumeIcon name="arrow-down" size={16} />
    </button>
  {/if}
  </div>

  {#if sourceEntry}
    <button class="sources-scrim" type="button" aria-label={tr("Close sources", "Fechar fontes")} onclick={() => (sourceEntryId = null)}></button>
    <aside class="sources-sidebar" aria-label={tr("Sources for this response", "Fontes desta resposta")}>
      <header>
        <span><strong>{tr("Sources", "Fontes")}</strong><small>{tr("Used in this prompt", "Usadas neste prompt")}</small></span>
        <button type="button" title={tr("Close sources", "Fechar fontes")} aria-label={tr("Close sources", "Fechar fontes")} onclick={() => (sourceEntryId = null)}><LumeIcon name="close" size={15} /></button>
      </header>
      <div class="sources-list">
        {#each responseSources as source (source.id)}
          {#if source.href}
            <a href={source.href} target="_blank" rel="noreferrer">
              <span class="source-icon"><LumeIcon name={source.kind === "file" ? "file" : "sources"} size={14} /></span>
              <span><strong>{source.label}</strong><small>{source.detail}</small></span>
            </a>
          {:else}
            <div class="source-item">
              <span class="source-icon"><LumeIcon name={source.kind === "file" ? "file" : "search"} size={14} /></span>
              <span><strong>{source.label}</strong><small>{source.detail}</small></span>
            </div>
          {/if}
        {:else}
          <div class="sources-empty"><LumeIcon name="sources" size={19} /><strong>{tr("No explicit sources", "Nenhuma fonte explícita")}</strong><span>{tr("This response did not cite links, files, or web searches.", "Esta resposta não citou links, arquivos ou pesquisas na web.")}</span></div>
        {/each}
      </div>
    </aside>
  {/if}

  {#if promptIndexOpen}
    <button class="sources-scrim" type="button" aria-label={tr("Close prompt list", "Fechar lista de prompts")} onclick={() => (promptIndexOpen = false)}></button>
    <aside id={`workspace-prompt-index-${session.id}`} class="sources-sidebar prompt-sidebar" aria-label={tr("Conversation prompts", "Prompts da conversa")}>
      <header>
        <span><strong>{tr("Prompts", "Prompts")}</strong><small>{tr("Jump to a message", "Ir para uma mensagem")}</small></span>
        <button type="button" title={tr("Close prompt list", "Fechar lista de prompts")} aria-label={tr("Close prompt list", "Fechar lista de prompts")} onclick={() => (promptIndexOpen = false)}><LumeIcon name="close" size={15} /></button>
      </header>
      <label class="prompt-index-search">
        <LumeIcon name="search" size={14} />
        <input
          bind:this={promptSearchInputElement}
          bind:value={promptSearchQuery}
          type="search"
          autocomplete="off"
          spellcheck="false"
          placeholder={tr("Search prompts", "Pesquisar prompts")}
          aria-label={tr("Search conversation prompts", "Pesquisar prompts da conversa")}
          oninput={schedulePromptSearch}
        />
        {#if promptSearchQuery}
          <button type="button" title={tr("Clear search", "Limpar pesquisa")} aria-label={tr("Clear prompt search", "Limpar pesquisa de prompts")} onclick={clearPromptSearch}><LumeIcon name="close" size={12} /></button>
        {/if}
      </label>
      <div class="prompt-index-list">
        {#each visiblePromptIndexItems as item (item.id)}
          <button class:locating={jumpingToPromptId === item.id} type="button" disabled={Boolean(jumpingToPromptId) || historyLoading} title={promptExcerpt(item.detail)} onclick={() => void jumpToPrompt(item)}>
            <time>{promptTime(item.createdAt)}</time>
            <span>{promptExcerpt(item.detail)}</span>
          </button>
        {:else}
          {#if !(promptIndexLoading || promptSearchLoading)}
            <p class="prompt-index-empty">{normalizedPromptSearch
              ? tr("No prompts match this search.", "Nenhum prompt corresponde à pesquisa.")
              : tr("No prompts in this conversation yet.", "Ainda não há prompts nesta conversa.")}</p>
          {/if}
        {/each}
        {#if promptIndexError}<p class="prompt-index-error" role="alert">{promptIndexError}</p>{/if}
        {#if promptIndexLoading || promptSearchLoading || jumpingToPromptId}
          <p class="prompt-index-loading" role="status">{jumpingToPromptId
            ? tr("Finding prompt in history…", "Buscando prompt no histórico…")
            : normalizedPromptSearch
              ? tr("Searching prompts…", "Pesquisando prompts…")
              : tr("Loading prompts…", "Carregando prompts…")}</p>
        {:else if normalizedPromptSearch ? promptSearchHasMore : promptIndexHasMore}
          <button class="prompt-index-more" type="button" onclick={() => void (normalizedPromptSearch ? loadPromptSearch() : loadOlderPromptIndex())}>{normalizedPromptSearch
            ? tr("More results", "Mais resultados")
            : tr("Load older prompts", "Carregar prompts anteriores")}</button>
        {/if}
      </div>
    </aside>
  {/if}

  {#if externalWriterConflict}
    <div class="writer-conflict-backdrop" role="presentation" onclick={(event) => {
      if (event.currentTarget === event.target && !writerConflictBusy) onDismissExternalWriterConflict?.(externalWriterConflict);
    }}>
      <dialog open class="writer-conflict-dialog" aria-modal="true" aria-labelledby={`writer-conflict-${session.id}`} aria-describedby={`writer-conflict-copy-${session.id}`} oncancel={(event) => {
        event.preventDefault();
        if (!writerConflictBusy) onDismissExternalWriterConflict?.(externalWriterConflict);
      }}>
        <span class="writer-conflict-mark" aria-hidden="true"><LumeIcon name="warning" size={17} /></span>
        <div class="writer-conflict-copy">
          <strong id={`writer-conflict-${session.id}`}>{tr("This conversation is open in Lume", "Esta conversa já está aberta no Lume")}</strong>
          <p id={`writer-conflict-copy-${session.id}`}>{tr(
            "An external Codex CLI tried to resume this same thread. Only one app can write to it at a time.",
            "Uma CLI externa do Codex tentou retomar esta mesma thread. Só um aplicativo pode gravar nela por vez.",
          )}</p>
          <p>{tr(
            "Keep it here to close only that detected CLI, or open a separate branch in a Lume terminal. The original conversation stays here.",
            "Mantenha-a aqui para fechar somente essa CLI detectada, ou abra uma ramificação em outro terminal do Lume. A conversa original continua aqui.",
          )}</p>
        </div>
        {#if writerConflictError}
          <p class="writer-conflict-error" role="alert">{writerConflictError}</p>
        {/if}
        <footer>
          <button type="button" disabled={Boolean(writerConflictBusy)} onclick={() => onDismissExternalWriterConflict?.(externalWriterConflict)}>{tr("Not now", "Agora não")}</button>
          <button type="button" disabled={Boolean(writerConflictBusy)} onclick={() => void resolveWriterConflict("open_branch")}>{writerConflictBusy === "open_branch" ? tr("Opening branch…", "Abrindo ramificação…") : tr("Open branch in terminal", "Abrir ramificação no terminal")}</button>
          <button bind:this={writerConflictPrimaryButton} class="writer-conflict-primary" type="button" disabled={Boolean(writerConflictBusy)} onclick={() => void resolveWriterConflict("keep_lume")}>{writerConflictBusy === "keep_lume" ? tr("Keeping in Lume…", "Mantendo no Lume…") : tr("Continue in Lume", "Continuar no Lume")}</button>
        </footer>
      </dialog>
    </div>
  {:else if takeoverConfirm}
    <div class="takeover-backdrop" role="presentation" onclick={(event) => {
      if (event.currentTarget === event.target && !takingControl) takeoverConfirm = false;
    }}>
      <dialog open class="takeover-dialog" aria-modal="true" aria-labelledby={`workspace-takeover-${session.id}`} oncancel={(event) => {
        event.preventDefault();
        if (!takingControl) takeoverConfirm = false;
      }}>
        <span class="takeover-mark" aria-hidden="true">
          <LumeIcon name="download" size={16} />
        </span>
        <div>
          <strong id={`workspace-takeover-${session.id}`}>{tr("Take control of this CLI?", "Assumir o controle desta CLI?")}</strong>
          <p>{tr(
            "Lume will close the external writer, resume the same thread and send this prompt.",
            "O Lume fechará o processo externo, retomará a mesma thread e enviará este prompt.",
          )}</p>
        </div>
        <footer>
          <button type="button" disabled={takingControl} onclick={() => (takeoverConfirm = false)}>{tr("Cancel", "Cancelar")}</button>
          <button bind:this={takeoverAcceptButton} class="takeover-primary" type="button" disabled={takingControl} onclick={() => void confirmTakeover()}>
            {takingControl ? tr("Taking control…", "Assumindo controle…") : tr("Take control & send", "Assumir e enviar")}
          </button>
        </footer>
      </dialog>
    </div>
  {/if}

  {#if !composerInIntroPosition}
    <div class="environment-dock" style:bottom={`${composerHeight + 14}px`}>
      <SessionEnvironmentMenu sessionId={session.id} {language} />
    </div>
  {/if}
  <form bind:this={composerElement} bind:clientHeight={composerHeight} class:fresh={composerInIntroPosition} class:crossfade-out={composerTransition === "out"} class:crossfade-in={composerTransition === "in"} class:unavailable={!canCompose} class="composer" onpaste={(event) => void pasteAttachments(event)} onsubmit={(event) => { event.preventDefault(); void sendPrompt(); }}>
    {#if composerInIntroPosition}
      <div class="composer-welcome"><LumeMascot status="idle" awake size={45} /><strong>{tr("Hello. What shall we work on?", "Olá. No que vamos trabalhar?")}</strong></div>
    {/if}
    {#if queuedPrompts.length}
      <div class="queue-tray">
        <span><i>{queuedPrompts.length}</i><b>{nextQueuedPrompt?.kind === "codex_queued_prompt" ? tr("Codex CLI · read only", "CLI do Codex · somente leitura") : tr("Queued", "Na fila")}</b><small>{nextQueuedPrompt?.detail || nextQueuedPrompt?.title}</small></span>
        {#if canSteer || canRunQueued}
          <button type="button" disabled={steeringQueued} onclick={() => void steerNextPrompt()}
            title={session.agent === "claude_code"
              ? (canRunQueued ? tr("Send the next queued message now", "Enviar agora a próxima mensagem da fila") : tr("Stop the running message and send this one", "Interromper a mensagem em andamento e enviar esta"))
              : tr("Steer into the current task", "Enviar para a tarefa atual")}>
            <LumeIcon name="steer" size={14} />
            {session.agent === "claude_code" && canSteer ? tr("Interrupt and send", "Interromper e enviar") : tr("Send now", "Enviar agora")}
          </button>
        {/if}
      </div>
    {/if}
    {#if promptAttachments.length}
      <div class="pending-attachments" aria-label={tr("Attached files", "Arquivos anexados")}>
        {#each promptAttachments as attachment, index}
          <span class:file={!(attachment.previewDataUrl)} title={attachment.name}>
            {#if attachment.previewDataUrl}
              <img src={attachment.previewDataUrl} alt={attachment.name} />
            {:else}
              <LumeIcon name="file" size={16} />
              <small>{attachment.name}</small>
            {/if}
            <button type="button" onclick={() => removeAttachment(index)} aria-label={tr(`Remove ${attachment.name}`, `Remover ${attachment.name}`)}>×</button>
          </span>
        {/each}
      </div>
    {/if}
    {#if pendingPermission}
      <section class="agent-permission risk-{pendingPermission.risk}" role="group" aria-label={tr("Permission request", "Pedido de permissão")}>
        <header>
          <LumeIcon name="warning" size={14} />
          <strong>{displayText(language, pendingPermission.summary)}</strong>
        </header>
        {#if pendingPermission.resource}<code title={pendingPermission.resource}>{pendingPermission.resource}</code>{/if}
        <div class="permission-actions">
          {#each session.permissionProfile.availableActions as action (action)}
            <button class:allow={action === "allow_once"} class:danger={action === "deny"} type="button" disabled={permissionBusy} onclick={() => void resolvePermission(action)}>
              {permissionActionLabel(action)}
            </button>
          {/each}
        </div>
      </section>
    {/if}
    {#if pendingQuestion}
      <section class="agent-question" aria-label={tr("Agent question", "Pergunta do agente")}>
        {#each pendingQuestion.questions as question (question.id)}
          <div class="agent-question-item">
            <small>{displayText(language, question.header)}</small>
            <strong>{displayText(language, question.question)}</strong>
            {#if question.options.length}
              <div class="question-options">
                {#each question.options as option, index (option.label)}
                  <button
                    class:selected={questionSelections[question.id] === option.label}
                    disabled={questionSending}
                    type="button"
                    onclick={() => void chooseQuestionOption(question.id, option.label)}
                  >
                    <b>{index + 1}</b>
                    <span>{displayText(language, option.label)}{#if option.description}<small>{displayText(language, option.description)}</small>{/if}</span>
                  </button>
                {/each}
              </div>
            {/if}
          </div>
        {/each}
        <p class="question-hint">{tr("Click an option, or type its number or your own answer below.", "Clique em uma opção ou digite o número ou sua própria resposta abaixo.")}</p>
        {#if pendingQuestion.questions.length > 1}
          <button class="question-submit" type="button" disabled={questionSending} onclick={() => void submitSelectedQuestionAnswers()}>{tr("Answer", "Responder")}</button>
        {/if}
      </section>
    {/if}
    <div class:beam={composerInIntroPosition && canCompose} class="composer-field">
      <div class="composer-input-row">
      {#if canAttach}
        <button
          class="attach-button"
          type="button"
          disabled={sending || takingControl || promptAttachments.length >= 4}
          aria-label={tr("Attach file", "Anexar arquivo")}
          title={tr("Attach file", "Anexar arquivo")}
          onclick={() => void chooseAttachments()}
        >
          <LumeIcon name="attachment" size={16} />
        </button>
      {/if}
      {#if slashMenuVisible}
        <div bind:this={slashCommandMenu} class="slash-command-menu" aria-label={tr("Slash commands", "Comandos com barra")} transition:slide={{ duration: 140, easing: cubicOut }}>
          <div class="slash-command-heading">
            <strong>{tr("Commands", "Comandos")}</strong>
            <small><kbd>↑↓</kbd> {tr("navigate", "navegar")} · <kbd>Enter</kbd> {tr("select", "selecionar")}</small>
          </div>
          {#if agentCommandsLoading}
            <p class="slash-command-loading">{tr(`Loading ${session.agentLabel} commands…`, `Carregando comandos do ${session.agentLabel}…`)}</p>
          {/if}
          {#each filteredSlashCommands as command, index (`${command.source}:${command.prefix}${command.name}`)}
            <button
              class:active={slashCommandIndex === index}
              data-slash-index={index}
              type="button"
              onmouseenter={() => (slashCommandIndex = index)}
              onclick={() => void selectSlashCommand(command)}
            >
              <code>{command.prefix}{command.name}</code>
              <span>{command.description}<small>{command.source === "agent" ? session.agentLabel : "Lume"}{command.argumentHint ? ` · ${command.argumentHint}` : ""}</small></span>
            </button>
          {/each}
        </div>
      {/if}
      {#if activeMention && mentionResults.length}
        <div bind:this={mentionMenu} class="slash-command-menu mention-menu" aria-label={tr("Files and folders", "Arquivos e pastas")} transition:slide={{ duration: 140, easing: cubicOut }}>
          <div class="slash-command-heading">
            <strong>{tr("Files and folders", "Arquivos e pastas")}</strong>
            <small><kbd>↑↓</kbd> {tr("navigate", "navegar")} · <kbd>Enter</kbd> {tr("select", "selecionar")}</small>
          </div>
          {#each mentionResults as result, index (result.path)}
            <button
              class:active={mentionIndex === index}
              data-mention-index={index}
              type="button"
              onmouseenter={() => (mentionIndex = index)}
              onclick={() => void selectMention(result)}
            >
              {#if result.isDirectory}<LumeIcon name="folder" size={14} />{:else}<FileTypeIcon path={result.path} size={14} />{/if}
              <span>{result.path}</span>
            </button>
          {/each}
        </div>
      {/if}
      {#if promptHistory.index >= 0}
        <span class="history-indicator" aria-live="polite">{tr("History", "Histórico")} {promptHistory.index + 1}/{promptHistory.entries.length}{promptHistoryHasMore ? "+" : ""}</span>
      {/if}
      <textarea
        bind:this={promptInput}
        bind:value={prompt}
        oninput={handlePromptInput}
        onclick={() => void refreshMention()}
        onkeyup={(event) => { if (["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) void refreshMention(); }}
        rows="1"
        placeholder={composerPlaceholder()}
        disabled={!canCompose || sending || takingControl}
        aria-label={composerPlaceholder()}
        onkeydown={handleComposerKeydown}
      ></textarea>
      {#if promptIsRunning && canQueue}<span class="queue-label">{tr("Next", "Próximo")}</span>{/if}
      {#if promptIsRunning && session.capabilities.canInterrupt && !planeLaunching}
        <button class="stop-button" type="button" disabled={interrupting} onclick={() => void interruptAgentPrompt()} aria-label={tr("Interrupt prompt", "Interromper prompt")} title={tr("Interrupt prompt · Enter queues", "Interromper prompt · Enter adiciona à fila")}>
          {#if interrupting}<i class="send-spinner"></i>{:else}<LumeIcon name="stop" size={15} />{/if}
        </button>
      {:else}
        <button class:launching={planeLaunching} class="send-button" type="submit" disabled={!prompt.trim() || !canSend || sending || takingControl} aria-label={promptIsRunning ? tr("Add to queue", "Adicionar à fila") : tr("Send prompt", "Enviar prompt")}>
          {#if planeLaunching}
            {#key planeLaunchId}<span class="plane-launch"><SendPlaneIcon size={16} /></span>{/key}
          {:else if sending || takingControl}
            <i class="send-spinner"></i>
          {:else}
            <SendPlaneIcon size={16} />
          {/if}
        </button>
      {/if}
      </div>
      {#if supportsAgentControls}
      <div class="composer-tools">
        <div class="agent-controls" bind:this={controlsRoot}>
          <button class:active={controlsOpen} class="model-trigger" type="button"
            aria-label={tr("Choose model and effort", "Escolher modelo e esforço")}
            aria-haspopup="dialog" aria-expanded={controlsOpen}
            onclick={() => void toggleAgentControls()}>
            <span>{session.agent === "codex" || session.agent === "opencode" || session.agent === "antigravity"
              ? (modelSettings?.models.find((option) => option.model === selectedModel)?.displayName || selectedModel || "Model")
              : (claudeModels.find((option) => option.model === claudeModel)?.displayName || claudeModel || tr("Model", "Modelo"))}</span>
            {#if (session.agent === "codex" || session.agent === "claude_code") && fastMode}<span class="fast-indicator" title={tr("Fast mode is on", "Modo Fast ligado")}><WorkspaceChatIcon name="fast" size={13} active /></span>{/if}
            <LumeIcon name="chevron-down" size={12} />
          </button>
          {#if controlsOpen}
            <section class="agent-controls-popover" aria-label={tr("Model and effort", "Modelo e esforço")}>
              {#if session.controlOrigin !== "lume"}
                <p class="controls-note">{tr("Take control of this session to change its model.", "Assuma o controle desta sessão para mudar o modelo.")}</p>
              {:else if controlsLoading}
                <div class="controls-loading"><i></i>{tr("Loading settings…", "Carregando ajustes…")}</div>
              {:else}
                {#snippet modelResetButton()}
                  <button class="controls-icon-button model-reset" type="button" disabled={!controlsChanged || modelControlsDisabled}
                    aria-label={tr("Restore the original settings", "Restaurar as configurações originais")}
                    title={tr("Restore the original settings", "Restaurar as configurações originais")}
                    onclick={() => void resetAgentControls()}>
                    <LumeIcon name="reset" size={15} />
                  </button>
                {/snippet}
                {#snippet modelFastButton()}
                  {#if session.agent === "codex" || session.agent === "claude_code"}
                    <button class="controls-icon-button model-fast" class:enabled={fastMode} type="button"
                      disabled={runtimeControlsDisabled || fastSaving || (session.agent === "codex" && !modelSettings) || (session.agent === "claude_code" && !claudeFastModeAvailable)} aria-pressed={fastMode}
                      aria-label={fastMode ? tr("Disable Fast mode", "Desativar modo Fast") : tr("Enable Fast mode", "Ativar modo Fast")}
                      title={session.agent === "claude_code" ? tr("Fast mode needs a supported Opus model and can use more credits", "O modo Fast exige um modelo Opus compatível e pode consumir mais créditos") : tr("Fast mode · higher credit usage", "Modo Fast · maior consumo de créditos")}
                      onclick={() => void toggleFastMode()}>
                      {#key fastMode}<WorkspaceChatIcon name="fast" size={18} active={fastMode} />{/key}
                    </button>
                  {/if}
                {/snippet}
                {#if session.agent === "codex" || session.agent === "opencode" || session.agent === "antigravity"}
                  {#if modelSettings}
                    <div class="controls-model-row">
                      {@render modelResetButton()}
                      <div class="model-picker">
                      <LumeSelect value={selectedModel}
                        options={modelSettings.models.map((option) => ({ value: option.model, label: option.displayName, description: option.isDefault ? tr("Default", "Padrão") : option.description }))}
                        ariaLabel={tr("Model", "Modelo")} disabled={modelControlsDisabled} minWidth={0} variant="heading" onValueChange={chooseModel} />
                      </div>
                      {@render modelFastButton()}
                    </div>
                  {/if}
                {:else}
                  <div class="controls-model-row">
                    {@render modelResetButton()}
                    <div class="model-picker">
                      <LumeSelect value={claudeModel} options={claudeModelOptions(claudeModels, tr)}
                        ariaLabel={tr("Model", "Modelo")} disabled={modelControlsDisabled} minWidth={0} variant="heading" onValueChange={chooseClaudeModel} />
                    </div>
                    {@render modelFastButton()}
                  </div>
                {/if}
                {#if session.agent === "opencode" && modelSettings?.sessionModes?.options.length}
                  <label class="controls-field"><span>{tr("Agent mode", "Modo do agente")}</span>
                    <LumeSelect value={modelSettings.sessionModes.currentMode} options={modelSettings.sessionModes.options}
                      disabled={modelControlsDisabled} ariaLabel={tr("Agent mode", "Modo do agente")} minWidth={190} onValueChange={changeAgentMode} />
                  </label>
                {/if}
                {#if effortValues().length}
                  <div class:max={currentEffort().toLowerCase() === "max"} class:ultra={currentEffort().toLowerCase() === "ultra"} class="controls-field effort-field"
                    style={`--effort-tone:${effortTone(currentEffort())};--effort-progress:${effortProgress()}%;--effort-ratio:${effortProgress() / 100};`}>
                    <span>{tr("Reasoning effort", "Esforço de raciocínio")}<b aria-live="polite">{effortLabel()}</b></span>
                    <div class="effort-track">
                      <span class="effort-progress" aria-hidden="true"></span>
                      <span class="effort-thumb" aria-hidden="true"></span>
                      <input type="range" min="0" max={Math.max(0, effortValues().length - 1)} step="1"
                        value={currentEffortIndex()} disabled={modelControlsDisabled}
                        aria-label={tr("Reasoning effort", "Nível de raciocínio")} aria-valuetext={effortLabel()}
                        oninput={chooseEffortIndex} onchange={() => void saveAgentControls()} />
                    </div>
                  </div>
                {/if}
                {#if promptIsRunning && session.agent !== "opencode"}<p class="controls-note">{session.agent === "codex" || session.agent === "antigravity"
                  ? tr("Changes made now will apply when this prompt finishes.", "Mudanças feitas agora serão aplicadas ao final deste prompt.")
                  : tr("Finish or interrupt the current prompt to apply changes.", "Finalize ou interrompa o prompt atual para aplicar mudanças.")}</p>{/if}
                {#if controlsSaving}<p class="controls-saving" role="status">{tr("Saving…", "Salvando…")}</p>{/if}
              {/if}
            </section>
          {/if}
        </div>
        {#if supportsPermissionPicker && sessionPermission}
          <div class="permission-picker" bind:this={permissionMenuRoot}>
            <button class:active={permissionMenuOpen} class="model-trigger permission-trigger tone-{permissionTone(sessionPermission.mode)}" type="button"
              disabled={session.controlOrigin !== "lume" || permissionMenuSaving}
              aria-label={tr("Choose permissions", "Escolher permissões")}
              aria-haspopup="listbox" aria-expanded={permissionMenuOpen}
              title={session.controlOrigin !== "lume" ? tr("Take control of this session to change permissions", "Assuma o controle desta sessão para mudar as permissões") : permissionDescription(sessionPermission.mode, tr)}
              onclick={() => void toggleSessionPermissionMenu()}>
              <LumeIcon name="shield" size={14} />
              <span>{permissionLabel(sessionPermission.mode, tr)}</span>
              <LumeIcon name="chevron-down" size={12} />
            </button>
            {#if permissionMenuOpen}
              <div class="permission-menu" role="listbox" aria-label={tr("Permissions", "Permissões")} transition:slide={{ duration: 140, easing: cubicOut }}>
                {#each sessionPermission.modes as mode (mode)}
                  <button class:selected={mode === sessionPermission.mode} class="tone-{permissionTone(mode)}" role="option" aria-selected={mode === sessionPermission.mode} type="button" disabled={permissionMenuSaving || permissionMenuLoading} onclick={() => void chooseSessionPermission(mode)}>
                    <span><strong>{permissionLabel(mode, tr)}</strong><small>{permissionDescription(mode, tr)}</small></span>
                    {#if mode === sessionPermission.mode}<LumeIcon name="check" size={14} />{/if}
                  </button>
                {/each}
                <p class="permission-note">{promptIsRunning ? tr("Applies to the next message.", "Vale para a próxima mensagem.") : tr("Applies to the messages you send from Lume.", "Vale para as mensagens enviadas pelo Lume.")}</p>
              </div>
            {/if}
          </div>
        {/if}
        {#if session.agent === "codex" && collaborationMode === "plan"}
          <span class="plan-chip" title={tr("Plan mode · /plan switches back", "Modo Plan · /plan volta ao padrão")}><WorkspaceChatIcon name="mode-plan" size={13} active />Plan</span>
        {/if}
      </div>
      {/if}
    </div>
  </form>
</article>

{#if (connectionRequired || (session.status === "failed" && session.statusLabel !== dismissedConnectionError && agentConnectionMessage(session.statusLabel))) && ["claude_code", "opencode", "antigravity", "deepseek", "codex", "gemini"].includes(session.agent)}
  <AgentConnectionDialog
    agent={(session.agent === "claude_code" ? "claude" : session.agent) as ConnectableAgent}
    message={connectionRequired ?? agentConnectionMessage(session.statusLabel) ?? ""}
    {language}
    onClose={() => { dismissedConnectionError = session.statusLabel; connectionRequired = null; }} />
{/if}

<style>
  .session-pane { --pane-status-color: #84948c; --workspace-chat-font-size: calc(12px + var(--workspace-chat-font-adjust)); --workspace-chat-small-size: calc(10px + var(--workspace-chat-small-adjust)); --workspace-chat-tiny-size: calc(8px + var(--workspace-chat-tiny-adjust)); --chat-small-font-size: var(--workspace-chat-small-size); --chat-tiny-font-size: var(--workspace-chat-tiny-size); --activity-summary-height: calc(44px + var(--workspace-chat-font-adjust)); --activity-row-height: calc(42px + var(--workspace-chat-font-adjust)); --activity-title-size: calc(11px + var(--workspace-chat-small-adjust)); --activity-detail-size: calc(9px + var(--workspace-chat-tiny-adjust)); position: relative; min-width: 0; min-height: 0; height: 100%; container-type: inline-size; display: flex; flex-direction: column; overflow: hidden; background: transparent; animation: pane-arrive 280ms cubic-bezier(.16, 1, .3, 1) both; }
  .session-pane.status-running { --pane-status-color: #4e98ca; }
  .session-pane.status-completed { --pane-status-color: #50aa79; }
  .session-pane.status-permission_required { --pane-status-color: #d0a142; }
  .session-pane.status-failed { --pane-status-color: #c66762; }
  .pane-header { position: relative; z-index: 2; min-width: 0; min-height: 64px; padding: 10px 14px 10px 17px; display: flex; align-items: center; gap: 10px; flex: 0 0 auto; border-bottom: 1px solid var(--workspace-line); background: radial-gradient(ellipse 250px 115px at 100% 0%, color-mix(in srgb, var(--pane-status-color) 18%, transparent), transparent 85%), var(--workspace-pane); cursor: grab; user-select: none; }
  .pane-header:active { cursor: grabbing; }
  .pane-header::after { position: absolute; right: 0; bottom: -1px; left: 0; height: 1px; background: color-mix(in srgb, var(--workspace-accent) 20%, transparent); content: ""; transform: scaleX(.18); transform-origin: left; transition: transform 320ms cubic-bezier(.16, 1, .3, 1); }
  .session-pane:hover .pane-header::after,
  .session-pane.focused .pane-header::after { transform: scaleX(1); }
  .session-pane.focused .pane-header::after { background: color-mix(in srgb, var(--workspace-accent) 58%, transparent); }
  .agent-mark { width: 38px; height: 38px; display: grid; place-items: center; flex: 0 0 auto; color: var(--workspace-accent); }
  .pane-identity { min-width: 0; flex: 1; display: grid; gap: 2px; }
  .pane-identity strong, .pane-identity small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .pane-identity strong { color: var(--workspace-strong); font-size: 12px; font-weight: 740; letter-spacing: -.015em; }
  .pane-identity small { display: flex; align-items: center; gap: 4px; color: var(--workspace-muted); font-size: 9px; }
  .fork-mark { flex: 0 0 auto; margin-left: 3px; padding: 1px 5px 1px 4px; display: inline-flex; align-items: center; gap: 3px; border-radius: 999px; color: var(--workspace-accent); background: var(--workspace-accent-soft); font-weight: 720; }
  .source-badge, .status-badge { min-height: 23px; padding: 0 7px; display: inline-flex; align-items: center; gap: 5px; flex: 0 0 auto; border: 1px solid var(--workspace-line); border-radius: 7px; color: var(--workspace-muted); background: transparent; font-size: 8px; font-weight: 740; }
  .status-badge { border-color: transparent; background: var(--workspace-subtle); }
  .status-badge i { width: 6px; height: 6px; flex: 0 0 auto; border-radius: 50%; background: var(--pane-status-color); }
  .status-badge.status-running i { animation: status-pulse 1.8s ease-out infinite; }
  .status-badge.status-subagents i { background: var(--workspace-accent); }
  .pane-actions { display: flex; align-items: center; gap: 2px; }
  .agent-alerts { position: relative; flex: 0 0 auto; }
  .agent-alert-trigger { position: relative; width: 29px; height: 29px; padding: 0; display: grid; place-items: center; border: 0; border-radius: 8px; color: #c99535; background: color-mix(in srgb, #c99535 9%, transparent); cursor: pointer; transition: color 140ms ease, background 140ms ease, transform 180ms cubic-bezier(.16, 1, .3, 1); }
  .agent-alert-trigger:hover, .agent-alert-trigger.active { color: #dcaa48; background: color-mix(in srgb, #c99535 16%, var(--workspace-subtle)); transform: translateY(-1px); }
  .agent-alert-trigger > span { position: absolute; top: -3px; right: -3px; min-width: 14px; height: 14px; padding: 0 3px; display: grid; place-items: center; border: 2px solid var(--workspace-pane); border-radius: 8px; color: #fff8e5; background: #b77b25; font-size: 7px; font-weight: 820; line-height: 1; font-variant-numeric: tabular-nums; }
  .agent-alert-menu { position: absolute; z-index: 24; top: 36px; right: 0; width: min(310px, calc(100cqw - 20px)); max-height: min(330px, calc(100vh - 105px)); overflow: hidden; border: 1px solid var(--workspace-line); border-radius: 13px; color: var(--workspace-text); background: var(--workspace-raised); box-shadow: 0 18px 48px rgba(7, 20, 13, .22); animation: controls-arrive 180ms cubic-bezier(.16, 1, .3, 1) both; }
  .agent-alert-menu > header { min-height: 39px; padding: 0 11px; display: flex; align-items: center; gap: 8px; border-bottom: 1px solid var(--workspace-line); }
  .agent-alert-menu > header strong { flex: 1; color: var(--workspace-strong); font-size: 10px; }
  .agent-alert-menu > header small { min-width: 19px; height: 19px; display: grid; place-items: center; border-radius: 6px; color: #ba842f; background: color-mix(in srgb, #c99535 12%, transparent); font-size: 8px; font-weight: 790; }
  .agent-alert-menu > div { max-height: 286px; overflow-y: auto; overscroll-behavior: contain; scrollbar-width: thin; scrollbar-color: var(--workspace-scroll-thumb) transparent; }
  .agent-alert-menu article { min-height: 44px; padding: 9px 11px; display: grid; grid-template-columns: 22px minmax(0, 1fr); align-items: start; gap: 8px; }
  .agent-alert-menu article + article { border-top: 1px solid color-mix(in srgb, var(--workspace-line) 68%, transparent); }
  .agent-alert-menu article > span { width: 22px; height: 22px; display: grid; place-items: center; border-radius: 7px; color: #c28b31; background: color-mix(in srgb, #c99535 11%, transparent); }
  .agent-alert-menu article.tone-error > span { color: #c96761; background: color-mix(in srgb, #c96761 11%, transparent); }
  .agent-alert-menu p { margin: 1px 0 0; overflow-wrap: anywhere; color: var(--workspace-text); font-size: 9px; line-height: 1.45; }
  .pane-actions > button, .agent-controls > button, .text-zoom > button { width: 29px; height: 29px; padding: 0; display: grid; place-items: center; border: 0; border-radius: 8px; color: var(--workspace-muted); background: transparent; cursor: pointer; transition: color 120ms ease, background 120ms ease, transform 180ms cubic-bezier(.16, 1, .3, 1); }
  .pane-actions > button:hover, .agent-controls > button:hover, .agent-controls > button.active, .text-zoom > button:hover, .text-zoom > button.active { color: var(--workspace-accent); background: var(--workspace-subtle); transform: translateY(-1px); }
  .pane-actions > button.active { color: var(--workspace-accent); background: var(--workspace-accent-soft); }
  .text-zoom { position: relative; }
  .text-zoom-popover { position: absolute; z-index: 12; top: 36px; right: 0; width: 136px; min-height: 40px; padding: 5px; display: grid; grid-template-columns: 30px 1fr 30px; align-items: center; gap: 3px; border: 1px solid var(--workspace-line); border-radius: 10px; color: var(--workspace-text); background: var(--workspace-raised); box-shadow: 0 12px 30px rgba(8, 18, 13, .16); animation: controls-arrive 180ms cubic-bezier(.16, 1, .3, 1) both; }
  .text-zoom-popover button { width: 30px; height: 28px; border: 0; border-radius: 7px; color: var(--workspace-accent); background: var(--workspace-subtle); font-size: 16px; cursor: pointer; }
  .text-zoom-popover button:disabled { opacity: .35; cursor: default; }
  .text-zoom-popover output { color: var(--workspace-muted); font-size: 10px; font-weight: 720; text-align: center; font-variant-numeric: tabular-nums; }
  .agent-controls { position: static; }
  .agent-controls-popover { position: absolute; z-index: 12; bottom: calc(100% + 9px); left: 0; width: min(320px, calc(100cqw - 20px)); max-height: min(450px, calc(100vh - 115px)); padding: 15px; overflow: visible; border: 1px solid var(--workspace-line); border-radius: 16px; color: var(--workspace-text); background: var(--workspace-raised); box-shadow: 0 20px 60px rgba(8, 18, 13, .2); animation: controls-arrive 180ms cubic-bezier(.16, 1, .3, 1) both; }
  .controls-field { margin-top: 11px; display: grid; gap: 7px; }
  .agent-controls-popover .controls-field:first-child { margin-top: 0; }
  .controls-field > span { display: flex; align-items: center; justify-content: space-between; color: var(--workspace-muted); font-size: 8px; font-weight: 760; }
  .controls-field > span b { color: var(--effort-tone, var(--workspace-accent)); font-size: 9px; text-transform: capitalize; transition: color 180ms ease, transform 220ms cubic-bezier(.16, 1, .3, 1); }
  .controls-field :global(.lume-select) { width: 100%; }
  .effort-field { gap: 5px; }
  .effort-track { position: relative; height: 34px; margin: 0 12px 1px; display: flex; align-items: center; isolation: isolate; }
  .effort-track::before, .effort-progress { position: absolute; right: 0; left: 0; height: 12px; border-radius: 999px; content: ""; }
  .effort-track::before { border: 1px solid color-mix(in srgb, var(--workspace-muted) 16%, var(--workspace-line)); background: color-mix(in srgb, var(--workspace-muted) 14%, transparent); box-shadow: inset 0 1px 2px color-mix(in srgb, var(--workspace-strong) 8%, transparent); }
  .effort-progress { z-index: 0; background: linear-gradient(90deg, #4e98ca, var(--effort-tone)); box-shadow: 0 2px 7px color-mix(in srgb, var(--effort-tone) 18%, transparent); transform: scaleX(var(--effort-ratio)); transform-origin: left; transition: transform 240ms cubic-bezier(.16, 1, .3, 1), background 180ms ease, box-shadow 180ms ease; }
  .effort-thumb { position: absolute; z-index: 1; left: var(--effort-progress); box-sizing: border-box; width: 24px; height: 24px; border: 4px solid var(--workspace-raised); border-radius: 48% 52% 46% 54%; background: var(--effort-tone); box-shadow: 0 0 0 1px color-mix(in srgb, var(--effort-tone) 76%, transparent), 0 4px 10px rgba(0, 0, 0, .2); pointer-events: none; transform: translateX(-50%); transition: left 240ms cubic-bezier(.16, 1, .3, 1), border-radius 180ms ease, background 180ms ease, transform 180ms cubic-bezier(.16, 1, .3, 1), box-shadow 180ms ease; }
  .effort-thumb::before { position: absolute; z-index: -1; top: 3px; right: calc(100% - 5px); width: 12px; height: 10px; border-radius: 999px 4px 4px 999px; background: var(--effort-tone); content: ""; opacity: var(--effort-ratio); transform: scaleX(.86); transform-origin: right; transition: opacity 180ms ease, transform 180ms cubic-bezier(.16, 1, .3, 1); }
  .effort-track input { position: absolute; z-index: 2; inset: 0; width: 100%; height: 100%; margin: 0; appearance: none; opacity: 0; cursor: pointer; -webkit-tap-highlight-color: transparent; }
  .effort-track:hover .effort-thumb { border-radius: 56% 44% 52% 48%; transform: translateX(-50%) scale(1.06); }
  .effort-track:hover .effort-thumb::before { transform: scaleX(1.08); }
  .effort-track:has(input:active) .effort-thumb { border-radius: 42% 58% 45% 55%; transform: translateX(-50%) scale(1.16, .88); }
  .effort-track:has(input:active) .effort-thumb::before { transform: scaleX(1.32); }
  .effort-track:has(input:focus-visible) { border-radius: 999px; outline: 2px solid color-mix(in srgb, var(--workspace-accent) 68%, transparent); outline-offset: 2px; }
  .effort-track:has(input:disabled) { opacity: .46; }
  .effort-track:has(input:disabled) input { cursor: default; }
  .effort-field.max .effort-thumb { animation: max-thumb-pulse 1.8s ease-in-out infinite alternate; }
  .effort-field.ultra .effort-track::before { background: linear-gradient(90deg, color-mix(in srgb, #7652c6 32%, transparent), color-mix(in srgb, #b48aff 54%, transparent), color-mix(in srgb, #7652c6 32%, transparent)); background-size: 220% 100%; box-shadow: inset 0 1px 2px rgba(50, 31, 83, .18), 0 3px 12px rgba(154, 112, 232, .2); animation: ultra-slider-flow 2.4s linear infinite; }
  .effort-field.ultra .effort-progress { background: linear-gradient(90deg, #7652c6, #c6a9ff, #8d62df); background-size: 180% 100%; box-shadow: 0 3px 12px rgba(154, 112, 232, .35); animation: ultra-slider-flow 2.4s linear infinite; }
  .effort-field.ultra .effort-thumb { box-shadow: 0 0 0 1px #9a70e8, 0 0 12px rgba(154, 112, 232, .48); }
  .controls-model-row { min-width: 0; min-height: 34px; margin-top: 0; display: grid; grid-template-columns: 32px minmax(0, 1fr) 32px; align-items: center; gap: 5px; }
  .model-picker { min-width: 0; width: 100%; }
  .model-picker :global(.lume-select) { width: 100%; }
  .model-picker :global(.lume-select.heading .lume-select-trigger) { box-sizing: border-box; position: relative; width: 100%; height: 32px; min-height: 32px; padding: 0 12px; justify-content: center; border: 1px solid transparent; border-radius: 9px; color: var(--workspace-strong); background: transparent; text-align: center; transition: border-color 150ms ease, background 150ms ease, color 150ms ease; }
  .model-picker :global(.lume-select.heading .lume-select-trigger:hover), .model-picker :global(.lume-select.heading .lume-select-trigger.open) { border-color: color-mix(in srgb, var(--workspace-accent) 24%, var(--workspace-line)); background: var(--workspace-subtle); }
  .model-picker :global(.lume-select.heading .lume-select-trigger > span:first-child) { min-width: 0; flex: 1; overflow: hidden; text-align: center; text-overflow: ellipsis; }
  .model-picker :global(.select-chevron) { display: none; }
  .composer .controls-icon-button { width: 32px; height: 32px; padding: 0; display: grid; place-items: center; border: 0; border-radius: 9px; color: var(--workspace-muted); background: transparent; }
  .composer .controls-icon-button:hover:not(:disabled), .composer .controls-icon-button.enabled { transform: none; color: var(--workspace-accent); background: var(--workspace-subtle); }
  .composer .controls-icon-button:disabled { opacity: .32; }
  .model-control-spacer { width: 32px; height: 32px; }
  .composer .controls-icon-button:focus-visible { outline: 2px solid var(--workspace-accent); outline-offset: 2px; }
  .fast-indicator { display: inline-flex; flex: 0 0 auto; color: var(--workspace-accent); }
  .plan-chip { height: 22px; margin-left: 2px; padding: 0 8px 0 5px; display: inline-flex; align-items: center; gap: 4px; border-radius: 999px; color: var(--workspace-accent); background: var(--workspace-accent-soft); font-size: 9px; font-weight: 740; }
  .controls-note { margin: 10px 0 0; color: var(--workspace-muted); font-size: 8px; line-height: 1.5; }
  .controls-loading { min-height: 68px; display: flex; align-items: center; justify-content: center; gap: 8px; color: var(--workspace-muted); font-size: 9px; }
  .controls-loading i { width: 12px; height: 12px; border: 1.5px solid currentColor; border-right-color: transparent; border-radius: 50%; animation: spin 650ms linear infinite; }
  .controls-saving { margin: 10px 0 0; color: var(--workspace-muted); font-size: 8px; }
  .agent-controls-popover input:disabled { opacity: .45; cursor: default; }
  .conversation-shell { position: relative; min-width: 0; min-height: 0; display: flex; flex: 1 1 auto; overflow: hidden; }
  .conversation { --chat-edge-gutter: clamp(28px, 5cqw, 54px); min-width: 0; min-height: 0; width: 100%; padding: 25px var(--chat-edge-gutter) calc(26px + var(--composer-overlap, 0px)); display: flex; flex-direction: column-reverse; flex: 1 1 auto; overflow: auto; overflow-anchor: none; overscroll-behavior: none; scrollbar-width: thin; scrollbar-color: var(--workspace-scroll-thumb) transparent; }
  .conversation-content { width: 100%; max-width: 1040px; min-width: 0; margin-inline: auto; display: flex; flex-direction: column; gap: 15px; flex: 1 0 auto; }
  .conversation::-webkit-scrollbar { width: 9px; height: 9px; }.conversation::-webkit-scrollbar-track { background: transparent; }.conversation::-webkit-scrollbar-thumb { border: 3px solid transparent; border-radius: 9px; background: var(--workspace-scroll-thumb); background-clip: content-box; }
  .load-earlier-chat { max-width: min(240px, 100%); min-height: 30px; margin: 0 auto 7px; padding: 0 10px; display: inline-flex; align-items: center; justify-content: center; gap: 6px; border: 1px solid var(--workspace-line); border-radius: 8px; color: var(--workspace-muted); background: var(--workspace-subtle); font-size: var(--workspace-chat-tiny-size); font-weight: 700; cursor: pointer; }
  .load-earlier-chat:hover:not(:disabled) { color: var(--workspace-accent); border-color: color-mix(in srgb, var(--workspace-accent) 32%, var(--workspace-line)); }
  .load-earlier-chat:disabled { opacity: .65; cursor: wait; }
  .load-earlier-icon { display: inline-flex; transform: rotate(180deg); }
  .load-earlier-icon.loading { animation: history-loading 900ms linear infinite; }
  .workspace-event-trace { box-sizing: border-box; width: fit-content; max-width: min(100%, 620px); min-width: 0; align-self: flex-start; }
  .workspace-event-trace :global(.activity-cluster) { width: fit-content; max-width: 100%; }
  .conversation-row { position: relative; width: 100%; min-width: 0; }
  .conversation-entry { min-width: 0; display: flex; flex-direction: column; gap: 9px; }
  .time-gutter { position: absolute; top: 0; bottom: 0; left: calc(-1 * var(--chat-edge-gutter)); width: var(--chat-edge-gutter); padding: 6px 4px 0 0; display: flex; justify-content: flex-end; align-items: flex-start; box-sizing: border-box; cursor: default; }
  .time-gutter time { font-size: calc(11px + var(--workspace-chat-small-adjust)); opacity: 0; transform: translateX(3px); transition: opacity 120ms ease, transform 160ms cubic-bezier(.16, 1, .3, 1); }
  .time-gutter:hover time, .conversation-row:focus-within .time-gutter time { opacity: 1; transform: translateX(0); }
  .message { width: min(92%, 940px); min-width: 0; color: var(--workspace-text); font-size: var(--workspace-chat-font-size); }
  .user-message { --message-collapse-surface: var(--workspace-user); --user-message-font: inherit; --user-message-muted: var(--workspace-muted); --user-message-accent: var(--workspace-accent); width: fit-content; max-width: min(92%, 940px); align-self: flex-end; padding: 9px 12px; border: 1px solid var(--workspace-user-line); border-radius: 14px 14px 4px 14px; background: var(--workspace-user); }
  .outgoing-row.in-flight .user-message { visibility: hidden; }
  .outgoing-row.delivery-pending .user-message { border-style: dashed; opacity: .76; }
  .user-row.outgoing-replacement { display: none; }
  .user-row.targeted .user-message { outline: 2px solid var(--workspace-accent); outline-offset: 3px; }
  .agent-message { align-self: flex-start; padding: 3px 0 7px; }
  :global(.workspace.agent-message-surface) .agent-message { padding: 11px 15px 9px; border: 1px solid var(--workspace-line); border-radius: 14px 14px 14px 4px; background: color-mix(in srgb, var(--workspace-raised) 90%, transparent); box-shadow: 0 6px 22px rgba(8, 18, 13, .08); backdrop-filter: blur(14px) saturate(1.1); }
  .analysis-message header { margin-bottom: 7px; display: flex; align-items: center; gap: 8px; }
  .analysis-message header strong { min-width: 0; flex: 1; color: var(--workspace-strong); font-size: var(--workspace-chat-small-size); font-weight: 750; }
  time { color: var(--workspace-faint); font-size: var(--workspace-chat-tiny-size); font-variant-numeric: tabular-nums; }
  .response-footer { min-height: 25px; margin-top: 8px; display: flex; align-items: center; gap: 8px; }
  .final-actions { min-height: 24px; display: flex; justify-content: flex-start; gap: 2px; opacity: .35; transition: opacity 140ms ease; }
  .agent-row:hover .final-actions, .final-actions:focus-within { opacity: 1; }
  .final-actions button { width: 24px; height: 24px; padding: 0; display: grid; place-items: center; border: 0; border-radius: 7px; color: var(--workspace-faint); background: transparent; cursor: pointer; transition: color 120ms ease, background 120ms ease, transform 120ms ease; }
  .final-actions button:hover, .final-actions button:focus-visible, .final-actions button.active { color: var(--workspace-accent); background: var(--workspace-subtle); }
  .final-actions button:active { transform: scale(.92); }
  .final-actions button:disabled { cursor: wait; opacity: .55; }
  .final-actions button.loading :global(.lume-icon) { animation: history-loading 800ms linear infinite; }
  .markdown-content { min-width: 0; overflow-wrap: anywhere; font-size: var(--workspace-chat-font-size); line-height: 1.68; word-break: break-word; }
  .agent-message .markdown-content { font-family: var(--lume-font-ui, Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif); font-size: calc(13.5px + var(--workspace-chat-font-adjust)); font-weight: 430; line-height: 1.72; letter-spacing: -.012em; font-kerning: normal; }
  .agent-message .markdown-content :global(strong) { font-weight: 720; letter-spacing: -.018em; }
  .agent-message .markdown-content :global(pre) { font-size: calc(11px + var(--workspace-chat-small-adjust)); line-height: 1.62; }
  .markdown-content :global(p) { margin: 0 0 .75em; }.markdown-content :global(p:last-child) { margin-bottom: 0; }
  .markdown-content :global(strong) { color: var(--workspace-strong); font-weight: 790; }
  .markdown-content :global(em) { font-style: italic; }
  .markdown-content :global(del) { color: var(--workspace-faint); }
  .markdown-content :global(a) { border-radius: 3px; color: var(--workspace-accent); font-weight: 650; text-decoration: underline; text-decoration-color: color-mix(in srgb, var(--workspace-accent) 42%, transparent); text-underline-offset: 2px; box-decoration-break: clone; transition: background-color 140ms ease, text-decoration-color 140ms ease; }
  .markdown-content :global(a:hover), .markdown-content :global(a:focus-visible) { background-color: color-mix(in srgb, var(--workspace-accent) 10%, transparent); text-decoration-color: currentColor; }
  .markdown-content :global(.inline-file-badge) { max-width: min(100%, 250px); min-height: 19px; padding: 1px 5px 1px 4px; display: inline-flex; align-items: center; gap: 4px; vertical-align: -.24em; border: 1px solid var(--workspace-line); border-radius: 6px; color: var(--workspace-strong); background: var(--workspace-subtle); font-size: .9em; line-height: 1.3; white-space: nowrap; }
  .markdown-content :global(button.inline-file-badge) { font-family: inherit; cursor: pointer; }
  .markdown-content :global(button.inline-file-badge:hover), .markdown-content :global(button.inline-file-badge:focus-visible) { border-color: color-mix(in srgb, var(--workspace-accent) 45%, var(--workspace-line)); color: var(--workspace-accent); }
  .markdown-content :global(.inline-file-name) { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .markdown-content :global(.inline-file-icon) { width: 13px; height: 13px; flex: 0 0 auto; fill: none; stroke: currentColor; stroke-width: 1.35; stroke-linecap: round; stroke-linejoin: round; }
  .markdown-content :global(.inline-file-icon.brand) { stroke: none; opacity: .9; }
  .markdown-content :global(.inline-file-icon.brand.monochrome) { filter: var(--file-monochrome-filter, grayscale(1) brightness(0) contrast(.88)); opacity: var(--file-monochrome-opacity, .72); }
  .markdown-content :global(.inline-file-icon.brand.multicolor) { filter: none; opacity: .94; }
  .markdown-content :global(.inline-file-icon.brand *) { stroke: none !important; }
  .markdown-content :global(.inline-file-icon.maintained) { fill: currentColor; stroke: none; }
  .markdown-content :global(ul), .markdown-content :global(ol) { margin: .65em 0 .95em; padding-left: 1.6em; }
  .markdown-content :global(li) { margin: .35em 0; padding-left: .12em; }
  .markdown-content :global(ol > li::marker) { color: var(--workspace-accent); font-weight: 800; font-variant-numeric: tabular-nums; }
  .markdown-content :global(blockquote) { margin: .6em 0; padding: .3em .75em; border-left: 2px solid var(--workspace-accent); color: var(--workspace-muted); background: var(--workspace-subtle); }
  .markdown-content :global(:not(pre) > code) { padding: .08em .3em; border-radius: 4px; color: var(--workspace-accent); background: var(--workspace-subtle); font-size: .92em; }
  .markdown-content :global(h1), .markdown-content :global(h2), .markdown-content :global(h3) { margin: 1.5em 0 .58em; color: var(--workspace-strong); line-height: 1.25; letter-spacing: -.02em; }
  .markdown-content :global(h1) { font-size: 1.42em; font-weight: 790; }
  .markdown-content :global(h2) { font-size: 1.24em; font-weight: 770; }
  .markdown-content :global(h3) { font-size: 1.1em; font-weight: 750; }
  .markdown-content :global(h1 + p), .markdown-content :global(h2 + p), .markdown-content :global(h3 + p) { margin-top: .1em; }
  .markdown-content :global(pre) { max-width: 100%; padding: 11px 12px; overflow: auto; border: 1px solid var(--workspace-line); border-radius: 10px; background: var(--workspace-code); font: var(--workspace-chat-small-size)/1.6 var(--lume-font-code, "SFMono-Regular", Consolas, monospace); }
  .markdown-content :global(pre code) { padding: 0; color: inherit; background: transparent; white-space: pre-wrap; word-break: break-word; }
  .markdown-content :global(code) { overflow-wrap: anywhere; font-family: var(--lume-font-code, "SFMono-Regular", Consolas, monospace); }
  /* The wrapper scrolls; the table stays a real table so head and body share one set of column widths. */
  .markdown-content :global(.markdown-table-wrap) { box-sizing: border-box; width: 100%; max-width: 100%; margin: .7em 0; overflow-x: auto; border: 1px solid var(--workspace-line); border-radius: 9px; }
  .markdown-content :global(table) { width: 100%; border-collapse: collapse; }
  .markdown-content :global(th), .markdown-content :global(td) { min-width: 88px; padding: 7px 9px; overflow-wrap: anywhere; border-right: 1px solid var(--workspace-line); border-bottom: 1px solid var(--workspace-line); text-align: left; vertical-align: top; word-break: normal; }
  .markdown-content :global(th:last-child), .markdown-content :global(td:last-child) { border-right: 0; }
  .markdown-content :global(tbody tr:last-child td) { border-bottom: 0; }
  .markdown-content :global(th) { color: var(--workspace-strong); background: var(--workspace-subtle); font-size: var(--workspace-chat-small-size); }
  .markdown-content :global(.align-center) { text-align: center; }
  .markdown-content :global(.align-right) { text-align: right; }
  .markdown-content :global(img) { max-width: 100%; height: auto; border-radius: 9px; }
  .markdown-content :global(hr) { margin: 1em 0; border: 0; border-top: 1px solid var(--workspace-line); }
  .response-duration { width: max-content; display: block; color: var(--workspace-faint); font-size: var(--workspace-chat-tiny-size); font-variant-numeric: tabular-nums; }
  .interrupt-notice { margin: 2px 0 4px; padding: 3px 9px 3px 7px; width: fit-content; max-width: 100%; display: flex; align-items: center; gap: 6px; border: 1px solid var(--workspace-line); border-radius: 999px; color: var(--workspace-muted); background: var(--workspace-subtle); font-size: var(--workspace-chat-tiny-size); font-weight: 680; }
  .interrupt-notice :global(.lume-icon) { color: #d85c64; }
  .analysis-message { min-width: 0; padding: 5px 0 7px; color: var(--workspace-muted); }
  .analysis-message .markdown-content { font-size: calc(11px + var(--workspace-chat-small-adjust)); line-height: 1.62; }
  .changed-files { width: fit-content; max-width: 100%; min-width: 0; margin-top: 2px; }
  .changed-files.open { width: min(100%, 560px); }
  .changed-files-toggle { min-height: 29px; max-width: 100%; padding: 5px 8px; display: flex; align-items: center; gap: 7px; border: 1px solid var(--workspace-line); border-radius: 8px; color: var(--workspace-muted); background: var(--workspace-subtle); font: 720 var(--workspace-chat-tiny-size)/1.3 var(--lume-font-ui, Inter, sans-serif); cursor: pointer; }
  .changed-files-toggle:hover, .changed-files-toggle:focus-visible { color: var(--workspace-strong); border-color: var(--workspace-accent); }
  .changed-files-toggle > span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .changed-files-toggle > :global(.lume-icon:last-child) { margin-left: 3px; transition: transform 160ms cubic-bezier(.16, 1, .3, 1); }
  .changed-files.open .changed-files-toggle > :global(.lume-icon:last-child) { transform: rotate(180deg); }
  .changed-files-list { min-width: 0; margin: 4px 0 0 2px; display: grid; gap: 2px; }
  .changed-file { min-width: 0; min-height: 28px; padding: 3px 5px; display: flex; align-items: center; gap: 7px; border: 0; border-radius: 7px; color: var(--workspace-text); background: transparent; font: calc(9px + var(--workspace-chat-tiny-adjust))/1.4 var(--lume-font-code, "SFMono-Regular", Consolas, monospace); text-align: left; cursor: pointer; transition: color 120ms ease, background 120ms ease; }
  .changed-file:hover { color: var(--workspace-strong); background: var(--workspace-subtle); }
  .changed-file > span { min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .changed-file > :global(.lume-icon:last-child) { color: var(--workspace-faint); opacity: 0; transform: translateX(-3px) rotate(-90deg); transition: color 120ms ease, opacity 120ms ease, transform 180ms cubic-bezier(.16, 1, .3, 1); }
  .changed-file:hover > :global(.lume-icon:last-child), .changed-file:focus-visible > :global(.lume-icon:last-child) { color: var(--workspace-accent); opacity: 1; transform: translateX(0) rotate(-90deg); }
  .changed-files b { font-size: var(--workspace-chat-tiny-size); font-variant-numeric: tabular-nums; }.added { color: #438f67; }.removed { color: #b96862; }
  .attachments { margin-top: 10px; display: flex; flex-wrap: wrap; gap: 7px; }
  .attachments img { width: 92px; height: 68px; object-fit: cover; border-radius: 10px; }
  .attachments span { max-width: 190px; padding: 6px 8px; overflow: hidden; border: 1px solid var(--workspace-line); border-radius: 7px; font-size: 9px; text-overflow: ellipsis; white-space: nowrap; }
  .empty-chat { margin: auto; display: grid; justify-items: center; gap: 7px; color: var(--workspace-faint); text-align: center; }
  .empty-chat strong { color: var(--workspace-muted); font-size: 12px; }.empty-chat span { max-width: 270px; font-size: 10px; line-height: 1.5; }
  .agent-typing { width: fit-content; min-height: 46px; padding: 0 3px; display: flex; align-items: center; gap: 10px; color: #4e98ca; }
  .agent-typing .typing-label { color: transparent; background: linear-gradient(90deg, #67837a 10%, #5aa1ce 44%, #a3d2ef 53%, #5aa1ce 62%, #67837a 90%); background-size: 240% 100%; background-clip: text; font-size: 12px; font-weight: 720; letter-spacing: -.01em; animation: thinking-label-shimmer 1.75s linear infinite; }
  .typing-elapsed { padding-left: 9px; border-left: 1px solid var(--workspace-line); color: var(--workspace-muted); font-size: 10px; font-weight: 650; font-variant-numeric: tabular-nums; white-space: nowrap; }
  .latest-button { position: absolute; right: 18px; bottom: calc(30px + var(--composer-overlap, 0px)); z-index: 3; width: 30px; height: 30px; display: grid; place-items: center; border: 1px solid var(--workspace-line); border-radius: 10px; color: var(--workspace-accent); background: var(--workspace-raised); box-shadow: 0 5px 16px rgba(17, 35, 27, .09); cursor: pointer; animation: latest-arrive 180ms cubic-bezier(.16, 1, .3, 1) both; }
  .latest-button:hover { transform: translateY(-1px); }
  .environment-dock { position: absolute; left: 18px; z-index: 4; }
  .session-pane.has-environments .conversation { padding-bottom: calc(66px + var(--composer-overlap, 0px)); }
  .sources-scrim { position: absolute; z-index: 5; inset: 64px 0 0; padding: 0; border: 0; background: color-mix(in srgb, var(--workspace-pane) 28%, transparent); backdrop-filter: blur(1px); cursor: default; animation: sources-fade 150ms ease-out both; }
  .sources-sidebar { position: absolute; z-index: 6; top: 64px; right: 0; bottom: 0; width: min(340px, 88%); min-width: 0; display: flex; flex-direction: column; border-left: 1px solid var(--workspace-line); color: var(--workspace-text); background: var(--workspace-raised); box-shadow: -16px 0 46px rgba(8, 18, 13, .13); animation: sources-arrive 220ms cubic-bezier(.16, 1, .3, 1) both; }
  .sources-sidebar > header { min-height: 57px; padding: 10px 12px 10px 15px; display: flex; align-items: center; gap: 10px; border-bottom: 1px solid var(--workspace-line); }
  .sources-sidebar > header > span { min-width: 0; flex: 1; display: grid; gap: 2px; }
  .sources-sidebar > header strong { color: var(--workspace-strong); font-size: 12px; }
  .sources-sidebar > header small { color: var(--workspace-muted); font-size: 9px; }
  .sources-sidebar > header button { width: 29px; height: 29px; display: grid; place-items: center; border: 0; border-radius: 8px; color: var(--workspace-muted); background: transparent; cursor: pointer; }
  .sources-sidebar > header button:hover { color: var(--workspace-strong); background: var(--workspace-subtle); }
  .sources-list { min-height: 0; padding: 9px; display: grid; align-content: start; gap: 3px; overflow-y: auto; scrollbar-width: thin; }
  .sources-list a, .source-item { min-width: 0; padding: 9px; display: grid; grid-template-columns: 27px minmax(0, 1fr); gap: 8px; border-radius: 9px; color: inherit; text-decoration: none; }
  .sources-list a:hover { background: var(--workspace-subtle); }
  .source-icon { width: 27px; height: 27px; display: grid; place-items: center; border-radius: 8px; color: var(--workspace-accent); background: var(--workspace-accent-soft); }
  .sources-list a > span:last-child, .source-item > span:last-child { min-width: 0; display: grid; gap: 2px; }
  .sources-list strong, .sources-list small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sources-list strong { color: var(--workspace-strong); font-size: 10px; }
  .sources-list small { color: var(--workspace-muted); font-size: 8px; }
  .sources-empty { min-height: 190px; padding: 28px 18px; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 6px; color: var(--workspace-faint); text-align: center; }
  .sources-empty strong { color: var(--workspace-muted); font-size: 11px; }.sources-empty span { max-width: 220px; font-size: 9px; line-height: 1.5; }
  .prompt-index-search { min-height: 38px; margin: 9px 11px 3px; padding: 0 8px 0 10px; display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 7px; border: 1px solid var(--workspace-line); border-radius: 10px; color: var(--workspace-muted); background: var(--workspace-pane); transition: border-color 140ms ease, background 140ms ease, box-shadow 180ms cubic-bezier(.16, 1, .3, 1); }
  .prompt-index-search:focus-within { color: var(--workspace-accent); border-color: color-mix(in srgb, var(--workspace-accent) 56%, var(--workspace-line)); background: var(--workspace-raised); box-shadow: 0 5px 16px color-mix(in srgb, var(--workspace-accent) 10%, transparent); }
  .prompt-index-search input { width: 100%; min-width: 0; padding: 9px 0; border: 0; outline: 0; color: var(--workspace-strong); background: transparent; font: inherit; font-size: 10px; line-height: 1.2; caret-color: var(--workspace-accent); }
  .prompt-index-search input::placeholder { color: var(--workspace-faint); opacity: 1; }
  .prompt-index-search input::-webkit-search-cancel-button { display: none; }
  .prompt-index-search button { width: 24px; height: 24px; padding: 0; display: grid; place-items: center; border: 0; border-radius: 7px; color: var(--workspace-muted); background: transparent; cursor: pointer; }
  .prompt-index-search button:hover { color: var(--workspace-strong); background: var(--workspace-subtle); }
  .prompt-index-search button:focus-visible { outline: 2px solid var(--workspace-accent); outline-offset: 1px; }
  .prompt-index-list { min-height: 0; padding: 8px; display: flex; flex-direction: column; gap: 3px; overflow-y: auto; scrollbar-width: thin; }
  .prompt-index-list > button:not(.prompt-index-more) { min-width: 0; padding: 10px 11px; display: grid; gap: 5px; border: 0; border-radius: 9px; color: var(--workspace-text); background: transparent; text-align: left; cursor: pointer; }
  .prompt-index-list > button:not(.prompt-index-more):hover, .prompt-index-list > button.locating { background: var(--workspace-subtle); }
  .prompt-index-list > button:focus-visible { outline: 2px solid var(--workspace-accent); outline-offset: -2px; }
  .prompt-index-list > button:disabled { cursor: wait; }
  .prompt-index-list time { color: var(--workspace-muted); font-size: 9px; font-variant-numeric: tabular-nums; }
  .prompt-index-list button > span { min-width: 0; overflow: hidden; display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2; line-clamp: 2; color: var(--workspace-strong); font-size: 11px; line-height: 1.45; overflow-wrap: anywhere; }
  .prompt-index-empty, .prompt-index-error, .prompt-index-loading { margin: 12px; color: var(--workspace-muted); font-size: 10px; line-height: 1.5; }
  .prompt-index-error { color: var(--workspace-danger, #b96862); }
  .prompt-index-more { min-height: 32px; margin: 7px 4px; border: 1px solid var(--workspace-line); border-radius: 8px; color: var(--workspace-accent); background: var(--workspace-subtle); font-size: 10px; font-weight: 700; cursor: pointer; }
  .prompt-index-more:hover { border-color: var(--workspace-accent); }
  @keyframes history-loading { to { transform: rotate(540deg); } }
  @keyframes sources-fade { from { opacity: 0; } }
  @keyframes sources-arrive { from { opacity: 0; transform: translateX(18px); } }
  .takeover-backdrop { position: absolute; z-index: 8; inset: 0; padding: 18px; display: grid; place-items: center; background: color-mix(in srgb, var(--workspace-pane) 58%, transparent); backdrop-filter: blur(3px); animation: takeover-fade 140ms ease-out both; }
  .takeover-dialog { position: relative; inset: auto; width: min(360px, 100%); margin: 0; padding: 16px; display: grid; grid-template-columns: 34px minmax(0, 1fr); gap: 11px; border: 1px solid var(--workspace-line); border-radius: 14px; color: var(--workspace-text); background: var(--workspace-raised); box-shadow: 0 18px 55px rgba(8, 18, 13, .18); animation: takeover-arrive 220ms cubic-bezier(.16, 1, .3, 1) both; }
  .takeover-mark { width: 32px; height: 32px; display: grid; place-items: center; border-radius: 9px; color: var(--workspace-accent); background: var(--workspace-accent-soft); }
  .takeover-dialog strong { display: block; margin-top: 1px; color: var(--workspace-strong); font-size: 11px; }
  .takeover-dialog p { margin: 5px 0 0; color: var(--workspace-muted); font-size: 9px; line-height: 1.5; }
  .takeover-dialog footer { grid-column: 1 / -1; margin-top: 4px; display: flex; justify-content: flex-end; gap: 7px; }
  .takeover-dialog footer button { min-height: 31px; padding: 0 11px; border: 1px solid var(--workspace-line); border-radius: 9px; color: var(--workspace-muted); background: transparent; font-size: 9px; font-weight: 720; cursor: pointer; }
  .takeover-dialog footer button:hover:not(:disabled) { color: var(--workspace-strong); background: var(--workspace-subtle); }
  .takeover-dialog footer .takeover-primary { border-color: transparent; color: #f5fbf7; background: var(--workspace-accent); }
  .takeover-dialog footer button:disabled { opacity: .55; cursor: default; }
  .writer-conflict-backdrop { position: absolute; z-index: 12; inset: 0; padding: clamp(12px, 3cqw, 24px); display: grid; place-items: center; background: color-mix(in srgb, var(--workspace-pane) 66%, transparent); backdrop-filter: blur(4px); animation: takeover-fade 150ms ease-out both; }
  .writer-conflict-dialog { position: relative; inset: auto; width: min(420px, 100%); max-height: min(92%, 440px); margin: 0; padding: 19px; overflow: auto; display: grid; grid-template-columns: 36px minmax(0, 1fr); gap: 12px; border: 1px solid color-mix(in srgb, var(--workspace-accent) 22%, var(--workspace-line)); border-radius: 16px; color: var(--workspace-text); background: var(--workspace-raised); box-shadow: 0 22px 72px rgba(8, 18, 13, .26); animation: takeover-arrive 240ms cubic-bezier(.16, 1, .3, 1) both; }
  .writer-conflict-mark { width: 34px; height: 34px; display: grid; place-items: center; border-radius: 10px; color: var(--workspace-accent); background: var(--workspace-accent-soft); }
  .writer-conflict-copy { min-width: 0; }
  .writer-conflict-copy strong { display: block; padding-top: 2px; color: var(--workspace-strong); font-size: 12px; line-height: 1.35; }
  .writer-conflict-copy p { margin: 8px 0 0; color: var(--workspace-muted); font-size: 10px; line-height: 1.55; }
  .writer-conflict-error { grid-column: 1 / -1; margin: 0; padding: 8px 10px; border-radius: 8px; color: var(--workspace-danger, #b96862); background: color-mix(in srgb, var(--workspace-danger, #b96862) 9%, transparent); font-size: 10px; line-height: 1.45; overflow-wrap: anywhere; }
  .writer-conflict-dialog footer { grid-column: 1 / -1; margin-top: 5px; display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 7px; }
  .writer-conflict-dialog footer button { min-height: 34px; padding: 0 11px; border: 1px solid var(--workspace-line); border-radius: 9px; color: var(--workspace-muted); background: transparent; font-size: 9px; font-weight: 720; cursor: pointer; transition: color 130ms ease, background 130ms ease, border-color 130ms ease, transform 160ms cubic-bezier(.16, 1, .3, 1); }
  .writer-conflict-dialog footer button:hover:not(:disabled) { color: var(--workspace-strong); background: var(--workspace-subtle); transform: translateY(-1px); }
  .writer-conflict-dialog footer .writer-conflict-primary { border-color: transparent; color: #f5fbf7; background: var(--workspace-accent); }
  .writer-conflict-dialog footer button:disabled { opacity: .55; cursor: default; }
  /* Only the field is solid. The form floats over the chat; the chat fades out behind it. */
  .composer { position: relative; z-index: 2; min-width: 0; flex: 0 0 auto; padding: 12px clamp(14px, 4cqw, 28px) 15px; border-top: 0; background: transparent; transition: opacity 160ms ease; }
  .composer.crossfade-out { opacity: 0; pointer-events: none; transition-duration: 110ms; }
  .composer.crossfade-in { opacity: 0; pointer-events: none; transition: none; }
  .composer:not(.fresh) { position: absolute; right: 0; bottom: 0; left: 0; padding-top: 6px; pointer-events: none; }
  .composer:not(.fresh) > :global(*) { pointer-events: auto; }
  .composer:not(.fresh)::before { position: absolute; z-index: -1; top: -34px; right: 0; bottom: 0; left: 0; background: linear-gradient(to bottom, transparent, color-mix(in srgb, var(--workspace-pane) 74%, transparent) 64%); content: ""; pointer-events: none; -webkit-backdrop-filter: blur(7px); backdrop-filter: blur(7px); -webkit-mask-image: linear-gradient(to bottom, transparent, #000 60%); mask-image: linear-gradient(to bottom, transparent, #000 60%); }
  .composer.fresh { position: absolute; top: 50%; right: 0; left: 0; z-index: 3; border-top: 0; background: transparent; transform: translateY(-50%); }
  .composer-welcome { max-width: 760px; margin: 0 auto 24px; display: flex; align-items: center; justify-content: center; gap: 13px; }
  .composer-welcome strong { color: var(--workspace-strong); font-size: clamp(15px, 2.2cqw, 22px); font-weight: 710; letter-spacing: -.025em; }
  .queue-tray { max-width: 760px; min-height: 34px; margin: 0 auto 8px; padding: 4px 5px 4px 8px; display: flex; align-items: center; gap: 8px; border: 1px solid color-mix(in srgb, #4e98ca 25%, var(--workspace-line)); border-radius: 10px; background: color-mix(in srgb, #4e98ca 7%, var(--workspace-raised)); }
  .queue-tray > span { min-width: 0; flex: 1; display: flex; align-items: center; gap: 7px; }
  .queue-tray i { width: 18px; height: 18px; display: grid; place-items: center; flex: 0 0 auto; border-radius: 6px; color: #4e98ca; background: color-mix(in srgb, #4e98ca 12%, transparent); font-size: 8px; font-style: normal; font-weight: 800; }
  .queue-tray b { color: #4e98ca; font-size: 8px; }.queue-tray small { min-width: 0; overflow: hidden; color: var(--workspace-muted); font-size: 8px; text-overflow: ellipsis; white-space: nowrap; }
  .composer .queue-tray button { width: auto; min-width: max-content; height: 26px; padding: 0 8px; display: flex; gap: 5px; border-radius: 7px; color: #4e98ca; background: transparent; font-size: 7px; font-weight: 780; }
  .composer .queue-tray button:hover:not(:disabled) { background: color-mix(in srgb, #4e98ca 10%, transparent); transform: none; }
  .pending-attachments { max-width: 760px; margin: 0 auto 8px; display: flex; flex-wrap: wrap; gap: 7px; }
  .pending-attachments > span { position: relative; width: 54px; height: 42px; display: flex; align-items: center; gap: 6px; overflow: hidden; border: 1px solid var(--workspace-line); border-radius: 9px; color: var(--workspace-muted); background: var(--workspace-raised); }
  .pending-attachments img { width: 100%; height: 100%; object-fit: cover; }
  .pending-attachments > span.file { width: min(170px, 44cqw); padding: 0 26px 0 9px; }
  .pending-attachments small { min-width: 0; overflow: hidden; font-size: 8px; font-weight: 650; text-overflow: ellipsis; white-space: nowrap; }
  .composer .pending-attachments button { position: absolute; top: 3px; right: 3px; width: 18px; height: 18px; padding: 0; display: grid; place-items: center; border: 1px solid color-mix(in srgb, var(--workspace-line) 75%, transparent); border-radius: 6px; color: var(--workspace-strong); background: color-mix(in srgb, var(--workspace-raised) 90%, transparent); font-size: 13px; cursor: pointer; }
  .composer-field { --field-radius: 18px; --field-inset: 7px; --field-button-radius: calc(var(--field-radius) - var(--field-inset)); position: relative; max-width: 760px; min-height: 47px; margin: 0 auto; padding: var(--field-inset); display: flex; flex-direction: column; align-items: stretch; gap: 3px; border: 1px solid var(--workspace-line); border-radius: var(--field-radius); background: var(--workspace-raised); box-shadow: 0 5px 18px rgba(17, 35, 27, .055); transition: border-color 140ms ease, box-shadow 140ms ease; }
  @property --composer-beam-angle { syntax: "<angle>"; inherits: true; initial-value: 0deg; }
  /* The three-layer md/colorful beam is adapted for Svelte from Libraries.dev BorderBeam (MIT). */
  .composer-field.beam {
    --composer-beam-strength: .9;
    --composer-beam-stroke-opacity: .4;
    --composer-beam-inner-opacity: .42;
    --composer-beam-window: conic-gradient(from var(--composer-beam-angle), transparent 0% 30%, rgba(255, 255, 255, .1) 36%, rgba(255, 255, 255, .35) 44%, white 52% 80%, rgba(255, 255, 255, .35) 86%, rgba(255, 255, 255, .1) 92%, transparent 95% 100%);
    --composer-beam-colors: radial-gradient(ellipse 70px 40px at 33% -7.4%, rgb(255, 50, 100), transparent), radial-gradient(ellipse 60px 35px at 12% -5%, rgb(40, 140, 255), transparent), radial-gradient(ellipse 40px 70px at 2.1% 68.3%, rgb(50, 200, 80), transparent), radial-gradient(ellipse 20px 35px at 2.1% 68.3%, rgb(30, 185, 170), transparent), radial-gradient(ellipse 180px 32px at 74.4% 100%, rgb(100, 70, 255), transparent), radial-gradient(ellipse 85px 26px at 55% 100%, rgb(40, 140, 255), transparent), radial-gradient(ellipse 74px 32px at 93.9% 0%, rgb(255, 120, 40), transparent), radial-gradient(ellipse 26px 42px at 100% 27.1%, rgb(240, 50, 180), transparent), radial-gradient(ellipse 52px 48px at 100% 27.1%, rgb(180, 40, 240), transparent);
    isolation: isolate;
    animation: composer-beam-orbit 4.4s linear infinite;
  }
  :global(.workspace.dark) .composer-field.beam {
    --composer-beam-stroke-opacity: .54;
    --composer-beam-inner-opacity: .56;
  }
  .composer-field.beam::before, .composer-field.beam::after { position: absolute; inset: 0; border-radius: inherit; clip-path: inset(0 round var(--field-radius)); content: ""; pointer-events: none; animation: composer-beam-appear .6s ease-out both, composer-beam-hue 12s ease-in-out infinite; }
  .composer-field.beam::after {
    z-index: 2;
    padding: 1px;
    background: var(--composer-beam-colors);
    -webkit-mask: var(--composer-beam-window), linear-gradient(#fff 0 0) content-box, linear-gradient(#fff 0 0);
    -webkit-mask-composite: source-in, xor;
    mask: var(--composer-beam-window), linear-gradient(#fff 0 0) content-box, linear-gradient(#fff 0 0);
    mask-composite: intersect, exclude;
    opacity: calc(var(--composer-beam-strength) * var(--composer-beam-stroke-opacity));
  }
  .composer-field.beam::before {
    z-index: 1;
    background: var(--composer-beam-colors);
    box-shadow: inset 0 0 9px 1px color-mix(in srgb, var(--workspace-strong) 14%, transparent);
    -webkit-mask: var(--composer-beam-window), linear-gradient(white, transparent 28px, transparent calc(100% - 28px), white), linear-gradient(to right, white, transparent 28px, transparent calc(100% - 28px), white);
    -webkit-mask-composite: source-in, source-over;
    mask: var(--composer-beam-window), linear-gradient(white, transparent 28px, transparent calc(100% - 28px), white), linear-gradient(to right, white, transparent 28px, transparent calc(100% - 28px), white);
    mask-composite: intersect, add;
    opacity: calc(var(--composer-beam-strength) * var(--composer-beam-inner-opacity) * .45);
  }
  @keyframes composer-beam-orbit { to { --composer-beam-angle: 360deg; } }
  @keyframes composer-beam-appear { from { opacity: 0; } }
  @keyframes composer-beam-hue { 0%, 100% { filter: hue-rotate(-30deg) brightness(1.3) saturate(1.2); } 50% { filter: hue-rotate(30deg) brightness(1.3) saturate(1.2); } }
    .composer-field:focus-within { border-color: color-mix(in srgb, var(--workspace-accent) 48%, transparent); box-shadow: 0 7px 22px rgba(17, 35, 27, .075), 0 0 0 3px color-mix(in srgb, var(--workspace-accent) 8%, transparent); }
  .composer-field.beam:focus-within { border-color: var(--workspace-line); }
  .composer textarea { field-sizing: content; min-width: 0; max-height: 130px; min-height: 31px; padding: 7px 0 5px; flex: 1; resize: none; overflow-y: auto; border: 0; outline: 0; color: var(--workspace-strong); background: transparent; font-size: 11px; line-height: 1.5; }
  .composer textarea::placeholder { color: var(--workspace-faint); }.composer textarea:disabled { cursor: default; }
  .composer.unavailable .composer-field { background: var(--workspace-subtle); box-shadow: none; }
  .queue-label { margin-bottom: 8px; padding: 3px 6px; border-radius: 5px; color: #4d8cb8; background: rgba(78, 152, 202, .1); font-size: 7px; font-weight: 800; text-transform: uppercase; }
  .composer button { width: 33px; height: 33px; display: grid; place-items: center; flex: 0 0 auto; border: 0; border-radius: var(--field-button-radius, 11px); color: #f5fbf7; background: var(--workspace-accent); cursor: pointer; transition: transform 180ms cubic-bezier(.16, 1, .3, 1), opacity 120ms ease; }
  .composer .send-button { position: relative; overflow: hidden; isolation: isolate; }
  .composer .attach-button { color: var(--workspace-muted); background: transparent; }
  .composer .attach-button:hover:not(:disabled) { color: var(--workspace-accent); background: var(--workspace-subtle); }
  .composer .stop-button { color: #fff7f6; background: #b96862; }
  .composer-input-row { min-width: 0; display: flex; align-items: flex-end; gap: 7px; }
  .composer-field .composer-input-row > textarea { padding-left: 5px; }
  .composer-tools { min-height: 30px; margin: 0; padding: 0; display: flex; flex-wrap: wrap; align-items: center; gap: 2px 3px; }
  .composer-tools .model-trigger { width: auto; min-width: 30px; height: 28px; padding: 0 9px 0 8px; display: inline-flex; align-items: center; justify-content: center; gap: 5px; border-radius: var(--field-button-radius); color: var(--workspace-muted); background: transparent; font-size: 9px; font-weight: 680; }
  .composer-tools .model-trigger { max-width: min(180px, 42cqw); }
  .permission-picker { position: static; display: inline-flex; }
  .composer-tools .permission-trigger.tone-auto { color: var(--workspace-accent); }
  .composer-tools .permission-trigger.tone-danger { color: #d85c64; }
  .permission-menu { position: absolute; z-index: 13; bottom: calc(100% + 8px); left: 0; width: min(330px, calc(100cqw - 24px)); padding: 5px; display: grid; gap: 2px; border: 1px solid var(--workspace-line); border-radius: 13px; color: var(--workspace-text); background: var(--workspace-raised); box-shadow: 0 16px 44px rgba(8, 18, 13, .18); }
  .composer .permission-menu > button { width: 100%; height: auto; min-height: 40px; padding: 7px 9px; display: flex; align-items: center; justify-content: space-between; gap: 10px; border-radius: 9px; color: var(--workspace-text); background: transparent; text-align: left; font-size: var(--workspace-chat-small-size); }
  .composer .permission-menu > button:hover:not(:disabled), .composer .permission-menu > button.selected { transform: none; background: var(--workspace-accent-soft); }
  .permission-menu button > span { min-width: 0; display: grid; gap: 2px; }
  .permission-menu button strong { color: var(--workspace-strong); font-weight: 720; }
  .permission-menu button.tone-danger strong { color: #d85c64; }
  .permission-menu button small { color: var(--workspace-muted); font-size: var(--workspace-chat-tiny-size); font-weight: 560; line-height: 1.35; }
  .permission-menu button > :global(.lume-icon) { flex: 0 0 auto; color: var(--workspace-accent); }
  .permission-note { margin: 2px 9px 4px; color: var(--workspace-faint); font-size: var(--workspace-chat-tiny-size); }
  .model-trigger span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .composer-tools .model-trigger:hover:not(:disabled), .composer-tools .model-trigger.active { color: var(--workspace-accent); background: var(--workspace-subtle); }
  .composer-tools .model-trigger:focus-visible { outline: 2px solid var(--workspace-accent); outline-offset: 2px; }
  .composer button:hover:not(:disabled) { transform: translateY(-1px) scale(1.03); }.composer button:disabled { opacity: .28; cursor: default; }
  .composer button.launching:disabled { opacity: 1; }
  .composer .agent-permission { max-width: 760px; margin: 0 auto 8px; padding: 11px 12px; display: grid; gap: 9px; border: 1px solid color-mix(in srgb, #d0a142 55%, var(--workspace-line)); border-radius: 13px; color: var(--workspace-text); background: color-mix(in srgb, #d0a142 11%, var(--workspace-raised)); }
  .agent-permission.risk-high { border-color: color-mix(in srgb, #d85c64 60%, var(--workspace-line)); background: color-mix(in srgb, #d85c64 9%, var(--workspace-raised)); }
  .agent-permission header { min-width: 0; display: flex; align-items: center; gap: 8px; color: #b9852a; }
  .agent-permission.risk-high header { color: #d85c64; }
  .agent-permission header strong { min-width: 0; color: var(--workspace-strong); font-size: var(--workspace-chat-font-size); font-weight: 720; line-height: 1.35; overflow-wrap: anywhere; }
  .agent-permission code { padding: 6px 8px; overflow: hidden; border: 1px solid var(--workspace-line); border-radius: 8px; color: var(--workspace-muted); background: color-mix(in srgb, var(--workspace-raised) 70%, transparent); font: 600 var(--workspace-chat-small-size) var(--lume-font-code, "SFMono-Regular", Consolas, "Liberation Mono", monospace); text-overflow: ellipsis; white-space: nowrap; }
  .permission-actions { display: flex; flex-wrap: wrap; gap: 6px; }
  .composer .permission-actions > button { width: auto; height: 30px; padding: 0 13px; border: 1px solid var(--workspace-line); border-radius: 9px; color: var(--workspace-text); background: var(--workspace-raised); font-size: var(--workspace-chat-small-size); font-weight: 750; }
  .composer .permission-actions > button.allow { border-color: transparent; color: #f5fbf7; background: var(--workspace-accent); }
  .composer .permission-actions > button.danger { color: #d85c64; }
  .composer .permission-actions > button:hover:not(:disabled) { transform: none; border-color: var(--workspace-accent); }
  .composer .agent-question { max-width: 760px; margin: 0 auto 8px; padding: 11px 12px; display: grid; gap: 10px; border: 1px solid color-mix(in srgb, var(--workspace-accent) 34%, var(--workspace-line)); border-radius: 13px; color: var(--workspace-text); background: color-mix(in srgb, var(--workspace-accent-soft) 70%, var(--workspace-raised)); }
  .agent-question-item { min-width: 0; display: grid; gap: 6px; }
  .agent-question-item > small { color: var(--workspace-accent); font-size: var(--workspace-chat-tiny-size); font-weight: 800; letter-spacing: .06em; text-transform: uppercase; }
  .agent-question-item > strong { color: var(--workspace-strong); font-size: var(--workspace-chat-font-size); font-weight: 700; line-height: 1.4; overflow-wrap: anywhere; }
  .question-options { display: grid; gap: 5px; }
  .composer .question-options > button { width: 100%; height: auto; min-height: 34px; padding: 6px 9px; display: flex; align-items: flex-start; gap: 9px; place-items: initial; border: 1px solid var(--workspace-line); border-radius: 9px; color: var(--workspace-text); background: var(--workspace-raised); text-align: left; }
  .composer .question-options > button:hover:not(:disabled), .composer .question-options > button.selected { border-color: var(--workspace-accent); background: var(--workspace-accent-soft); transform: none; }
  .question-options button b { color: var(--workspace-accent); font-size: var(--workspace-chat-small-size); font-variant-numeric: tabular-nums; }
  .question-options button > span { min-width: 0; display: grid; gap: 2px; font-size: var(--workspace-chat-small-size); font-weight: 700; line-height: 1.35; overflow-wrap: anywhere; }
  .question-options button > span small { color: var(--workspace-muted); font-size: var(--workspace-chat-tiny-size); font-weight: 500; }
  .question-hint { margin: 0; color: var(--workspace-muted); font-size: var(--workspace-chat-tiny-size); }
  .composer .question-submit { width: auto; height: 30px; padding: 0 14px; justify-self: end; border-radius: 9px; font-size: var(--workspace-chat-small-size); font-weight: 750; }
  .composer .slash-command-menu { position: absolute; z-index: 12; right: 0; bottom: calc(100% + 7px); left: 0; max-height: min(260px, 46vh); padding: 5px; display: grid; gap: 2px; overflow-x: hidden; overflow-y: auto; border: 1px solid var(--workspace-line); border-radius: 13px; color: var(--workspace-text); background: var(--workspace-raised); box-shadow: 0 16px 44px rgba(8, 18, 13, .18); }
  .slash-command-heading { min-height: 24px; padding: 2px 8px 5px; display: flex; align-items: center; justify-content: space-between; gap: 8px; border-bottom: 1px solid var(--workspace-line); }
  .slash-command-heading strong { color: var(--workspace-muted); font-size: var(--workspace-chat-tiny-size); font-weight: 800; letter-spacing: .08em; text-transform: uppercase; }
  .slash-command-heading small { color: var(--workspace-faint); font-size: var(--workspace-chat-tiny-size); font-weight: 650; white-space: nowrap; }
  .slash-command-heading kbd { padding: 1px 3px; border: 1px solid var(--workspace-line); border-radius: 4px; color: var(--workspace-muted); background: transparent; font: inherit; }
  .composer .slash-command-menu > button { width: 100%; min-height: 36px; height: auto; padding: 5px 8px; display: grid; grid-template-columns: minmax(64px, auto) minmax(0, 1fr); align-items: center; gap: 9px; place-items: initial; border-radius: 8px; color: var(--workspace-text); background: transparent; text-align: left; transition: background 120ms ease; }
  .composer .slash-command-menu > button:hover:not(:disabled) { transform: none; }
  .composer .slash-command-menu > button.active { color: var(--workspace-strong); background: var(--workspace-accent-soft); }
  .slash-command-loading { margin: 0; padding: 7px 8px; color: var(--workspace-muted); font-size: var(--workspace-chat-small-size); font-weight: 650; }
  .slash-command-menu code { color: var(--workspace-accent); font: 750 var(--workspace-chat-small-size) var(--lume-font-code, "SFMono-Regular", Consolas, "Liberation Mono", monospace); white-space: nowrap; }
  .slash-command-menu button > span { min-width: 0; display: grid; gap: 2px; overflow: hidden; font-size: var(--workspace-chat-small-size); font-weight: 620; text-overflow: ellipsis; white-space: nowrap; }
  .composer .mention-menu > button { grid-template-columns: 16px minmax(0, 1fr); }
  .mention-menu button > span { font-family: var(--lume-font-code, "SFMono-Regular", Consolas, "Liberation Mono", monospace); }
  .history-indicator { position: absolute; right: 10px; bottom: calc(100% + 5px); padding: 2px 7px; border: 1px solid var(--workspace-line); border-radius: 7px; color: var(--workspace-muted); background: var(--workspace-raised); font-size: var(--workspace-chat-tiny-size); font-weight: 720; font-variant-numeric: tabular-nums; pointer-events: none; }
  .slash-command-menu button > span small { overflow: hidden; color: var(--workspace-muted); font-size: var(--workspace-chat-tiny-size); font-weight: 650; text-overflow: ellipsis; text-transform: uppercase; }
  .plane-launch { width: 16px; height: 16px; display: grid; place-items: center; pointer-events: none; animation: plane-takeoff 450ms cubic-bezier(.22, .72, .26, 1) both; }
  .composer button:hover:not(:disabled) :global(.lume-icon) { transform: translate(1px, -1px); }.composer button :global(.lume-icon) { transition: transform 180ms cubic-bezier(.16, 1, .3, 1); }
  .composer button:hover:not(:disabled) :global(.send-plane-icon) { transform: translate(1px, -1px); }.composer button :global(.send-plane-icon) { transition: transform 180ms cubic-bezier(.16, 1, .3, 1); }
  .send-spinner { width: 13px; height: 13px; border: 1.5px solid currentColor; border-right-color: transparent; border-radius: 50%; animation: spin 650ms linear infinite; }
  @keyframes pane-arrive { from { opacity: .4; transform: translateY(5px); } }
  @keyframes latest-arrive { from { opacity: 0; transform: translateY(5px); } }
  @keyframes spin { to { transform: rotate(360deg); } }
  @keyframes plane-takeoff { 0% { opacity: 1; transform: translate(0, 0) rotate(0); } 20% { opacity: 1; transform: translate(-2px, 2px) rotate(-5deg); } 68% { opacity: 1; transform: translate(12px, -12px) rotate(3deg); } 100% { opacity: 0; transform: translate(26px, -26px) rotate(7deg); } }
  @keyframes status-pulse { 0%, 45% { box-shadow: 0 0 0 0 rgba(78, 152, 202, .28); } 80%, 100% { box-shadow: 0 0 0 4px rgba(78, 152, 202, 0); } }
  @keyframes thinking-label-shimmer { to { background-position: -240% 0; } }
  @keyframes takeover-fade { from { opacity: 0; } }
  @keyframes takeover-arrive { from { opacity: 0; transform: translateY(7px) scale(.975); } }
  @keyframes controls-arrive { from { opacity: 0; transform: translateY(-5px) scale(.985); } }
  @keyframes control-icon-arrive { from { opacity: .5; transform: scale(.74) translateY(2px); } }
  @keyframes max-thumb-pulse { to { box-shadow: 0 0 0 3px color-mix(in srgb, var(--effort-tone) 24%, transparent), 0 0 18px color-mix(in srgb, var(--effort-tone) 34%, transparent); } }
  @keyframes ultra-slider-flow { to { background-position: -220% 0; } }
  @container (max-width: 500px) {
    .pane-header { padding-right: 9px; padding-left: 12px; gap: 7px; }
    .source-badge { width: 23px; padding: 0; justify-content: center; }
    .source-badge .badge-label { display: none; }
    .status-badge.status-permission_required, .status-badge.status-failed { width: auto; max-width: 45cqw; padding: 0 7px; }
    .status-badge.status-permission_required .badge-label, .status-badge.status-failed .badge-label { min-width: 0; display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .source-badge :global(.brand-icon), .status-badge i { flex: 0 0 auto; }
    .conversation { --chat-edge-gutter: 28px; }.message { width: 94%; }.user-message { width: fit-content; max-width: 94%; }.composer { padding-right: 10px; padding-left: 10px; }
  }
  @media (max-height: 640px) { .conversation { padding-top: 17px; padding-bottom: 18px; }.pane-header { min-height: 56px; }.composer { padding-top: 9px; padding-bottom: 10px; } }
  @media (prefers-reduced-motion: reduce) { .session-pane, .latest-button, .status-badge.status-running i, .send-spinner, .plane-launch, .takeover-backdrop, .takeover-dialog, .writer-conflict-backdrop, .writer-conflict-dialog, .agent-controls-popover, .controls-loading i, .load-earlier-icon.loading, .agent-typing .typing-label, .composer-field.beam, .composer-field.beam::before, .composer-field.beam::after, .sources-scrim, .sources-sidebar, .final-actions button.loading :global(.lume-icon) { animation: none; }.agent-typing .typing-label { color: #5a91b5; background: none; }.pane-header::after, .pane-actions button, .writer-conflict-dialog footer button, .changed-file, .changed-file > :global(.lume-icon:last-child), .changed-files-toggle > :global(.lume-icon:last-child), .composer-field, .composer, .composer.crossfade-out, .composer.crossfade-in, .composer button, .composer button :global(.lume-icon), .composer button :global(.send-plane-icon), .time-gutter time, .final-actions, .final-actions button { transition: none; } }
  @media (prefers-reduced-motion: reduce) { .effort-field.max .effort-thumb, .effort-field.ultra .effort-track::before, .effort-field.ultra .effort-progress { animation: none; }.effort-thumb, .effort-thumb::before, .effort-progress { transition: none; } }
</style>
