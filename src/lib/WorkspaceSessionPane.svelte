<script lang="ts">
  import { onMount, tick } from "svelte";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import type { SessionActivity } from "$lib/domain";
  import type { PromptAttachmentInput } from "$lib/domain";
  import type { HubSession, WorkItem } from "$lib/hubProtocol";
  import type { Language } from "$lib/i18n";
  import ActivityTraceGroup from "$lib/ActivityTraceGroup.svelte";
  import StreamedMessage from "$lib/StreamedMessage.svelte";
  import ThinkingOrb from "$lib/ThinkingOrb.svelte";
  import ThreadAvatar from "$lib/ThreadAvatar.svelte";
  import BrandIcon from "$lib/BrandIcon.svelte";
  import LumeMascot from "$lib/LumeMascot.svelte";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import LumeSelect from "$lib/LumeSelect.svelte";
  import SystemBannerStack, { type SystemBannerItem } from "$lib/SystemBannerStack.svelte";
  import FileTypeIcon from "$lib/FileTypeIcon.svelte";
  import ResponseAttachments from "$lib/ResponseAttachments.svelte";
  import { displayText } from "$lib/i18n";
  import { displayFileChangePath, summarizeFileChanges } from "$lib/fileChanges";
  import { activityThinkingLabel, activityThinkingState, formatAgentDuration, isGenericAnalysisPlaceholder } from "$lib/activityPresentation";
  import { renderSafeMarkdown } from "$lib/markdown.js";
  import { BoundedRenderCache } from "$lib/boundedRenderCache";
  import { buildConversationEntries, buildConversationFeed } from "$lib/sessionConversation";
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
    getSessionCollaborationMode,
    getSessionModelSettings,
    forkSessionFromMessage,
    interruptPrompt,
    loadWorkspaceConversationPage,
    readLocalImageDataUrl,
    setClaudeSessionModelSettings,
    setSessionCollaborationMode,
    setSessionFastMode,
    setSessionModelSettings,
    steerQueuedPrompt,
    submitPrompt,
    takeControlSession,
    type CodexThreadModelSettings,
    type CollaborationMode,
  } from "$lib/lume";

  let {
    session,
    language = "en",
    closable = false,
    focused = false,
    maximized = false,
    streamMessages = true,
    onClose,
    onFocus,
    onOpenReview,
    onFork,
    onToggleMaximize,
  } = $props<{
    session: HubSession;
    language?: Language;
    closable?: boolean;
    focused?: boolean;
    maximized?: boolean;
    streamMessages?: boolean;
    onClose?: () => void;
    onFocus?: () => void;
    onOpenReview?: (path: string) => void;
    onFork?: (threadId: string) => void | Promise<void>;
    onToggleMaximize?: () => void;
  }>();

  type ResponseSource = {
    id: string;
    kind: "web" | "file" | "search";
    label: string;
    detail: string;
    href?: string;
  };

  const markdownCache = new BoundedRenderCache(64, 5 * 1024 * 1024);
  const paneOpenedAt = Date.now();
  let prompt = $state("");
  let sending = $state(false);
  let takingControl = $state(false);
  let takeoverConfirm = $state(false);
  let sendError = $state("");
  let promptAttachments = $state<PromptAttachmentInput[]>([]);
  let conversationElement = $state<HTMLDivElement | null>(null);
  let composerElement = $state<HTMLFormElement | null>(null);
  let introDismissed = $state(false);
  let controlsRoot = $state<HTMLDivElement | null>(null);
  let controlsOpen = $state(false);
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
  let interrupting = $state(false);
  let steeringQueued = $state(false);
  let followingTail = $state(true);
  let olderActivities = $state<SessionActivity[]>([]);
  let olderCursor = $state<{ createdAt: number; id: string } | null>(null);
  let historyHasMore = $state<boolean | null>(null);
  let historyLoading = $state(false);
  let historyError = $state("");
  let visibleFeedLimit = $state(120);
  const conversationActivities = $derived.by<SessionActivity[]>(() => {
    if (!olderActivities.length) return session.activities;
    const byId = new Map(olderActivities.map((activity) => [activity.id, activity]));
    for (const activity of session.activities) byId.set(activity.id, activity);
    return [...byId.values()].sort((left, right) => left.createdAt - right.createdAt || left.id.localeCompare(right.id));
  });
  const chatActivities = $derived(conversationActivities.filter((activity: SessionActivity) =>
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
  const feed = $derived(buildConversationFeed(entries, { includeAnalysisInTrace: true }));
  const hasConversationMessage = $derived(entries.some((entry) =>
    entry.activity.kind === "prompt" || entry.activity.kind === "message"
  ));
  const hiddenCount = $derived(Math.max(0, feed.length - visibleFeedLimit));
  const visibleFeed = $derived(hiddenCount ? feed.slice(-visibleFeedLimit) : feed);
  const canLoadEarlier = $derived(Boolean(
    hiddenCount > 0 || (historyHasMore ?? (session.nativeSessionId && session.activities.length >= 60))
  ));
  const activeTraceId = $derived.by(() => {
    if (!["running", "permission_required"].includes(session.status)) return null;
    const traceIndex = feed.findLastIndex((item) => item.kind === "trace");
    const promptIndex = feed.findLastIndex((item) =>
      item.kind === "entry" && item.entry.activity.kind === "prompt"
    );
    return traceIndex >= promptIndex && traceIndex >= 0 ? feed[traceIndex].id : null;
  });
  const activeThinkingState = $derived.by(() => {
    const activity = chatActivities.at(-1);
    return activity ? activityThinkingState(activity) : "breathing";
  });
  const promptIsRunning = $derived(session.status === "running");
  const freshChat = $derived(!hasConversationMessage && !promptIsRunning && !introDismissed);
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
      .filter((activity: SessionActivity) => activity.kind === "queued_prompt" && activity.status === "waiting")
      .sort((left: SessionActivity, right: SessionActivity) => left.createdAt - right.createdAt),
  );
  const nextQueuedPrompt = $derived(queuedPrompts[0] ?? null);
  const canSteer = $derived(Boolean(
    promptIsRunning
    && nextQueuedPrompt
    && session.capabilities.promptDeliveries.includes("steer")
  ));
  const supportsAgentControls = $derived(["codex", "claude_code"].includes(session.agent));
  const workItems = $derived(session.workSummary.plan?.items?.length
    ? session.workSummary.plan.items : session.workSummary.todo?.items ?? []);
  const workPlanContent = $derived(session.workSummary.plan?.content?.trim() ?? "");
  const workGoal = $derived(session.workSummary.goal ?? null);
  const completedWorkItems = $derived(workItems.filter((item: WorkItem) => item.status === "completed").length);
  let workOpen = $state(false);
  let sourceEntryId = $state<string | null>(null);
  let actionNotice = $state("");
  let forkingEntryId = $state<string | null>(null);
  let workClock = $state(Date.now());
  const controlsDisabled = $derived(
    ["running", "permission_required"].includes(session.status)
      || session.controlOrigin !== "lume" || controlsLoading || controlsSaving || fastSaving || modeSaving
  );
  const sourceEntry = $derived(sourceEntryId
    ? entries.find((entry) => entry.id === sourceEntryId) ?? null
    : null);
  const responseSources = $derived.by(() => sourceEntry ? sourcesForEntry(sourceEntry) : []);

  $effect(() => {
    const refreshKey = `${session.id}:${session.updatedAt}:${visibleFeed.length}`;
    if (!refreshKey || !followingTail) return;
    void tick().then(() => {
      if (conversationElement && followingTail) {
        conversationElement.scrollTop = conversationElement.scrollHeight;
      }
    });
  });

  $effect(() => {
    session.id;
    olderActivities = [];
    olderCursor = null;
    historyHasMore = null;
    historyLoading = false;
    historyError = "";
    visibleFeedLimit = 120;
    controlsOpen = false;
    zoomOpen = false;
    controlsError = "";
    modelSettings = null;
    fastMode = false;
    workOpen = false;
    sourceEntryId = null;
    actionNotice = "";
    forkingEntryId = null;
  });

  $effect(() => {
    if (workGoal?.status !== "active") return;
    const timer = window.setInterval(() => (workClock = Date.now()), 60_000);
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
      if (zoomOpen && zoomRoot && !zoomRoot.contains(event.target as Node)) {
        zoomOpen = false;
      }
    };
    const closeSources = (event: KeyboardEvent) => {
      if (event.key === "Escape" && sourceEntryId) sourceEntryId = null;
    };
    document.addEventListener("pointerdown", closeControls);
    window.addEventListener("keydown", closeSources);
    return () => {
      document.removeEventListener("pointerdown", closeControls);
      window.removeEventListener("keydown", closeSources);
    };
  });

  function tr(english: string, portuguese: string) {
    return language === "pt-BR" ? portuguese : english;
  }

  const systemBanners = $derived.by<SystemBannerItem[]>(() => {
    const items: SystemBannerItem[] = [];
    if (sendError) items.push({ id: "send-error", message: sendError, tone: "error", onDismiss: () => { sendError = ""; } });
    if (controlsError) items.push({ id: "controls-error", message: controlsError, tone: "error", onDismiss: () => { controlsError = ""; } });
    if (historyError) items.push({ id: "history-error", message: historyError, tone: "error", onDismiss: () => { historyError = ""; } });
    if (actionNotice) items.push({ id: "message-action", message: actionNotice, tone: "success", onDismiss: () => { actionNotice = ""; } });
    return items;
  });

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

  function renderMarkdown(key: string, value: string) {
    return markdownCache.render(key, value, renderSafeMarkdown);
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
      if (session.capabilities.canTakeControl) {
        return tr(
          "Write a prompt to take control of this CLI…",
          "Escreva um prompt para assumir o controle desta CLI…",
        );
      }
      return tr("Open the original source to respond", "Abra a origem original para responder");
    }
    if (promptIsRunning && canQueue) return tr("Queue the next prompt…", "Coloque o próximo prompt na fila…");
    if (promptIsRunning) return tr("Agent is working…", "O agente está trabalhando…");
    return tr(`Message ${sessionName()}…`, `Mensagem para ${sessionName()}…`);
  }

  async function sendPrompt() {
    const value = prompt.trim();
    if (!value || !canSend || sending || takingControl) return;
    if (!session.capabilities.canPrompt && session.capabilities.canTakeControl) {
      takeoverConfirm = true;
      return;
    }
    if (freshChat) {
      const previousTop = composerElement?.getBoundingClientRect().top;
      introDismissed = true;
      await tick();
      if (previousTop !== undefined && composerElement && !window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
        const distance = previousTop - composerElement.getBoundingClientRect().top;
        composerElement.animate(
          [{ transform: `translateY(${distance}px)` }, { transform: "translateY(0)" }],
          { duration: 420, easing: "cubic-bezier(0.16, 1, 0.3, 1)" },
        );
      }
    }
    sending = true;
    sendError = "";
    try {
      await submitPrompt(session.id, value, promptAttachments, promptIsRunning ? "queue" : "new_turn");
      prompt = "";
      promptAttachments = [];
    } catch (error) {
      sendError = String(error).replace(/^Error:\s*/, "");
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
    if (!nextQueuedPrompt || !canSteer || steeringQueued) return;
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
    if (session.agent === "claude_code") return ["", "low", "medium", "high", "xhigh", "max"];
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

  function chooseEffort(effort: string) {
    if (session.agent === "claude_code") claudeEffort = effort;
    else selectedEffort = effort;
    void saveAgentControls();
  }

  function chooseModel(model: string) {
    selectedModel = model;
    const option = modelSettings?.models.find((candidate) => candidate.model === model);
    if (option && !option.supportedReasoningEfforts.some((effort) => effort.value === selectedEffort)) {
      selectedEffort = option.defaultReasoningEffort;
    }
    void saveAgentControls();
  }

  async function loadAgentControls() {
    controlsLoading = true;
    controlsError = "";
    try {
      if (session.agent === "codex") {
        const [mode, settings] = await Promise.all([
          getSessionCollaborationMode(session.id),
          getSessionModelSettings(session.id),
        ]);
        collaborationMode = mode;
        modelSettings = settings;
        fastMode = settings.serviceTier === "fast";
        selectedModel = settings.model;
        const option = settings.models.find((candidate) => candidate.model === settings.model);
        selectedEffort = settings.reasoningEffort
          ?? option?.defaultReasoningEffort
          ?? option?.supportedReasoningEfforts[0]?.value
          ?? "";
      } else if (session.agent === "claude_code") {
        const settings = await getClaudeSessionModelSettings(session.id);
        claudeModel = settings.model ?? "";
        claudeEffort = settings.reasoningEffort ?? "";
      }
    } catch (error) {
      controlsError = String(error).replace(/^Error:\s*/, "");
    } finally {
      controlsLoading = false;
    }
  }

  async function toggleAgentControls() {
    controlsOpen = !controlsOpen;
    if (!controlsOpen || session.controlOrigin !== "lume") return;
    await loadAgentControls();
  }

  async function toggleFastMode() {
    if (session.agent !== "codex" || controlsDisabled || fastSaving) return;
    fastSaving = true;
    sendError = "";
    try {
      fastMode = await setSessionFastMode(session.id, !fastMode);
      if (modelSettings) modelSettings = { ...modelSettings, serviceTier: fastMode ? "fast" : "default" };
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
    if (session.agent !== "codex" || controlsDisabled || modeSaving) return;
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
    if (controlsDisabled) return;
    controlsSaving = true;
    controlsError = "";
    try {
      if (session.agent === "codex") {
        if (!selectedModel || !selectedEffort) return;
        modelSettings = await setSessionModelSettings(session.id, selectedModel, selectedEffort);
        fastMode = modelSettings.serviceTier === "fast";
      } else if (session.agent === "claude_code") {
        await setClaudeSessionModelSettings(
          session.id,
          claudeModel.trim() || undefined,
          claudeEffort || undefined,
        );
      }
    } catch (error) {
      controlsError = String(error).replace(/^Error:\s*/, "");
    } finally {
      controlsSaving = false;
    }
  }

  async function confirmTakeover() {
    const value = prompt.trim();
    if (!value || !session.capabilities.canTakeControl || takingControl) return;
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

  function handleComposerKeydown(event: KeyboardEvent) {
    if (event.key !== "Enter" || event.shiftKey || event.isComposing) return;
    event.preventDefault();
    void sendPrompt();
  }

  async function previewLocalImage(path: string) {
    return createImagePreview(await readLocalImageDataUrl(path), language);
  }

  async function chooseAttachments() {
    if (!canAttach || sending || takingControl || promptAttachments.length >= 4) return;
    sendError = "";
    try {
      const selected = await openDialog({ multiple: true, directory: false });
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

  function trackConversationScroll() {
    if (!conversationElement) return;
    followingTail = conversationElement.scrollHeight - conversationElement.scrollTop - conversationElement.clientHeight < 72;
  }

  async function revealEarlierMessages() {
    if (historyLoading) return;
    const previousHeight = conversationElement?.scrollHeight ?? 0;
    const previousTop = conversationElement?.scrollTop ?? 0;
    followingTail = false;
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
    await tick();
    if (conversationElement) conversationElement.scrollTop = previousTop + conversationElement.scrollHeight - previousHeight;
  }

  function scrollToLatest() {
    followingTail = true;
    conversationElement?.scrollTo({ top: conversationElement.scrollHeight, behavior: "smooth" });
  }
</script>

<article
  class:focused
  class="session-pane"
  data-workspace-pane={session.id}
  style:--workspace-chat-font-adjust={`${(textZoom - 1) * 9}px`}
  style:--workspace-chat-small-adjust={`${(textZoom - 1) * 8}px`}
  style:--workspace-chat-tiny-adjust={`${(textZoom - 1) * 7}px`}
  tabindex="-1"
  onpointerdown={() => onFocus?.()}
  onfocusin={() => onFocus?.()}
>
  <SystemBannerStack items={systemBanners} contained dismissLabel={tr("Dismiss", "Fechar")} />
  <header
    class="pane-header"
    role="group"
    aria-label={tr("Session header", "Cabeçalho da sessão")}
  >
    <span class="agent-mark"><ThreadAvatar seed={session.nativeSessionId || session.sessionName || session.id} label={sessionName()} size={38} /></span>
    <span class="pane-identity">
      <strong>{sessionName()}</strong>
      <small title={session.workingDirectory}><BrandIcon name={session.agent} size={10} />{session.agentLabel} · {session.project}</small>
    </span>
    {#if session.controlOrigin === "external"}
      <span class="source-badge" title={sourceLabel()}>
        <BrandIcon name={sourceIcon()} size={11} />
        {sourceLabel()}
      </span>
    {/if}
    <span class="status-badge status-{session.status}"><i></i>{displayText(language, session.statusLabel)}</span>
    {#if supportsAgentControls || onToggleMaximize || closable}
      <div class="pane-actions">
        <div class="text-zoom" bind:this={zoomRoot}>
          <button
            class:active={zoomOpen}
            type="button"
            title={tr("Chat text size", "Tamanho do texto do chat")}
            aria-label={tr("Chat text size", "Tamanho do texto do chat")}
            aria-expanded={zoomOpen}
            onclick={() => (zoomOpen = !zoomOpen)}
          >Aa</button>
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
              <LumeIcon name="restore" size={16} />
            {:else}
              <LumeIcon name="maximize" size={16} />
            {/if}
          </button>
        {/if}
        {#if closable}
        <button type="button" title={tr("Close pane", "Fechar painel")} aria-label={tr("Close pane", "Fechar painel")} onclick={onClose}>
          <LumeIcon name="close" size={16} />
        </button>
        {/if}
      </div>
    {/if}
  </header>

  {#if workItems.length || workPlanContent || workGoal}
    <section class="work-overview" aria-label={tr("Agent plan and goal", "Plano e objetivo do agente")}>
      <button class="work-toggle" type="button" aria-expanded={workOpen} onclick={() => (workOpen = !workOpen)}>
        <LumeIcon name="mode-plan" size={15} />
        <strong>{workGoal ? "GOAL" : "TO DO"}</strong>
        {#if workItems.length}<span class="work-count">{workGoal ? "TO DO · " : ""}{completedWorkItems}/{workItems.length}</span>{/if}
        {#if workGoal}<span class="work-objective">{workGoal.objective}</span>{/if}
        {#if workGoal?.status === "active"}<small>{Math.max(0, Math.floor((workClock - workGoal.startedAt) / 60_000))}m</small>{/if}
        <LumeIcon name="chevron-down" size={13} />
      </button>
      {#if workItems.length}<div class="work-progress" role="progressbar" aria-label={tr("Plan progress", "Progresso do plano")} aria-valuemin="0" aria-valuemax={workItems.length} aria-valuenow={completedWorkItems}><span style={`width:${completedWorkItems / workItems.length * 100}%`}></span></div>{/if}
      {#if workOpen}
        <div class="work-detail">
          {#if workGoal}<p class="goal-line"><b class:complete={workGoal.status === "complete"} class:blocked={workGoal.status === "blocked"}>{workGoal.status === "active" ? tr("In progress", "Em andamento") : workGoal.status === "complete" ? tr("Completed", "Concluído") : tr("Blocked", "Bloqueado")}</b>{workGoal.objective}</p>{/if}
          {#if workItems.length}
            <ol>
              {#each workItems as item}
                <li class:done={item.status === "completed"} class:doing={item.status === "in_progress"}><i>{item.status === "completed" ? "✓" : item.status === "in_progress" ? "·" : ""}</i><span>{item.label}</span></li>
              {/each}
            </ol>
          {:else if workPlanContent}
            <p class="work-plan-content">{workPlanContent}</p>
          {/if}
        </div>
      {/if}
    </section>
  {/if}

  <div class="conversation-shell">
  <div class="conversation" bind:this={conversationElement} onscroll={trackConversationScroll}>
    {#if canLoadEarlier}
      <button class="load-earlier-chat" type="button" disabled={historyLoading} onclick={() => void revealEarlierMessages()}>
        <span class:loading={historyLoading} class="load-earlier-icon"><LumeIcon name="chevron-down" size={14} /></span>
        {historyLoading ? tr("Loading earlier messages…", "Carregando mensagens anteriores…") : tr("Load earlier messages", "Carregar mensagens anteriores")}
      </button>
    {/if}
    {#each visibleFeed as feedItem (feedItem.id)}
      {#if feedItem.kind === "trace"}
        <div class="workspace-event-trace">
          <ActivityTraceGroup
            activities={feedItem.entries.map((entry) => entry.activity)}
            active={feedItem.id === activeTraceId}
            plain
            {language}
          />
        </div>
        {#if feedItem.files.length}
          <div class="changed-files">
            <strong>{tr("Files changed", "Arquivos alterados")}</strong>
            {#each feedItem.files as file}
              <button class="changed-file" type="button" title={tr(`Review ${file.path}`, `Revisar ${file.path}`)} onclick={() => onOpenReview?.(file.path)}>
                <FileTypeIcon path={file.path} />
                <span>{displayFileChangePath(file.path)}</span>
                <b class="added">+{file.added}</b><b class="removed">-{file.removed}</b>
                <LumeIcon name="chevron-down" size={12} />
              </button>
            {/each}
          </div>
        {/if}
      {:else}
        {@const entry = feedItem.entry}
        {@const item = entry.activity}
        {#if item.kind === "prompt" && (item.detail || item.attachments?.length)}
          <div class="conversation-row user-row">
            <span class="time-gutter"><time>{time(item.createdAt)}</time></span>
            <div class="conversation-entry">
              <section class="message user-message">
                {#if item.detail}<pre>{item.detail}</pre>{/if}
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
                <div class="markdown-content"><StreamedMessage text={item.detail} animate={streamMessages && (item.createdAt > paneOpenedAt || item.status === "running")} live={item.status === "running"} startFromBeginning={item.createdAt > paneOpenedAt} render={(value) => renderMarkdown(entry.id, value)} /></div>
                <ResponseAttachments
                  text={item.detail}
                  attachments={item.attachments ?? []}
                  workingDirectory={session.workingDirectory}
                  {language}
                  includeCitations
                  onError={(error) => (sendError = error)}
                />
                {#if entry.isFinalResponse}
                  <footer class="response-footer">
                    {#if entry.durationMs !== undefined}
                      <span class="response-duration">{tr("Worked for", "Trabalhou por")} {formatAgentDuration(entry.durationMs)}</span>
                    {/if}
                    <div class="final-actions" aria-label={tr("Response actions", "Ações da resposta")}>
                      <button type="button" title={tr("Copy response", "Copiar resposta")} aria-label={tr("Copy response", "Copiar resposta")} onclick={() => void copyResponse(item.detail ?? "")}><LumeIcon name="copy" size={14} /></button>
                      <button class:active={sourceEntryId === entry.id} type="button" title={tr("View sources", "Visualizar fontes")} aria-label={tr("View sources", "Visualizar fontes")} aria-expanded={sourceEntryId === entry.id} onclick={() => (sourceEntryId = sourceEntryId === entry.id ? null : entry.id)}><LumeIcon name="sources" size={14} /></button>
                      {#if session.agent === "codex" && session.nativeSessionId && promptForEntry(entry)}
                        <button class:loading={forkingEntryId === entry.id} type="button" disabled={Boolean(forkingEntryId)} title={tr("Fork from this prompt", "Criar fork a partir deste prompt")} aria-label={tr("Fork from this prompt", "Criar fork a partir deste prompt")} onclick={() => void forkFromEntry(entry)}><LumeIcon name="fork" size={14} /></button>
                      {/if}
                    </div>
                  </footer>
                {/if}
              </section>
              {#if entry.files.length}
                <div class="changed-files">
                  <strong>{tr("Files changed", "Arquivos alterados")}</strong>
                  {#each entry.files as file}
                    <button class="changed-file" type="button" title={tr(`Review ${file.path}`, `Revisar ${file.path}`)} onclick={() => onOpenReview?.(file.path)}>
                      <FileTypeIcon path={file.path} />
                      <span>{displayFileChangePath(file.path)}</span>
                      <b class="added">+{file.added}</b><b class="removed">-{file.removed}</b>
                      <LumeIcon name="chevron-down" size={12} />
                    </button>
                  {/each}
                </div>
              {/if}
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
        {#if entry.files.length && item.kind !== "message"}
          <div class="changed-files">
            <strong>{tr("Files changed", "Arquivos alterados")}</strong>
            {#each entry.files as file}
              <button class="changed-file" type="button" title={tr(`Review ${file.path}`, `Revisar ${file.path}`)} onclick={() => onOpenReview?.(file.path)}>
                <FileTypeIcon path={file.path} />
                <span>{displayFileChangePath(file.path)}</span>
                <b class="added">+{file.added}</b><b class="removed">-{file.removed}</b>
                <LumeIcon name="chevron-down" size={12} />
              </button>
            {/each}
          </div>
        {/if}
      {/if}
    {:else}
      {#if !freshChat}
        <div class="empty-chat">
          <BrandIcon name={session.agent} size={26} />
          <strong>{tr("No conversation yet", "Nenhuma conversa ainda")}</strong>
          <span>{tr("Messages and activity will appear here in real time.", "Mensagens e atividades aparecerão aqui em tempo real.")}</span>
        </div>
      {/if}
    {/each}
    {#if session.status === "running"}
      <div class="agent-typing">
        <ThinkingOrb
          state={activeThinkingState}
          size={42}
          speed={0.92}
          label={tr(`${sessionName()} is working`, `${sessionName()} está trabalhando`)}
        />
        <span>{activityThinkingLabel(activeThinkingState, language)}</span>
      </div>
    {/if}
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

  {#if takeoverConfirm}
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
          <button class="takeover-primary" type="button" disabled={takingControl} onclick={() => void confirmTakeover()}>
            {takingControl ? tr("Taking control…", "Assumindo controle…") : tr("Take control & send", "Assumir e enviar")}
          </button>
        </footer>
      </dialog>
    </div>
  {/if}

  <form bind:this={composerElement} class:fresh={freshChat} class:unavailable={!canCompose} class="composer" onpaste={(event) => void pasteAttachments(event)} onsubmit={(event) => { event.preventDefault(); void sendPrompt(); }}>
    {#if freshChat}
      <div class="composer-welcome"><LumeMascot status="idle" awake size={45} /><strong>{tr("Hello. What shall we work on?", "Olá. No que vamos trabalhar?")}</strong></div>
    {/if}
    {#if queuedPrompts.length}
      <div class="queue-tray">
        <span><i>{queuedPrompts.length}</i><b>{tr("Queued", "Na fila")}</b><small>{nextQueuedPrompt?.detail || nextQueuedPrompt?.title}</small></span>
        {#if canSteer}
          <button type="button" disabled={steeringQueued} onclick={() => void steerNextPrompt()} title={tr("Steer into the current task", "Enviar para a tarefa atual")}>
            <LumeIcon name="steer" size={14} />
            {tr("Steer now", "Enviar agora")}
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
    <div class:beam={freshChat && canCompose} class="composer-field">
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
      <textarea
        bind:value={prompt}
        rows="1"
        placeholder={composerPlaceholder()}
        disabled={!canCompose || sending || takingControl}
        aria-label={composerPlaceholder()}
        onkeydown={handleComposerKeydown}
      ></textarea>
      {#if promptIsRunning && canQueue}<span class="queue-label">{tr("Next", "Próximo")}</span>{/if}
      {#if promptIsRunning && session.capabilities.canInterrupt}
        <button class="stop-button" type="button" disabled={interrupting} onclick={() => void interruptAgentPrompt()} aria-label={tr("Interrupt prompt", "Interromper prompt")} title={tr("Interrupt prompt · Enter queues", "Interromper prompt · Enter adiciona à fila")}>
          {#if interrupting}<i class="send-spinner"></i>{:else}<LumeIcon name="stop" size={15} />{/if}
        </button>
      {:else}
        <button type="submit" disabled={!prompt.trim() || !canSend || sending || takingControl} aria-label={promptIsRunning ? tr("Add to queue", "Adicionar à fila") : tr("Send prompt", "Enviar prompt")}>
          {#if sending || takingControl}
            <i class="send-spinner"></i>
          {:else}
            <LumeIcon name="send" size={16} />
          {/if}
        </button>
      {/if}
      {#if freshChat && canCompose}<span class="composer-beam-bloom" aria-hidden="true"></span>{/if}
    </div>
    {#if supportsAgentControls}
      <div class="composer-tools">
        <div class="agent-controls" bind:this={controlsRoot}>
          <button class:active={controlsOpen} class="model-trigger" type="button"
            aria-label={tr("Choose model and effort", "Escolher modelo e esforço")}
            aria-haspopup="dialog" aria-expanded={controlsOpen}
            onclick={() => void toggleAgentControls()}>
            <span>{session.agent === "codex"
              ? (modelSettings?.models.find((option) => option.model === selectedModel)?.displayName || selectedModel || "Model")
              : (claudeModel || tr("Model", "Modelo"))}</span>
            <LumeIcon name="chevron-down" size={12} />
          </button>
          {#if controlsOpen}
            <section class="agent-controls-popover" aria-label={tr("Model and effort", "Modelo e esforço")}>
              {#if session.controlOrigin !== "lume"}
                <p class="controls-note">{tr("Take control of this session to change its model.", "Assuma o controle desta sessão para mudar o modelo.")}</p>
              {:else if controlsLoading}
                <div class="controls-loading"><i></i>{tr("Loading settings…", "Carregando ajustes…")}</div>
              {:else}
                {#if session.agent === "codex"}
                  {#if modelSettings}
                    <label class="controls-field"><span>{tr("Model", "Modelo")}</span>
                      <LumeSelect value={selectedModel}
                        options={modelSettings.models.map((option) => ({ value: option.model, label: option.displayName, description: option.isDefault ? tr("Default", "Padrão") : option.description }))}
                        ariaLabel={tr("Model", "Modelo")} minWidth={190} onValueChange={chooseModel} />
                    </label>
                  {/if}
                {:else}
                  <label class="controls-field"><span>{tr("Model", "Modelo")}</span><input bind:value={claudeModel} disabled={controlsDisabled} maxlength="128" placeholder={tr("Session default", "Padrão da sessão")} onchange={() => void saveAgentControls()} /></label>
                {/if}
                {#if effortValues().length}
                  <div class:max={currentEffort().toLowerCase() === "max"} class:ultra={currentEffort().toLowerCase() === "ultra"} class="controls-field effort-field"
                    style={`--effort-tone:${effortTone(currentEffort())};--effort-progress:${effortProgress()}%;--effort-ratio:${effortProgress() / 100};`}>
                    <span>{tr("Reasoning effort", "Esforço de raciocínio")}<b aria-live="polite">{effortLabel()}</b></span>
                    <div class="effort-track">
                      <span class="effort-progress" aria-hidden="true"></span>
                      <span class="effort-thumb" aria-hidden="true"></span>
                      <input type="range" min="0" max={Math.max(0, effortValues().length - 1)} step="1"
                        value={currentEffortIndex()} disabled={controlsDisabled}
                        aria-label={tr("Reasoning effort", "Nível de raciocínio")} oninput={chooseEffortIndex} onchange={() => void saveAgentControls()} />
                    </div>
                    <div class="effort-scale">{#each effortValues() as effort (effort || "default")}<button type="button" class:active={currentEffort() === effort} disabled={controlsDisabled} aria-label={`${tr("Set effort to", "Definir esforço como")} ${effortLabel(effort)}`} onclick={() => chooseEffort(effort)}><span>{effortLabel(effort)}</span></button>{/each}</div>
                  </div>
                {/if}
                {#if promptIsRunning}<p class="controls-note">{tr("Finish or interrupt the current prompt to apply changes.", "Finalize ou interrompa o prompt atual para aplicar mudanças.")}</p>{/if}
                {#if controlsSaving}<p class="controls-saving" role="status">{tr("Saving…", "Salvando…")}</p>{/if}
              {/if}
            </section>
          {/if}
        </div>
        {#if session.agent === "codex"}
          <button class="tool-icon fast-toggle" class:enabled={fastMode} type="button"
            disabled={controlsDisabled || fastSaving || !modelSettings} aria-pressed={fastMode}
            aria-label={fastMode ? tr("Disable Fast mode", "Desativar modo Fast") : tr("Enable Fast mode", "Ativar modo Fast")}
            title={tr("Fast mode · higher credit usage", "Modo Fast · maior consumo de créditos")}
            onclick={() => void toggleFastMode()}>
            {#key fastMode}<LumeIcon name={fastMode ? "bolt-filled" : "bolt"} size={16} />{/key}
          </button>
          <button class="tool-icon mode-toggle" class:enabled={collaborationMode === "plan"} type="button"
            disabled={controlsDisabled || modeSaving || !modelSettings} aria-pressed={collaborationMode === "plan"}
            aria-label={collaborationMode === "plan" ? tr("Switch to Default mode", "Mudar para modo padrão") : tr("Switch to Plan mode", "Mudar para modo planejamento")}
            title={collaborationMode === "plan" ? "Plan" : tr("Default", "Padrão")}
            onclick={() => void toggleCollaborationMode()}>
            {#key collaborationMode}<LumeIcon name={collaborationMode === "plan" ? "mode-plan" : "mode-default"} size={16} />{/key}
          </button>
        {/if}
      </div>
    {/if}
  </form>
</article>

<style>
  .session-pane { --workspace-chat-font-size: calc(12px + var(--workspace-chat-font-adjust)); --workspace-chat-small-size: calc(10px + var(--workspace-chat-small-adjust)); --workspace-chat-tiny-size: calc(8px + var(--workspace-chat-tiny-adjust)); --chat-small-font-size: var(--workspace-chat-small-size); --chat-tiny-font-size: var(--workspace-chat-tiny-size); --activity-summary-height: calc(44px + var(--workspace-chat-font-adjust)); --activity-row-height: calc(42px + var(--workspace-chat-font-adjust)); --activity-title-size: calc(11px + var(--workspace-chat-small-adjust)); --activity-detail-size: calc(9px + var(--workspace-chat-tiny-adjust)); position: relative; min-width: 0; min-height: 0; height: 100%; container-type: inline-size; display: flex; flex-direction: column; overflow: hidden; background: transparent; animation: pane-arrive 280ms cubic-bezier(.16, 1, .3, 1) both; }
  .pane-header { position: relative; z-index: 2; min-width: 0; min-height: 64px; padding: 10px 14px 10px 17px; display: flex; align-items: center; gap: 10px; flex: 0 0 auto; border-bottom: 1px solid var(--workspace-line); background: var(--workspace-pane); }
  .pane-header::after { position: absolute; right: 0; bottom: -1px; left: 0; height: 1px; background: color-mix(in srgb, var(--workspace-accent) 20%, transparent); content: ""; transform: scaleX(.18); transform-origin: left; transition: transform 320ms cubic-bezier(.16, 1, .3, 1); }
  .session-pane:hover .pane-header::after,
  .session-pane.focused .pane-header::after { transform: scaleX(1); }
  .session-pane.focused .pane-header::after { background: color-mix(in srgb, var(--workspace-accent) 58%, transparent); }
  .agent-mark { width: 38px; height: 38px; display: grid; place-items: center; flex: 0 0 auto; color: var(--workspace-accent); }
  .pane-identity { min-width: 0; flex: 1; display: grid; gap: 2px; }
  .pane-identity strong, .pane-identity small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .pane-identity strong { color: var(--workspace-strong); font-size: 12px; font-weight: 740; letter-spacing: -.015em; }
  .pane-identity small { display: flex; align-items: center; gap: 4px; color: var(--workspace-muted); font-size: 9px; }
  .source-badge, .status-badge { min-height: 23px; padding: 0 7px; display: inline-flex; align-items: center; gap: 5px; flex: 0 0 auto; border: 1px solid var(--workspace-line); border-radius: 7px; color: var(--workspace-muted); background: transparent; font-size: 8px; font-weight: 740; }
  .status-badge { border-color: transparent; background: var(--workspace-subtle); }
  .status-badge i { width: 6px; height: 6px; flex: 0 0 auto; border-radius: 50%; background: #84948c; }
  .status-badge.status-running i { background: #4e98ca; animation: status-pulse 1.8s ease-out infinite; }
  .status-badge.status-completed i { background: #50aa79; }
  .status-badge.status-permission_required i { background: #d0a142; }
  .status-badge.status-failed i { background: #c66762; }
  .pane-actions { display: flex; align-items: center; gap: 2px; }
  .pane-actions > button, .agent-controls > button, .text-zoom > button { width: 29px; height: 29px; padding: 0; display: grid; place-items: center; border: 0; border-radius: 8px; color: var(--workspace-muted); background: transparent; cursor: pointer; transition: color 120ms ease, background 120ms ease, transform 180ms cubic-bezier(.16, 1, .3, 1); }
  .pane-actions > button:hover, .agent-controls > button:hover, .agent-controls > button.active, .text-zoom > button:hover, .text-zoom > button.active { color: var(--workspace-accent); background: var(--workspace-subtle); transform: translateY(-1px); }
  .text-zoom { position: relative; }
  .text-zoom > button { font-size: 11px; font-weight: 760; letter-spacing: -.06em; }
  .text-zoom-popover { position: absolute; z-index: 12; top: 36px; right: 0; width: 136px; min-height: 40px; padding: 5px; display: grid; grid-template-columns: 30px 1fr 30px; align-items: center; gap: 3px; border: 1px solid var(--workspace-line); border-radius: 10px; color: var(--workspace-text); background: var(--workspace-raised); box-shadow: 0 12px 30px rgba(8, 18, 13, .16); animation: controls-arrive 180ms cubic-bezier(.16, 1, .3, 1) both; }
  .text-zoom-popover button { width: 30px; height: 28px; border: 0; border-radius: 7px; color: var(--workspace-accent); background: var(--workspace-subtle); font-size: 16px; cursor: pointer; }
  .text-zoom-popover button:disabled { opacity: .35; cursor: default; }
  .text-zoom-popover output { color: var(--workspace-muted); font-size: 10px; font-weight: 720; text-align: center; font-variant-numeric: tabular-nums; }
  .agent-controls { position: relative; }
  .agent-controls-popover { position: absolute; z-index: 12; bottom: calc(100% + 9px); left: 0; width: min(320px, calc(100cqw - 20px)); max-height: min(450px, calc(100vh - 115px)); padding: 15px; overflow: visible; border: 1px solid var(--workspace-line); border-radius: 16px; color: var(--workspace-text); background: var(--workspace-raised); box-shadow: 0 20px 60px rgba(8, 18, 13, .2); animation: controls-arrive 180ms cubic-bezier(.16, 1, .3, 1) both; }
  .controls-field { margin-top: 11px; display: grid; gap: 7px; }
  .agent-controls-popover .controls-field:first-child { margin-top: 0; }
  .controls-field > span { display: flex; align-items: center; justify-content: space-between; color: var(--workspace-muted); font-size: 8px; font-weight: 760; }
  .controls-field > span b { color: var(--effort-tone, var(--workspace-accent)); font-size: 8px; text-transform: capitalize; transition: color 180ms ease, transform 220ms cubic-bezier(.16, 1, .3, 1); }
  .controls-field > input:not([type="range"]) { min-width: 0; height: 32px; padding: 0 10px; border: 1px solid var(--workspace-line); border-radius: 9px; outline: 0; color: var(--workspace-strong); background: var(--workspace-pane); font-size: 9px; }
  .controls-field > input:not([type="range"]):focus { border-color: color-mix(in srgb, var(--workspace-accent) 52%, transparent); }
  .controls-field :global(.lume-select) { width: 100%; }
  .effort-track { position: relative; height: 22px; margin: 1px 6px 0; display: flex; align-items: center; }
  .effort-track::before, .effort-progress { position: absolute; right: 0; left: 0; height: 4px; border-radius: 999px; content: ""; }
  .effort-track::before { background: color-mix(in srgb, var(--workspace-muted) 18%, transparent); box-shadow: inset 0 1px 1px color-mix(in srgb, var(--workspace-strong) 7%, transparent); }
  .effort-progress { background: var(--effort-tone); transform: scaleX(var(--effort-ratio)); transform-origin: left; transition: transform 220ms cubic-bezier(.16, 1, .3, 1), background 180ms ease; }
  .effort-thumb { position: absolute; z-index: 1; left: var(--effort-progress); width: 15px; height: 15px; border: 3px solid var(--workspace-raised); border-radius: 50%; background: var(--effort-tone); box-shadow: 0 0 0 1px color-mix(in srgb, var(--effort-tone) 76%, transparent), 0 3px 8px rgba(0, 0, 0, .18); pointer-events: none; transform: translateX(-50%); transition: left 220ms cubic-bezier(.16, 1, .3, 1), background 180ms ease, transform 180ms cubic-bezier(.16, 1, .3, 1), box-shadow 180ms ease; }
  .effort-track input { position: absolute; z-index: 2; inset: 0; width: 100%; height: 100%; margin: 0; appearance: none; opacity: 0; cursor: pointer; }
  .effort-track:hover .effort-thumb { transform: translateX(-50%) scale(1.14); }
  .effort-track:active .effort-thumb { transform: translateX(-50%) scale(.9); }
  .effort-track:focus-within { border-radius: 999px; outline: 2px solid color-mix(in srgb, var(--workspace-accent) 68%, transparent); outline-offset: 3px; }
  .effort-field.max .effort-thumb { animation: max-thumb-pulse 1.8s ease-in-out infinite alternate; }
  .effort-field.ultra .effort-track::before { background: linear-gradient(90deg, #7652c6, #b48aff, #7652c6); background-size: 220% 100%; box-shadow: 0 0 10px rgba(154, 112, 232, .34); animation: ultra-slider-flow 2.1s linear infinite; }
  .effort-field.ultra .effort-progress { background: rgba(207, 183, 255, .82); box-shadow: 0 0 9px rgba(154, 112, 232, .44); }
  .effort-field.ultra .effort-thumb { box-shadow: 0 0 0 1px #9a70e8, 0 0 12px rgba(154, 112, 232, .48); }
  .effort-scale { display: flex; justify-content: space-between; gap: 1px; }
  .effort-scale button { min-width: 0; padding: 3px 0 1px; display: grid; justify-items: center; flex: 1; border: 0; color: var(--workspace-faint); background: transparent; font-size: 7px; text-transform: capitalize; cursor: pointer; transition: color 140ms ease, transform 180ms cubic-bezier(.16, 1, .3, 1); }
  .effort-scale button:hover:not(:disabled) { color: var(--workspace-muted); transform: translateY(-1px); }
  .effort-scale button.active { color: var(--effort-tone); font-weight: 760; }
  .effort-scale button:disabled { cursor: default; }
  .controls-note { margin: 10px 0 0; color: var(--workspace-muted); font-size: 8px; line-height: 1.5; }
  .controls-loading { min-height: 68px; display: flex; align-items: center; justify-content: center; gap: 8px; color: var(--workspace-muted); font-size: 9px; }
  .controls-loading i { width: 12px; height: 12px; border: 1.5px solid currentColor; border-right-color: transparent; border-radius: 50%; animation: spin 650ms linear infinite; }
  .controls-saving { margin: 10px 0 0; color: var(--workspace-muted); font-size: 8px; }
  .agent-controls-popover input:disabled { opacity: .45; cursor: default; }
  .work-overview { min-width: 0; flex: 0 0 auto; border-bottom: 1px solid var(--workspace-line); background: var(--workspace-pane); }
  .work-toggle { width: 100%; min-width: 0; min-height: 34px; padding: 5px clamp(20px, 5cqw, 54px); display: flex; align-items: center; gap: 7px; border: 0; color: var(--workspace-muted); background: transparent; cursor: pointer; text-align: left; }
  .work-toggle:hover { color: var(--workspace-strong); background: var(--workspace-subtle); }
  .work-toggle strong { color: var(--workspace-accent); font-size: 8px; letter-spacing: .07em; }
  .work-count, .work-toggle small { color: var(--workspace-muted); font-size: 9px; font-weight: 700; font-variant-numeric: tabular-nums; }
  .work-objective { min-width: 0; flex: 1; overflow: hidden; color: var(--workspace-strong); font-size: 9px; text-overflow: ellipsis; white-space: nowrap; }
  .work-toggle :global(.lume-icon:last-child) { margin-left: auto; transition: transform 160ms cubic-bezier(.16, 1, .3, 1); }
  .work-toggle[aria-expanded="true"] :global(.lume-icon:last-child) { transform: rotate(180deg); }
  .work-progress { height: 2px; background: var(--workspace-subtle); }.work-progress span { height: 100%; display: block; background: var(--workspace-accent); transition: width 260ms cubic-bezier(.16, 1, .3, 1); }
  .work-detail { max-height: min(180px, 30vh); padding: 5px clamp(20px, 5cqw, 54px) 12px; overflow: auto; scrollbar-width: thin; color: var(--workspace-text); font-size: 10px; line-height: 1.5; }
  .work-detail ol { margin: 0; padding: 0; display: grid; gap: 7px; list-style: none; }
  .work-detail li { min-width: 0; display: flex; gap: 8px; overflow-wrap: anywhere; }.work-detail li i { width: 14px; height: 14px; flex: 0 0 auto; display: grid; place-items: center; border: 1px solid var(--workspace-line); border-radius: 50%; color: var(--workspace-faint); font-size: 9px; font-style: normal; }.work-detail li.done i, .work-detail li.doing i { border-color: var(--workspace-accent); color: var(--workspace-accent); }
  .goal-line { margin: 0 0 9px; display: flex; align-items: baseline; gap: 8px; overflow-wrap: anywhere; }.goal-line b { flex: 0 0 auto; color: #4e91bf; font-size: 8px; }.goal-line b.complete { color: var(--workspace-accent); }.goal-line b.blocked { color: #b96862; }
  .work-plan-content { margin: 0; white-space: pre-wrap; overflow-wrap: anywhere; }
  .conversation-shell { position: relative; min-width: 0; min-height: 0; display: flex; flex: 1 1 auto; overflow: hidden; }
  .conversation { --chat-edge-gutter: clamp(28px, 5cqw, 54px); min-width: 0; min-height: 0; width: 100%; padding: 25px var(--chat-edge-gutter) 26px; display: flex; flex-direction: column; gap: 15px; flex: 1 1 auto; overflow: auto; overscroll-behavior: contain; scrollbar-width: thin; scrollbar-color: var(--workspace-scroll-thumb) transparent; }
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
  .time-gutter time { opacity: 0; transform: translateX(3px); transition: opacity 120ms ease, transform 160ms cubic-bezier(.16, 1, .3, 1); }
  .time-gutter:hover time, .conversation-row:focus-within .time-gutter time { opacity: 1; transform: translateX(0); }
  .message { width: min(88%, 720px); min-width: 0; color: var(--workspace-text); font-size: var(--workspace-chat-font-size); }
  .user-message { width: fit-content; max-width: min(88%, 720px); align-self: flex-end; padding: 9px 12px; border: 1px solid var(--workspace-user-line); border-radius: 14px 14px 4px 14px; background: var(--workspace-user); }
  .agent-message { align-self: flex-start; padding: 3px 0 7px; }
  .analysis-message header { margin-bottom: 7px; display: flex; align-items: center; gap: 8px; }
  .analysis-message header strong { min-width: 0; flex: 1; color: var(--workspace-strong); font-size: var(--workspace-chat-small-size); font-weight: 750; }
  time { color: var(--workspace-faint); font-size: var(--workspace-chat-tiny-size); font-variant-numeric: tabular-nums; }
  .response-footer { min-height: 25px; margin-top: 8px; display: flex; align-items: center; gap: 8px; }
  .final-actions { min-height: 24px; margin-left: auto; display: flex; justify-content: flex-end; gap: 2px; opacity: .35; transition: opacity 140ms ease; }
  .agent-row:hover .final-actions, .final-actions:focus-within { opacity: 1; }
  .final-actions button { width: 24px; height: 24px; padding: 0; display: grid; place-items: center; border: 0; border-radius: 7px; color: var(--workspace-faint); background: transparent; cursor: pointer; transition: color 120ms ease, background 120ms ease, transform 120ms ease; }
  .final-actions button:hover, .final-actions button:focus-visible, .final-actions button.active { color: var(--workspace-accent); background: var(--workspace-subtle); }
  .final-actions button:active { transform: scale(.92); }
  .final-actions button:disabled { cursor: wait; opacity: .55; }
  .final-actions button.loading :global(.lume-icon) { animation: history-loading 800ms linear infinite; }
  pre { max-width: 100%; margin: 0; overflow-wrap: anywhere; color: inherit; font: inherit; line-height: 1.55; white-space: pre-wrap; word-break: break-word; }
  .markdown-content { min-width: 0; overflow-wrap: anywhere; font-size: var(--workspace-chat-font-size); line-height: 1.68; word-break: break-word; }
  .markdown-content :global(p) { margin: 0 0 .75em; }.markdown-content :global(p:last-child) { margin-bottom: 0; }
  .markdown-content :global(strong) { color: var(--workspace-strong); font-weight: 790; }
  .markdown-content :global(em) { font-style: italic; }
  .markdown-content :global(del) { color: var(--workspace-faint); }
  .markdown-content :global(a) { color: var(--workspace-accent); font-weight: 650; text-decoration: underline; text-decoration-color: color-mix(in srgb, var(--workspace-accent) 42%, transparent); text-underline-offset: 2px; }
  .markdown-content :global(ul), .markdown-content :global(ol) { margin: .4em 0 .7em; padding-left: 1.5em; }
  .markdown-content :global(li) { margin: .2em 0; }
  .markdown-content :global(blockquote) { margin: .6em 0; padding: .3em .75em; border-left: 2px solid var(--workspace-accent); color: var(--workspace-muted); background: var(--workspace-subtle); }
  .markdown-content :global(:not(pre) > code) { padding: .08em .3em; border-radius: 4px; color: var(--workspace-accent); background: var(--workspace-subtle); font-size: .92em; }
  .markdown-content :global(h1), .markdown-content :global(h2), .markdown-content :global(h3) { margin: 1.2em 0 .45em; color: var(--workspace-strong); line-height: 1.25; letter-spacing: -.02em; }
  .markdown-content :global(pre) { max-width: 100%; padding: 11px 12px; overflow: auto; border: 1px solid var(--workspace-line); border-radius: 10px; background: var(--workspace-code); font: var(--workspace-chat-small-size)/1.6 "SFMono-Regular", Consolas, monospace; }
  .markdown-content :global(pre code) { padding: 0; color: inherit; background: transparent; white-space: pre-wrap; word-break: break-word; }
  .markdown-content :global(code) { overflow-wrap: anywhere; font-family: "SFMono-Regular", Consolas, monospace; }
  .markdown-content :global(.markdown-table-wrap) { max-width: 100%; margin: .7em 0; overflow-x: auto; border: 1px solid var(--workspace-line); border-radius: 9px; }
  .markdown-content :global(table) { width: 100%; display: block; overflow-x: auto; border-collapse: collapse; }
  .markdown-content :global(th), .markdown-content :global(td) { padding: 7px 9px; border-bottom: 1px solid var(--workspace-line); text-align: left; }
  .markdown-content :global(th) { color: var(--workspace-strong); background: var(--workspace-subtle); font-size: var(--workspace-chat-small-size); }
  .markdown-content :global(img) { max-width: 100%; height: auto; border-radius: 9px; }
  .markdown-content :global(hr) { margin: 1em 0; border: 0; border-top: 1px solid var(--workspace-line); }
  .response-duration { width: max-content; display: block; color: var(--workspace-faint); font-size: var(--workspace-chat-tiny-size); font-variant-numeric: tabular-nums; }
  .analysis-message { min-width: 0; padding: 5px 0 7px; color: var(--workspace-muted); }
  .analysis-message .markdown-content { font-size: calc(11px + var(--workspace-chat-small-adjust)); line-height: 1.62; }
  .changed-files { min-width: 0; padding: 10px 0; display: grid; gap: 2px; border-top: 1px solid var(--workspace-line); border-bottom: 1px solid var(--workspace-line); }
  .changed-files > strong { margin: 0 4px 4px; color: var(--workspace-muted); font-size: var(--workspace-chat-tiny-size); font-weight: 760; }
  .changed-file { min-width: 0; min-height: 28px; padding: 3px 5px; display: flex; align-items: center; gap: 7px; border: 0; border-radius: 7px; color: var(--workspace-text); background: transparent; font: calc(9px + var(--workspace-chat-tiny-adjust))/1.4 "SFMono-Regular", Consolas, monospace; text-align: left; cursor: pointer; transition: color 120ms ease, background 120ms ease; }
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
  .agent-typing > span { color: transparent; background: linear-gradient(90deg, #67837a 10%, #5aa1ce 44%, #a3d2ef 53%, #5aa1ce 62%, #67837a 90%); background-size: 240% 100%; background-clip: text; font-size: 12px; font-weight: 720; letter-spacing: -.01em; animation: thinking-label-shimmer 1.75s linear infinite; }
  .latest-button { position: absolute; right: 18px; bottom: 30px; z-index: 3; width: 30px; height: 30px; display: grid; place-items: center; border: 1px solid var(--workspace-line); border-radius: 10px; color: var(--workspace-accent); background: var(--workspace-raised); box-shadow: 0 5px 16px rgba(17, 35, 27, .09); cursor: pointer; animation: latest-arrive 180ms cubic-bezier(.16, 1, .3, 1) both; }
  .latest-button:hover { transform: translateY(-1px); }
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
  .composer { position: relative; z-index: 2; min-width: 0; flex: 0 0 auto; padding: 12px clamp(14px, 4cqw, 28px) 15px; border-top: 1px solid var(--workspace-line); background: var(--workspace-pane); }
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
  .composer-field { position: relative; max-width: 760px; min-height: 45px; margin: 0 auto; padding: 6px 6px 6px 12px; display: flex; align-items: flex-end; gap: 7px; border: 1px solid var(--workspace-line); border-radius: 13px; background: var(--workspace-raised); box-shadow: 0 5px 18px rgba(17, 35, 27, .055); transition: border-color 140ms ease, box-shadow 140ms ease; }
  @property --composer-beam-angle { syntax: "<angle>"; inherits: true; initial-value: 0deg; }
  /* The three-layer md/colorful beam is adapted for Svelte from Libraries.dev BorderBeam (MIT). */
  .composer-field.beam {
    --composer-beam-strength: .9;
    --composer-beam-stroke-opacity: .4;
    --composer-beam-inner-opacity: .42;
    --composer-beam-bloom-opacity: .44;
    --composer-beam-highlight: conic-gradient(from var(--composer-beam-angle), transparent 0% 54%, rgba(0, 0, 0, .08) 57%, rgba(0, 0, 0, .2) 60%, rgba(0, 0, 0, .4) 63%, rgba(0, 0, 0, .55) 66%, rgba(0, 0, 0, .4) 69%, rgba(0, 0, 0, .2) 72%, rgba(0, 0, 0, .08) 75%, transparent 78% 100%);
    --composer-beam-halo: conic-gradient(from var(--composer-beam-angle), transparent 0% 58%, rgba(0, 0, 0, .08) 65%, rgba(0, 0, 0, .4) 69%, rgba(0, 0, 0, .6) 70% 70.5%, rgba(0, 0, 0, .4) 71.5%, rgba(0, 0, 0, .08) 75%, transparent 82% 100%);
    --composer-beam-window: conic-gradient(from var(--composer-beam-angle), transparent 0% 30%, rgba(255, 255, 255, .1) 36%, rgba(255, 255, 255, .35) 44%, white 52% 80%, rgba(255, 255, 255, .35) 86%, rgba(255, 255, 255, .1) 92%, transparent 95% 100%);
    --composer-beam-colors: radial-gradient(ellipse 70px 40px at 33% -7.4%, rgb(255, 50, 100), transparent), radial-gradient(ellipse 60px 35px at 12% -5%, rgb(40, 140, 255), transparent), radial-gradient(ellipse 40px 70px at 2.1% 68.3%, rgb(50, 200, 80), transparent), radial-gradient(ellipse 20px 35px at 2.1% 68.3%, rgb(30, 185, 170), transparent), radial-gradient(ellipse 180px 32px at 74.4% 100%, rgb(100, 70, 255), transparent), radial-gradient(ellipse 85px 26px at 55% 100%, rgb(40, 140, 255), transparent), radial-gradient(ellipse 74px 32px at 93.9% 0%, rgb(255, 120, 40), transparent), radial-gradient(ellipse 26px 42px at 100% 27.1%, rgb(240, 50, 180), transparent), radial-gradient(ellipse 52px 48px at 100% 27.1%, rgb(180, 40, 240), transparent);
    overflow: hidden;
    isolation: isolate;
    animation: composer-beam-orbit 4.4s linear infinite;
  }
  :global(.workspace.dark) .composer-field.beam {
    --composer-beam-stroke-opacity: .54;
    --composer-beam-inner-opacity: .56;
    --composer-beam-bloom-opacity: .36;
    --composer-beam-highlight: conic-gradient(from var(--composer-beam-angle), transparent 0% 54%, rgba(255, 255, 255, .1) 57%, rgba(255, 255, 255, .3) 60%, rgba(255, 255, 255, .6) 63%, rgba(255, 255, 255, .75) 66%, rgba(255, 255, 255, .6) 69%, rgba(255, 255, 255, .3) 72%, rgba(255, 255, 255, .1) 75%, transparent 78% 100%);
    --composer-beam-halo: conic-gradient(from var(--composer-beam-angle), transparent 0% 58%, rgba(255, 255, 255, .08) 65%, rgba(255, 255, 255, .45) 69%, rgba(255, 255, 255, .85) 70% 70.5%, rgba(255, 255, 255, .45) 71.5%, rgba(255, 255, 255, .08) 75%, transparent 82% 100%);
  }
  .composer-field.beam::before, .composer-field.beam::after, .composer-beam-bloom { position: absolute; inset: 0; border-radius: inherit; content: ""; pointer-events: none; animation: composer-beam-appear .6s ease-out both, composer-beam-hue 12s ease-in-out infinite; }
  .composer-field.beam::after {
    z-index: 2;
    padding: 1px;
    background: var(--composer-beam-highlight), var(--composer-beam-colors);
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
  .composer-beam-bloom {
    z-index: 3;
    padding: 1px;
    background: var(--composer-beam-halo);
    -webkit-mask: linear-gradient(#fff 0 0) content-box, linear-gradient(#fff 0 0);
    -webkit-mask-composite: xor;
    mask: linear-gradient(#fff 0 0) content-box, linear-gradient(#fff 0 0);
    mask-composite: exclude;
    opacity: calc(var(--composer-beam-strength) * var(--composer-beam-bloom-opacity));
    animation-name: composer-beam-appear, composer-beam-hue-bloom;
  }
  @keyframes composer-beam-orbit { to { --composer-beam-angle: 360deg; } }
  @keyframes composer-beam-appear { from { opacity: 0; } }
  @keyframes composer-beam-hue { 0%, 100% { filter: hue-rotate(-30deg) brightness(1.3) saturate(1.2); } 50% { filter: hue-rotate(30deg) brightness(1.3) saturate(1.2); } }
  @keyframes composer-beam-hue-bloom { 0%, 100% { filter: blur(8px) hue-rotate(-30deg) brightness(1.3) saturate(1.2); } 50% { filter: blur(8px) hue-rotate(30deg) brightness(1.3) saturate(1.2); } }
  .composer-field:focus-within { border-color: color-mix(in srgb, var(--workspace-accent) 48%, transparent); box-shadow: 0 7px 22px rgba(17, 35, 27, .075), 0 0 0 3px color-mix(in srgb, var(--workspace-accent) 8%, transparent); }
  .composer-field.beam:focus-within { border-color: var(--workspace-line); }
  .composer textarea { field-sizing: content; min-width: 0; max-height: 130px; min-height: 31px; padding: 7px 0 5px; flex: 1; resize: none; overflow-y: auto; border: 0; outline: 0; color: var(--workspace-strong); background: transparent; font-size: 11px; line-height: 1.5; }
  .composer textarea::placeholder { color: var(--workspace-faint); }.composer textarea:disabled { cursor: default; }
  .composer.unavailable .composer-field { background: var(--workspace-subtle); box-shadow: none; }
  .queue-label { margin-bottom: 8px; padding: 3px 6px; border-radius: 5px; color: #4d8cb8; background: rgba(78, 152, 202, .1); font-size: 7px; font-weight: 800; text-transform: uppercase; }
  .composer button { width: 33px; height: 33px; display: grid; place-items: center; flex: 0 0 auto; border: 0; border-radius: 10px; color: #f5fbf7; background: var(--workspace-accent); cursor: pointer; transition: transform 180ms cubic-bezier(.16, 1, .3, 1), opacity 120ms ease; }
  .composer .attach-button { color: var(--workspace-muted); background: transparent; }
  .composer .attach-button:hover:not(:disabled) { color: var(--workspace-accent); background: var(--workspace-subtle); }
  .composer .stop-button { color: #fff7f6; background: #b96862; }
  .composer-tools { max-width: 760px; min-height: 31px; margin: 6px auto 0; display: flex; align-items: center; gap: 3px; }
  .composer-tools .model-trigger, .composer-tools .tool-icon { width: auto; min-width: 30px; height: 28px; padding: 0 7px; display: inline-flex; align-items: center; justify-content: center; gap: 5px; border-radius: 8px; color: var(--workspace-muted); background: transparent; font-size: 9px; font-weight: 680; }
  .composer-tools .model-trigger { max-width: min(180px, 42cqw); padding-left: 4px; }
  .model-trigger span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .composer-tools .model-trigger:hover:not(:disabled), .composer-tools .tool-icon:hover:not(:disabled), .composer-tools .model-trigger.active, .composer-tools .tool-icon.enabled { color: var(--workspace-accent); background: var(--workspace-subtle); }
  .composer-tools .tool-icon { width: 30px; padding: 0; }
  .composer-tools .tool-icon:focus-visible, .composer-tools .model-trigger:focus-visible { outline: 2px solid var(--workspace-accent); outline-offset: 2px; }
  .composer-tools .tool-icon :global(.lume-icon) { animation: control-icon-arrive 210ms cubic-bezier(.16, 1, .3, 1) both; }
  .composer button:hover:not(:disabled) { transform: translateY(-1px) scale(1.03); }.composer button:disabled { opacity: .28; cursor: default; }
  .composer button:hover:not(:disabled) :global(.lume-icon) { transform: translate(1px, -1px); }.composer button :global(.lume-icon) { transition: transform 180ms cubic-bezier(.16, 1, .3, 1); }
  .send-spinner { width: 13px; height: 13px; border: 1.5px solid currentColor; border-right-color: transparent; border-radius: 50%; animation: spin 650ms linear infinite; }
  @keyframes pane-arrive { from { opacity: .4; transform: translateY(5px); } }
  @keyframes latest-arrive { from { opacity: 0; transform: translateY(5px); } }
  @keyframes spin { to { transform: rotate(360deg); } }
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
    .source-badge, .status-badge { width: 23px; padding: 0; justify-content: center; overflow: hidden; color: transparent; }
    .source-badge :global(.brand-icon), .status-badge i { flex: 0 0 auto; }
    .conversation { --chat-edge-gutter: 28px; }.message { width: 94%; }.user-message { width: fit-content; max-width: 94%; }.composer { padding-right: 10px; padding-left: 10px; }
  }
  @media (max-height: 640px) { .conversation { padding-top: 17px; padding-bottom: 18px; }.pane-header { min-height: 56px; }.composer { padding-top: 9px; padding-bottom: 10px; } }
  @media (prefers-reduced-motion: reduce) { .session-pane, .latest-button, .status-badge.status-running i, .send-spinner, .takeover-backdrop, .takeover-dialog, .agent-controls-popover, .controls-loading i, .load-earlier-icon.loading, .agent-typing > span, .composer-field.beam, .composer-field.beam::before, .composer-field.beam::after, .composer-beam-bloom, .sources-scrim, .sources-sidebar, .final-actions button.loading :global(.lume-icon) { animation: none; }.agent-typing > span { color: #5a91b5; background: none; }.pane-header::after, .pane-actions button, .changed-file, .changed-file > :global(.lume-icon:last-child), .composer-field, .composer button, .composer button :global(.lume-icon), .time-gutter time, .final-actions, .final-actions button { transition: none; } }
  @media (prefers-reduced-motion: reduce) { .composer-tools .tool-icon :global(.lume-icon), .effort-field.max .effort-thumb, .effort-field.ultra .effort-track::before { animation: none; }.work-progress span, .work-toggle :global(.lume-icon:last-child), .effort-field input[type="range"]::-webkit-slider-thumb { transition: none; } }
</style>
