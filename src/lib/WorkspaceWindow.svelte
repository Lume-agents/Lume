<script lang="ts">
  import { onMount, tick } from "svelte";
  import { fade, fly, slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { animatedDisclosure } from "$lib/animatedDisclosure";
  import { sessionLauncherTransition } from "$lib/sessionLauncherTransition";
  import { emit, listen } from "@tauri-apps/api/event";
  import { getVersion } from "@tauri-apps/api/app";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { relaunch } from "@tauri-apps/plugin-process";
  import { check, type Update } from "@tauri-apps/plugin-updater";
  import { availableMonitors, getCurrentWindow } from "@tauri-apps/api/window";
  import QRCode from "qrcode";
  import BrandIcon from "$lib/BrandIcon.svelte";
  import LumeLogo from "$lib/LumeLogo.svelte";
  import AccentColorPicker from "$lib/AccentColorPicker.svelte";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import CodexCliAssociationDialog from "$lib/CodexCliAssociationDialog.svelte";
  import AgentConnectionDialog from "$lib/AgentConnectionDialog.svelte";
  import MacosAutomationDialog from "$lib/MacosAutomationDialog.svelte";
  import { agentConnectionMessage } from "$lib/agentConnection";
  import { macosAutomationMessage } from "$lib/macosAutomation";
  import WorkspaceHeaderIcon from "$lib/WorkspaceHeaderIcon.svelte";
  import WorkspaceSidebarToggleIcon from "$lib/WorkspaceSidebarToggleIcon.svelte";
  import { copyResolvedColorTokens } from "$lib/floatingTheme";
  import { codeFonts, ensureCustomFonts, importCustomFont, listCustomFonts, removeCustomFont, uiFonts, type CustomFont } from "$lib/fonts";
  import { appearanceAttributes, appearanceThemes, darkBases, lightBases, type AppearanceTheme } from "$lib/appearance";
  import LumeSelect from "$lib/LumeSelect.svelte";
  import RemoteComputers from "$lib/RemoteComputers.svelte";
  import { collectAgentAlerts } from "$lib/agentAlerts";
  import { usageAlertDismissals } from "$lib/usageAlertDismissals";
  import SystemBannerStack, { type SystemBannerItem } from "$lib/SystemBannerStack.svelte";
  import WorkspaceInspector from "$lib/WorkspaceInspector.svelte";
  import WorkspaceReviewCenter from "$lib/WorkspaceReviewCenter.svelte";
  import WorkspaceSessionPane from "$lib/WorkspaceSessionPane.svelte";
  import WorkflowBoard from "$lib/WorkflowBoard.svelte";
  import ThreadAvatar from "$lib/ThreadAvatar.svelte";
  import SessionEnvironmentMenu from "$lib/SessionEnvironmentMenu.svelte";
  import { hasOpenWorkspacePane, resolveLiveResumableSession } from "$lib/sessionIdentity";
  import { noteSubagentInteraction, parentWaitingForSubagents, subagentsForSession } from "$lib/workspaceAgents";
  import { WorkspaceStartup } from "$lib/workspaceStartup";
  import type { AgentKind, CompanionStatus, ExternalAgentPlugin, IntegrationDiagnostic, IntegrationStatus, InternalService, MobileGatewayStatus, MobilePairingOffer, MobileScope, PairedDevice, Preferences, ResumableSession, WorkflowGroupDefinition } from "$lib/domain";
  import type { ExternalWriterConflict, HubSession } from "$lib/hubProtocol";
  import type { Language } from "$lib/i18n";
  import { displayText } from "$lib/i18n";
  import {
    beginMobilePairing,
    canLinkCodexCli,
    isUnidentifiedCodexCli,
    configureIntegration,
    configureVscode,
    defaultPreferences,
    diagnoseIntegration,
    disableMobileGateway,
    enableMobileGateway,
    installExternalPlugin,
    loadExternalPlugins,
    loadHubSnapshot,
    listExternalWriterConflicts,
    loadIntegrationStatuses,
    loadResumableSessions,
    loadMobileGatewayStatus,
    loadOverlayPosition,
    loadPairedDevices,
    loadPreferences,
    markWorkspaceFrontendReady,
    reportWorkspaceFrontendFailure,
    loadVscodeStatus,
    launchAgentSession,
    refreshAgentRateLimits,
    removeExternalPlugin,
    renameSession,
    revealBrowserCompanion,
    revealPluginDirectory,
    revokePairedDevice,
    savePreferences,
    watchShortcutRegistrationError,
    setNativeFileDialogActive,
    setPairedDeviceScopes,
    takeControlSession,
    cancelExternalWriterAttempt,
    forkCodexThread,
    terminateSession,
  } from "$lib/lume";

  type WorkspaceNamedLayout = {
    id: string;
    name: string;
    paneKeys: string[];
    splitRatio: number;
    tertiaryRatio: number;
    projectFilter: string;
    inspectorOpen: boolean;
    updatedAt: number;
  };

  type SettingsSectionKey =
    | "appearance"
    | "preferences"
    | "agents"
    | "companions"
    | "externalDetectors"
    | "shortcuts"
    | "projectProfiles"
    | "remoteComputers"
    | "mobileAccess"
    | "about"
    | "reset";
  type SettingsDataResource =
    | "integrations"
    | "vscode"
    | "externalPlugins"
    | "mobileStatus"
    | "pairedDevices"
    | "monitors"
    | "version";

  const settingsSectionResources: Record<SettingsSectionKey, SettingsDataResource[]> = {
    appearance: [],
    preferences: ["monitors"],
    agents: ["integrations"],
    companions: ["vscode"],
    externalDetectors: ["externalPlugins"],
    shortcuts: [],
    projectProfiles: ["integrations", "monitors"],
    remoteComputers: [],
    mobileAccess: ["mobileStatus", "pairedDevices"],
    about: ["version"],
    reset: [],
  };

  type WorkspaceDropIntent = {
    kind: "insert" | "replace" | "move";
    index: number;
    left: number;
    width: number;
  };

  type WorkspaceDropGeometry = {
    bounds: DOMRect;
    panes: Array<{ node: HTMLElement; bounds: DOMRect }>;
  };

  type PaneHeaderGesture = {
    pointerId: number;
    captureTarget: HTMLElement;
    sessionId: string;
    startX: number;
    startY: number;
    lastX: number;
    lastY: number;
    holdTimer: number;
    mode: "pending" | "relocating";
    layoutReady: boolean;
    previousMaximizedPaneId: string | null;
  };

  let sessions = $state<HubSession[]>([]);
  let externalWriterConflicts = $state<Record<string, ExternalWriterConflict>>({});
  let preferences = $state<Preferences>(structuredClone(defaultPreferences));
  let language = $state<Language>("en");
  let systemDark = $state(false);
  let settingsOpen = $state(false);
  let settingsLoading = $state(false);
  let settingsLoadingSections = $state<SettingsSectionKey[]>([]);
  let settingsSaving = $state(false);
  let settingsError = $state("");
  let customFonts = $state<CustomFont[]>([]);
  const fontOptions = (choices: typeof uiFonts) => [
    ...choices.map((choice) => ({ value: choice.value, label: tr(choice.label, choice.labelPt) })),
    ...customFonts.map((font) => ({ value: "custom:" + font.id, label: font.name })),
  ];
  async function importFont(target: "uiFont" | "codeFont") {
    const selected = await withNativeDialog(() => openDialog({ multiple: false, directory: false, title: tr("Import a font", "Importar uma fonte"), filters: [{ name: tr("Fonts", "Fontes"), extensions: ["ttf", "otf", "woff", "woff2"] }] }));
    if (typeof selected !== "string") return;
    try {
      const font = await importCustomFont(selected);
      customFonts = [...customFonts, font];
      await savePreferencePatch({ [target]: "custom:" + font.id } as Partial<Preferences>);
    } catch (reason) { settingsError = String(reason).replace(/^Error:\s*/, ""); }
  }
  async function deleteFont(font: CustomFont) {
    await removeCustomFont(font.id).catch(() => undefined);
    customFonts = customFonts.filter((item) => item.id !== font.id);
    const patch: Partial<Preferences> = {};
    if (preferences.uiFont === "custom:" + font.id) patch.uiFont = "default";
    if (preferences.codeFont === "custom:" + font.id) patch.codeFont = "default";
    if (Object.keys(patch).length) await savePreferencePatch(patch);
  }
  onMount(() => { void listCustomFonts().then((fonts) => { customFonts = fonts; }).catch(() => undefined); });
  let settingsSections = $state({
    appearance: false,
    preferences: false,
    agents: false,
    companions: false,
    externalDetectors: false,
    shortcuts: false,
    projectProfiles: false,
    remoteComputers: false,
    mobileAccess: false,
    about: false,
    reset: false,
  });
  const settingsResourceLoadedAt = new Map<SettingsDataResource, number>();
  const settingsResourceRequests = new Map<SettingsDataResource, Promise<void>>();
  const settingsSectionRequests = new Map<SettingsSectionKey, Promise<void>>();
  let settingsResourceLoadingCount = 0;
  let shortcutRegistrationError = $state<string | null>(null);
  let settingsMessage = $state("");
  let integrations = $state<IntegrationStatus[]>([]);
  let connectionAgent = $state<IntegrationStatus["kind"] | null>(null);
  let connectionMessage = $state("");
  let automationRequired = $state(false);
  let integrationDiagnostics = $state<Partial<Record<IntegrationStatus["kind"], IntegrationDiagnostic>>>({});
  let configuringIntegration = $state<IntegrationStatus["kind"] | null>(null);
  let diagnosingIntegration = $state<IntegrationStatus["kind"] | null>(null);
  let antigravityHookConfirmation = $state(false);
  let vscodeStatus = $state<CompanionStatus>({ installed: false, configured: false, detail: "" });
  let configuringVscode = $state(false);
  let externalPlugins = $state<ExternalAgentPlugin[]>([]);
  let installingPlugin = $state(false);
  let monitors = $state<Array<{ id: string; label: string }>>([]);
  let selectedProfileKey = $state<string | null>(null);
  let shortcutEditorKey = $state<"openShortcut" | "globalShortcut" | "newSessionShortcut" | "whiteboardShortcut" | "workspaceShortcut" | null>(null);
  let shortcutDraft = $state("");
  let mobileStatus = $state<MobileGatewayStatus | null>(null);
  let pairedDevices = $state<PairedDevice[]>([]);
  let pairingOffer = $state<MobilePairingOffer | null>(null);
  let pairingQr = $state<string | null>(null);
  let mobileBusy = $state(false);
  let appVersion = $state("0.15.0");
  let dismissedAgentAlertIds = $state<string[]>([]);
  let rateLimitRefreshRequested = false;
  let antigravityRateLimitRefreshRequested = false;
  let updateState = $state<"idle" | "checking" | "available" | "up_to_date" | "downloading" | "ready" | "error">("idle");
  let availableVersion = $state<string | null>(null);
  let updateDetail = $state("");
  let updateProgress = $state<number | null>(null);
  let pendingUpdate: Update | null = null;
  let resetConfirming = $state(false);
  let loading = $state(true);
  let internalServices = $state<InternalService[]>([]);
  let expandedSubagentSessions = $state<Set<string>>(new Set());
  let sidebarCollapsed = $state(false);
  let sidebarTextHidden = $state(false);
  let sidebarElement = $state<HTMLElement | null>(null);
  const sidebarMotion = new WeakMap<HTMLElement, Animation>();
  let sidebarToggleBusy = false;
  const observedSubagentIds = new Map<string, Set<string>>();
  let subagentRevision = $state(0);
  let error = $state("");
  let query = $state("");
  let searchOpen = $state(false);
  let searchInput = $state<HTMLInputElement | null>(null);
  let headerControl = $state<"project" | "layout" | null>(null);
  let sessionContextMenu = $state<{ sessionId: string; x: number; y: number; confirming: boolean; renaming: boolean } | null>(null);
  let sessionContextBusy = $state(false);
  let sessionContextError = $state("");
  let sessionContextNode = $state<HTMLDivElement | null>(null);
  let sessionRenameDraft = $state("");
  let cliAssociationSessionId = $state<string | null>(null);
  let streamMessages = $state(true);
  let workspaceBackgroundImage = $state("");
  let workspaceBackgroundImageOpacity = $state(100);
  let agentMessageSurface = $state(true);
  let workspaceBackgroundInput = $state<HTMLInputElement | null>(null);
  let filter = $state<"all" | "active" | "attention">("all");
  let projectFilter = $state("all");
  let primaryId = $state<string | null>(null);
  let secondaryId = $state<string | null>(null);
  let tertiaryId = $state<string | null>(null);
  let focusedPaneId = $state<string | null>(null);
  let maximizedPaneId = $state<string | null>(null);
  let inspectorOpen = $state(true);
  let boardOpen = $state(false);
  let boardMounted = $state(false);
  let inspectorSection = $state<"session" | "repository">("session");
  let reviewOpen = $state(false);
  let reviewWide = $state(false);
  let reviewInitialPath = $state<string | undefined>();
  let inspectorBeforeReview = $state(false);
  let splitRatio = $state(0.5);
  let tertiaryRatio = $state(0.34);
  let resizingDivider = $state<0 | 1 | null>(null);
  let draggingSessionId = $state<string | null>(null);
  let headerRelocatingSessionId = $state<string | null>(null);
  let workspaceDropIntent = $state<WorkspaceDropIntent | null>(null);
  let sidebarReleaseIntent = $state(false);
  let showDragPreview = $state(false);
  /** While the dragged agent is over the board, the board shows it as a card and the floating preview steps aside. */
  let dragOverBoard = $state(false);
  let dragPreviewElement = $state<HTMLDivElement | null>(null);
  let dragPreviewFrame = 0;
  let dragPreviewPosition: { x: number; y: number } | null = null;
  let dragPreviewTarget: { x: number; y: number } | null = null;
  let dragPreviewVelocity = { x: 0, y: 0 };
  let dragPreviewReducedMotion = false;
  let paneHeaderGesture: PaneHeaderGesture | null = null;
  let namedLayouts = $state<WorkspaceNamedLayout[]>([]);
  let selectedNamedLayoutId = $state("");
  let namingLayout = $state(false);
  let layoutName = $state("");
  let layoutNameInput = $state<HTMLInputElement | null>(null);
  let layoutMessage = $state("");
  let launcherOpen = $state(false);
  let launcherRoot = $state<HTMLDivElement | null>(null);
  let launcherPopoverNode: HTMLDivElement | null = null;
  let launching = $state<IntegrationStatus["kind"] | null>(null);
  let launchingSessionId = $state<string | null>(null);
  let launchingPhase = $state<"choosing" | "opening" | null>(null);
  let resumeAgent = $state<IntegrationStatus["kind"] | null>(null);
  let resumableSessions = $state<ResumableSession[]>([]);
  let loadingResumeAgent = $state<IntegrationStatus["kind"] | null>(null);

  async function openAgentSearch() {
    launcherOpen = false;
    sidebarCollapsed = false;
    searchOpen = true;
    await tick();
    searchInput?.focus();
  }

  function closeAgentSearch() {
    searchOpen = false;
    query = "";
  }

  async function toggleSidebar() {
    if (sidebarToggleBusy) return;
    sidebarToggleBusy = true;
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (sidebarCollapsed && !reducedMotion) sidebarTextHidden = true;
    if (!sidebarCollapsed && !reducedMotion) {
      sidebarTextHidden = true;
      await new Promise<void>((resolve) => window.setTimeout(resolve, 110));
    }
    const movingNodes = sidebarElement
      ? [...sidebarElement.querySelectorAll<HTMLElement>(".brand-mark, .sidebar-toggle, .header-control-icon, .header-utilities button, .session-launcher > button, .session-icon")]
        .filter((node) => node.getClientRects().length && node.getBoundingClientRect().bottom > 0)
        .slice(0, 32)
      : [];
    const originalRects = new Map(movingNodes.map((node) => [node, node.getBoundingClientRect()]));
    for (const node of movingNodes) sidebarMotion.get(node)?.cancel();
    sidebarCollapsed = !sidebarCollapsed;
    if (sidebarCollapsed) {
      closeAgentSearch();
      headerControl = null;
      namingLayout = false;
      launcherOpen = false;
      sessionContextMenu = null;
      filter = "all";
    }
    await tick();
    if (reducedMotion) {
      sidebarTextHidden = false;
      sidebarToggleBusy = false;
      return;
    }
    for (const node of movingNodes) {
      if (!node.isConnected || !node.getClientRects().length) continue;
      const before = originalRects.get(node);
      if (!before) continue;
      const after = node.getBoundingClientRect();
      const x = before.left - after.left;
      const y = before.top - after.top;
      if (Math.abs(x) < 2 && Math.abs(y) < 2) continue;
      const animation = node.animate(
        [{ transform: `translate(${x}px, ${y}px)` }, { transform: "translate(0, 0)" }],
        { duration: sidebarCollapsed ? 220 : 260, easing: "cubic-bezier(.16, 1, .3, 1)" },
      );
      sidebarMotion.set(node, animation);
    }
    if (!sidebarCollapsed) {
      await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    }
    sidebarTextHidden = false;
    sidebarToggleBusy = false;
  }

  function openHeaderControl(control: "project" | "layout") {
    sidebarCollapsed = false;
    headerControl = control;
  }
  let launchError = $state("");
  let pendingOpenedSession: { nativeId?: string; agent: string; knownIds: Set<string>; startedAt: number; replacePaneId?: string } | null = null;
  let namedLayoutRestored = false;
  let workbenchElement = $state<HTMLElement | null>(null);
  let workspaceDropGeometry: WorkspaceDropGeometry | null = null;

  const workspaceLayoutKey = "lume:workspace-layout:v1";
  const workspaceNamedLayoutsKey = "lume:workspace-named-layouts:v1";
  const workspaceStreamMessagesKey = "lume:workspace-stream-messages:v1";
  const workspaceBackgroundImageKey = "lume:workspace-background-image:v1";
  const workspaceBackgroundImageOpacityKey = "lume:workspace-background-image-opacity:v1";
  const agentMessageSurfaceKey = "lume:workspace-agent-message-surface:v1";

  function setStreamMessages(enabled: boolean) {
    streamMessages = enabled;
    try { localStorage.setItem(workspaceStreamMessagesKey, String(enabled)); }
    catch { /* The preference remains active for this window. */ }
  }

  function loadBackgroundImage(file: File) {
    if (!file.type.startsWith("image/") || file.size > 24 * 1024 * 1024) {
      settingsError = tr("Choose an image smaller than 24 MB.", "Escolha uma imagem menor que 24 MB.");
      return;
    }
    settingsError = "";
    const source = URL.createObjectURL(file);
    const image = new Image();
    image.onload = () => {
      try {
        const maximum = 2200;
        const scale = Math.min(1, maximum / Math.max(image.naturalWidth, image.naturalHeight));
        const canvas = document.createElement("canvas");
        canvas.width = Math.max(1, Math.round(image.naturalWidth * scale));
        canvas.height = Math.max(1, Math.round(image.naturalHeight * scale));
        const context = canvas.getContext("2d");
        if (!context) throw new Error("canvas");
        context.drawImage(image, 0, 0, canvas.width, canvas.height);
        let dataUrl = canvas.toDataURL("image/webp", .84);
        if (dataUrl.length > 3_600_000) {
          const compactScale = Math.min(1, 1600 / Math.max(canvas.width, canvas.height));
          const compact = document.createElement("canvas");
          compact.width = Math.max(1, Math.round(canvas.width * compactScale));
          compact.height = Math.max(1, Math.round(canvas.height * compactScale));
          compact.getContext("2d")?.drawImage(canvas, 0, 0, compact.width, compact.height);
          dataUrl = compact.toDataURL("image/webp", .76);
        }
        if (dataUrl.length > 3_600_000) throw new Error("size");
        localStorage.setItem(workspaceBackgroundImageKey, dataUrl);
        workspaceBackgroundImage = dataUrl;
        // A new image starts with readable agent messages; the user can turn it off.
        setAgentMessageSurface(true);
        settingsMessage = tr("Workspace background updated.", "Fundo do Workspace atualizado.");
      } catch {
        settingsError = tr("This image could not be saved as a background.", "Não foi possível salvar esta imagem como fundo.");
      } finally {
        URL.revokeObjectURL(source);
        if (workspaceBackgroundInput) workspaceBackgroundInput.value = "";
      }
    };
    image.onerror = () => {
      URL.revokeObjectURL(source);
      settingsError = tr("This image could not be read.", "Não foi possível ler esta imagem.");
      if (workspaceBackgroundInput) workspaceBackgroundInput.value = "";
    };
    image.src = source;
  }

  function removeBackgroundImage() {
    workspaceBackgroundImage = "";
    try { localStorage.removeItem(workspaceBackgroundImageKey); }
    catch { /* The background still resets for this window. */ }
    settingsMessage = tr("Custom background removed.", "Fundo personalizado removido.");
  }

  function setAgentMessageSurface(enabled: boolean) {
    agentMessageSurface = enabled;
    try { localStorage.setItem(agentMessageSurfaceKey, String(enabled)); }
    catch { /* Keep the choice for this window. */ }
  }

  function setBackgroundImageOpacity(value: number) {
    workspaceBackgroundImageOpacity = Math.max(0, Math.min(100, Math.round(value)));
    try { localStorage.setItem(workspaceBackgroundImageOpacityKey, String(workspaceBackgroundImageOpacity)); }
    catch { /* Keep the selected opacity for this window. */ }
  }

  function floatLauncher(node: HTMLDivElement) {
    const anchorElement = node.parentElement as HTMLElement;
    launcherPopoverNode = node;
    document.body.appendChild(node);
    const position = () => {
      const anchor = anchorElement.getBoundingClientRect();
      const width = Math.min(248, window.innerWidth - 16);
      const availableBelow = window.innerHeight - anchor.bottom - 12;
      const availableAbove = anchor.top - 12;
      const above = availableBelow < 220 && availableAbove > availableBelow;
      const maxHeight = Math.max(100, Math.min(420, above ? availableAbove : availableBelow));
      node.dataset.placement = above ? "above" : "below";
      node.style.width = `${width}px`;
      node.style.left = `${Math.max(8, Math.min(window.innerWidth - width - 8, anchor.right - width))}px`;
      node.style.maxHeight = `${maxHeight}px`;
      node.style.top = above ? "auto" : `${anchor.bottom + 5}px`;
      node.style.bottom = above ? `${window.innerHeight - anchor.top + 5}px` : "auto";
      const root = anchorElement.closest<HTMLElement>(".workspace");
      if (root) {
        const styles = getComputedStyle(root);
        copyResolvedColorTokens(root, node, ["raised", "text", "strong", "muted", "line", "subtle", "accent", "accent-soft"].map((token) => ({ source: `--workspace-${token}` })));
        node.style.fontFamily = styles.fontFamily;
      }
    };
    position();
    window.addEventListener("resize", position);
    window.addEventListener("scroll", position, true);
    return {
      destroy() {
        window.removeEventListener("resize", position);
        window.removeEventListener("scroll", position, true);
        if (launcherPopoverNode === node) launcherPopoverNode = null;
        node.remove();
      },
    };
  }
  const workspaceResizeEdges = [
    "North",
    "NorthEast",
    "East",
    "SouthEast",
    "South",
    "SouthWest",
    "West",
    "NorthWest",
  ] as const;

  const orderedSessions = $derived.by(() => {
    const paneOrder = [primaryId, secondaryId, tertiaryId];
    const priority: Record<HubSession["status"], number> = {
      permission_required: 0,
      running: 1,
      completed: 2,
      failed: 3,
      waiting_for_input: 4,
    };
    return sessions.filter((session) => !isUnidentifiedCodexCli(session)).sort((left, right) => {
      const leftPane = paneOrder.indexOf(left.id);
      const rightPane = paneOrder.indexOf(right.id);
      if (leftPane !== -1 || rightPane !== -1) {
        if (leftPane === -1) return 1;
        if (rightPane === -1) return -1;
        return leftPane - rightPane;
      }
      return priority[left.status] - priority[right.status] || right.updatedAt - left.updatedAt;
    });
  });
  const workspaceProjects = $derived.by(() => {
    const projects = new Map<string, string>();
    for (const session of orderedSessions) {
      const key = projectKey(session.workingDirectory ?? session.project);
      if (key) projects.set(key, session.project || sessionName(session));
    }
    return Array.from(projects, ([value, label]) => ({ value, label })).sort((left, right) => left.label.localeCompare(right.label));
  });
  const projectSessions = $derived(projectFilter === "all" ? orderedSessions : orderedSessions.filter((session) => projectKey(session.workingDirectory ?? session.project) === projectFilter));
  const subagentsBySession = $derived.by(() => {
    // Expanding a parent records a recent interaction, which can keep completed
    // children visible even when the session snapshot itself did not change.
    void subagentRevision;
    return new Map(sessions.map((session) => [session.id, subagentsForSession(session)]));
  });
  const filteredSessions = $derived.by(() => {
    const needle = query.trim().toLocaleLowerCase();
    return projectSessions.filter((session) => {
      const matchesFilter = filter === "all"
        || (filter === "active" && session.status === "running")
        || (filter === "attention" && ["permission_required", "failed"].includes(session.status));
      if (!matchesFilter) return false;
      if (!needle) return true;
      return [
        sessionName(session),
        session.project,
        session.agentLabel,
        session.workingDirectory ?? "",
        ...(subagentsBySession.get(session.id) ?? []).map((agent) => agent.label),
      ].some((value) => value.toLocaleLowerCase().includes(needle));
    });
  });
  // Agent groups: folders in the sidebar. Kept on this computer; a session belongs to one group.
  type SidebarGroup = { id: string; name: string; collapsed: boolean };
  type SidebarRow =
    | { key: string; kind: "header"; group: SidebarGroup | null; count: number }
    | { key: string; kind: "session"; session: HubSession; index: number };
  const sidebarGroupsKey = "lume:sidebar-groups:v1";
  let sidebarGroups = $state<SidebarGroup[]>([]);
  let groupAssignments = $state<Record<string, string>>({});
  let groupDropTarget = $state<string | null>(null);
  // Sections (the groups and the "No group" list) can be dragged into any order.
  let sectionOrder = $state<string[]>([]);
  let draggingSectionId = $state<string | null>(null);
  let reorderTarget = $state<{ id: string; after: boolean } | null>(null);
  const orderedSections = $derived.by(() => {
    const valid = new Set(["none", ...sidebarGroups.map((group) => group.id)]);
    const order = sectionOrder.filter((id) => valid.has(id));
    const missing = sidebarGroups.map((group) => group.id).filter((id) => !order.includes(id));
    const next = [...order, ...missing];
    if (!next.includes("none")) next.unshift("none");
    return next;
  });
  let renamingGroupId = $state<string | null>(null);
  let groupNameDraft = $state("");
  let confirmingGroupId = $state<string | null>(null);
  let confirmGroupTimer: ReturnType<typeof setTimeout> | undefined;
  const sessionGroupKey = (session: HubSession) => session.nativeSessionId || session.id;

  function loadSidebarGroups() {
    try {
      const raw = JSON.parse(localStorage.getItem(sidebarGroupsKey) || "{}");
      sidebarGroups = Array.isArray(raw.groups)
        ? raw.groups.filter((group: SidebarGroup) => typeof group?.id === "string" && typeof group?.name === "string").map((group: SidebarGroup) => ({ id: group.id, name: group.name, collapsed: Boolean(group.collapsed) }))
        : [];
      groupAssignments = raw.assignments && typeof raw.assignments === "object" ? raw.assignments : {};
      sectionOrder = Array.isArray(raw.order) ? raw.order.filter((id: unknown) => typeof id === "string") : [];
    } catch { /* Start without groups when the saved ones cannot be read. */ }
  }
  function saveSidebarGroups() {
    try { localStorage.setItem(sidebarGroupsKey, JSON.stringify({ groups: sidebarGroups, assignments: groupAssignments, order: sectionOrder })); }
    catch { /* Groups still work for this run when storage is unavailable. */ }
  }
  function newSidebarGroup(): SidebarGroup {
    const group = { id: crypto.randomUUID(), name: tr(`Group ${sidebarGroups.length + 1}`, `Grupo ${sidebarGroups.length + 1}`), collapsed: false };
    sidebarGroups = [...sidebarGroups, group];
    saveSidebarGroups();
    return group;
  }
  function createSidebarGroup() {
    const group = newSidebarGroup();
    renamingGroupId = group.id;
    groupNameDraft = group.name;
  }
  function assignToGroup(session: HubSession, groupId: string | null) {
    const next = { ...groupAssignments };
    if (groupId) next[sessionGroupKey(session)] = groupId;
    else delete next[sessionGroupKey(session)];
    groupAssignments = next;
    saveSidebarGroups();
  }
  function commitGroupRename() {
    const id = renamingGroupId;
    renamingGroupId = null;
    const name = groupNameDraft.trim().slice(0, 40);
    if (!id || !name) return;
    sidebarGroups = sidebarGroups.map((group) => group.id === id ? { ...group, name } : group);
    saveSidebarGroups();
  }
  function toggleSidebarGroup(id: string) {
    sidebarGroups = sidebarGroups.map((group) => group.id === id ? { ...group, collapsed: !group.collapsed } : group);
    saveSidebarGroups();
  }
  function requestDeleteGroup(id: string) {
    if (confirmingGroupId !== id) {
      confirmingGroupId = id;
      clearTimeout(confirmGroupTimer);
      confirmGroupTimer = setTimeout(() => { confirmingGroupId = null; }, 3500);
      return;
    }
    confirmingGroupId = null;
    sidebarGroups = sidebarGroups.filter((group) => group.id !== id);
    groupAssignments = Object.fromEntries(Object.entries(groupAssignments).filter(([, value]) => value !== id));
    saveSidebarGroups();
  }
  function dragOverGroup(event: DragEvent, id: string) {
    if (draggingSectionId) {
      if (draggingSectionId === id) return;
      event.preventDefault();
      event.stopPropagation();
      if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
      const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
      const after = event.clientY > rect.top + rect.height / 2;
      if (reorderTarget?.id !== id || reorderTarget.after !== after) reorderTarget = { id, after };
      return;
    }
    if (!draggingSessionId) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
    groupDropTarget = id;
  }
  function dropOnGroup(event: DragEvent, id: string | null) {
    event.preventDefault();
    event.stopPropagation();
    if (draggingSectionId) {
      const target = reorderTarget;
      const moving = draggingSectionId;
      draggingSectionId = null;
      reorderTarget = null;
      if (target && target.id !== moving) {
        const order = orderedSections.filter((item) => item !== moving);
        const at = order.indexOf(target.id) + (target.after ? 1 : 0);
        order.splice(at, 0, moving);
        sectionOrder = order;
        saveSidebarGroups();
      }
      return;
    }
    const session = sessions.find((item) => item.id === draggingSessionId);
    groupDropTarget = null;
    if (session) assignToGroup(session, id);
    finishSidebarSessionDrag();
  }
  function beginSectionDrag(event: DragEvent, id: string) {
    if ((event.target as HTMLElement | null)?.closest("input, .group-actions")) { event.preventDefault(); return; }
    draggingSectionId = id;
    if (event.dataTransfer) {
      event.dataTransfer.effectAllowed = "move";
      event.dataTransfer.setData("text/x-lume-section", id);
    }
  }
  function endSectionDrag() { draggingSectionId = null; reorderTarget = null; groupDropTarget = null; }
  function focusOnMount(node: HTMLInputElement) { node.focus(); node.select(); }

  const sidebarLayout = $derived.by(() => {
    const filtering = Boolean(query.trim()) || filter !== "all";
    const rows: SidebarRow[] = [];
    const visible: HubSession[] = [];
    const push = (session: HubSession) => { rows.push({ key: session.id, kind: "session", session, index: visible.length }); visible.push(session); };
    if (sidebarCollapsed || !sidebarGroups.length) { filteredSessions.forEach(push); return { rows, visible }; }
    const members = new Map<string, HubSession[]>(sidebarGroups.map((group) => [group.id, []]));
    const loose: HubSession[] = [];
    for (const session of filteredSessions) {
      const list = members.get(groupAssignments[sessionGroupKey(session)]);
      if (list) list.push(session); else loose.push(session);
    }
    for (const id of orderedSections) {
      if (id === "none") {
        if (loose.length || (!filtering && (draggingSessionId || draggingSectionId))) {
          rows.push({ key: "group:none", kind: "header", group: null, count: loose.length });
          loose.forEach(push);
        }
        continue;
      }
      const group = sidebarGroups.find((item) => item.id === id);
      const list = members.get(id) ?? [];
      if (!group || (filtering && !list.length)) continue;
      rows.push({ key: `group:${id}`, kind: "header", group, count: list.length });
      if (!group.collapsed || filtering) list.forEach(push);
    }
    return { rows, visible };
  });
  onMount(loadSidebarGroups);

  const filteredInternalServices = $derived(
    projectFilter === "all" && filter !== "attention"
      ? internalServices.filter((service) =>
          !query.trim() || `${service.label} ${service.agent}`.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase())
        )
      : []
  );

  $effect(() => {
    const currentSessions = sessions;
    const liveSessionIds = new Set(currentSessions.map((session) => session.id));
    const newlyActiveParents: string[] = [];
    for (const session of currentSessions) {
      const children = subagentsBySession.get(session.id) ?? [];
      const observed = observedSubagentIds.get(session.id) ?? new Set<string>();
      if (children.some((child) => !observed.has(child.id) && (child.status === "running" || Date.now() - child.startedAt < 2_500))) {
        newlyActiveParents.push(session.id);
      }
      observedSubagentIds.set(session.id, new Set(children.map((child) => child.id)));
    }
    for (const sessionId of observedSubagentIds.keys()) {
      if (!liveSessionIds.has(sessionId)) observedSubagentIds.delete(sessionId);
    }
    if (newlyActiveParents.length) {
      expandedSubagentSessions = new Set([...expandedSubagentSessions, ...newlyActiveParents]);
    }
  });
  const primary = $derived(sessions.find((session) => session.id === primaryId) ?? null);
  const secondary = $derived(sessions.find((session) => session.id === secondaryId) ?? null);
  const tertiary = $derived(sessions.find((session) => session.id === tertiaryId) ?? null);
  const focusedSession = $derived(
    sessions.find((session) => session.id === focusedPaneId)
      ?? primary
  );
  const contextSession = $derived.by(() => {
    const sessionId = sessionContextMenu?.sessionId;
    return sessionId ? sessions.find((session) => session.id === sessionId) ?? null : null;
  });
  const maximizedSession = $derived(
    sessions.find((session) => session.id === maximizedPaneId)
      ?? null
  );
  // While the review is open only the chat it was opened from keeps its column (the others stay mounted at
  // zero width, so drafts and streams survive), which leaves the room to the diff.
  const reviewFocusSession = $derived(reviewOpen && !boardOpen && !maximizedSession && secondary ? focusedSession : null);
  const darkMode = $derived(preferences.darkMode ?? systemDark);
  const appearanceMode = $derived<"system" | "light" | "dark">(
    preferences.darkMode === undefined ? "system" : preferences.darkMode ? "dark" : "light"
  );
  const appearance = $derived(appearanceAttributes(preferences));
  $effect(() => { void ensureCustomFonts(preferences); });
  const selectedAppearanceTheme = $derived(
    appearanceThemes.find((theme) => theme.value === appearance.theme) ?? appearanceThemes[0]
  );
  const workspaceCanvasColor = $derived(
    darkMode
      ? preferences.workspaceDarkBackgroundColor ?? preferences.workspaceBackgroundColor ?? selectedAppearanceTheme.darkSurface
      : preferences.workspaceLightBackgroundColor ?? selectedAppearanceTheme.lightSurface
  );
  const workspaceCanvasOpacity = $derived(
    darkMode
      ? preferences.workspaceDarkBackgroundColor
        ? preferences.workspaceDarkBackgroundOpacity
        : preferences.workspaceBackgroundColor
          ? preferences.workspaceBackgroundOpacity
          : preferences.workspaceDarkBackgroundOpacity
      : preferences.workspaceLightBackgroundOpacity
  );
  const detectedProjects = $derived.by(() => {
    const projects = new Map<string, string>();
    for (const [key, profile] of Object.entries(preferences.projectProfiles)) {
      if (profile.label) projects.set(key, profile.label);
    }
    for (const session of sessions) {
      const raw = session.workingDirectory ?? session.project;
      const key = raw.trim().replaceAll("\\", "/").replace(/\/+$/, "").toLocaleLowerCase();
      if (key) projects.set(key, session.project || sessionName(session));
    }
    return Array.from(projects, ([key, label]) => ({ key, label })).sort((left, right) => left.label.localeCompare(right.label));
  });
  const selectedProjectProfile = $derived(selectedProfileKey ? preferences.projectProfiles[selectedProfileKey] : undefined);
  const selectedNamedLayout = $derived(namedLayouts.find((layout) => layout.id === selectedNamedLayoutId) ?? null);
  const selectedLayoutDirty = $derived.by(() => selectedNamedLayout ? !layoutMatchesCurrent(selectedNamedLayout) : false);

  $effect(() => {
    if (typeof document !== "undefined") {
      const root = document.documentElement;
      root.dataset.theme = darkMode ? "dark" : "light";
      root.dataset.appearance = appearance.theme;
      if (appearance.accentCss) {
        root.style.setProperty("--lume-accent", appearance.accentCss);
        root.style.setProperty("--lume-accent-strong", appearance.accentCss);
      } else {
        root.style.removeProperty("--lume-accent");
        root.style.removeProperty("--lume-accent-strong");
      }
    }
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

  const systemBanners = $derived.by<SystemBannerItem[]>(() => {
    const items: SystemBannerItem[] = [];
    if (settingsError) items.push({ id: "settings-error", message: settingsError, tone: "error", onDismiss: () => { settingsError = ""; } });
    if (shortcutRegistrationError) items.push({
      id: "shortcut-registration-error",
      message: `${tr("Global shortcuts could not be registered. Review them in Settings.", "Não foi possível registrar os atalhos globais. Revise-os nas Configurações.")} ${shortcutRegistrationError}`,
      tone: "warning",
      onDismiss: () => { shortcutRegistrationError = null; },
    });
    if (launchError) items.push({ id: "launch-error", message: launchError, tone: "error", onDismiss: () => { launchError = ""; } });
    if (sessionContextError) items.push({ id: "session-error", message: sessionContextError, tone: "error", onDismiss: () => { sessionContextError = ""; } });
    const openedSessionIds = new Set([primary?.id, secondary?.id, tertiary?.id].filter(Boolean));
    for (const alert of collectAgentAlerts(orderedSessions.filter((session) => !openedSessionIds.has(session.id)), language, Date.now(), { usageScope: "active" })) {
      if (dismissedAgentAlertIds.includes(alert.id) || $usageAlertDismissals.includes(alert.id)) continue;
      items.push({
        id: alert.id,
        message: alert.message,
        tone: alert.tone,
        duration: alert.duration,
        onDismiss: () => dismissAgentAlert(alert.id),
      });
    }
    if (settingsMessage) items.push({ id: "settings-message", message: settingsMessage, tone: "success", onDismiss: () => { settingsMessage = ""; } });
    if (layoutMessage) items.push({ id: "layout-message", message: layoutMessage, onDismiss: () => { layoutMessage = ""; } });
    return items;
  });

  function dismissAgentAlert(id: string) {
    usageAlertDismissals.dismiss(id);
    if (dismissedAgentAlertIds.includes(id)) return;
    dismissedAgentAlertIds = [...dismissedAgentAlertIds, id].slice(-200);
  }

  function sessionName(session: HubSession) {
    return session.sessionName?.trim() || session.project?.trim() || session.agentLabel;
  }

  function projectKey(value: string) {
    return value.trim().replaceAll("\\", "/").replace(/\/+$/, "").toLocaleLowerCase();
  }

  function sessionSubtitle(session: HubSession) {
    const directory = session.workingDirectory?.trim().replace(/[\\/]+$/, "");
    return directory?.split(/[\\/]/).pop() || session.project || session.agentLabel;
  }

  function sessionLayoutKey(session: HubSession) {
    return session.nativeSessionId
      ? `native:${session.agent}:${session.nativeSessionId}`
      : `session:${session.id}`;
  }

  function currentPaneKeys() {
    return [primary, secondary, tertiary]
      .filter((session): session is HubSession => Boolean(session))
      .map(sessionLayoutKey);
  }

  function layoutMatchesCurrent(layout: WorkspaceNamedLayout) {
    const currentKeys = currentPaneKeys();
    return currentKeys.length === layout.paneKeys.length
      && currentKeys.every((key, index) => key === layout.paneKeys[index])
      && (currentKeys.length < 2 || Math.abs(splitRatio - layout.splitRatio) < 0.002)
      && (currentKeys.length < 3 || Math.abs(tertiaryRatio - layout.tertiaryRatio) < 0.002)
      && projectFilter === layout.projectFilter
      && inspectorOpen === layout.inspectorOpen;
  }

  function layoutSnapshot(id: string, name: string): WorkspaceNamedLayout {
    return {
      id,
      name,
      paneKeys: currentPaneKeys(),
      splitRatio,
      tertiaryRatio,
      projectFilter,
      inspectorOpen,
      updatedAt: Date.now(),
    };
  }

  function persistNamedLayouts() {
    if (typeof localStorage === "undefined") return;
    localStorage.setItem(workspaceNamedLayoutsKey, JSON.stringify(namedLayouts));
  }

  function restoreNamedLayouts() {
    try {
      const parsed: unknown = JSON.parse(localStorage.getItem(workspaceNamedLayoutsKey) ?? "[]");
      if (!Array.isArray(parsed)) return;
      namedLayouts = parsed.filter((candidate): candidate is WorkspaceNamedLayout => {
        if (!candidate || typeof candidate !== "object") return false;
        const value = candidate as Partial<WorkspaceNamedLayout>;
        return typeof value.id === "string"
          && typeof value.name === "string"
          && Array.isArray(value.paneKeys)
          && value.paneKeys.length > 0
          && value.paneKeys.length <= 3
          && value.paneKeys.every((key) => typeof key === "string")
          && typeof value.splitRatio === "number"
          && typeof value.tertiaryRatio === "number"
          && typeof value.projectFilter === "string"
          && typeof value.inspectorOpen === "boolean"
          && typeof value.updatedAt === "number";
      }).slice(0, 20);
    } catch {
      namedLayouts = [];
    }
  }

  async function beginNamingLayout() {
    if (!primary) return;
    namingLayout = true;
    layoutName = `${tr("Layout", "Layout")} ${namedLayouts.length + 1}`;
    layoutMessage = "";
    await tick();
    layoutNameInput?.focus();
    layoutNameInput?.select();
  }

  function saveNamedLayout() {
    const name = layoutName.trim();
    if (!primary || !name) return;
    if (namedLayouts.some((layout) => layout.name.toLocaleLowerCase() === name.toLocaleLowerCase())) {
      layoutMessage = tr("A layout with this name already exists.", "Já existe um layout com este nome.");
      return;
    }
    const id = typeof crypto.randomUUID === "function" ? crypto.randomUUID() : `layout-${Date.now()}`;
    const layout = layoutSnapshot(id, name);
    namedLayouts = [layout, ...namedLayouts].slice(0, 20);
    selectedNamedLayoutId = id;
    namingLayout = false;
    layoutName = "";
    layoutMessage = tr("Layout saved.", "Layout salvo.");
    persistNamedLayouts();
    persistWorkspaceLayout();
  }

  function updateNamedLayout() {
    if (!selectedNamedLayout) return;
    const updated = layoutSnapshot(selectedNamedLayout.id, selectedNamedLayout.name);
    namedLayouts = namedLayouts.map((layout) => layout.id === updated.id ? updated : layout);
    layoutMessage = tr("Layout updated.", "Layout atualizado.");
    persistNamedLayouts();
  }

  function deleteNamedLayout() {
    if (!selectedNamedLayout) return;
    namedLayouts = namedLayouts.filter((layout) => layout.id !== selectedNamedLayout.id);
    selectedNamedLayoutId = "";
    layoutMessage = tr("Layout removed.", "Layout removido.");
    persistNamedLayouts();
    persistWorkspaceLayout();
  }

  function applyNamedLayout(layoutId: string) {
    selectedNamedLayoutId = layoutId;
    layoutMessage = "";
    if (!layoutId) {
      persistWorkspaceLayout();
      return;
    }
    const layout = namedLayouts.find((candidate) => candidate.id === layoutId);
    if (!layout) return;
    const resolved: HubSession[] = [];
    for (const key of layout.paneKeys) {
      const session = sessions.find((candidate) => sessionLayoutKey(candidate) === key);
      if (session && !resolved.some((candidate) => candidate.id === session.id)) resolved.push(session);
    }
    if (!resolved.length) {
      selectedNamedLayoutId = "";
      layoutMessage = tr("None of this layout's sessions are currently open.", "Nenhuma sessão deste layout está aberta.");
      return;
    }
    const availableProject = layout.projectFilter === "all"
      || workspaceProjects.some((project) => project.value === layout.projectFilter);
    projectFilter = availableProject ? layout.projectFilter : "all";
    [primaryId, secondaryId, tertiaryId] = [
      resolved[0]?.id ?? null,
      resolved[1]?.id ?? null,
      resolved[2]?.id ?? null,
    ];
    focusedPaneId = primaryId;
    maximizedPaneId = null;
    splitRatio = Math.min(0.75, Math.max(0.25, layout.splitRatio));
    tertiaryRatio = Math.min(0.5, Math.max(0.22, layout.tertiaryRatio));
    inspectorOpen = layout.inspectorOpen;
    if (resolved.length !== layout.paneKeys.length) {
      layoutMessage = tr(
        `${layout.paneKeys.length - resolved.length} unavailable session omitted.`,
        `${layout.paneKeys.length - resolved.length} sessão indisponível foi omitida.`,
      );
    }
    persistWorkspaceLayout();
  }

  function statusLabel(session: HubSession) {
    return displayText(language, session.statusLabel);
  }

  function toggleSubagents(session: HubSession) {
    const next = new Set(expandedSubagentSessions);
    if (next.has(session.id)) next.delete(session.id);
    else {
      next.add(session.id);
      for (const child of subagentsBySession.get(session.id) ?? []) noteSubagentInteraction(child.id);
      subagentRevision += 1;
    }
    expandedSubagentSessions = next;
  }

  function activateSidebarSession(session: HubSession, hasSubagents: boolean) {
    if (focusedPaneId === session.id) {
      if (hasSubagents) toggleSubagents(session);
      return;
    }
    selectSession(session);
  }

  function selectSession(session: HubSession) {
    if (![primaryId, secondaryId, tertiaryId].includes(session.id)) {
      if (focusedPaneId === secondaryId) secondaryId = session.id;
      else if (focusedPaneId === tertiaryId) tertiaryId = session.id;
      else primaryId = session.id;
    }
    focusedPaneId = session.id;
    if (maximizedPaneId) maximizedPaneId = session.id;
    persistWorkspaceLayout();
  }

  async function toggleLauncher() {
    launcherOpen = !launcherOpen;
    launchError = "";
    if (!launcherOpen) {
      resumeAgent = null;
      resumableSessions = [];
    } else if (!integrations.length) {
      try { integrations = await loadIntegrationStatuses(); }
      catch (reason) { launchError = String(reason).replace(/^Error:\s*/, ""); }
    }
  }

  async function startSession(agent: IntegrationStatus["kind"]) {
    launching = agent;
    launchingSessionId = null;
    launchingPhase = "choosing";
    launchError = "";
    try {
      const selected = await withNativeDialog(() => openDialog({ directory: true, multiple: false, title: tr("Project for the new session", "Projeto da nova sessão") }));
      if (!selected || Array.isArray(selected)) return;

      launchingPhase = "opening";
      const profile = preferences.projectProfiles[projectKey(selected)];
      pendingOpenedSession = { agent: agent === "claude" ? "claude_code" : agent, knownIds: new Set(sessions.map((session) => session.id)), startedAt: Date.now() };
      await launchAgentSession(agent, selected, false, undefined, profile?.launchTarget ?? preferences.launchTarget, profile?.permissionMode, profile?.approvalPolicy);
      launcherOpen = false;
    } catch (reason) {
      pendingOpenedSession = null;
      const connection = agentConnectionMessage(reason);
      if (connection) { connectionAgent = agent; connectionMessage = connection; }
      else if (macosAutomationMessage(reason)) automationRequired = true;
      else launchError = String(reason).replace(/^Error:\s*/, "");
    } finally {
      launching = null;
      launchingSessionId = null;
      launchingPhase = null;
    }
  }

  async function toggleResumeSessions(agent: IntegrationStatus["kind"]) {
    if (resumeAgent === agent) {
      resumeAgent = null;
      resumableSessions = [];
      return;
    }
    resumeAgent = agent;
    resumableSessions = [];
    loadingResumeAgent = agent;
    launchError = "";
    try { resumableSessions = await loadResumableSessions(agent); }
    catch (reason) { launchError = String(reason).replace(/^Error:\s*/, ""); }
    finally { loadingResumeAgent = null; }
  }

  async function resumeStoredSession(stored: ResumableSession) {
    const liveSession = resolveLiveResumableSession(stored, sessions);
    if (liveSession) {
      projectFilter = "all";
      if (!hasOpenWorkspacePane(sessions, [primaryId, secondaryId, tertiaryId])) {
        maximizedPaneId = null;
        selectSession(liveSession);
      }
      launcherOpen = false;
      return;
    }
    launching = stored.agent;
    launchingSessionId = stored.id;
    launchingPhase = "opening";
    launchError = "";
    try {
      const profile = preferences.projectProfiles[projectKey(stored.workingDirectory)];
      pendingOpenedSession = { nativeId: stored.id, agent: stored.agent === "claude" ? "claude_code" : stored.agent, knownIds: new Set(sessions.map((session) => session.id)), startedAt: Date.now() };
      await launchAgentSession(stored.agent, stored.workingDirectory, true, stored.id, profile?.launchTarget ?? preferences.launchTarget);
      launcherOpen = false;
      resumeAgent = null;
      resumableSessions = [];
    } catch (reason) {
      pendingOpenedSession = null;
      const connection = agentConnectionMessage(reason);
      if (connection) { connectionAgent = stored.agent; connectionMessage = connection; }
      else if (macosAutomationMessage(reason)) automationRequired = true;
      else launchError = String(reason).replace(/^Error:\s*/, "");
    } finally {
      launching = null;
      launchingSessionId = null;
      launchingPhase = null;
    }
  }

  function flagAutomationError(error: unknown): unknown {
    const automation = macosAutomationMessage(error);
    if (!automation) return error;
    automationRequired = true;
    return new Error(automation);
  }

  /** `/clear`: a fresh conversation of the same agent and project takes the place of this pane. */
  async function startNewConversation(source: HubSession) {
    const workingDirectory = source.workingDirectory?.trim();
    if (!workingDirectory) throw new Error(tr("This session has no project folder to start from.", "Esta sessão não tem uma pasta de projeto para começar."));
    const agent = (source.agent === "claude_code" ? "claude" : source.agent) as IntegrationStatus["kind"];
    const profile = preferences.projectProfiles[projectKey(workingDirectory)];
    pendingOpenedSession = {
      agent: source.agent,
      knownIds: new Set(sessions.map((session) => session.id)),
      startedAt: Date.now(),
      replacePaneId: source.id,
    };
    try {
      await launchAgentSession(agent, workingDirectory, false, undefined, profile?.launchTarget ?? preferences.launchTarget, profile?.permissionMode, profile?.approvalPolicy);
    } catch (error) {
      pendingOpenedSession = null;
      throw flagAutomationError(error);
    }
  }

  async function openForkedCodexSession(threadId: string, source: HubSession) {
    const workingDirectory = source.workingDirectory?.trim() || ".";
    const profile = preferences.projectProfiles[projectKey(workingDirectory)];
    pendingOpenedSession = {
      nativeId: threadId,
      agent: "codex",
      knownIds: new Set(sessions.map((session) => session.id)),
      startedAt: Date.now(),
    };
    try {
      await launchAgentSession(
        "codex",
        workingDirectory,
        true,
        threadId,
        profile?.launchTarget ?? preferences.launchTarget,
        profile?.permissionMode,
        profile?.approvalPolicy,
      );
    } catch (error) {
      pendingOpenedSession = null;
      throw flagAutomationError(error);
    }
  }

  function dismissExternalWriterConflict(conflict: ExternalWriterConflict) {
    if (externalWriterConflicts[conflict.sessionId]?.processId !== conflict.processId) return;
    const next = { ...externalWriterConflicts };
    delete next[conflict.sessionId];
    externalWriterConflicts = next;
  }

  async function resolveExternalWriterConflict(
    session: HubSession,
    conflict: ExternalWriterConflict,
    action: "keep_lume" | "open_branch",
  ) {
    await cancelExternalWriterAttempt(conflict);
    dismissExternalWriterConflict(conflict);
    if (action === "keep_lume") {
      await refreshSessionsAfterContextAction();
      return;
    }

    const threadId = await forkCodexThread(session.id);
    const workingDirectory = session.workingDirectory?.trim() || ".";
    const profile = preferences.projectProfiles[projectKey(workingDirectory)];
    pendingOpenedSession = {
      nativeId: threadId,
      agent: "codex",
      knownIds: new Set(sessions.map((item) => item.id)),
      startedAt: Date.now(),
    };
    try {
      await launchAgentSession(
        "codex",
        workingDirectory,
        true,
        threadId,
        "terminal",
        profile?.permissionMode,
        profile?.approvalPolicy,
      );
    } catch (reason) {
      pendingOpenedSession = null;
      throw flagAutomationError(reason);
    }
  }

  function closeSidePane(sessionId: string) {
    if (secondaryId === sessionId) {
      secondaryId = tertiaryId;
      tertiaryId = null;
    } else if (tertiaryId === sessionId) {
      tertiaryId = null;
    }
    if (focusedPaneId === sessionId) focusedPaneId = primaryId;
    if (maximizedPaneId === sessionId) maximizedPaneId = null;
    persistWorkspaceLayout();
  }

  function focusPane(sessionId: string) {
    if (focusedPaneId === sessionId) return;
    focusedPaneId = sessionId;
    persistWorkspaceLayout();
  }

  function currentPaneIds() {
    return [primaryId, secondaryId, tertiaryId].filter((id): id is string => Boolean(id));
  }

  function beginSidebarSessionDrag(event: DragEvent, sessionId: string) {
    draggingSessionId = sessionId;
    workspaceDropIntent = null;
    workspaceDropGeometry = null;
    if (!boardOpen) maximizedPaneId = null;
    if (event.dataTransfer) {
      event.dataTransfer.effectAllowed = "move";
      event.dataTransfer.setData("text/x-lume-session", sessionId);
      event.dataTransfer.setData("text/plain", sessionId);
      const image = document.createElement("canvas");
      image.width = image.height = 1;
      image.style.cssText = "position:fixed;top:0;left:0;pointer-events:none;";
      document.body.append(image);
      try {
        event.dataTransfer.setDragImage(image, 0, 0);
        dragPreviewReducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
        showDragPreview = true;
        window.addEventListener("dragover", trackSidebarDragPreview);
        void tick().then(() => {
          if (draggingSessionId === sessionId && showDragPreview) positionDragPreview(event.clientX, event.clientY);
        });
      } catch {
        // Unsupported WebViews keep their native drag image.
      } finally {
        window.setTimeout(() => image.remove(), 0);
      }
    }
  }

  function finishSidebarSessionDrag() {
    window.removeEventListener("dragover", trackSidebarDragPreview);
    if (dragPreviewFrame) cancelAnimationFrame(dragPreviewFrame);
    dragPreviewFrame = 0;
    dragPreviewPosition = null;
    dragPreviewTarget = null;
    dragPreviewVelocity = { x: 0, y: 0 };
    showDragPreview = false;
    dragOverBoard = false;
    draggingSessionId = null;
    workspaceDropIntent = null;
    workspaceDropGeometry = null;
    sidebarReleaseIntent = false;
  }

  // Dragging a visible chat out of the connected group in the sidebar hides it again.
  function sidebarReleaseTarget(event: DragEvent) {
    const sourceId = draggingSessionId;
    if (!sourceId || headerRelocatingSessionId || currentPaneIds().length < 2 || !currentPaneIds().includes(sourceId)) return false;
    const target = event.target instanceof Element ? event.target : null;
    return !target?.closest(".session-tree-item.connected");
  }

  function trackSidebarRelease(event: DragEvent) {
    const releasing = sidebarReleaseTarget(event);
    sidebarReleaseIntent = releasing;
    if (!releasing) return;
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
  }

  function dropSidebarRelease(event: DragEvent) {
    if (sidebarReleaseTarget(event) && draggingSessionId) {
      event.preventDefault();
      removePane(draggingSessionId);
    }
    finishSidebarSessionDrag();
  }

  function removePane(sessionId: string) {
    const remaining = currentPaneIds().filter((id) => id !== sessionId);
    if (!remaining.length) return;
    [primaryId, secondaryId, tertiaryId] = [remaining[0] ?? null, remaining[1] ?? null, remaining[2] ?? null];
    if (!remaining.includes(focusedPaneId ?? "")) focusedPaneId = primaryId;
    if (maximizedPaneId === sessionId) maximizedPaneId = null;
    persistWorkspaceLayout();
  }

  function trackSidebarDragPreview(event: DragEvent) {
    if (event.clientX || event.clientY) positionDragPreview(event.clientX, event.clientY);
  }

  function animateDragPreview() {
    dragPreviewFrame = 0;
    if (!dragPreviewElement || !dragPreviewTarget || !dragPreviewPosition) return;
    const target = dragPreviewTarget;
    const position = dragPreviewPosition;
    if (dragPreviewReducedMotion) {
      position.x = target.x;
      position.y = target.y;
      dragPreviewVelocity = { x: 0, y: 0 };
    } else {
      dragPreviewVelocity.x = (dragPreviewVelocity.x + (target.x - position.x) * .18) * .68;
      dragPreviewVelocity.y = (dragPreviewVelocity.y + (target.y - position.y) * .18) * .68;
      position.x += dragPreviewVelocity.x;
      position.y += dragPreviewVelocity.y;
      const lag = Math.hypot(target.x - position.x, target.y - position.y);
      if (lag > 52) {
        position.x = target.x - (target.x - position.x) * 52 / lag;
        position.y = target.y - (target.y - position.y) * 52 / lag;
      }
    }
    const tilt = dragPreviewReducedMotion ? 0 : Math.max(-4, Math.min(4, dragPreviewVelocity.x * .55));
    dragPreviewElement.style.transform = `translate3d(${position.x}px, ${position.y}px, 0) rotate(${tilt}deg)`;
    dragPreviewElement.style.visibility = "visible";
    if (Math.abs(target.x - position.x) > .2 || Math.abs(target.y - position.y) > .2
      || Math.abs(dragPreviewVelocity.x) > .2 || Math.abs(dragPreviewVelocity.y) > .2) {
      dragPreviewFrame = requestAnimationFrame(animateDragPreview);
    }
  }

  function positionDragPreview(clientX: number, clientY: number) {
    const x = Math.max(8, Math.min(clientX + 15, window.innerWidth - 238));
    const y = Math.max(8, Math.min(clientY + 15, window.innerHeight - 80));
    dragPreviewTarget = { x, y };
    if (!dragPreviewPosition) dragPreviewPosition = { x, y };
    if (!dragPreviewFrame) dragPreviewFrame = requestAnimationFrame(animateDragPreview);
  }

  function finishPaneHeaderGesture(commit = false) {
    const gesture = paneHeaderGesture;
    if (!gesture) return;
    window.clearTimeout(gesture.holdTimer);
    window.removeEventListener("pointermove", movePaneHeaderGesture);
    window.removeEventListener("pointerup", releasePaneHeaderGesture);
    window.removeEventListener("pointercancel", cancelPaneHeaderGesture);
    window.removeEventListener("blur", cancelPaneHeaderGesture);
    window.removeEventListener("keydown", escapePaneHeaderGesture, true);
    if (gesture.captureTarget.hasPointerCapture(gesture.pointerId)) {
      gesture.captureTarget.releasePointerCapture(gesture.pointerId);
    }
    paneHeaderGesture = null;
    if (gesture.mode !== "relocating") return;
    const intent = workspaceDropIntent;
    if (commit && intent?.kind === "move" && currentPaneIds().includes(gesture.sessionId)
      && intent.index < currentPaneIds().length) applySessionDrop(gesture.sessionId, intent);
    else if (gesture.previousMaximizedPaneId) maximizedPaneId = gesture.previousMaximizedPaneId;
    headerRelocatingSessionId = null;
    finishSidebarSessionDrag();
  }

  function movePaneHeaderGesture(event: PointerEvent) {
    const gesture = paneHeaderGesture;
    if (!gesture || event.pointerId !== gesture.pointerId) return;
    if (!event.buttons) {
      finishPaneHeaderGesture();
      return;
    }
    gesture.lastX = event.clientX;
    gesture.lastY = event.clientY;
    if (gesture.mode === "pending") {
      if (Math.hypot(event.clientX - gesture.startX, event.clientY - gesture.startY) < 6) return;
      finishPaneHeaderGesture();
      event.preventDefault();
      void getCurrentWindow().startDragging();
      return;
    }
    event.preventDefault();
    positionDragPreview(event.clientX, event.clientY);
    if (gesture.layoutReady) trackWorkspaceDropAt(event.clientX, event.clientY, gesture.sessionId);
  }

  function releasePaneHeaderGesture(event: PointerEvent) {
    if (event.pointerId === paneHeaderGesture?.pointerId) finishPaneHeaderGesture(true);
  }

  function cancelPaneHeaderGesture() {
    finishPaneHeaderGesture();
  }

  function escapePaneHeaderGesture(event: KeyboardEvent) {
    if (event.key !== "Escape") return;
    event.preventDefault();
    event.stopPropagation();
    finishPaneHeaderGesture();
  }

  function beginPaneHeaderGesture(event: PointerEvent) {
    if (event.button !== 0 || !event.isPrimary || paneHeaderGesture) return;
    const target = event.target instanceof Element ? event.target : null;
    const header = target?.closest(".pane-header");
    if (!(header instanceof HTMLElement) || target?.closest("button, input, textarea, select, a, summary, [role='button']")) return;
    const pane = header.closest<HTMLElement>("[data-workspace-pane]");
    const sessionId = pane?.dataset.workspacePane;
    if (!sessionId || !currentPaneIds().includes(sessionId)) return;
    if (isHeaderDoublePress(event)) {
      event.preventDefault();
      toggleWorkspaceMaximized();
      return;
    }
    const gesture: PaneHeaderGesture = {
      pointerId: event.pointerId,
      captureTarget: header,
      sessionId,
      startX: event.clientX,
      startY: event.clientY,
      lastX: event.clientX,
      lastY: event.clientY,
      holdTimer: 0,
      mode: "pending",
      layoutReady: false,
      previousMaximizedPaneId: null,
    };
    paneHeaderGesture = gesture;
    gesture.holdTimer = window.setTimeout(() => {
      if (paneHeaderGesture !== gesture) return;
      gesture.mode = "relocating";
      try { header.setPointerCapture(event.pointerId); } catch { /* The window listeners still handle the gesture. */ }
      gesture.layoutReady = !maximizedPaneId;
      gesture.previousMaximizedPaneId = maximizedPaneId;
      draggingSessionId = sessionId;
      headerRelocatingSessionId = sessionId;
      dragPreviewReducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
      showDragPreview = true;
      workspaceDropIntent = null;
      workspaceDropGeometry = null;
      maximizedPaneId = null;
      void tick().then(() => {
        if (paneHeaderGesture !== gesture) return;
        gesture.layoutReady = true;
        positionDragPreview(gesture.lastX, gesture.lastY);
        trackWorkspaceDropAt(gesture.lastX, gesture.lastY, sessionId);
      });
    }, 360);
    window.addEventListener("pointermove", movePaneHeaderGesture);
    window.addEventListener("pointerup", releasePaneHeaderGesture);
    window.addEventListener("pointercancel", cancelPaneHeaderGesture);
    window.addEventListener("blur", cancelPaneHeaderGesture);
    window.addEventListener("keydown", escapePaneHeaderGesture, true);
  }

  function setWorkspaceDropIntent(next: WorkspaceDropIntent) {
    const current = workspaceDropIntent;
    if (
      current?.kind === next.kind
      && current.index === next.index
      && Math.abs(current.left - next.left) < .01
      && Math.abs(current.width - next.width) < .01
    ) return;
    workspaceDropIntent = next;
  }

  function workspaceDropBounds() {
    if (workspaceDropGeometry || !workbenchElement) return workspaceDropGeometry;
    workspaceDropGeometry = {
      bounds: workbenchElement.getBoundingClientRect(),
      panes: Array.from(workbenchElement.querySelectorAll<HTMLElement>("[data-workspace-pane]"))
        .map((node) => ({ node, bounds: node.getBoundingClientRect() }))
        .sort((left, right) => left.bounds.left - right.bounds.left),
    };
    return workspaceDropGeometry;
  }

  function paneAtCursor(clientX: number, panes: WorkspaceDropGeometry["panes"]) {
    const directIndex = panes.findIndex(({ bounds }) => clientX >= bounds.left && clientX <= bounds.right);
    if (directIndex >= 0) return directIndex;
    return panes.reduce((closest, pane, index) => {
      const distance = clientX < pane.bounds.left
        ? pane.bounds.left - clientX
        : clientX - pane.bounds.right;
      return distance < closest.distance ? { index, distance } : closest;
    }, { index: 0, distance: Number.POSITIVE_INFINITY }).index;
  }

  function trackWorkspaceDrop(event: DragEvent) {
    const sourceId = draggingSessionId ?? event.dataTransfer?.getData("text/x-lume-session");
    if (!sourceId || !workbenchElement) return;
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
    trackWorkspaceDropAt(event.clientX, event.clientY, sourceId);
  }

  function trackWorkspaceDropAt(clientX: number, clientY: number, sourceId: string) {
    const paneIds = currentPaneIds();
    const geometry = workspaceDropBounds();
    if (!geometry) return;
    const { bounds, panes: paneNodes } = geometry;
    if (clientX < bounds.left || clientX > bounds.right || clientY < bounds.top || clientY > bounds.bottom) {
      workspaceDropIntent = null;
      return;
    }
    if (!paneNodes.length) {
      setWorkspaceDropIntent({ kind: "insert", index: 0, left: 0, width: 100 });
      return;
    }
    const cursorPaneIndex = paneAtCursor(clientX, paneNodes);
    const existingIndex = paneIds.indexOf(sourceId);
    if (existingIndex >= 0) {
      if (cursorPaneIndex === existingIndex) {
        workspaceDropIntent = null;
        return;
      }
      const target = paneNodes[cursorPaneIndex]?.bounds ?? bounds;
      setWorkspaceDropIntent({
        kind: "move",
        index: cursorPaneIndex,
        left: (target.left - bounds.left) / bounds.width * 100,
        width: target.width / bounds.width * 100,
      });
      return;
    }
    if (paneIds.length < 3) {
      const cursorPane = paneNodes[cursorPaneIndex]?.bounds ?? bounds;
      const replaceInset = Math.min(110, Math.max(54, cursorPane.width * .28));
      const replaceHysteresis = workspaceDropIntent?.kind === "replace"
        && workspaceDropIntent.index === cursorPaneIndex ? 12 : 0;
      const insideReplaceZone = clientX >= cursorPane.left + replaceInset - replaceHysteresis
        && clientX <= cursorPane.right - replaceInset + replaceHysteresis;
      if (insideReplaceZone) {
        setWorkspaceDropIntent({
          kind: "replace",
          index: cursorPaneIndex,
          left: (cursorPane.left - bounds.left) / bounds.width * 100,
          width: cursorPane.width / bounds.width * 100,
        });
        return;
      }
      const paneCenter = cursorPane.left + cursorPane.width / 2;
      let insertIndex = clientX < paneCenter ? cursorPaneIndex : cursorPaneIndex + 1;
      const currentInsertIndex = workspaceDropIntent?.kind === "insert" ? workspaceDropIntent.index : null;
      const currentBelongsToPane = currentInsertIndex === cursorPaneIndex || currentInsertIndex === cursorPaneIndex + 1;
      if (currentBelongsToPane && Math.abs(clientX - paneCenter) <= 18) {
        insertIndex = currentInsertIndex;
      }
      const nextCount = paneIds.length + 1;
      setWorkspaceDropIntent({
        kind: "insert",
        index: insertIndex,
        left: insertIndex / nextCount * 100,
        width: 100 / nextCount,
      });
      return;
    }
    const target = paneNodes[cursorPaneIndex]?.bounds ?? bounds;
    setWorkspaceDropIntent({
      kind: "replace",
      index: cursorPaneIndex,
      left: (target.left - bounds.left) / bounds.width * 100,
      width: target.width / bounds.width * 100,
    });
  }

  function leaveWorkspaceDrop(event: DragEvent) {
    const geometry = workspaceDropBounds();
    if (
      geometry
      && event.clientX >= geometry.bounds.left
      && event.clientX <= geometry.bounds.right
      && event.clientY >= geometry.bounds.top
      && event.clientY <= geometry.bounds.bottom
    ) return;
    const next = event.relatedTarget;
    if (next instanceof Node && workbenchElement?.contains(next)) return;
    workspaceDropIntent = null;
  }

  function dropSessionInWorkspace(event: DragEvent) {
    event.preventDefault();
    const sourceId = draggingSessionId
      ?? event.dataTransfer?.getData("text/x-lume-session")
      ?? event.dataTransfer?.getData("text/plain");
    const intent = workspaceDropIntent;
    if (!sourceId || !intent || !sessions.some((session) => session.id === sourceId)) {
      finishSidebarSessionDrag();
      return;
    }
    applySessionDrop(sourceId, intent);
    finishSidebarSessionDrag();
  }

  function applySessionDrop(sourceId: string, intent: WorkspaceDropIntent) {
    const paneIds = currentPaneIds();
    const previousCount = paneIds.length;
    const sourceIndex = paneIds.indexOf(sourceId);
    if (intent.kind === "insert" && sourceIndex < 0 && paneIds.length < 3) {
      paneIds.splice(Math.min(intent.index, paneIds.length), 0, sourceId);
    } else if (intent.kind === "move" && sourceIndex >= 0 && intent.index < paneIds.length && intent.index !== sourceIndex) {
      [paneIds[sourceIndex], paneIds[intent.index]] = [paneIds[intent.index], paneIds[sourceIndex]];
    } else if (intent.kind === "replace") {
      paneIds[Math.min(intent.index, paneIds.length - 1)] = sourceId;
    }
    const normalized = paneIds.filter((id, index) => paneIds.indexOf(id) === index).slice(0, 3);
    [primaryId, secondaryId, tertiaryId] = [normalized[0] ?? null, normalized[1] ?? null, normalized[2] ?? null];
    if (previousCount < 2 && normalized.length === 2) splitRatio = .5;
    if (previousCount < 3 && normalized.length === 3) {
      splitRatio = .5;
      tertiaryRatio = .34;
    }
    focusedPaneId = sourceId;
    maximizedPaneId = null;
    persistWorkspaceLayout();
  }

  function togglePaneMaximize(sessionId: string) {
    maximizedPaneId = maximizedPaneId === sessionId ? null : sessionId;
    focusedPaneId = sessionId;
    persistWorkspaceLayout();
  }

  function selectProject(value: string) {
    projectFilter = value;
    const available = value === "all"
      ? orderedSessions
      : orderedSessions.filter((session) => projectKey(session.workingDirectory ?? session.project) === value);
    if (!available.some((session) => session.id === primaryId)) primaryId = available[0]?.id ?? null;
    if (!available.some((session) => session.id === secondaryId)) secondaryId = null;
    if (!available.some((session) => session.id === tertiaryId)) tertiaryId = null;
    if (!secondaryId && tertiaryId) {
      secondaryId = tertiaryId;
      tertiaryId = null;
    }
    focusedPaneId = [primaryId, secondaryId, tertiaryId].includes(focusedPaneId) ? focusedPaneId : primaryId;
    if (![primaryId, secondaryId, tertiaryId].includes(maximizedPaneId)) maximizedPaneId = null;
    persistWorkspaceLayout();
  }

  function toggleInspector() {
    if (reviewOpen) {
      reviewOpen = false;
      reviewInitialPath = undefined;
      inspectorBeforeReview = false;
      inspectorOpen = true;
      persistWorkspaceLayout();
      return;
    }
    inspectorOpen = !inspectorOpen;
    persistWorkspaceLayout();
  }

  function motionDuration(duration: number) {
    return typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches ? 0 : duration;
  }

  function openSessionContextMenu(session: HubSession, x: number, y: number) {
    sessionContextError = "";
    sessionContextMenu = {
      sessionId: session.id,
      x: Math.max(8, Math.min(x, window.innerWidth - 230)),
      y: Math.max(8, Math.min(y, window.innerHeight - 170)),
      confirming: false,
      renaming: false,
    };
    void tick().then(() => (sessionContextNode?.querySelector<HTMLButtonElement>("button:not(:disabled)") ?? sessionContextNode)?.focus());
  }

  function beginSidebarSessionRename(session: HubSession) {
    sessionRenameDraft = sessionName(session);
    sessionContextError = "";
    if (sessionContextMenu) {
      sessionContextMenu = { ...sessionContextMenu, confirming: false, renaming: true };
    }
    void tick().then(() => {
      const input = sessionContextNode?.querySelector<HTMLInputElement>(".session-context-rename input");
      input?.focus();
      input?.select();
    });
  }

  async function saveSidebarSessionRename(session: HubSession) {
    if (sessionContextBusy) return;
    const requested = sessionRenameDraft.trim();
    if (!requested) {
      sessionContextError = tr("Enter a name for this session.", "Digite um nome para esta sessão.");
      return;
    }
    sessionContextBusy = true;
    sessionContextError = "";
    try {
      const finalName = await renameSession(session.id, requested);
      sessions = sessions.map((item) => item.id === session.id ? { ...item, sessionName: finalName } : item);
      sessionContextMenu = null;
      sessionRenameDraft = "";
    } catch (reason) {
      sessionContextError = String(reason).replace(/^Error:\s*/, "");
    } finally {
      sessionContextBusy = false;
    }
  }

  async function closeSidebarAgent(session: HubSession) {
    if (sessionContextBusy || !session.capabilities.canTerminate) return;
    sessionContextBusy = true;
    sessionContextError = "";
    try {
      await terminateSession(session.id);
      sessionContextMenu = null;
      await refreshSessionsAfterContextAction();
    } catch (reason) {
      sessionContextError = String(reason).replace(/^Error:\s*/, "");
    } finally {
      sessionContextBusy = false;
    }
  }

  async function takeControlFromSidebar(session: HubSession) {
    if (sessionContextBusy || !session.capabilities.canTakeControl) return;
    sessionContextBusy = true;
    sessionContextError = "";
    try {
      await takeControlSession(session.id);
      sessionContextMenu = null;
      await refreshSessionsAfterContextAction();
    } catch (reason) {
      sessionContextError = String(reason).replace(/^Error:\s*/, "");
    } finally {
      sessionContextBusy = false;
    }
  }

  async function refreshSessionsAfterContextAction() {
    try {
      const snapshot = await loadHubSnapshot();
      sessions = snapshot.sessions;
      internalServices = snapshot.internalServices ?? [];
      reconcileSelection();
    } catch {
      // The backend also emits a sessions-changed event; keep the successful action.
    }
  }

  function openReview(path?: string, sessionId = focusedSession?.id) {
    if (!sessionId) return;
    focusedPaneId = sessionId;
    reviewInitialPath = path;
    if (!reviewOpen) inspectorBeforeReview = inspectorOpen;
    inspectorOpen = false;
    reviewOpen = true;
  }

  function openRepository(sessionId: string) {
    focusedPaneId = sessionId;
    reviewOpen = false;
    inspectorOpen = true;
    inspectorSection = "repository";
  }

  function closeReview() {
    reviewOpen = false;
    reviewInitialPath = undefined;
    if (inspectorBeforeReview) inspectorOpen = true;
    inspectorBeforeReview = false;
  }

  function persistWorkspaceLayout() {
    if (typeof localStorage === "undefined") return;
    localStorage.setItem(workspaceLayoutKey, JSON.stringify({ primaryId, secondaryId, tertiaryId, focusedPaneId, maximizedPaneId, splitRatio, tertiaryRatio, projectFilter, inspectorOpen, selectedNamedLayoutId }));
  }

  function restoreWorkspaceLayout() {
    try {
      const saved = JSON.parse(localStorage.getItem(workspaceLayoutKey) ?? "null") as {
        primaryId?: string;
        secondaryId?: string;
        tertiaryId?: string;
        focusedPaneId?: string;
        maximizedPaneId?: string;
        splitRatio?: number;
        tertiaryRatio?: number;
        projectFilter?: string;
        inspectorOpen?: boolean;
        selectedNamedLayoutId?: string;
      } | null;
      primaryId = saved?.primaryId ?? null;
      secondaryId = saved?.secondaryId ?? null;
      tertiaryId = saved?.tertiaryId ?? null;
      focusedPaneId = saved?.focusedPaneId ?? primaryId;
      maximizedPaneId = saved?.maximizedPaneId ?? null;
      if (Number.isFinite(saved?.splitRatio)) {
        splitRatio = Math.min(0.75, Math.max(0.25, saved?.splitRatio ?? 0.5));
      }
      if (Number.isFinite(saved?.tertiaryRatio)) {
        tertiaryRatio = Math.min(0.5, Math.max(0.22, saved?.tertiaryRatio ?? 0.34));
      }
      projectFilter = saved?.projectFilter ?? "all";
      inspectorOpen = saved?.inspectorOpen ?? true;
      selectedNamedLayoutId = namedLayouts.some((layout) => layout.id === saved?.selectedNamedLayoutId)
        ? saved?.selectedNamedLayoutId ?? ""
        : "";
    } catch {
      localStorage.removeItem(workspaceLayoutKey);
    }
  }

  function resizeSplit(clientX: number, divider: 0 | 1) {
    if (!workbenchElement) return;
    const bounds = workbenchElement.getBoundingClientRect();
    if (!bounds.width) return;
    const pointerRatio = Math.min(1, Math.max(0, (clientX - bounds.left) / bounds.width));
    if (divider === 1 && tertiary) {
      tertiaryRatio = Math.min(0.5, Math.max(0.22, 1 - pointerRatio));
      return;
    }
    const availableRatio = tertiary ? 1 - tertiaryRatio : 1;
    splitRatio = Math.min(0.75, Math.max(0.25, pointerRatio / availableRatio));
  }

  function beginSplitResize(event: PointerEvent, divider: 0 | 1) {
    if (event.button !== 0) return;
    event.preventDefault();
    resizingDivider = divider;
    event.currentTarget instanceof HTMLElement && event.currentTarget.setPointerCapture(event.pointerId);
    resizeSplit(event.clientX, divider);
  }

  function moveSplitResize(event: PointerEvent) {
    if (resizingDivider === null) return;
    resizeSplit(event.clientX, resizingDivider);
  }

  function endSplitResize(event: PointerEvent) {
    if (resizingDivider === null) return;
    resizingDivider = null;
    if (event.currentTarget instanceof HTMLElement && event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    persistWorkspaceLayout();
  }

  function handleSplitKeydown(event: KeyboardEvent, divider: 0 | 1) {
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    if (divider === 1 && tertiary) {
      if (event.key === "Home") tertiaryRatio = 0.5;
      else if (event.key === "End") tertiaryRatio = 0.22;
      else tertiaryRatio = Math.min(0.5, Math.max(0.22, tertiaryRatio + (event.key === "ArrowLeft" ? 0.04 : -0.04)));
    } else if (event.key === "Home") splitRatio = 0.25;
    else if (event.key === "End") splitRatio = 0.75;
    else splitRatio = Math.min(0.75, Math.max(0.25, splitRatio + (event.key === "ArrowLeft" ? -0.04 : 0.04)));
    persistWorkspaceLayout();
  }

  function workbenchColumns() {
    if (maximizedSession || !secondary) return undefined;
    if (reviewFocusSession) {
      const shown = [primary, secondary, tertiary].filter(Boolean).map((item) => item?.id === reviewFocusSession.id ? "minmax(0, 1fr)" : "0px");
      return shown.join(" 0px ");
    }
    if (!tertiary) return `minmax(0, ${splitRatio}fr) 7px minmax(0, ${1 - splitRatio}fr)`;
    const shared = 1 - tertiaryRatio;
    return `minmax(0, ${splitRatio * shared}fr) 7px minmax(0, ${(1 - splitRatio) * shared}fr) 7px minmax(0, ${tertiaryRatio}fr)`;
  }

  // startDragging hands the press to the window manager, which can swallow the
  // browser's dblclick, so a double press is recognized from the two pointerdowns.
  let lastHeaderPress = { at: 0, x: 0, y: 0 };

  function isHeaderDoublePress(event: PointerEvent) {
    const double = event.timeStamp - lastHeaderPress.at < 420
      && Math.hypot(event.clientX - lastHeaderPress.x, event.clientY - lastHeaderPress.y) < 6;
    lastHeaderPress = double
      ? { at: 0, x: 0, y: 0 }
      : { at: event.timeStamp, x: event.clientX, y: event.clientY };
    return double;
  }

  function toggleWorkspaceMaximized() {
    void getCurrentWindow().toggleMaximize();
  }

  function beginWorkspaceDrag(event: PointerEvent) {
    if (event.button !== 0) return;
    const target = event.target instanceof Element ? event.target : null;
    if (target?.closest("button, input, textarea, select, a, summary, .header-selectors, [role='button']")) return;
    event.preventDefault();
    if (isHeaderDoublePress(event)) {
      toggleWorkspaceMaximized();
      return;
    }
    void getCurrentWindow().startDragging();
  }

  function beginWorkspaceResize(event: PointerEvent, direction: typeof workspaceResizeEdges[number]) {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    void getCurrentWindow().startResizeDragging(direction);
  }

  async function returnToOrb() {
    await getCurrentWindow().close();
  }

  async function updatePreference<K extends keyof Preferences>(key: K, value: Preferences[K]) {
    return savePreferencePatch({ [key]: value } as Pick<Preferences, K>);
  }

  function showWorkflowBoard(open: boolean) {
    finishSidebarSessionDrag();
    boardOpen = open;
    if (open) boardMounted = true;
    try { localStorage.setItem("lume:workflow-board:open", String(open)); }
    catch { /* Opening the board is still available when storage is disabled. */ }
  }

  async function deleteWorkflowBoardGroup(id: string) {
    const deadline = Date.now() + 15_000;
    while (settingsSaving && Date.now() < deadline) {
      await new Promise<void>((resolve) => setTimeout(resolve, 50));
    }
    if (settingsSaving) throw new Error(tr("Preferences are still being saved. Try again.", "Os ajustes ainda estão sendo salvos. Tente novamente."));
    if (!preferences.workflowGroups.some((item) => item.id === id)) return;
    if (!await updatePreference("workflowGroups", preferences.workflowGroups.filter((item) => item.id !== id))) {
      throw new Error(settingsError || tr("Could not delete the workflow.", "Não foi possível excluir o workflow."));
    }
  }

  async function saveWorkflowBoardGroup(group: WorkflowGroupDefinition) {
    const deadline = Date.now() + 15_000;
    while (settingsSaving && Date.now() < deadline) {
      await new Promise<void>((resolve) => setTimeout(resolve, 50));
    }
    if (settingsSaving) throw new Error(tr("Preferences are still being saved. Try again.", "Os ajustes ainda estão sendo salvos. Tente novamente."));
    const next = preferences.workflowGroups.some((item) => item.id === group.id)
      ? preferences.workflowGroups.map((item) => item.id === group.id ? group : item)
      : [...preferences.workflowGroups, group];
    if (!await updatePreference("workflowGroups", next)) {
      throw new Error(settingsError || tr("Could not save the workflow.", "Não foi possível salvar o workflow."));
    }
  }

  function selectAppearance(mode: "system" | "light" | "dark") {
    void updatePreference("darkMode", mode === "system" ? undefined : mode === "dark");
  }

  function selectTheme(theme: AppearanceTheme) {
    void savePreferencePatch({ appearanceTheme: theme });
  }

  async function savePreferencePatch(patch: Partial<Preferences>) {
    if (settingsSaving) return false;
    const previous = preferences;
    const next = { ...preferences, ...patch };
    preferences = next;
    language = next.language;
    settingsSaving = true;
    settingsError = "";
    try {
      await savePreferences(next);
      await emit("lume://preferences-changed", next);
      return true;
    } catch (reason) {
      preferences = previous;
      language = previous.language;
      settingsError = String(reason).replace(/^Error:\s*/, "");
      return false;
    } finally {
      settingsSaving = false;
    }
  }

  async function loadSettingsResource(resource: SettingsDataResource) {
    const inFlight = settingsResourceRequests.get(resource);
    if (inFlight) return inFlight;
    if (Date.now() - (settingsResourceLoadedAt.get(resource) ?? 0) < 30_000) return;

    settingsResourceLoadingCount += 1;
    settingsLoading = true;
    const request = (async () => {
      try {
        switch (resource) {
          case "integrations": integrations = await loadIntegrationStatuses(); break;
          case "vscode": vscodeStatus = await loadVscodeStatus(); break;
          case "externalPlugins": externalPlugins = await loadExternalPlugins(); break;
          case "mobileStatus": mobileStatus = await loadMobileGatewayStatus(); break;
          case "pairedDevices": pairedDevices = await loadPairedDevices(); break;
          case "monitors": monitors = (await availableMonitors()).map((monitor, index) => ({
            id: monitor.name ?? `monitor-${index}`,
            label: monitor.name || `${tr("Monitor", "Monitor")} ${index + 1}`,
          })); break;
          case "version": appVersion = await getVersion(); break;
        }
        settingsResourceLoadedAt.set(resource, Date.now());
        if (!selectedProfileKey) selectedProfileKey = detectedProjects[0]?.key ?? null;
      } catch (reason) {
        settingsError = String(reason).replace(/^Error:\s*/, "");
      } finally {
        settingsResourceLoadingCount = Math.max(0, settingsResourceLoadingCount - 1);
        settingsLoading = settingsResourceLoadingCount > 0;
      }
    })();
    settingsResourceRequests.set(resource, request);
    try {
      await request;
    } finally {
      if (settingsResourceRequests.get(resource) === request) settingsResourceRequests.delete(resource);
    }
  }

  async function loadSettingsSectionData(section: SettingsSectionKey) {
    const inFlight = settingsSectionRequests.get(section);
    if (inFlight) return inFlight;
    const resources = settingsSectionResources[section];
    if (!resources.length) return;

    settingsLoadingSections = [...settingsLoadingSections, section];
    const request = Promise.all(resources.map(loadSettingsResource)).then(() => undefined);
    settingsSectionRequests.set(section, request);
    try {
      await request;
    } finally {
      settingsLoadingSections = settingsLoadingSections.filter((item) => item !== section);
      if (settingsSectionRequests.get(section) === request) settingsSectionRequests.delete(section);
    }
  }

  function closeSettings() {
    settingsOpen = false;
    settingsSections = {
      appearance: false,
      preferences: false,
      agents: false,
      companions: false,
      externalDetectors: false,
      shortcuts: false,
      projectProfiles: false,
      remoteComputers: false,
      mobileAccess: false,
      about: false,
      reset: false,
    };
  }

  function setSettingsSectionOpen(section: SettingsSectionKey, open: boolean) {
    settingsSections[section] = open;
    if (open) void loadSettingsSectionData(section);
  }

  function openSettings() {
    settingsOpen = true;
  }

  async function toggleIntegration(integration: IntegrationStatus) {
    if (!integration.installed || !integration.canConfigure) return;
    if (integration.kind === "antigravity" && !integration.configured) {
      antigravityHookConfirmation = true;
      return;
    }
    await setIntegrationConfigured(integration, !integration.configured);
  }

  async function setIntegrationConfigured(integration: IntegrationStatus, enabled: boolean) {
    if (!integration.installed || !integration.canConfigure) return;
    configuringIntegration = integration.kind;
    settingsMessage = "";
    try {
      await configureIntegration(integration.kind, enabled);
      integrations = await loadIntegrationStatuses();
    } catch (reason) {
      settingsError = String(reason).replace(/^Error:\s*/, "");
    } finally {
      configuringIntegration = null;
    }
  }

  async function confirmAntigravityHooks() {
    antigravityHookConfirmation = false;
    const integration = integrations.find((item) => item.kind === "antigravity");
    if (integration) await setIntegrationConfigured(integration, true);
  }

  async function runIntegrationDiagnostic(integration: IntegrationStatus) {
    diagnosingIntegration = integration.kind;
    try {
      integrationDiagnostics = { ...integrationDiagnostics, [integration.kind]: await diagnoseIntegration(integration.kind) };
    } catch (reason) {
      settingsError = String(reason).replace(/^Error:\s*/, "");
    } finally {
      diagnosingIntegration = null;
    }
  }

  async function toggleVscode() {
    if (!vscodeStatus.installed) return;
    configuringVscode = true;
    try {
      await configureVscode(!vscodeStatus.configured);
      vscodeStatus = await loadVscodeStatus();
    } catch (reason) {
      settingsError = String(reason).replace(/^Error:\s*/, "");
    } finally {
      configuringVscode = false;
    }
  }

  async function addExternalPlugin() {
    const selected = await withNativeDialog(() => openDialog({ multiple: false, directory: false, filters: [{ name: "Lume plugin", extensions: ["json"] }] }));
    if (!selected || Array.isArray(selected)) return;
    installingPlugin = true;
    try {
      await installExternalPlugin(selected);
      externalPlugins = await loadExternalPlugins();
    } catch (reason) {
      settingsError = String(reason).replace(/^Error:\s*/, "");
    } finally {
      installingPlugin = false;
    }
  }

  async function uninstallExternalPlugin(id: string) {
    try {
      await removeExternalPlugin(id);
      externalPlugins = await loadExternalPlugins();
    } catch (reason) {
      settingsError = String(reason).replace(/^Error:\s*/, "");
    }
  }

  function shortcutFromEvent(event: KeyboardEvent) {
    if (["Control", "Shift", "Alt", "Meta"].includes(event.key)) return null;
    const modifiers = [event.ctrlKey ? "Ctrl" : "", event.altKey ? "Alt" : "", event.shiftKey ? "Shift" : "", event.metaKey ? "Super" : ""].filter(Boolean);
    if (!modifiers.length) return null;
    const key = event.code.startsWith("Key") ? event.code.slice(3) : event.code.startsWith("Digit") ? event.code.slice(5) : event.code;
    return key && key !== "Unidentified" ? [...modifiers, key].join("+") : null;
  }

  function captureShortcut(event: KeyboardEvent) {
    event.preventDefault();
    event.stopPropagation();
    if (event.key === "Escape") {
      shortcutEditorKey = null;
      return;
    }
    const shortcut = shortcutFromEvent(event);
    if (shortcut) shortcutDraft = shortcut;
  }

  async function openShortcutEditor(key: Exclude<typeof shortcutEditorKey, null>) {
    shortcutEditorKey = key;
    shortcutDraft = preferences[key];
    await tick();
    document.querySelector<HTMLElement>("[data-shortcut-capture]")?.focus();
  }

  async function saveShortcut() {
    if (!shortcutEditorKey || !shortcutDraft) return;
    if (await updatePreference(shortcutEditorKey, shortcutDraft)) shortcutEditorKey = null;
  }

  async function updateProjectProfile(patch: Partial<Preferences["projectProfiles"][string]>) {
    if (!selectedProfileKey) return;
    const label = detectedProjects.find((project) => project.key === selectedProfileKey)?.label ?? selectedProfileKey;
    const current = selectedProjectProfile ?? { label, soundEnabled: true, preferredAgents: [] };
    await updatePreference("projectProfiles", {
      ...preferences.projectProfiles,
      [selectedProfileKey]: { ...current, ...patch },
    });
  }

  function integrationAgentKind(kind: IntegrationStatus["kind"]): AgentKind {
    return kind === "claude" ? "claude_code" : kind;
  }

  async function togglePreferredAgent(agent: AgentKind) {
    const current = selectedProjectProfile?.preferredAgents ?? [];
    await updateProjectProfile({ preferredAgents: current.includes(agent) ? current.filter((item) => item !== agent) : [...current, agent] });
  }

  async function captureProfilePosition() {
    try {
      const position = await loadOverlayPosition();
      await updateProjectProfile({ overlayX: Math.round(position.x), overlayY: Math.round(position.y) });
    } catch (reason) {
      settingsError = String(reason).replace(/^Error:\s*/, "");
    }
  }

  async function applyProjectProfile() {
    if (!selectedProjectProfile) return;
    await savePreferencePatch({
      monitorId: selectedProjectProfile.monitorId ?? preferences.monitorId,
      overlayX: selectedProjectProfile.overlayX ?? preferences.overlayX,
      overlayY: selectedProjectProfile.overlayY ?? preferences.overlayY,
    });
    settingsMessage = tr("Project profile applied.", "Perfil de projeto aplicado.");
  }

  async function toggleMobileAccess() {
    if (mobileBusy) return;
    mobileBusy = true;
    pairingOffer = null;
    pairingQr = null;
    try {
      mobileStatus = mobileStatus?.networkReachable ? await disableMobileGateway() : await enableMobileGateway();
      if (mobileStatus.networkReachable) await createMobilePairing();
    } catch (reason) {
      settingsError = String(reason).replace(/^Error:\s*/, "");
    } finally {
      mobileBusy = false;
    }
  }

  async function createMobilePairing() {
    try {
      pairingOffer = await beginMobilePairing();
      pairingQr = await QRCode.toDataURL(pairingOffer.payload, { width: 184, margin: 3, errorCorrectionLevel: "M" });
    } catch (reason) {
      settingsError = String(reason).replace(/^Error:\s*/, "");
    }
  }

  async function removePairedDevice(id: string) {
    if (mobileBusy) return;
    mobileBusy = true;
    try {
      await revokePairedDevice(id);
      pairedDevices = await loadPairedDevices();
    } catch (reason) {
      settingsError = String(reason).replace(/^Error:\s*/, "");
    } finally {
      mobileBusy = false;
    }
  }

  async function toggleDeviceScope(device: PairedDevice, scope: MobileScope) {
    const scopes = device.scopes.includes(scope) ? device.scopes.filter((item) => item !== scope) : [...device.scopes, scope];
    try {
      await setPairedDeviceScopes(device.id, scopes);
      pairedDevices = await loadPairedDevices();
    } catch (reason) {
      settingsError = String(reason).replace(/^Error:\s*/, "");
    }
  }

  async function checkForUpdates() {
    if (["checking", "downloading", "ready"].includes(updateState)) return;
    updateState = "checking";
    updateDetail = tr("Checking for updates…", "Procurando atualizações…");
    updateProgress = null;
    try {
      pendingUpdate = await check({ timeout: 15_000, headers: { "Cache-Control": "no-cache" } });
      availableVersion = pendingUpdate?.version ?? null;
      updateState = pendingUpdate ? "available" : "up_to_date";
      updateDetail = pendingUpdate
        ? tr(`Version ${pendingUpdate.version} is available.`, `A versão ${pendingUpdate.version} está disponível.`)
        : tr("You are up to date.", "Você está atualizado.");
    } catch {
      updateState = "error";
      updateDetail = tr("Could not check for updates.", "Não foi possível verificar atualizações.");
    }
  }

  async function installUpdate() {
    if (!pendingUpdate || updateState === "downloading") return;
    updateState = "downloading";
    updateDetail = tr("Downloading and preparing the update…", "Baixando e preparando a atualização…");
    updateProgress = 0;
    let downloaded = 0;
    let total: number | undefined;
    try {
      await pendingUpdate.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength;
          return;
        }
        if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          updateProgress = total ? Math.min(99, Math.round((downloaded / total) * 100)) : null;
          return;
        }
        updateProgress = 100;
      });
      updateState = "ready";
      updateDetail = tr("Update installed. Restarting Lume…", "Atualização instalada. Reiniciando o Lume…");
      await relaunch();
    } catch {
      updateState = "error";
      updateDetail = tr("The update could not be installed.", "A atualização não pôde ser instalada.");
      updateProgress = null;
    }
  }

  async function resetSettings() {
    if (!resetConfirming) {
      resetConfirming = true;
      return;
    }
    if (await savePreferencePatch(structuredClone(defaultPreferences))) {
      removeBackgroundImage();
      workspaceBackgroundImageOpacity = 100;
      try { localStorage.removeItem(workspaceBackgroundImageOpacityKey); }
      catch { /* The image opacity still resets for this window. */ }
      resetConfirming = false;
      settingsMessage = tr("Settings restored.", "Ajustes restaurados.");
    }
  }

  function reconcileSelection() {
    if (projectFilter !== "all" && !workspaceProjects.some((project) => project.value === projectFilter)) projectFilter = "all";
    const ids = new Set(projectSessions.map((session) => session.id));
    if (!primaryId || !ids.has(primaryId)) {
      primaryId = projectSessions[0]?.id ?? null;
    }
    if (secondaryId && (!ids.has(secondaryId) || secondaryId === primaryId)) {
      secondaryId = null;
    }
    if (tertiaryId && (!ids.has(tertiaryId) || tertiaryId === primaryId || tertiaryId === secondaryId)) {
      tertiaryId = null;
    }
    if (!secondaryId && tertiaryId) {
      secondaryId = tertiaryId;
      tertiaryId = null;
    }
    if (!focusedPaneId || ![primaryId, secondaryId, tertiaryId].includes(focusedPaneId)) {
      focusedPaneId = primaryId;
    }
    if (maximizedPaneId && ![primaryId, secondaryId, tertiaryId].includes(maximizedPaneId)) {
      maximizedPaneId = null;
    }
  }

  onMount(() => {
    try { if (localStorage.getItem("lume:workflow-board:open") === "true") showWorkflowBoard(true); }
    catch { /* Start with chats when storage is unavailable. */ }
    const narrowSidebar = window.matchMedia("(max-width: 800px)");
    const syncSidebarWidth = (event: MediaQueryListEvent | MediaQueryList) => {
      sidebarCollapsed = event.matches;
    };
    syncSidebarWidth(narrowSidebar);
    narrowSidebar.addEventListener("change", syncSidebarWidth);
    restoreNamedLayouts();
    restoreWorkspaceLayout();
    try { streamMessages = localStorage.getItem(workspaceStreamMessagesKey) !== "false"; }
    catch { /* Use the default when local storage is unavailable. */ }
    try { workspaceBackgroundImage = localStorage.getItem(workspaceBackgroundImageKey) ?? ""; }
    catch { /* Keep the theme background when local storage is unavailable. */ }
    try { agentMessageSurface = localStorage.getItem(agentMessageSurfaceKey) !== "false"; }
    catch { /* Agent messages keep their surface by default. */ }
    try {
      const savedOpacity = Number(localStorage.getItem(workspaceBackgroundImageOpacityKey) ?? "100");
      workspaceBackgroundImageOpacity = Number.isFinite(savedOpacity) ? Math.max(0, Math.min(100, savedOpacity)) : 100;
    } catch { /* Keep the default image opacity when local storage is unavailable. */ }
    const colorScheme = window.matchMedia("(prefers-color-scheme: dark)");
    const syncSystemTheme = (event: MediaQueryListEvent | MediaQueryList) => {
      systemDark = event.matches;
    };
    const handleWorkspaceKeydown = (event: KeyboardEvent) => {
      if (boardOpen && !settingsOpen && !launcherOpen && !sessionContextMenu && !headerControl && !searchOpen) return;
      if (event.key === "Escape" && (settingsOpen || launcherOpen || sessionContextMenu || headerControl || searchOpen)) {
        event.preventDefault();
        event.stopPropagation();
      }
      if (event.key === "Escape" && sessionContextMenu) {
        sessionContextMenu = null;
        return;
      }
      if (event.key === "Escape" && headerControl) {
        headerControl = null;
        namingLayout = false;
        return;
      }
      if (event.key === "Escape" && searchOpen) {
        closeAgentSearch();
        return;
      }
      if (event.key === "Escape" && launcherOpen) {
        launcherOpen = false;
        return;
      }
      if (event.key === "Escape" && reviewOpen && !boardOpen) {
        closeReview();
        return;
      }
      if (event.key === "Escape" && settingsOpen) {
        closeSettings();
        return;
      }
      if (boardOpen) return;
      if (event.key === "Escape" && maximizedPaneId) {
        maximizedPaneId = null;
        persistWorkspaceLayout();
        return;
      }
      const target = event.target;
      if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement || (target instanceof HTMLElement && target.isContentEditable)) return;
      if (event.key === "/" && !event.altKey && !event.ctrlKey && !event.metaKey) {
        event.preventDefault();
        void openAgentSearch();
        return;
      }
      if (!event.altKey || !/^Digit[1-3]$/.test(event.code)) return;
      const sessionId = [primaryId, secondaryId, tertiaryId][Number(event.code.at(-1)) - 1];
      if (!sessionId) return;
      event.preventDefault();
      focusedPaneId = sessionId;
      void tick().then(() => document.querySelector<HTMLElement>(`[data-workspace-pane="${CSS.escape(sessionId)}"]`)?.focus());
      persistWorkspaceLayout();
    };
    syncSystemTheme(colorScheme);
    colorScheme.addEventListener("change", syncSystemTheme);
    window.addEventListener("keydown", handleWorkspaceKeydown);
    const closeLauncher = (event: PointerEvent) => {
      const target = event.target as Node;
      if (launcherOpen && launcherRoot && !launcherRoot.contains(target) && !launcherPopoverNode?.contains(target)) launcherOpen = false;
      if (sessionContextMenu && !(target instanceof Element && target.closest(".session-context-menu"))) sessionContextMenu = null;
    };
    document.addEventListener("pointerdown", closeLauncher);
    const paneDragHost = workbenchElement;
    paneDragHost?.addEventListener("pointerdown", beginPaneHeaderGesture);
    let disposed = false;
    let preferenceEventRevision = 0;
    let conflictEventRevision = 0;
    let refreshTimer: ReturnType<typeof setTimeout> | undefined;
    let refreshPromise: Promise<boolean> | undefined;
    let refreshAgain = false;
    const startup = new WorkspaceStartup();

    const refresh = (): Promise<boolean> => {
      if (refreshPromise) {
        refreshAgain = true;
        return refreshPromise;
      }
      refreshPromise = fetchSnapshot();
      return refreshPromise;
    };

    async function fetchSnapshot(): Promise<boolean> {
      try {
        const snapshot = await loadHubSnapshot();
        if (!disposed) {
          const hadOpenPane = hasOpenWorkspacePane(sessions, [primaryId, secondaryId, tertiaryId]);
          sessions = snapshot.sessions;
          internalServices = snapshot.internalServices ?? [];
          if (!rateLimitRefreshRequested && sessions.some((session) => session.agent === "codex")) {
            rateLimitRefreshRequested = true;
            void refreshAgentRateLimits("codex").catch(() => undefined);
          }
          if (!antigravityRateLimitRefreshRequested && sessions.some((session) => session.agent === "antigravity")) {
            antigravityRateLimitRefreshRequested = true;
            void refreshAgentRateLimits("antigravity").catch(() => undefined);
          }
          error = "";
          reconcileSelection();
          if (pendingOpenedSession) {
            const pending = pendingOpenedSession;
            const opened = sessions.find((session) => session.agent === pending.agent && (
              pending.nativeId ? session.nativeSessionId === pending.nativeId && !pending.knownIds.has(session.id) : !pending.knownIds.has(session.id)
            ));
            if (opened) {
              projectFilter = "all";
              if (pending.replacePaneId && [primaryId, secondaryId, tertiaryId].includes(pending.replacePaneId)) {
                if (primaryId === pending.replacePaneId) primaryId = opened.id;
                else if (secondaryId === pending.replacePaneId) secondaryId = opened.id;
                else tertiaryId = opened.id;
                if (focusedPaneId === pending.replacePaneId) focusedPaneId = opened.id;
                if (maximizedPaneId === pending.replacePaneId) maximizedPaneId = opened.id;
                persistWorkspaceLayout();
              } else if (!hadOpenPane) {
                maximizedPaneId = null;
                selectSession(opened);
              }
              pendingOpenedSession = null;
            } else if (Date.now() - pending.startedAt > 30_000) {
              pendingOpenedSession = null;
            }
          }
          if (!namedLayoutRestored && sessions.length) {
            namedLayoutRestored = true;
            if (selectedNamedLayoutId) applyNamedLayout(selectedNamedLayoutId);
          }
        }
        return !disposed;
      } catch (reason) {
        if (!disposed) error = String(reason);
        return false;
      } finally {
        refreshPromise = undefined;
        if (refreshAgain && !disposed) {
          refreshAgain = false;
          void refresh();
        }
      }
    }

    const queueRefresh = () => {
      if (refreshTimer) return;
      if (refreshPromise) {
        refreshAgain = true;
        return;
      }
      refreshTimer = setTimeout(() => {
        refreshTimer = undefined;
        void refresh();
      }, 160);
    };

    // The window opens as soon as the shell is mounted, with its loading state, instead of staying
    // hidden until the sessions have loaded. No animation frame is awaited: a hidden webview never runs one.
    void tick().then(() => { if (!disposed) void markWorkspaceFrontendReady().catch(() => undefined); });

    void startup.run(async () => {
      await Promise.all([
        startup.subscribe(() => watchShortcutRegistrationError((error) => {
          if (startup.active) shortcutRegistrationError = error;
        })),
        startup.subscribe(() => listen<ExternalWriterConflict[]>("lume://external-writer-conflicts-changed", ({ payload }) => {
          conflictEventRevision += 1;
          externalWriterConflicts = Object.fromEntries(payload.map((conflict) => [conflict.sessionId, conflict]));
        })),
        startup.subscribe(() => listen("lume://sessions-changed", queueRefresh)),
        startup.subscribe(() => listen<Preferences>("lume://preferences-changed", ({ payload }) => {
          preferenceEventRevision += 1;
          preferences = payload;
          language = payload.language;
        })),
      ]);
      if (!startup.active) return;
      const conflictRevisionAtRequest = conflictEventRevision;
      void listExternalWriterConflicts()
        .then((conflicts) => {
          if (startup.active && conflictEventRevision === conflictRevisionAtRequest) {
            externalWriterConflicts = Object.fromEntries(conflicts.map((conflict) => [conflict.sessionId, conflict]));
          }
        })
        .catch(() => {
          // Older app processes may not expose the conflict query yet; live events still work.
        });
      const preferenceRevisionAtRequest = preferenceEventRevision;
      const [loadedPreferences, loadedSessions] = await Promise.all([
        loadPreferences(),
        refresh(),
      ]);
      if (!startup.active) return;
      if (preferenceEventRevision === preferenceRevisionAtRequest) {
        preferences = loadedPreferences;
        language = loadedPreferences.language;
      }
      if (!loadedSessions) throw new Error(error || tr("Could not load your sessions", "Não foi possível carregar suas sessões"));
    }, {
      onLoaded: () => { loading = false; },
      onError: (reason) => { error = reason; },
      ready: async () => { await tick(); await markWorkspaceFrontendReady(); },
      failed: reportWorkspaceFrontendFailure,
      timeoutMessage: tr("Workspace loading timed out. Try opening it again.", "O Workspace demorou demais para carregar. Tente abri-lo novamente."),
    });

    return () => {
      disposed = true;
      startup.dispose();
      if (refreshTimer) clearTimeout(refreshTimer);
      narrowSidebar.removeEventListener("change", syncSidebarWidth);
      colorScheme.removeEventListener("change", syncSystemTheme);
      window.removeEventListener("keydown", handleWorkspaceKeydown);
      document.removeEventListener("pointerdown", closeLauncher);
      paneDragHost?.removeEventListener("pointerdown", beginPaneHeaderGesture);
      finishPaneHeaderGesture();
      finishSidebarSessionDrag();
    };
  });
</script>

<main
  class:dark={darkMode}
  class:sidebar-collapsed={sidebarCollapsed}
  class:sidebar-text-hidden={sidebarTextHidden}
  class:searching={searchOpen}
  class:selecting={headerControl !== null}
  class="workspace terminal-window"
  class:agent-message-surface={Boolean(workspaceBackgroundImage) && agentMessageSurface}
  data-appearance={appearance.theme}
  style={appearance.baseCss}
  style:--lume-accent={appearance.accentCss}
  style:--lume-accent-strong={appearance.accentCss}
  style:--workspace-background-color={workspaceCanvasColor}
  style:--workspace-background-opacity={`${workspaceCanvasOpacity}%`}
>
  <SystemBannerStack items={systemBanners} language={preferences.language} dismissLabel={tr("Dismiss", "Fechar")} />
  {#each workspaceResizeEdges as direction}
    <button
      class="window-resize-edge edge-{direction.toLocaleLowerCase()}"
      type="button"
      tabindex="-1"
      aria-label={tr(`Resize ${direction}`, `Redimensionar ${direction}`)}
      onpointerdown={(event) => beginWorkspaceResize(event, direction)}
    ></button>
  {/each}
  <aside class="sidebar" bind:this={sidebarElement}>
    <header class="brand-header" role="group" aria-label={tr("Workspace controls", "Controles do Workspace")} onpointerdown={beginWorkspaceDrag}>
      <div class="brand-top">
        <span class="brand-mark"><BrandIcon name="lume" size={25} /></span>
        <span class="brand-copy"><strong>Lume</strong><small>Workspace</small></span>
        <button class="sidebar-toggle" type="button" aria-label={sidebarCollapsed ? tr("Expand sidebar", "Expandir barra lateral") : tr("Collapse sidebar", "Recolher barra lateral")} title={sidebarCollapsed ? tr("Expand sidebar", "Expandir barra lateral") : tr("Collapse sidebar", "Recolher barra lateral")} onclick={() => void toggleSidebar()}><WorkspaceSidebarToggleIcon collapsed={sidebarCollapsed} /></button>
      </div>
    <div class:expanded={headerControl !== null} class="header-selectors">
      {#if headerControl === "project"}
        <div class="project-picker">
          <LumeSelect
            ariaLabel={tr("Workspace project", "Projeto do Workspace")}
            value={projectFilter}
            options={[{ value: "all", label: tr("All projects", "Todos os projetos") }, ...workspaceProjects]}
            minWidth={0}
            onValueChange={selectProject}
          />
        </div>
        <button class="header-control-close" type="button" aria-label={tr("Close project selector", "Fechar seletor de projetos")} onclick={() => (headerControl = null)}><LumeIcon name="close" size={14} /></button>
      {:else if headerControl === "layout"}
        <div class="layout-picker">
          <LumeSelect
            ariaLabel={tr("Saved layouts", "Layouts salvos")}
            value={selectedNamedLayoutId}
            options={[
              { value: "", label: tr("Current layout", "Layout atual") },
              ...namedLayouts.map((layout) => ({
                value: layout.id,
                label: layout.id === selectedNamedLayoutId && selectedLayoutDirty
                  ? `${layout.name} · ${tr("Modified", "Modificado")}`
                  : layout.name,
              })),
            ]}
            minWidth={0}
            onValueChange={applyNamedLayout}
          />
        </div>
        <button class="header-control-close" type="button" aria-label={tr("Close layout selector", "Fechar seletor de layouts")} onclick={() => { headerControl = null; namingLayout = false; }}><LumeIcon name="close" size={14} /></button>
      {:else}
        <button class="header-control-icon" type="button" aria-label={tr("Select project", "Selecionar projeto")} title={tr("Select project", "Selecionar projeto")} onclick={() => openHeaderControl("project")}><WorkspaceHeaderIcon name="project" /></button>
        <button class="header-control-icon" type="button" aria-label={tr("Saved layouts", "Layouts salvos")} title={tr("Saved layouts", "Layouts salvos")} onclick={() => openHeaderControl("layout")}><WorkspaceHeaderIcon name="layout" /></button>
      {/if}
      {#if headerControl === null}
      <div class="header-utilities">
        <button class:active={boardOpen} class="inspector-button" type="button" title={tr("Workflow board", "Board de workflow")} aria-label={tr("Workflow board", "Board de workflow")} aria-pressed={boardOpen} onclick={() => showWorkflowBoard(!boardOpen)}><WorkspaceHeaderIcon name="workflow" /></button>
        <button class:active={inspectorOpen && !boardOpen} class="inspector-button" type="button" title={tr("Toggle inspector", "Alternar inspector")} aria-label={tr("Toggle inspector", "Alternar inspector")} aria-pressed={inspectorOpen && !boardOpen} onclick={() => { if (boardOpen) { showWorkflowBoard(false); if (!inspectorOpen) toggleInspector(); } else toggleInspector(); }}><WorkspaceHeaderIcon name="inspector" /></button>
        <button class:active={settingsOpen} class="settings-button" type="button" title={tr("Workspace settings", "Ajustes do Workspace")} aria-label={tr("Workspace settings", "Ajustes do Workspace")} aria-expanded={settingsOpen} onclick={() => settingsOpen ? closeSettings() : void openSettings()}><WorkspaceHeaderIcon name="settings" /></button>
        <button class="compact-mode" type="button" title={tr("Return to Orb", "Voltar ao Orb")} aria-label={tr("Return to Orb", "Voltar ao Orb")} onclick={() => void returnToOrb()}><WorkspaceHeaderIcon name="orb" /></button>
      </div>
      {/if}
    </div>
    {#if headerControl === "layout"}
      <div class="layout-actions" role="group" aria-label={tr("Layout actions", "Ações do layout")}>
        <button type="button" disabled={!primary} title={tr("Save as new layout", "Salvar como novo layout")} aria-label={tr("Save as new layout", "Salvar como novo layout")} onclick={() => void beginNamingLayout()}><LumeIcon name="plus" size={15} /></button>
        {#if selectedNamedLayout}
          <button type="button" disabled={!selectedLayoutDirty} title={tr("Update this layout", "Atualizar este layout")} aria-label={tr("Update this layout", "Atualizar este layout")} onclick={updateNamedLayout}><LumeIcon name="save" size={14} /></button>
          <button class="delete-layout" type="button" title={tr("Remove this layout", "Remover este layout")} aria-label={tr("Remove this layout", "Remover este layout")} onclick={deleteNamedLayout}><LumeIcon name="trash" size={14} /></button>
        {/if}
      </div>
      {#if namingLayout}
        <form class="layout-name-editor" onsubmit={(event) => { event.preventDefault(); saveNamedLayout(); }}>
          <LumeIcon name="layout" size={14} />
          <input bind:this={layoutNameInput} bind:value={layoutName} maxlength="36" aria-label={tr("Layout name", "Nome do layout")} onkeydown={(event) => { if (event.key === "Escape") { namingLayout = false; layoutName = ""; } }} />
          <button type="submit" disabled={!layoutName.trim()} title={tr("Save layout", "Salvar layout")} aria-label={tr("Save layout", "Salvar layout")}><LumeIcon name="check" size={13} /></button>
          <button type="button" title={tr("Cancel", "Cancelar")} aria-label={tr("Cancel", "Cancelar")} onclick={() => { namingLayout = false; layoutName = ""; }}><LumeIcon name="close" size={13} /></button>
        </form>
      {/if}
    {/if}
    </header>

    <div class:searching={searchOpen} class="session-heading">
      {#if searchOpen}
        <div class="search-inline">
          <input bind:this={searchInput} bind:value={query} placeholder={tr("Search agents", "Buscar agentes")} aria-label={tr("Search agents", "Buscar agentes")} />
          <button type="button" aria-label={tr("Close agent search", "Fechar busca de agentes")} onclick={closeAgentSearch}><LumeIcon name="close" size={14} /></button>
        </div>
      {:else}
        <button class="search-toggle" type="button" aria-label={tr("Search agents", "Buscar agentes")} title={tr("Search agents · /", "Buscar agentes · /")} onclick={() => void openAgentSearch()}><LumeIcon name="search" size={16} /></button>
        <strong>{tr("Agents", "Agentes")}</strong>
        <span>{orderedSessions.length}</span>
      {/if}
      <div class="session-launcher" bind:this={launcherRoot}>
        <button class:active={launcherOpen} type="button" aria-label={launcherOpen ? tr("Close session launcher", "Fechar iniciador de sessões") : tr("New or resume chat", "Novo chat ou retomar")} title={launcherOpen ? tr("Close session launcher", "Fechar iniciador de sessões") : tr("New or resume chat", "Novo chat ou retomar")} aria-controls="workspace-session-launcher" aria-expanded={launcherOpen} onclick={() => void toggleLauncher()}><LumeIcon name="plus" size={16} /></button>
        {#if launcherOpen}
          <div id="workspace-session-launcher" class="session-launcher-popover" use:floatLauncher transition:sessionLauncherTransition={{ duration: 190, radius: 12 }} role="group" aria-label={tr("Open session", "Abrir sessão")}>
            <strong>{tr("Open session", "Abrir sessão")}</strong>
            {#each integrations.filter((item) => item.installed && item.canLaunch) as integration (integration.kind)}
              <div class="launcher-agent">
                <div class="launcher-agent-row">
                  <BrandIcon name={integration.kind} size={17} />
                  <span>{integration.label}</span>
                  <button class:loading={launching === integration.kind && launchingSessionId === null} type="button" disabled={launching !== null} onclick={() => void startSession(integration.kind)} aria-busy={launching === integration.kind && launchingSessionId === null}>
                    <span class="launcher-button-content">
                      {#if launching === integration.kind && launchingSessionId === null}<span class="launcher-spinner" aria-hidden="true"></span>{/if}
                      <span>{#if launching === integration.kind && launchingSessionId === null}{launchingPhase === "choosing" ? tr("Choose…", "Escolher…") : tr("Opening…", "Abrindo…")}{:else}{tr("New", "Novo")}{/if}</span>
                    </span>
                  </button>
                  {#if integration.kind !== "gemini"}
                    <button type="button" class:active={resumeAgent === integration.kind} class:loading={loadingResumeAgent === integration.kind} disabled={launching !== null || loadingResumeAgent !== null} onclick={() => void toggleResumeSessions(integration.kind)} aria-busy={loadingResumeAgent === integration.kind}>
                      <span class="launcher-button-content">
                        {#if loadingResumeAgent === integration.kind}<span class="launcher-spinner" aria-hidden="true"></span>{/if}
                        <span>{loadingResumeAgent === integration.kind ? tr("Loading…", "Buscando…") : tr("Resume", "Retomar")}</span>
                      </span>
                    </button>
                  {/if}
                </div>
                {#if resumeAgent === integration.kind}
                  <div class="launcher-resume-list" transition:slide={{ duration: 145, easing: cubicOut }}>
                    {#if loadingResumeAgent === integration.kind}
                      <div class="launcher-loading-note" role="status" aria-live="polite" transition:fade={{ duration: 120 }}>
                        <span class="launcher-spinner" aria-hidden="true"></span>
                        <span>{tr("Finding recent sessions…", "Buscando sessões recentes…")}</span>
                      </div>
                    {:else if resumableSessions.length > 0}
                      {#each resumableSessions as stored (stored.id)}
                        <button class:loading={launchingSessionId === stored.id} type="button" disabled={launching !== null} title={stored.workingDirectory} onclick={() => void resumeStoredSession(stored)} aria-busy={launchingSessionId === stored.id}>
                          <span class="launcher-session-copy">
                            <strong>{stored.name}</strong>
                            <small>{launchingSessionId === stored.id ? tr("Opening session…", "Abrindo sessão…") : stored.project}</small>
                          </span>
                          {#if launchingSessionId === stored.id}<span class="launcher-spinner" aria-hidden="true"></span>{/if}
                        </button>
                      {/each}
                    {:else}
                      <p>{tr("No saved chats found.", "Nenhum chat salvo encontrado.")}</p>
                    {/if}
                  </div>
                {/if}
              </div>
            {:else}
              <p>{tr("No compatible CLI was found.", "Nenhuma CLI compatível foi encontrada.")}</p>
            {/each}
          </div>
        {/if}
      </div>
    </div>

    <div class="session-tools">
    <div class="session-filters" aria-label={tr("Filter agents", "Filtrar agentes")}>
      <button class:active={filter === "all"} type="button" onclick={() => filter = "all"}>{tr("All", "Todos")}</button>
      <button class:active={filter === "active"} type="button" onclick={() => filter = "active"}>{tr("Active", "Ativos")}</button>
      <button class:active={filter === "attention"} type="button" onclick={() => filter = "attention"}>{tr("Attention", "Atenção")}</button>
    </div>
    {#if !sidebarCollapsed}
      <button class="new-group" type="button" onclick={createSidebarGroup}><LumeIcon name="plus" size={12} />{tr("New group", "Novo grupo")}</button>
    {/if}
    </div>

    <nav class:releasing={sidebarReleaseIntent} class="session-list" aria-label={tr("Agent sessions", "Sessões de agentes")} ondragover={trackSidebarRelease} ondrop={dropSidebarRelease}>
      {#if loading}
        {#each [1, 2, 3] as item}
          <div class="session-skeleton" aria-hidden="true"><i></i><span></span></div>
        {/each}
      {:else if filteredSessions.length}
        {#each sidebarLayout.rows as row (row.key)}
          {#if row.kind === "header"}
            <div class:drop-target={groupDropTarget === (row.group?.id ?? "none")} class:collapsed={row.group?.collapsed} class:dragging-section={draggingSectionId === (row.group?.id ?? "none")} class:reorder-before={reorderTarget?.id === (row.group?.id ?? "none") && !reorderTarget.after} class:reorder-after={reorderTarget?.id === (row.group?.id ?? "none") && reorderTarget.after} class="group-heading" role="group" aria-label={row.group?.name ?? tr("No group", "Sem grupo")}
              draggable={renamingGroupId === null} title={tr("Drag to reorder", "Arraste para reordenar")}
              ondragstart={(event) => beginSectionDrag(event, row.group?.id ?? "none")} ondragend={endSectionDrag}
              ondragenter={(event) => dragOverGroup(event, row.group?.id ?? "none")} ondragover={(event) => dragOverGroup(event, row.group?.id ?? "none")} ondragleave={() => { groupDropTarget = null; reorderTarget = null; }} ondrop={(event) => dropOnGroup(event, row.group?.id ?? null)}>
              {#if row.group}
                {#if renamingGroupId === row.group.id}
                  <form class="group-rename" onsubmit={(event) => { event.preventDefault(); commitGroupRename(); }}>
                    <input use:focusOnMount bind:value={groupNameDraft} maxlength="40" aria-label={tr("Group name", "Nome do grupo")} onblur={commitGroupRename} onkeydown={(event) => { if (event.key === "Escape") { event.stopPropagation(); renamingGroupId = null; } }} />
                  </form>
                {:else}
                  <button class="group-toggle" type="button" aria-expanded={!row.group.collapsed} onclick={() => toggleSidebarGroup(row.group!.id)}>
                    <span class="group-chevron"><LumeIcon name="chevron-down" size={12} /></span><LumeIcon name="folder" size={13} /><strong>{row.group.name}</strong><small>{row.count}</small>
                  </button>
                  <span class="group-actions">
                    <button type="button" title={tr("Rename group", "Renomear grupo")} aria-label={tr("Rename group", "Renomear grupo")} onclick={() => { renamingGroupId = row.group!.id; groupNameDraft = row.group!.name; }}><LumeIcon name="rename" size={12} /></button>
                    <button class:confirm={confirmingGroupId === row.group.id} type="button" title={confirmingGroupId === row.group.id ? tr("Click again: agents stay, only the group goes", "Clique de novo: os agentes ficam, só o grupo some") : tr("Delete group", "Excluir grupo")} aria-label={tr("Delete group", "Excluir grupo")} onclick={() => requestDeleteGroup(row.group!.id)}><LumeIcon name="trash" size={12} /></button>
                  </span>
                {/if}
              {:else}
                <span class="group-label">{tr("No group", "Sem grupo")}</span><small>{row.count}</small>
              {/if}
            </div>
          {:else}
          {@const session = row.session}
          {@const index = row.index}
          {@const childAgents = subagentsBySession.get(session.id) ?? []}
          {@const waitingForChildren = parentWaitingForSubagents(session, childAgents)}
          {@const selected = currentPaneIds().includes(session.id)}
          <div class:focused={session.id === focusedPaneId} class:secondary-selected={selected && session.id !== focusedPaneId} class:connected={selected} class:connected-above={selected && index > 0 && currentPaneIds().includes(sidebarLayout.visible[index - 1].id)} class:connected-below={selected && index < sidebarLayout.visible.length - 1 && currentPaneIds().includes(sidebarLayout.visible[index + 1].id)} class="session-tree-item" transition:slide={{ duration: motionDuration(190), easing: cubicOut }}>
          <div
            class:dragging={draggingSessionId === session.id}
            class="session-row"
            role="group"
            aria-label={sessionName(session)}
            title={sidebarCollapsed ? sessionName(session) : tr("Drag to arrange this agent", "Arraste para organizar este agente")}
            draggable={true}
            ondragstart={(event) => beginSidebarSessionDrag(event, session.id)}
            ondragend={finishSidebarSessionDrag}
            oncontextmenu={(event) => { event.preventDefault(); openSessionContextMenu(session, event.clientX, event.clientY); }}
          >
            <button class="session-select" type="button" aria-label={`${sessionName(session)} · ${waitingForChildren ? tr("Waiting for subagents", "Aguardando subagentes") : statusLabel(session)}`} aria-current={session.id === focusedPaneId ? "page" : undefined} aria-expanded={childAgents.length ? expandedSubagentSessions.has(session.id) : undefined} aria-controls={childAgents.length ? `workspace-subagents-${session.id}` : undefined} onclick={() => activateSidebarSession(session, childAgents.length > 0)} onkeydown={(event) => { if (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) { event.preventDefault(); const rect = event.currentTarget.getBoundingClientRect(); openSessionContextMenu(session, rect.left + rect.width / 2, rect.bottom); } }}>
              <span class="session-icon"><ThreadAvatar seed={session.nativeSessionId || session.sessionName || session.id} label={sessionName(session)} size={34} /><i class="session-status-dot status-{waitingForChildren ? 'subagents' : session.status}" aria-hidden="true"></i></span>
              <span class="session-copy">
                <strong>{sessionName(session)}</strong>
                <small class="session-meta"><BrandIcon name={session.agent} size={10} /><span>{session.agentLabel} · {sessionSubtitle(session)}</span>{#if session.forkedFrom}<span class="session-fork" title={tr(`Forked from conversation ${session.forkedFrom.slice(0, 8)}`, `Fork da conversa ${session.forkedFrom.slice(0, 8)}`)} aria-label={tr("Fork", "Fork")}><LumeIcon name="fork" size={10} /></span>{/if}</small>
                <em class="status-{waitingForChildren ? 'subagents' : session.status}"><i></i>{waitingForChildren ? tr("Waiting for subagents", "Aguardando subagentes") : statusLabel(session)}</em>
              </span>
            {#if childAgents.length}
              <span class:open={expandedSubagentSessions.has(session.id)} class="subagent-toggle" aria-hidden="true">
                <span>{childAgents.length}</span><LumeIcon name="chevron-down" size={12} />
              </span>
            {/if}
            </button>
            <span class="session-environment-marker"><SessionEnvironmentMenu sessionId={session.id} {language} variant="sidebar" compact={sidebarCollapsed} /></span>
          </div>
          {#if childAgents.length}
            <div id={`workspace-subagents-${session.id}`} class:open={expandedSubagentSessions.has(session.id)} class="subagent-list-shell" aria-hidden={!expandedSubagentSessions.has(session.id)}>
            <div class="subagent-list" role="list" aria-label={tr(`Subagents of ${sessionName(session)}`, `Subagentes de ${sessionName(session)}`)}>
              {#each childAgents as child (child.id)}
                <div class="subagent-row" role="listitem">
                  <span class="subagent-branch" aria-hidden="true"></span>
                  <span class="subagent-avatar"><ThreadAvatar seed={`${session.id}:subagent:${child.id}`} label={child.label} size={24} /></span>
                  <span class="subagent-copy"><strong>{child.label}</strong><small class="subagent-status status-{child.status}">{child.status === "running" ? tr("Working", "Executando") : child.status === "failed" ? tr("Failed", "Falhou") : child.status === "waiting" ? tr("Waiting", "Aguardando") : child.status === "interrupted" ? tr("Interrupted", "Interrompido") : tr("Finished", "Concluído")}</small></span>
                </div>
              {/each}
            </div>
            </div>
          {/if}
          </div>
          {/if}
        {/each}
      {:else if !filteredInternalServices.length}
        <p class="no-results">{query ? tr("No matching agents", "Nenhum agente encontrado") : tr("No agents detected", "Nenhum agente detectado")}</p>
      {/if}
      {#if filteredInternalServices.length}
        <div class="internal-heading"><span>{tr("Internal services", "Serviços internos")}</span><small>{filteredInternalServices.length}</small></div>
        {#each filteredInternalServices as service (service.id)}
          <div class="internal-row" title={`${service.label} · PID ${service.processId}`}>
            <span class="session-icon"><BrandIcon name={service.agent} size={16} /></span>
            <span class="session-copy"><strong>{service.label}</strong><small>{tr("Codex service · active", "Serviço Codex · ativo")}</small></span>
            <i class="internal-live" aria-hidden="true"></i>
          </div>
        {/each}
      {/if}
    </nav>

  </aside>

  {#if sessionContextMenu && contextSession}
    <div
      class="session-context-menu"
      role="menu"
      aria-label={tr(`Actions for ${sessionName(contextSession)}`, `Ações para ${sessionName(contextSession)}`)}
      tabindex="-1"
      bind:this={sessionContextNode}
      style:left={`${sessionContextMenu.x}px`}
      style:top={`${sessionContextMenu.y}px`}
    >
      <strong>{sessionName(contextSession)}</strong>
      {#if sessionContextMenu.renaming}
        <form class="session-context-rename" onsubmit={(event) => { event.preventDefault(); void saveSidebarSessionRename(contextSession); }}>
          <label for={`workspace-session-name-${contextSession.id}`}>{tr("Session name", "Nome da sessão")}</label>
          <input id={`workspace-session-name-${contextSession.id}`} bind:value={sessionRenameDraft} maxlength="80" autocomplete="off" />
          <div class="session-context-actions">
            <button type="button" disabled={sessionContextBusy} onclick={() => (sessionContextMenu = null)}>{tr("Cancel", "Cancelar")}</button>
            <button class="primary" type="submit" disabled={sessionContextBusy}>{sessionContextBusy ? tr("Saving…", "Salvando…") : tr("Save", "Salvar")}</button>
          </div>
        </form>
      {:else if sessionContextMenu.confirming && contextSession.capabilities.canTerminate}
          <p>{tr("This closes the original CLI and stops its current task.", "Isso fecha a CLI original e interrompe a tarefa atual.")}</p>
          <div class="session-context-actions">
            <button type="button" role="menuitem" onclick={() => (sessionContextMenu = null)}>{tr("Cancel", "Cancelar")}</button>
            <button class="danger" type="button" role="menuitem" disabled={sessionContextBusy} onclick={() => void closeSidebarAgent(contextSession)}>{sessionContextBusy ? tr("Closing…", "Encerrando…") : tr("Close agent", "Encerrar agente")}</button>
          </div>
      {:else}
        <button class="session-context-command" type="button" role="menuitem" disabled={sessionContextBusy} onclick={() => beginSidebarSessionRename(contextSession)}>
          <LumeIcon name="rename" size={15} />
          <span>{tr("Rename session", "Renomear sessão")}</span>
        </button>
        <div class="session-context-groups" role="group" aria-label={tr("Group", "Grupo")}>
          <small>{tr("Group", "Grupo")}</small>
          {#each sidebarGroups as group (group.id)}
            <button class:current={groupAssignments[sessionGroupKey(contextSession)] === group.id} class="session-context-command" type="button" role="menuitem" onclick={() => { assignToGroup(contextSession, group.id); sessionContextMenu = null; }}>
              <LumeIcon name="folder" size={14} /><span>{group.name}</span>{#if groupAssignments[sessionGroupKey(contextSession)] === group.id}<LumeIcon name="check" size={13} />{/if}
            </button>
          {/each}
          {#if groupAssignments[sessionGroupKey(contextSession)]}
            <button class="session-context-command" type="button" role="menuitem" onclick={() => { assignToGroup(contextSession, null); sessionContextMenu = null; }}><LumeIcon name="close" size={14} /><span>{tr("Remove from group", "Tirar do grupo")}</span></button>
          {/if}
          <button class="session-context-command" type="button" role="menuitem" onclick={() => { const group = newSidebarGroup(); assignToGroup(contextSession, group.id); sessionContextMenu = null; renamingGroupId = group.id; groupNameDraft = group.name; }}><LumeIcon name="plus" size={14} /><span>{tr("New group…", "Novo grupo…")}</span></button>
        </div>
        {#if canLinkCodexCli(contextSession)}
          <button class="session-context-command" type="button" role="menuitem" onclick={() => { cliAssociationSessionId = contextSession.id; sessionContextMenu = null; }}>
            <LumeIcon name="split" size={15} />
            <span>{tr("Link conversation", "Vincular conversa")}</span>
          </button>
        {/if}
        {#if contextSession.capabilities.canTakeControl}
          <button class="session-context-command" type="button" role="menuitem" disabled={sessionContextBusy} onclick={() => void takeControlFromSidebar(contextSession)}>
            <LumeIcon name="take-control" size={15} />
            <span>{sessionContextBusy ? tr("Taking control…", "Assumindo controle…") : tr("Take control", "Assumir controle")}</span>
          </button>
        {/if}
        {#if contextSession.capabilities.canTerminate}
          <button class="session-context-command danger-command" type="button" role="menuitem" disabled={sessionContextBusy} onclick={() => { if (sessionContextMenu) sessionContextMenu = { ...sessionContextMenu, confirming: true }; }}>
            <LumeIcon name="stop" size={14} />
            <span>{tr("Close agent", "Encerrar agente")}</span>
          </button>
        {/if}
      {/if}
    </div>
  {/if}

  {#if cliAssociationSessionId}
    {#key cliAssociationSessionId}
      <CodexCliAssociationDialog sessionId={cliAssociationSessionId} language={preferences.language} dark={darkMode} onClose={() => { cliAssociationSessionId = null; }} onLinked={refreshSessionsAfterContextAction} />
    {/key}
  {/if}

  {#if settingsOpen}
    <div
      class="settings-scrim"
      role="presentation"
      in:fade={{ duration: motionDuration(180) }}
      out:fade={{ duration: motionDuration(120) }}
      onclick={(event) => {
        if (event.target === event.currentTarget) closeSettings();
      }}
    >
      <aside class="workspace-settings" aria-label={tr("Lume settings", "Ajustes do Lume")} aria-busy={settingsLoading} in:fly={{ x: 22, duration: motionDuration(210), easing: cubicOut }} out:fly={{ x: 14, duration: motionDuration(140), easing: cubicOut }}>
        <header>
          <span>
            <strong>{tr("Settings", "Ajustes")}</strong>
            <small>{tr("Workspace and Lume preferences", "Preferências do Workspace e do Lume")}</small>
          </span>
          <button type="button" aria-label={tr("Close settings", "Fechar ajustes")} onclick={closeSettings}>
            <LumeIcon name="close" size={16} />
          </button>
        </header>

        <div class="settings-content">
          <details class="settings-group" open={settingsSections.appearance} use:animatedDisclosure={(open) => setSettingsSectionOpen("appearance", open)}>
            <summary>{tr("Appearance", "Aparência")}</summary>
            <div class="settings-section-content">
            {#if settingsSections.appearance}
            <div class="appearance-options" aria-label={tr("Color mode", "Modo de cores")}>
              {#each [
                { id: "system", label: tr("System", "Sistema") },
                { id: "light", label: tr("Light", "Claro") },
                { id: "dark", label: tr("Dark", "Escuro") },
              ] as option (option.id)}
                <button
                  class:active={appearanceMode === option.id}
                  class="appearance-option mode-{option.id}"
                  type="button"
                  aria-pressed={appearanceMode === option.id}
                  disabled={settingsSaving}
                  onclick={() => selectAppearance(option.id as "system" | "light" | "dark")}
                >
                  <span class="appearance-preview" aria-hidden="true"><i></i><b></b><em></em></span>
                  <strong>{option.label}</strong>
                </button>
              {/each}
            </div>
            <p class="theme-label"><strong>{tr("Surface tint", "Tom das superfícies")}</strong>{#if (darkMode ? preferences.darkBase : preferences.lightBase) !== "theme"}<small>{tr("Not visible with a neutral surface.", "Sem efeito com superfície neutra.")}</small>{/if}</p>
            <div class="theme-options" class:overridden={(darkMode ? preferences.darkBase : preferences.lightBase) !== "theme"} aria-label={tr("Surface tint", "Tom das superfícies")}>
              {#each appearanceThemes as theme (theme.value)}
                <button
                  class:active={appearance.theme === theme.value}
                  type="button"
                  aria-pressed={appearance.theme === theme.value}
                  onclick={() => selectTheme(theme.value)}
                >
                  <span style:--theme-accent={theme.accent} style:--theme-surface={darkMode ? theme.darkSurface : theme.lightSurface}></span>
                  {theme.label}
                </button>
              {/each}
            </div>
            <p class="theme-label"><strong>{tr("Neutral surfaces", "Superfícies neutras")}</strong></p>
            {#if darkMode}
              <div class="theme-options" aria-label={tr("Dark surface", "Superfície escura")}>
                {#each darkBases as base (base.value)}
                  <button class:active={preferences.darkBase === base.value} type="button" aria-pressed={preferences.darkBase === base.value} onclick={() => void savePreferencePatch({ darkBase: base.value })}>
                    <span style:--theme-accent={appearance.accent ?? selectedAppearanceTheme.accent} style:--theme-surface={base.pigments?.surface ?? selectedAppearanceTheme.darkSurface}></span>
                    {tr(base.label, base.labelPt)}
                  </button>
                {/each}
              </div>
            {:else}
              <div class="theme-options" aria-label={tr("Light surface", "Superfície clara")}>
                {#each lightBases as base (base.value)}
                  <button class:active={preferences.lightBase === base.value} type="button" aria-pressed={preferences.lightBase === base.value} onclick={() => void savePreferencePatch({ lightBase: base.value })}>
                    <span style:--theme-accent={appearance.accent ?? selectedAppearanceTheme.accent} style:--theme-surface={base.pigments?.surface ?? selectedAppearanceTheme.lightSurface}></span>
                    {tr(base.label, base.labelPt)}
                  </button>
                {/each}
              </div>
            {/if}
            <div class="workspace-setting-row accent-setting">
              <span><strong>{tr("Accent color", "Cor de destaque")}</strong><small>{appearance.accent ?? tr("Following the surface tint", "Seguindo o tom das superfícies")}</small></span>
              <AccentColorPicker value={appearance.accent} opacity={preferences.accentOpacity} readyColors={appearanceThemes.map((theme) => theme.accent)} fallback={appearanceThemes.find((theme) => theme.value === appearance.theme)?.accent ?? "#43b47d"} {language} label={tr("Accent color", "Cor de destaque")} onValueChange={(color, opacity) => void savePreferencePatch({ accentColor: color, accentOpacity: opacity })} onReset={() => void savePreferencePatch({ accentColor: undefined, accentOpacity: 100 })} />
            </div>
            <div class="workspace-setting-row font-setting">
              <span><strong>{tr("Interface font", "Fonte da interface")}</strong></span>
              <LumeSelect ariaLabel={tr("Interface font", "Fonte da interface")} value={preferences.uiFont} minWidth={150} options={fontOptions(uiFonts)} onValueChange={(value) => void savePreferencePatch({ uiFont: value })} />
              <button class="font-import" type="button" title={tr("Import a font file", "Importar um arquivo de fonte")} onclick={() => void importFont("uiFont")}><LumeIcon name="plus" size={14} />{tr("Import", "Importar")}</button>
            </div>
            <div class="workspace-setting-row font-setting">
              <span><strong>{tr("Code and terminal font", "Fonte de código e terminal")}</strong></span>
              <LumeSelect ariaLabel={tr("Code and terminal font", "Fonte de código e terminal")} value={preferences.codeFont} minWidth={150} options={fontOptions(codeFonts)} onValueChange={(value) => void savePreferencePatch({ codeFont: value })} />
              <button class="font-import" type="button" title={tr("Import a font file", "Importar um arquivo de fonte")} onclick={() => void importFont("codeFont")}><LumeIcon name="plus" size={14} />{tr("Import", "Importar")}</button>
            </div>
            {#each customFonts as font (font.id)}
              <div class="workspace-setting-row font-setting imported-font">
                <span><strong>{font.name}</strong></span>
                <button class="font-import" type="button" onclick={() => void deleteFont(font)}><LumeIcon name="trash" size={14} />{tr("Remove", "Remover")}</button>
              </div>
            {/each}
            <div class="workspace-setting-row accent-setting">
              <span><strong>{tr("Light workspace", "Workspace claro")}</strong><small>{preferences.workspaceLightBackgroundColor ?? tr("Using the light preset", "Usando o preset claro")} · {preferences.workspaceLightBackgroundOpacity}%</small></span>
              <AccentColorPicker value={preferences.workspaceLightBackgroundColor} opacity={preferences.workspaceLightBackgroundOpacity} fallback={selectedAppearanceTheme.lightSurface} readyColors={["#f7f8f4", "#eef2ec", "#e9eee8", "#e9eee3", "#e5edf0", "#ece9f1", "#f0e9e2"]} minimumOpacity={35} {language} label={tr("Light workspace background", "Fundo claro do Workspace")} onValueChange={(color, opacity) => void savePreferencePatch({ workspaceLightBackgroundColor: color, workspaceLightBackgroundOpacity: opacity })} onReset={() => void savePreferencePatch({ workspaceLightBackgroundColor: undefined, workspaceLightBackgroundOpacity: 96 })} />
            </div>
            <div class="workspace-setting-row accent-setting">
              <span><strong>{tr("Dark workspace", "Workspace escuro")}</strong><small>{preferences.workspaceDarkBackgroundColor ?? preferences.workspaceBackgroundColor ?? tr("Using the dark preset", "Usando o preset escuro")} · {workspaceCanvasOpacity}%</small></span>
              <AccentColorPicker value={preferences.workspaceDarkBackgroundColor ?? preferences.workspaceBackgroundColor} opacity={workspaceCanvasOpacity} fallback={selectedAppearanceTheme.darkSurface} readyColors={["#0f1915", "#14231c", "#182116", "#101f28", "#1b1726", "#261a13", "#121916"]} minimumOpacity={35} {language} label={tr("Dark workspace background", "Fundo escuro do Workspace")} onValueChange={(color, opacity) => void savePreferencePatch({ workspaceDarkBackgroundColor: color, workspaceDarkBackgroundOpacity: opacity })} onReset={() => void savePreferencePatch({ workspaceDarkBackgroundColor: undefined, workspaceDarkBackgroundOpacity: 96, workspaceBackgroundColor: undefined })} />
            </div>
            <div class="workspace-setting-row wallpaper-setting" data-tooltip={workspaceBackgroundImage ? tr("Stored locally on this device", "Salva localmente neste dispositivo") : tr("Add your own workspace backdrop", "Adicione um plano de fundo ao Workspace")}>
              <span><strong>{tr("Background image", "Imagem de fundo")}</strong></span>
              {#if workspaceBackgroundImage}<i class="wallpaper-preview" style:background-image={`url("${workspaceBackgroundImage}")`} aria-hidden="true"></i>{/if}
              <div class="wallpaper-actions">
                <button type="button" title={workspaceBackgroundImage ? tr("Change background image", "Trocar imagem de fundo") : tr("Choose background image", "Escolher imagem de fundo")} aria-label={workspaceBackgroundImage ? tr("Change background image", "Trocar imagem de fundo") : tr("Choose background image", "Escolher imagem de fundo")} onclick={() => workspaceBackgroundInput?.click()}><LumeIcon name="image" size={14} /></button>
                {#if workspaceBackgroundImage}<button type="button" aria-label={tr("Remove background image", "Remover imagem de fundo")} onclick={removeBackgroundImage}><LumeIcon name="close" size={12} /></button>{/if}
              </div>
              <input bind:this={workspaceBackgroundInput} class="wallpaper-input" type="file" accept="image/png,image/jpeg,image/webp" onchange={(event) => { const file = event.currentTarget.files?.[0]; if (file) loadBackgroundImage(file); }} />
            </div>
            {#if workspaceBackgroundImage}
              <label class="workspace-setting-row wallpaper-opacity-setting">
                <span><strong>{tr("Image opacity", "Transparência da imagem")}</strong><small>{workspaceBackgroundImageOpacity}%</small></span>
                <input
                  class="settings-range"
                  aria-label={tr("Background image opacity", "Transparência da imagem de fundo")}
                  type="range"
                  min="0"
                  max="100"
                  step="5"
                  value={workspaceBackgroundImageOpacity}
                  oninput={(event) => setBackgroundImageOpacity(Number(event.currentTarget.value))}
                />
              </label>
              <label class="workspace-setting-row" data-tooltip={tr("Keeps replies readable over the image", "Mantém as respostas legíveis sobre a imagem")}>
                <span><strong>{tr("Agent message background", "Fundo nas mensagens do agente")}</strong></span>
                <input class="workspace-switch" type="checkbox" checked={agentMessageSurface} onchange={(event) => setAgentMessageSurface(event.currentTarget.checked)} />
              </label>
            {/if}
            {/if}
            </div>
          </details>

          <details class="settings-group compact-settings" open={settingsSections.preferences} use:animatedDisclosure={(open) => setSettingsSectionOpen("preferences", open)}>
            <summary>{tr("Preferences", "Preferências")}</summary>
            <div class="settings-section-content">
            {#if settingsSections.preferences}
            {#if settingsLoadingSections.includes("preferences")}<p class="settings-loading-hint" role="status">{tr("Loading display information…", "Carregando informações de tela…")}</p>{/if}
            <label class="workspace-setting-row" data-tooltip={tr("Used across Lume", "Usado em todo o Lume")}>
              <span><strong>{tr("Language", "Idioma")}</strong></span>
              <LumeSelect
                ariaLabel={tr("Language", "Idioma")}
                value={preferences.language}
                options={[{ value: "en", label: "English" }, { value: "pt-BR", label: "Português" }]}
                minWidth={118}
                onValueChange={(value) => void updatePreference("language", value as Preferences["language"])}
              />
            </label>
            <label class="workspace-setting-row" data-tooltip={tr("Default view for the next launch", "Visualização padrão da próxima abertura")}>
              <span><strong>{tr("Open Lume as", "Abrir o Lume como")}</strong></span>
              <LumeSelect
                ariaLabel={tr("Default startup view", "Visualização inicial padrão")}
                value={preferences.startupMode}
                options={[
                  { value: "ask", label: tr("Always ask", "Perguntar sempre") },
                  { value: "orb", label: "Orb" },
                  { value: "workspace", label: "Workspace" },
                ]}
                minWidth={126}
                onValueChange={(value) => void updatePreference("startupMode", value as Preferences["startupMode"])}
              />
            </label>
            <label class="workspace-setting-row" data-tooltip={tr("Alerts outside Lume", "Alertas fora do Lume")}>
              <span><strong>{tr("Desktop notifications", "Notificações no desktop")}</strong></span>
              <input class="workspace-switch" type="checkbox" checked={preferences.popupNotificationsEnabled} disabled={settingsSaving} onchange={(event) => void updatePreference("popupNotificationsEnabled", event.currentTarget.checked)} />
            </label>
            <label class="workspace-setting-row" data-tooltip={tr("Reveal new replies as they arrive", "Mostra novas respostas à medida que chegam")}>
              <span><strong>{tr("Stream agent messages", "Mensagens em Stream")}</strong></span>
              <input class="workspace-switch" type="checkbox" checked={streamMessages} onchange={(event) => setStreamMessages(event.currentTarget.checked)} />
            </label>
            <label class="workspace-setting-row" data-tooltip={tr("Keep Lume available", "Mantenha o Lume disponível")}>
              <span><strong>{tr("Start with the system", "Iniciar com o sistema")}</strong></span>
              <input class="workspace-switch" type="checkbox" checked={preferences.autostart} disabled={settingsSaving} onchange={(event) => void updatePreference("autostart", event.currentTarget.checked)} />
            </label>
            <label class="workspace-setting-row" data-tooltip={tr("Task and permission feedback", "Retorno de tarefas e permissões")}>
              <span><strong>{tr("Subtle sounds", "Sons sutis")}</strong></span>
              <input class="workspace-switch" type="checkbox" checked={preferences.soundEnabled} disabled={settingsSaving} onchange={(event) => void updatePreference("soundEnabled", event.currentTarget.checked)} />
            </label>
            <label class="workspace-setting-row">
              <span><strong>{tr("Sound volume", "Volume dos sons")}</strong><small>{preferences.soundVolume}%</small></span>
              <input class="settings-range" aria-label={tr("Sound volume", "Volume dos sons")} type="range" min="0" max="100" step="5" disabled={!preferences.soundEnabled} value={preferences.soundVolume} onchange={(event) => void updatePreference("soundVolume", Number(event.currentTarget.value))} />
            </label>
            <label class="workspace-setting-row" data-tooltip={tr("Keep Lume above fullscreen apps", "Mantém o Lume sobre apps em tela cheia")}>
              <span><strong>{tr("Show over fullscreen", "Sobre tela cheia")}</strong></span>
              <input class="workspace-switch" type="checkbox" checked={preferences.showOverFullscreen} disabled={settingsSaving} onchange={(event) => void updatePreference("showOverFullscreen", event.currentTarget.checked)} />
            </label>
            <label class="workspace-setting-row" data-tooltip={tr("Primary display by default", "Tela principal por padrão")}>
              <span><strong>{tr("Monitor", "Monitor")}</strong></span>
              <LumeSelect ariaLabel={tr("Monitor", "Monitor")} value={preferences.monitorId ?? ""} options={[{ value: "", label: tr("Primary", "Principal") }, ...monitors.map((monitor) => ({ value: monitor.id, label: monitor.label }))]} minWidth={128} onValueChange={(value) => void updatePreference("monitorId", value || undefined)} />
            </label>
            <label class="workspace-setting-row" data-tooltip={tr("Local retention", "Retenção local")}>
              <span><strong>{tr("History", "Histórico")}</strong></span>
              <LumeSelect ariaLabel={tr("History retention", "Retenção do histórico")} value={String(preferences.historyRetentionDays)} options={[{ value: "7", label: tr("7 days", "7 dias") }, { value: "30", label: tr("30 days", "30 dias") }, { value: "90", label: tr("90 days", "90 dias") }]} minWidth={112} onValueChange={(value) => void updatePreference("historyRetentionDays", Number(value))} />
            </label>
            <label class="workspace-setting-row" data-tooltip={tr("Default launch target", "Destino padrão")}>
              <span><strong>{tr("Open sessions in", "Abrir sessões em")}</strong></span>
              <LumeSelect ariaLabel={tr("Session destination", "Destino das sessões")} value={preferences.launchTarget} options={[{ value: "auto", label: "Auto" }, { value: "terminal", label: "Terminal" }, { value: "vscode", label: "VS Code" }]} minWidth={112} onValueChange={(value) => void updatePreference("launchTarget", value as Preferences["launchTarget"])} />
            </label>
            {/if}
            </div>
          </details>

          <details class="settings-group" open={settingsSections.agents} use:animatedDisclosure={(open) => setSettingsSectionOpen("agents", open)}>
            <summary>{tr("Agents", "Agentes")}</summary>
            <div class="settings-section-content">
            {#if settingsSections.agents}
            {#if settingsLoadingSections.includes("agents")}<p class="settings-loading-hint" role="status">{tr("Loading agent integrations…", "Carregando integrações de agentes…")}</p>{/if}
            {#each [
              { label: "", items: integrations.filter((integration) => integration.canLaunch) },
              { label: tr("Monitoring only", "Somente monitoramento"), items: integrations.filter((integration) => !integration.canLaunch) },
            ] as group}
              {#if group.label}<small class="group-label">{group.label}</small>{/if}
              {#each group.items as integration (integration.kind)}
                <div class="integration-row">
                  <span class="integration-icon"><BrandIcon name={integrationAgentKind(integration.kind)} size={18} /></span>
                  <span><strong>{integration.label}</strong><small>{integration.detail}</small></span>
                  <button type="button" disabled={diagnosingIntegration !== null} onclick={() => void runIntegrationDiagnostic(integration)}>{diagnosingIntegration === integration.kind ? "…" : tr("Test", "Testar")}</button>
                  {#if integration.canConfigure}
                    <button class:active={integration.configured} type="button" disabled={!integration.installed || configuringIntegration !== null} onclick={() => void toggleIntegration(integration)}>{configuringIntegration === integration.kind ? "…" : integration.configured ? tr("Connected", "Conectado") : tr("Connect", "Conectar")}</button>
                  {/if}
                </div>
                {#if integrationDiagnostics[integration.kind]}
                  <div class="diagnostic-list">
                    {#each integrationDiagnostics[integration.kind]?.checks ?? [] as item (item.id)}
                      <span class="diagnostic-{item.status}"><i></i><b>{item.label}</b><small>{item.detail}</small></span>
                    {/each}
                  </div>
                {/if}
              {/each}
            {/each}
            {/if}
            </div>
          </details>

          <details class="settings-group" open={settingsSections.companions} use:animatedDisclosure={(open) => setSettingsSectionOpen("companions", open)}>
            <summary>{tr("Companions", "Companions")}</summary>
            <div class="settings-section-content">
            {#if settingsSections.companions}
            {#if settingsLoadingSections.includes("companions")}<p class="settings-loading-hint" role="status">{tr("Loading companions…", "Carregando companions…")}</p>{/if}
            <div class="integration-row">
              <span class="integration-icon"><BrandIcon name="vscode" size={18} /></span>
              <span><strong>VS Code Companion</strong><small>{vscodeStatus.detail}</small><small>{tr("Does not control Antigravity IDE or Gemini Code Assist chats.", "Não controla chats da IDE Antigravity nem do Gemini Code Assist.")}</small></span>
              <button class:active={vscodeStatus.configured} type="button" disabled={!vscodeStatus.installed || configuringVscode} onclick={() => void toggleVscode()}>{configuringVscode ? "…" : vscodeStatus.configured ? tr("Uninstall", "Desinstalar") : tr("Install", "Instalar")}</button>
            </div>
            <div class="integration-row">
              <span class="integration-icon"><BrandIcon name="browsers" size={18} /></span>
              <span><strong>Chrome, Edge & Brave</strong><small>{tr("Browser companion extension", "Extensão companion do navegador")}</small></span>
              <button type="button" onclick={() => void revealBrowserCompanion()}>{tr("Open", "Abrir")}</button>
            </div>
            {/if}
            </div>
          </details>

          <details class="settings-group" data-external-detectors open={settingsSections.externalDetectors} use:animatedDisclosure={(open) => setSettingsSectionOpen("externalDetectors", open)}>
            <summary>{tr("External detectors", "Detectores externos")}</summary>
            <div class="settings-section-content">
            {#if settingsSections.externalDetectors}
            {#if settingsLoadingSections.includes("externalDetectors")}<p class="settings-loading-hint" role="status">{tr("Loading external detectors…", "Carregando detectores externos…")}</p>{/if}
            {#each externalPlugins as plugin (plugin.id)}
              <div class="integration-row">
                <span class="integration-icon"><BrandIcon name="unknown" size={17} /></span>
                <span><strong>{plugin.name}</strong><small>{plugin.executable}</small></span>
                <button type="button" onclick={() => void uninstallExternalPlugin(plugin.id)}>{tr("Remove", "Remover")}</button>
              </div>
            {:else}
              <p class="settings-hint">{tr("Install a JSON manifest to monitor another CLI. Detectors do not launch agents or change their tools.", "Instale um manifesto JSON para monitorar outra CLI. Detectores não iniciam agentes nem alteram suas ferramentas.")}</p>
            {/each}
            <div class="inline-actions">
              <button type="button" disabled={installingPlugin} onclick={() => void addExternalPlugin()}>{installingPlugin ? "…" : tr("Install manifest", "Instalar manifesto")}</button>
              <button type="button" onclick={() => void revealPluginDirectory()}>{tr("Open detector folder", "Abrir pasta dos detectores")}</button>
            </div>
            {/if}
            </div>
          </details>

          <details class="settings-group compact-settings" open={settingsSections.shortcuts} use:animatedDisclosure={(open) => setSettingsSectionOpen("shortcuts", open)}>
            <summary>{tr("Keyboard shortcuts", "Atalhos de teclado")}</summary>
            <div class="settings-section-content">
            {#if settingsSections.shortcuts}
            {#each [
              ["openShortcut", tr("Open Lume", "Abrir o Lume")],
              ["globalShortcut", tr("Command palette", "Paleta de comandos")],
              ["newSessionShortcut", tr("New session", "Nova sessão")],
              ["whiteboardShortcut", tr("Terminals", "Terminais")],
              ["workspaceShortcut", "Workspace"],
            ] as shortcut}
              <div class="workspace-setting-row">
                <span><strong>{shortcut[1]}</strong></span>
                <button class="shortcut-button" type="button" onclick={() => void openShortcutEditor(shortcut[0] as Exclude<typeof shortcutEditorKey, null>)}>{preferences[shortcut[0] as Exclude<typeof shortcutEditorKey, null>]}</button>
              </div>
            {/each}
            {/if}
            </div>
          </details>

          <details class="settings-group compact-settings" open={settingsSections.projectProfiles} use:animatedDisclosure={(open) => setSettingsSectionOpen("projectProfiles", open)}>
            <summary>{tr("Project profiles", "Perfis por projeto")}</summary>
            <div class="settings-section-content">
            {#if settingsSections.projectProfiles}
            {#if settingsLoadingSections.includes("projectProfiles")}<p class="settings-loading-hint" role="status">{tr("Loading project options…", "Carregando opções do projeto…")}</p>{/if}
            {#if detectedProjects.length}
              <label class="workspace-setting-row">
                <span><strong>{tr("Project", "Projeto")}</strong></span>
                <LumeSelect ariaLabel={tr("Project", "Projeto")} value={selectedProfileKey ?? ""} options={detectedProjects.map((project) => ({ value: project.key, label: project.label }))} minWidth={150} onValueChange={(value) => selectedProfileKey = value} />
              </label>
              <label class="workspace-setting-row">
                <span><strong>{tr("Project sounds", "Sons do projeto")}</strong></span>
                <input class="workspace-switch" type="checkbox" checked={selectedProjectProfile?.soundEnabled ?? true} onchange={(event) => void updateProjectProfile({ soundEnabled: event.currentTarget.checked })} />
              </label>
              <label class="workspace-setting-row">
                <span><strong>{tr("Session destination", "Destino das sessões")}</strong></span>
                <LumeSelect ariaLabel={tr("Session destination", "Destino das sessões")} value={selectedProjectProfile?.launchTarget ?? ""} options={[{ value: "", label: tr("Global", "Global") }, { value: "auto", label: "Auto" }, { value: "terminal", label: "Terminal" }, { value: "vscode", label: "VS Code" }]} minWidth={112} onValueChange={(value) => void updateProjectProfile({ launchTarget: (value || undefined) as Preferences["launchTarget"] | undefined })} />
              </label>
              <label class="workspace-setting-row">
                <span><strong>{tr("Profile monitor", "Monitor do perfil")}</strong></span>
                <LumeSelect ariaLabel={tr("Profile monitor", "Monitor do perfil")} value={selectedProjectProfile?.monitorId ?? ""} options={[{ value: "", label: tr("Global", "Global") }, ...monitors.map((monitor) => ({ value: monitor.id, label: monitor.label }))]} minWidth={128} onValueChange={(value) => void updateProjectProfile({ monitorId: value || undefined })} />
              </label>
              <label class="workspace-setting-row">
                <span><strong>{tr("Permission preset", "Preset de permissão")}</strong></span>
                <LumeSelect ariaLabel={tr("Permission preset", "Preset de permissão")} value={selectedProjectProfile?.permissionMode ?? ""} options={[{ value: "", label: tr("Agent default", "Padrão do agente") }, { value: "plan", label: "Plan" }, { value: "read_only", label: tr("Read only", "Somente leitura") }, { value: "workspace_write", label: "Workspace write" }, { value: "full_access", label: tr("Full access", "Acesso total") }]} minWidth={135} onValueChange={(value) => void updateProjectProfile({ permissionMode: (value || undefined) as Preferences["projectProfiles"][string]["permissionMode"] })} />
              </label>
              <label class="workspace-setting-row">
                <span><strong>{tr("Approval policy", "Política de aprovação")}</strong></span>
                <LumeSelect ariaLabel={tr("Approval policy", "Política de aprovação")} value={selectedProjectProfile?.approvalPolicy ?? ""} options={[{ value: "", label: tr("Agent default", "Padrão do agente") }, { value: "untrusted", label: "Untrusted" }, { value: "on-request", label: "On request" }, { value: "never", label: "Never" }]} minWidth={130} onValueChange={(value) => void updateProjectProfile({ approvalPolicy: (value || undefined) as Preferences["projectProfiles"][string]["approvalPolicy"] })} />
              </label>
              <label class="workspace-setting-row">
                <span><strong>Whiteboard</strong></span>
                <LumeSelect ariaLabel="Whiteboard" value={selectedProjectProfile?.whiteboardLayoutId ?? ""} options={[{ value: "", label: tr("No layout", "Sem layout") }, ...preferences.whiteboardLayouts.map((layout) => ({ value: layout.id, label: layout.name }))]} minWidth={130} onValueChange={(value) => void updateProjectProfile({ whiteboardLayoutId: value || undefined })} />
              </label>
              <div class="preferred-agents">
                <strong>{tr("Preferred agents", "Agentes preferidos")}</strong>
                <span>{#each integrations.filter((integration) => integration.canLaunch) as integration (integration.kind)}<button class:active={(selectedProjectProfile?.preferredAgents ?? []).includes(integrationAgentKind(integration.kind))} type="button" onclick={() => void togglePreferredAgent(integrationAgentKind(integration.kind))}><BrandIcon name={integrationAgentKind(integration.kind)} size={13} />{integration.label}</button>{/each}</span>
              </div>
              <div class="inline-actions"><button type="button" onclick={() => void captureProfilePosition()}>{tr("Use current position", "Usar posição atual")}</button><button class="primary" type="button" onclick={() => void applyProjectProfile()}>{tr("Apply profile", "Aplicar perfil")}</button></div>
            {:else}
              <p class="settings-empty">{tr("Profiles appear after a project is detected.", "Os perfis aparecem quando um projeto é detectado.")}</p>
            {/if}
            {/if}
            </div>
          </details>

          <details class="settings-group" data-remote-nodes-section open={settingsSections.remoteComputers} use:animatedDisclosure={(open) => setSettingsSectionOpen("remoteComputers", open)}>
            <summary>{tr("Remote computers", "Computadores remotos")}</summary>
            <div class="settings-section-content">
              {#if settingsSections.remoteComputers}<RemoteComputers language={preferences.language} dark={darkMode} />{/if}
            </div>
          </details>

          <details class="settings-group compact-settings" open={settingsSections.mobileAccess} use:animatedDisclosure={(open) => setSettingsSectionOpen("mobileAccess", open)}>
            <summary>{tr("Mobile access", "Acesso mobile")}</summary>
            <div class="settings-section-content">
            {#if settingsSections.mobileAccess}
            {#if settingsLoadingSections.includes("mobileAccess")}<p class="settings-loading-hint" role="status">{tr("Checking mobile access…", "Verificando acesso mobile…")}</p>{/if}
            <label class="workspace-setting-row">
              <span><strong>{tr("Local network access", "Acesso na rede local")}</strong><small>{mobileStatus?.address || tr("Paired devices only", "Apenas dispositivos pareados")}</small></span>
              <input class="workspace-switch" type="checkbox" checked={mobileStatus?.networkReachable ?? false} disabled={mobileBusy} onchange={() => void toggleMobileAccess()} />
            </label>
            {#if mobileStatus?.networkReachable}
              <div class="mobile-pairing-action"><button type="button" onclick={() => void createMobilePairing()}>{pairingOffer ? tr("New QR code", "Novo QR Code") : tr("Pair device", "Parear dispositivo")}</button></div>
              {#if pairingQr && pairingOffer}<div class="pairing-qr"><img src={pairingQr} alt={tr("Pairing QR code", "QR Code de pareamento")} /><span><strong>{pairingOffer.code}</strong><small>{tr("Same local network", "Mesma rede local")}</small></span></div>{/if}
            {/if}
            {#each pairedDevices as device (device.id)}
              <div class="device-card">
                <header><span><strong>{device.name}</strong><small>{device.lastSeenAt ? new Date(device.lastSeenAt).toLocaleString() : tr("Not used yet", "Ainda não utilizado")}</small></span><button type="button" onclick={() => void removePairedDevice(device.id)}>{tr("Revoke", "Revogar")}</button></header>
                {#each [["prompt", tr("Send prompts", "Enviar prompts")], ["approve", tr("Manage approvals", "Gerenciar aprovações")], ["terminate", tr("Stop agents", "Encerrar agentes")]] as permission}
                  <label class="workspace-setting-row"><span><strong>{permission[1]}</strong></span><input class="workspace-switch" type="checkbox" checked={device.scopes.includes(permission[0] as MobileScope)} onchange={() => void toggleDeviceScope(device, permission[0] as MobileScope)} /></label>
                {/each}
              </div>
            {/each}
            {/if}
            </div>
          </details>

          <details class="settings-group" open={settingsSections.about} use:animatedDisclosure={(open) => setSettingsSectionOpen("about", open)}>
            <summary>{tr("About Lume", "Sobre o Lume")}</summary>
            <div class="settings-section-content">
            {#if settingsSections.about}
            {#if settingsLoadingSections.includes("about")}<p class="settings-loading-hint" role="status">{tr("Loading app information…", "Carregando informações do aplicativo…")}</p>{/if}
            <section class="about-settings">
              <div class="about-update-card" data-update-card>
                <div class="about-update-header">
                  <LumeLogo size={38} />
                  <div class="about-update-identity">
                    <strong>Lume</strong>
                    <span><small>{tr("Current version", "Versão atual")}</small><b>{appVersion}</b></span>
                  </div>
                  {#if updateState === "available"}
                    <button class="about-update-action primary" type="button" onclick={() => void installUpdate()}>
                      {tr("Install", "Instalar")} {availableVersion}
                    </button>
                  {:else}
                    <button class="about-update-action" type="button" disabled={["checking", "downloading", "ready"].includes(updateState)} onclick={() => void checkForUpdates()}>
                      {updateState === "checking"
                        ? tr("Checking…", "Verificando…")
                        : updateState === "downloading"
                          ? tr("Downloading…", "Baixando…")
                          : updateState === "ready"
                            ? tr("Restarting…", "Reiniciando…")
                            : updateState === "error"
                              ? tr("Try again", "Tentar novamente")
                              : tr("Check updates", "Verificar atualizações")}
                    </button>
                  {/if}
                </div>
                {#if updateDetail}<p role="status">{updateDetail}</p>{/if}
                {#if updateState === "downloading" || updateState === "ready"}
                  <div class="about-update-progress-copy">
                    <span>{updateState === "ready" ? tr("Ready to restart", "Pronto para reiniciar") : tr("Download progress", "Progresso do download")}</span>
                    <b>{updateProgress === null ? "" : `${updateProgress}%`}</b>
                  </div>
                  <div
                    class:indeterminate={updateProgress === null}
                    class="about-update-progress"
                    role="progressbar"
                    aria-label={tr("Update download progress", "Progresso do download da atualização")}
                    aria-valuemin="0"
                    aria-valuemax="100"
                    aria-valuenow={updateProgress ?? undefined}
                    aria-valuetext={updateProgress === null ? tr("Downloading", "Baixando") : `${updateProgress}%`}
                  >
                    <span style:width={`${updateProgress ?? 24}%`}></span>
                  </div>
                {/if}
              </div>
            </section>
            {/if}
            </div>
          </details>

          <details class="settings-group reset-group" open={settingsSections.reset} use:animatedDisclosure={(open) => setSettingsSectionOpen("reset", open)}>
            <summary>{tr("Reset", "Redefinir")}</summary>
            <div class="settings-section-content">
            {#if settingsSections.reset}
            <div class="reset-control">
              {#if resetConfirming}<span>{tr("Restore every Lume setting?", "Restaurar todos os ajustes do Lume?")}</span><button type="button" onclick={() => resetConfirming = false}>{tr("Cancel", "Cancelar")}</button>{/if}
              <button class:danger={resetConfirming} type="button" onclick={() => void resetSettings()}>{resetConfirming ? tr("Confirm reset", "Confirmar redefinição") : tr("Reset settings", "Redefinir ajustes")}</button>
            </div>
            {/if}
            </div>
          </details>

        </div>
      </aside>
    </div>
  {/if}

  {#if shortcutEditorKey}
    <div class="shortcut-scrim" role="presentation">
      <div class="shortcut-dialog" data-shortcut-capture role="dialog" aria-modal="true" aria-label={tr("Shortcut editor", "Editor de atalho")} tabindex="0" onkeydown={captureShortcut}>
        <strong>{tr("Press a new shortcut", "Pressione um novo atalho")}</strong>
        <kbd>{shortcutDraft || "…"}</kbd>
        <span><button type="button" onclick={() => shortcutEditorKey = null}>{tr("Cancel", "Cancelar")}</button><button class="primary" type="button" onclick={() => void saveShortcut()}>{tr("Save", "Salvar")}</button></span>
      </div>
    </div>
  {/if}

  {#if antigravityHookConfirmation}
    <div class="shortcut-scrim" role="presentation">
      <div class="integration-warning-dialog" role="alertdialog" aria-modal="true" aria-labelledby="antigravity-hook-title" aria-describedby="antigravity-hook-description">
        <span class="integration-warning-icon"><BrandIcon name="antigravity" size={22} /></span>
        <strong id="antigravity-hook-title">{tr("Connect Antigravity CLI?", "Conectar a CLI Antigravity?")}</strong>
        <p id="antigravity-hook-description">{tr("Lume installs monitoring hooks only for Antigravity CLI. Tool approvals follow the CLI's native policy; Lume does not auto-approve them. In headless sessions, tools that need an interactive approval are denied unless you have granted a specific rule in Antigravity. This does not configure Antigravity IDE or Gemini Code Assist.", "O Lume instala hooks de monitoramento somente na CLI Antigravity. As aprovações seguem a política nativa da CLI; o Lume não aprova ferramentas automaticamente. Em sessões sem terminal, ações que exigem confirmação são bloqueadas, salvo se você tiver uma regra específica no Antigravity. Isso não configura a IDE Antigravity nem o Gemini Code Assist.")}</p>
        <span>
          <button type="button" onclick={() => antigravityHookConfirmation = false}>{tr("Cancel", "Cancelar")}</button>
          <button class="primary" type="button" onclick={() => void confirmAntigravityHooks()}>{tr("Enable CLI hooks", "Ativar hooks da CLI")}</button>
        </span>
      </div>
    </div>
  {/if}

  <section class:inspector-open={inspectorOpen && !boardOpen} class:review-open={reviewOpen && !boardOpen} class:maximized={Boolean(maximizedSession) && !boardOpen} class:review-wide={reviewWide && reviewOpen && !boardOpen} class="workspace-stage">
    <section
      bind:this={workbenchElement}
      class:split={Boolean(secondary) && !maximizedSession}
      class:three-pane={Boolean(tertiary) && !maximizedSession}
      class:resizing={resizingDivider !== null}
      class:review-focus={Boolean(reviewFocusSession)}
      inert={reviewWide && reviewOpen && !boardOpen}
      class:drag-active={draggingSessionId !== null}
      class:header-relocating={headerRelocatingSessionId !== null}
      class="workbench"
      aria-label={tr("Workspace layout", "Layout do Workspace")}
      style:grid-template-columns={workbenchColumns()}
      ondragover={trackWorkspaceDrop}
      ondragleave={leaveWorkspaceDrop}
      ondrop={dropSessionInWorkspace}
    >
    <div class:covered={boardOpen} class="board-chat-layer" inert={boardOpen} aria-hidden={boardOpen}>
    {#if workspaceBackgroundImage}
      <div
        class="workspace-wallpaper"
        style:background-image={`url("${workspaceBackgroundImage}")`}
        style:opacity={`${workspaceBackgroundImageOpacity / 100}`}
        aria-hidden="true"
      ></div>
    {/if}
    {#if maximizedSession}
      {#key maximizedSession.id}
        <WorkspaceSessionPane
          session={maximizedSession}
          externalWriterConflict={externalWriterConflicts[maximizedSession.id] ?? null}
          {language}
          {streamMessages}
          focused
          maximized
          onFocus={() => focusPane(maximizedSession.id)}
          onFork={(threadId) => openForkedCodexSession(threadId, maximizedSession)}
          onNewConversation={() => startNewConversation(maximizedSession)}
          onOpenReview={(path) => openReview(path, maximizedSession.id)}
          onOpenRepository={() => openRepository(maximizedSession.id)}
          onToggleMaximize={() => togglePaneMaximize(maximizedSession.id)}
          onDismissExternalWriterConflict={dismissExternalWriterConflict}
          onResolveExternalWriterConflict={(action) => resolveExternalWriterConflict(maximizedSession, externalWriterConflicts[maximizedSession.id]!, action)}
        />
      {/key}
    {:else if primary}
      {#key primary.id}
        <WorkspaceSessionPane
          session={primary}
          externalWriterConflict={externalWriterConflicts[primary.id] ?? null}
          {language}
          {streamMessages}
          focused={focusedPaneId === primary.id}
          onFocus={() => focusPane(primary.id)}
          onFork={(threadId) => openForkedCodexSession(threadId, primary)}
          onNewConversation={() => startNewConversation(primary)}
          onOpenReview={(path) => openReview(path, primary.id)}
          onOpenRepository={() => openRepository(primary.id)}
          onToggleMaximize={() => togglePaneMaximize(primary.id)}
          onDismissExternalWriterConflict={dismissExternalWriterConflict}
          onResolveExternalWriterConflict={(action) => resolveExternalWriterConflict(primary, externalWriterConflicts[primary.id]!, action)}
        />
      {/key}
      {#if secondary}
        <!-- svelte-ignore a11y_no_noninteractive_tabindex (ARIA separator becomes interactive when focusable and exposes aria-valuenow) -->
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions (pointer and keyboard input resize the separator) -->
        <div
          class="pane-divider"
          role="separator"
          tabindex="0"
          aria-label={tr("Resize chat panes", "Redimensionar painéis de chat")}
          aria-orientation="vertical"
          aria-valuemin="25"
          aria-valuemax="75"
          aria-valuenow={Math.round(splitRatio * 100)}
          title={tr("Drag to resize", "Arraste para redimensionar")}
          onpointerdown={(event) => beginSplitResize(event, 0)}
          onpointermove={moveSplitResize}
          onpointerup={endSplitResize}
          onpointercancel={endSplitResize}
          onkeydown={(event) => handleSplitKeydown(event, 0)}
        ></div>
        {#key secondary.id}
        <WorkspaceSessionPane
          session={secondary}
            externalWriterConflict={externalWriterConflicts[secondary.id] ?? null}
            {language}
            {streamMessages}
            closable
            focused={focusedPaneId === secondary.id}
            onClose={() => closeSidePane(secondary.id)}
            onFocus={() => focusPane(secondary.id)}
            onFork={(threadId) => openForkedCodexSession(threadId, secondary)}
          onNewConversation={() => startNewConversation(secondary)}
          onOpenReview={(path) => openReview(path, secondary.id)}
          onOpenRepository={() => openRepository(secondary.id)}
            onToggleMaximize={() => togglePaneMaximize(secondary.id)}
            onDismissExternalWriterConflict={dismissExternalWriterConflict}
            onResolveExternalWriterConflict={(action) => resolveExternalWriterConflict(secondary, externalWriterConflicts[secondary.id]!, action)}
          />
        {/key}
        {#if tertiary}
          <!-- svelte-ignore a11y_no_noninteractive_tabindex (ARIA separator becomes interactive when focusable and exposes aria-valuenow) -->
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions (pointer and keyboard input resize the separator) -->
          <div
            class="pane-divider"
            role="separator"
            tabindex="0"
            aria-label={tr("Resize third chat pane", "Redimensionar terceiro painel de chat")}
            aria-orientation="vertical"
            aria-valuemin="22"
            aria-valuemax="50"
            aria-valuenow={Math.round(tertiaryRatio * 100)}
            title={tr("Drag to resize", "Arraste para redimensionar")}
            onpointerdown={(event) => beginSplitResize(event, 1)}
            onpointermove={moveSplitResize}
            onpointerup={endSplitResize}
            onpointercancel={endSplitResize}
            onkeydown={(event) => handleSplitKeydown(event, 1)}
          ></div>
          {#key tertiary.id}
            <WorkspaceSessionPane
              session={tertiary}
              externalWriterConflict={externalWriterConflicts[tertiary.id] ?? null}
              {language}
              {streamMessages}
              closable
              focused={focusedPaneId === tertiary.id}
              onClose={() => closeSidePane(tertiary.id)}
              onFocus={() => focusPane(tertiary.id)}
              onFork={(threadId) => openForkedCodexSession(threadId, tertiary)}
          onNewConversation={() => startNewConversation(tertiary)}
              onOpenReview={(path) => openReview(path, tertiary.id)}
              onOpenRepository={() => openRepository(tertiary.id)}
              onToggleMaximize={() => togglePaneMaximize(tertiary.id)}
              onDismissExternalWriterConflict={dismissExternalWriterConflict}
              onResolveExternalWriterConflict={(action) => resolveExternalWriterConflict(tertiary, externalWriterConflicts[tertiary.id]!, action)}
            />
          {/key}
        {/if}
      {/if}
    {:else if loading}
      <div class="workspace-empty workspace-loading" role="status">
        <span class="empty-mark"><BrandIcon name="lume" size={34} /></span>
        <strong>{tr("Loading your workspace…", "Carregando seu workspace…")}</strong>
        <i class="workspace-loading-bar" aria-hidden="true"></i>
      </div>
    {:else}
      <div class="workspace-empty">
        <span class="empty-mark"><BrandIcon name="lume" size={34} /></span>
        <strong>{tr("Your agents, in one workspace", "Seus agentes, em um workspace")}</strong>
        <p>{tr("Open an agent to follow its conversation and work side by side.", "Abra um agente para acompanhar a conversa e trabalhar lado a lado.")}</p>
      </div>
    {/if}
      {#if error && !orderedSessions.length}<p class="workspace-error">{error}</p>{/if}
      {#if !boardOpen && draggingSessionId && workspaceDropIntent}
        <div
          class="layout-drop-preview {workspaceDropIntent.kind}"
          style:left={`${workspaceDropIntent.left}%`}
          style:width={`${workspaceDropIntent.width}%`}
          aria-hidden="true"
        >
          <span>
            <LumeIcon name={workspaceDropIntent.kind === "insert" ? "plus" : workspaceDropIntent.kind === "move" ? "layout" : "split"} size={15} />
            {workspaceDropIntent.kind === "insert"
              ? tr("Add pane", "Adicionar painel")
              : workspaceDropIntent.kind === "move"
                ? tr("Swap positions", "Trocar posições")
                : tr("Replace pane", "Substituir painel")}
          </span>
        </div>
      {/if}
    </div>
    {#if boardMounted}
      <WorkflowBoard
        {sessions}
        {preferences}
        {language}
        active={boardOpen}
        keyboardEnabled={!settingsOpen && !launcherOpen && !sessionContextMenu && !headerControl && !searchOpen}
        onSaveGroup={saveWorkflowBoardGroup}
        onDeleteGroup={deleteWorkflowBoardGroup}
        onOpenChat={(session) => { showWorkflowBoard(false); selectSession(session); }}
        onClose={() => showWorkflowBoard(false)}
        onFinishSidebarDrag={finishSidebarSessionDrag}
        {draggingSessionId}
        onSidebarDragOverBoard={(over) => dragOverBoard = over}
      />
    {/if}
    </section>
    <div class:open={inspectorOpen && !boardOpen} class="inspector-shell" aria-hidden={!inspectorOpen || boardOpen} inert={!inspectorOpen || boardOpen}>
      {#if inspectorOpen}
        <div class="inspector-content" in:fade={{ duration: motionDuration(140) }} out:fade={{ duration: motionDuration(100) }}>
          <WorkspaceInspector session={focusedSession} {language} {loading} bind:section={inspectorSection} onClose={toggleInspector} onOpenReview={openReview} />
        </div>
      {/if}
    </div>
    <div class:open={reviewOpen && !boardOpen} class="review-shell" aria-hidden={!reviewOpen || boardOpen} inert={!reviewOpen || boardOpen}>
      {#if reviewOpen && focusedSession}
        <div class="review-content" in:fly={{ x: 22, duration: motionDuration(220), easing: cubicOut }} out:fly={{ x: 16, duration: motionDuration(145), easing: cubicOut }}>
          <WorkspaceReviewCenter session={focusedSession} {language} initialPath={reviewInitialPath} wide={reviewWide} onToggleWide={() => (reviewWide = !reviewWide)} onClose={closeReview} />
        </div>
      {/if}
    </div>
  </section>
  {#if showDragPreview && draggingSessionId && !dragOverBoard}
    {@const dragSession = sessions.find((session) => session.id === draggingSessionId)}
    {#if dragSession}
      <div class="session-drag-preview" bind:this={dragPreviewElement} aria-hidden="true">
        <ThreadAvatar seed={dragSession.nativeSessionId || dragSession.sessionName || dragSession.id} label={sessionName(dragSession)} size={34} />
        <span><strong>{sessionName(dragSession)}</strong><small><BrandIcon name={dragSession.agent} size={11} />{dragSession.agentLabel} · {sessionSubtitle(dragSession)}</small></span>
        <LumeIcon name="layout" size={15} />
      </div>
    {/if}
  {/if}
  {#if connectionAgent}
    <AgentConnectionDialog agent={connectionAgent} message={connectionMessage} {language} onClose={() => { connectionAgent = null; }} />
  {/if}
  {#if automationRequired}
    <MacosAutomationDialog {language} onClose={() => { automationRequired = false; }} />
  {/if}
</main>

<style>
  .workspace {
    --workspace-background-color: var(--lume-canvas-light);
    --workspace-background-opacity: 96%;
    --workspace-bg: var(--lume-canvas-light);
    --workspace-sidebar: color-mix(in srgb, var(--lume-sidebar-light) 96%, transparent);
    --workspace-pane: color-mix(in srgb, var(--lume-surface-light) 96%, transparent);
    --workspace-chat-background: color-mix(in srgb, var(--workspace-background-color) var(--workspace-background-opacity), transparent);
    --workspace-raised: var(--lume-raised-light);
    --workspace-line: var(--lume-line-light);
    --workspace-strong: var(--lume-ink-strong-light);
    --workspace-text: var(--lume-ink-light);
    --workspace-muted: var(--lume-ink-muted-light);
    --workspace-faint: var(--lume-ink-faint-light);
    --workspace-accent: var(--lume-accent-strong);
    --workspace-accent-soft: var(--lume-accent-soft-light);
    --workspace-subtle: var(--lume-subtle-light);
    --workspace-message: var(--lume-message-light);
    --workspace-user: var(--lume-user-light);
    --workspace-user-line: var(--lume-user-line-light);
    --workspace-code: var(--lume-code-light);
    --workspace-scroll-thumb: var(--lume-scroll-light);
    --file-monochrome-filter: grayscale(1) brightness(0) contrast(.82);
    --file-monochrome-opacity: .78;
    --chat-small-font-size: 10px;
    --chat-tiny-font-size: 8px;
    width: 100%;
    max-width: 100vw;
    min-width: 0;
    height: 100vh;
    display: grid;
    grid-template-columns: 256px minmax(0, 1fr);
    grid-template-rows: minmax(0, 1fr);
    overflow: hidden;
    color: var(--workspace-text);
    background: var(--workspace-bg);
    font-family: var(--lume-font-ui, "Segoe UI Variable", "SF Pro Text", ui-sans-serif, system-ui, sans-serif);
    accent-color: var(--workspace-accent);
    transition: grid-template-columns 240ms cubic-bezier(.16, 1, .3, 1);
  }
  .workspace.dark {
    --workspace-background-color: var(--lume-canvas-dark);
    --workspace-bg: var(--lume-canvas-dark);
    --workspace-sidebar: color-mix(in srgb, var(--lume-sidebar-dark) 96%, transparent);
    --workspace-pane: color-mix(in srgb, var(--lume-surface-dark) 96%, transparent);
    --workspace-raised: var(--lume-raised-dark);
    --workspace-line: var(--lume-line-dark);
    --workspace-strong: var(--lume-ink-strong-dark);
    --workspace-text: var(--lume-ink-dark);
    --workspace-muted: var(--lume-ink-muted-dark);
    --workspace-faint: var(--lume-ink-faint-dark);
    --workspace-accent: var(--lume-accent);
    --workspace-accent-soft: var(--lume-accent-soft-dark);
    --workspace-subtle: var(--lume-subtle-dark);
    --workspace-message: var(--lume-message-dark);
    --workspace-user: var(--lume-user-dark);
    --workspace-user-line: var(--lume-user-line-dark);
    --workspace-code: var(--lume-code-dark);
    --workspace-scroll-thumb: var(--lume-scroll-dark);
    --file-monochrome-filter: grayscale(1) brightness(0) invert(.94) contrast(.9);
    --file-monochrome-opacity: .88;
  }
  ::selection { color: var(--workspace-strong); background: var(--workspace-accent-soft); }
  button, input { font: inherit; }
  button:focus-visible, input:focus-visible { outline: 2px solid color-mix(in srgb, var(--workspace-accent) 70%, white); outline-offset: 2px; }
  .window-resize-edge { position: fixed; z-index: 60; margin: 0; padding: 0; border: 0; outline: 0; background: transparent; }
  .edge-north, .edge-south { right: 6px; left: 6px; height: 5px; cursor: ns-resize; }.edge-north { top: 0; }.edge-south { bottom: 0; }
  .edge-east, .edge-west { top: 6px; bottom: 6px; width: 5px; cursor: ew-resize; }.edge-east { right: 0; }.edge-west { left: 0; }
  .edge-northeast, .edge-northwest, .edge-southeast, .edge-southwest { width: 9px; height: 9px; }
  .edge-northeast { top: 0; right: 0; cursor: nesw-resize; }.edge-northwest { top: 0; left: 0; cursor: nwse-resize; }
  .edge-southeast { right: 0; bottom: 0; cursor: nwse-resize; }.edge-southwest { bottom: 0; left: 0; cursor: nesw-resize; }
  .sidebar { min-width: 0; display: grid; grid-template-rows: auto auto auto minmax(0, 1fr); overflow: hidden; border-right: 1px solid var(--workspace-line); background: var(--workspace-sidebar); }
  .brand-header { position: relative; z-index: 24; min-width: 0; padding: 0 13px 11px; display: grid; gap: 1px; border-bottom: 1px solid var(--workspace-line); user-select: none; }
  .brand-top { min-width: 0; height: 52px; display: flex; align-items: center; gap: 10px; cursor: grab; }
  .brand-top:active { cursor: grabbing; }
  .brand-mark { width: 34px; height: 34px; display: grid; place-items: center; flex: 0 0 auto; color: var(--workspace-accent); }
  .brand-copy { min-width: 0; flex: 1; display: grid; opacity: 1; transform: translateX(0); transition: opacity 130ms ease, transform 180ms cubic-bezier(.16, 1, .3, 1); }
  .sidebar-toggle { width: 29px; height: 29px; padding: 0; display: grid; place-items: center; flex: 0 0 auto; border: 0; border-radius: 8px; color: var(--workspace-muted); background: transparent; cursor: pointer; transition: color 140ms ease, background 140ms ease; }
  .sidebar-toggle:hover, .sidebar-toggle:focus-visible { color: var(--workspace-accent); background: var(--workspace-subtle); }
  .brand-header strong { color: var(--workspace-strong); font-size: 14px; letter-spacing: -.02em; }
  .brand-header small { color: var(--workspace-muted); font-size: 9px; font-weight: 650; letter-spacing: .01em; }
  .compact-mode, .settings-button, .inspector-button { width: 29px; height: 29px; display: grid; place-items: center; flex: 0 0 auto; border: 0; border-radius: 8px; color: var(--workspace-muted); background: transparent; cursor: pointer; transition: color 140ms ease, background 140ms ease; }
  .compact-mode:hover, .settings-button:hover, .settings-button.active { color: var(--workspace-accent); background: var(--workspace-subtle); }
  .inspector-button:hover, .inspector-button.active { color: var(--workspace-accent); background: var(--workspace-subtle); }
  .settings-scrim { position: fixed; z-index: 30; inset: 0; display: flex; justify-content: flex-end; background: rgba(8, 17, 13, .22); }
  .workspace-settings { width: min(430px, calc(100vw - 72px)); height: 100%; display: grid; grid-template-rows: auto minmax(0, 1fr); color: var(--workspace-text); background: var(--workspace-raised); box-shadow: -18px 0 52px rgba(9, 23, 16, .16); }
  .workspace-settings > header { min-height: 72px; padding: 14px 16px 13px 19px; display: flex; align-items: center; gap: 12px; border-bottom: 1px solid var(--workspace-line); }
  .workspace-settings > header > span { min-width: 0; flex: 1; display: grid; gap: 3px; }
  .workspace-settings > header strong { color: var(--workspace-strong); font-size: 14px; letter-spacing: -.02em; }
  .workspace-settings > header small { color: var(--workspace-muted); font-size: 9px; }
  .workspace-settings > header button { width: 31px; height: 31px; display: grid; place-items: center; border: 0; border-radius: 9px; color: var(--workspace-muted); background: transparent; cursor: pointer; }
  .workspace-settings > header button:hover { color: var(--workspace-strong); background: var(--workspace-subtle); }
  .settings-content { min-height: 0; padding: 4px 19px 20px; overflow-y: auto; overscroll-behavior: contain; scrollbar-width: thin; scrollbar-color: var(--workspace-scroll-thumb) transparent; }
  .settings-content::-webkit-scrollbar { width: 7px; }.settings-content::-webkit-scrollbar-track { background: transparent; }.settings-content::-webkit-scrollbar-thumb { border: 2px solid transparent; border-radius: 7px; background: var(--workspace-scroll-thumb); background-clip: content-box; }
  .settings-group { padding: 16px 0; border-bottom: 1px solid var(--workspace-line); }
  details.settings-group { padding: 0; }
  .settings-group > summary { min-height: 48px; display: flex; align-items: center; gap: 8px; color: var(--workspace-strong); font-size: 10px; font-weight: 780; letter-spacing: -.01em; list-style: none; cursor: pointer; }
  .settings-group > summary::-webkit-details-marker { display: none; }
  .settings-group > summary::after { width: 7px; height: 7px; margin-left: auto; border-right: 1.5px solid currentColor; border-bottom: 1.5px solid currentColor; content: ""; opacity: .55; transform: rotate(45deg) translate(-2px, 2px); transition: transform 180ms cubic-bezier(.16, 1, .3, 1); }
  .settings-group[open] > summary::after { transform: rotate(225deg) translate(-1px, 0); }
  .settings-section-content { padding-bottom: 18px; }
  .settings-loading-hint { margin: 3px 0 9px; color: var(--workspace-muted); font-size: 8px; line-height: 1.4; }
  .settings-hint { margin: 2px 0 10px; color: var(--workspace-muted); font-size: 8px; line-height: 1.5; }
  .appearance-options { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 7px; }
  .appearance-option { min-width: 0; padding: 7px; display: grid; gap: 7px; border: 1px solid var(--workspace-line); border-radius: 11px; color: var(--workspace-muted); background: transparent; cursor: pointer; text-align: left; transition: color 140ms ease, border-color 140ms ease, background 140ms ease, transform 180ms cubic-bezier(.16, 1, .3, 1); }
  .appearance-option:hover { color: var(--workspace-strong); border-color: color-mix(in srgb, var(--workspace-accent) 32%, var(--workspace-line)); transform: translateY(-1px); }
  .appearance-option.active { color: var(--workspace-accent); border-color: color-mix(in srgb, var(--workspace-accent) 48%, transparent); background: var(--workspace-accent-soft); }
  .appearance-option > strong { overflow: hidden; font-size: 8px; font-weight: 760; text-overflow: ellipsis; white-space: nowrap; }
  .appearance-preview { height: 43px; padding: 6px; display: grid; grid-template-columns: 13px 1fr; grid-template-rows: 7px 1fr; gap: 4px; overflow: hidden; border: 1px solid rgba(46, 67, 56, .13); border-radius: 7px; background: #f6f5ef; }
  .appearance-preview i { grid-row: 1 / -1; border-radius: 3px; background: #dfe5db; }.appearance-preview b { border-radius: 2px; background: #d6dfd8; }.appearance-preview em { border-radius: 3px; background: #fffefa; }
  .mode-dark .appearance-preview { border-color: rgba(208, 229, 218, .11); background: #101815; }.mode-dark .appearance-preview i { background: #1d2923; }.mode-dark .appearance-preview b { background: #28372f; }.mode-dark .appearance-preview em { background: #17221d; }
  .mode-system .appearance-preview { background: linear-gradient(120deg, #f6f5ef 0 49.5%, #101815 50.5% 100%); }.mode-system .appearance-preview i { background: linear-gradient(120deg, #dfe5db 0 49.5%, #1d2923 50.5% 100%); }.mode-system .appearance-preview b { background: linear-gradient(120deg, #d6dfd8 0 49.5%, #28372f 50.5% 100%); }.mode-system .appearance-preview em { background: linear-gradient(120deg, #fffefa 0 49.5%, #17221d 50.5% 100%); }
  .theme-options { margin-top: 10px; display: grid; grid-template-columns: repeat(5, minmax(0, 1fr)); gap: 5px; }
  .theme-options button { min-width: 0; padding: 7px 4px; display: grid; justify-items: center; gap: 5px; border: 1px solid transparent; border-radius: 9px; color: var(--workspace-muted); background: transparent; font-size: 7px; font-weight: 720; cursor: pointer; }
  .theme-options button:hover, .theme-options button.active { color: var(--workspace-strong); background: var(--workspace-subtle); }
  .theme-options button.active { border-color: color-mix(in srgb, var(--theme-accent) 48%, transparent); }
  .theme-options button span { width: 30px; height: 20px; border: 5px solid var(--theme-surface); border-radius: 7px; background: var(--theme-accent); box-shadow: inset 0 0 0 1px rgba(255, 255, 255, .15); }
  .accent-setting { margin-top: 7px; }
  .wallpaper-setting { position: relative; max-width: 100%; overflow: hidden; }
  .wallpaper-input { position: absolute; width: 1px; height: 1px; overflow: hidden; opacity: 0; pointer-events: none; }
  .wallpaper-preview { width: 38px; height: 29px; flex: 0 0 auto; border-radius: 7px; background-position: center; background-size: cover; box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--workspace-strong) 15%, transparent); }
  .wallpaper-actions { min-width: 0; display: flex; gap: 4px; flex: 0 1 auto; }
  .wallpaper-actions button { width: 28px; min-height: 28px; padding: 0; display: grid; place-items: center; border: 1px solid var(--workspace-line); border-radius: 7px; color: var(--workspace-muted); background: transparent; cursor: pointer; transition: color 140ms ease, border-color 140ms ease, background 140ms ease; }
  .wallpaper-actions button:hover { color: var(--workspace-accent); border-color: color-mix(in srgb, var(--workspace-accent) 38%, var(--workspace-line)); background: var(--workspace-subtle); }
  .compact-settings { display: grid; }
  .workspace-setting-row { width: 100%; max-width: 100%; min-width: 0; min-height: 53px; display: flex; align-items: center; gap: 14px; overflow: hidden; border-bottom: 1px solid color-mix(in srgb, var(--workspace-line) 62%, transparent); }
  .workspace-setting-row:last-child { border-bottom: 0; }
  .workspace-setting-row > span { min-width: 0; flex: 1; display: grid; gap: 2px; }
  .workspace-setting-row > span strong { color: var(--workspace-strong); font-size: 10px; font-weight: 730; }.workspace-setting-row > span small { color: var(--workspace-muted); font-size: 8px; line-height: 1.35; }
  .workspace-switch { position: relative; width: 34px; height: 20px; flex: 0 0 auto; appearance: none; border: 1px solid var(--workspace-line); border-radius: 10px; background: var(--workspace-subtle); cursor: pointer; transition: border-color 140ms ease, background 180ms ease; }
  .workspace-switch::after { position: absolute; top: 3px; left: 3px; width: 12px; height: 12px; border-radius: 50%; background: var(--workspace-muted); content: ""; transition: background 140ms ease, transform 220ms cubic-bezier(.16, 1, .3, 1); }
  .workspace-switch:checked { border-color: transparent; background: var(--workspace-accent); }.workspace-switch:checked::after { background: #f7fbf8; transform: translateX(14px); }.workspace-switch:disabled { cursor: wait; opacity: .58; }
  .settings-range { width: 120px; accent-color: var(--workspace-accent); }
  .group-label { display: block; margin: 8px 0 5px; color: var(--workspace-faint); font-size: 7px; font-weight: 780; letter-spacing: .07em; text-transform: uppercase; }
  .integration-row { min-height: 49px; display: flex; align-items: center; gap: 7px; border-bottom: 1px solid color-mix(in srgb, var(--workspace-line) 65%, transparent); }
  .integration-row > span:nth-child(2) { min-width: 0; display: grid; gap: 2px; flex: 1; }
  .integration-row strong, .preferred-agents > strong, .about-settings strong, .device-card strong { color: var(--workspace-strong); font-size: 9px; }
  .integration-row small, .about-settings small, .device-card small { overflow: hidden; color: var(--workspace-muted); font-size: 7px; text-overflow: ellipsis; white-space: nowrap; }
  .integration-icon { width: 28px; height: 28px; display: grid; place-items: center; flex: 0 0 auto; color: var(--workspace-accent); }
  .integration-row button, .inline-actions button, .mobile-pairing-action button, .about-settings button, .reset-control button { min-height: 27px; padding: 0 8px; border: 1px solid var(--workspace-line); border-radius: 7px; color: var(--workspace-muted); background: transparent; font-size: 7px; font-weight: 720; cursor: pointer; }
  .integration-row button:hover, .integration-row button.active, .inline-actions button:hover, .mobile-pairing-action button:hover, .about-settings button:hover { color: var(--workspace-accent); border-color: color-mix(in srgb, var(--workspace-accent) 35%, var(--workspace-line)); background: var(--workspace-subtle); }
  button.primary { color: var(--workspace-raised); border-color: transparent; background: var(--workspace-accent); }
  .diagnostic-list { padding: 6px 0 8px 35px; display: grid; gap: 5px; }
  .diagnostic-list > span { display: grid; grid-template-columns: 6px auto 1fr; align-items: center; gap: 5px; color: var(--workspace-muted); font-size: 7px; }
  .diagnostic-list i { width: 5px; height: 5px; border-radius: 50%; background: #c38b3e; }.diagnostic-list .diagnostic-ok i { background: #50a677; }.diagnostic-list .diagnostic-error i { background: #bd615e; }
  .diagnostic-list b { color: var(--workspace-text); font-weight: 700; }.diagnostic-list small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .inline-actions { padding-top: 10px; display: flex; justify-content: flex-end; gap: 6px; }
  .theme-label { margin: 6px 0 -2px; display: grid; gap: 2px; }
  .theme-label strong { color: var(--workspace-strong); font-size: 11px; }
  .theme-label small { color: var(--workspace-muted); font-size: 10px; }
  .theme-options.overridden { opacity: .5; }
  .font-setting { align-items: center; }
  .font-import { min-height: 28px; padding: 0 10px; display: inline-flex; align-items: center; gap: 6px; border: 1px solid var(--workspace-line); border-radius: 8px; color: var(--workspace-text); background: var(--workspace-subtle); font-size: 10px; cursor: pointer; }
  .font-import:hover { border-color: var(--workspace-accent); color: var(--workspace-accent); }
  .shortcut-button { padding: 6px 8px; border: 1px solid var(--workspace-line); border-radius: 7px; color: var(--workspace-accent); background: var(--workspace-subtle); font: 700 8px/1.2 var(--lume-font-code, ui-monospace, monospace); cursor: pointer; }
  .preferred-agents { padding: 10px 0; display: grid; gap: 7px; }
  .preferred-agents > span { display: flex; flex-wrap: wrap; gap: 5px; }
  .preferred-agents button { padding: 5px 7px; display: inline-flex; align-items: center; gap: 4px; border: 1px solid var(--workspace-line); border-radius: 7px; color: var(--workspace-muted); background: transparent; font-size: 7px; cursor: pointer; }
  .preferred-agents button.active { color: var(--workspace-accent); border-color: color-mix(in srgb, var(--workspace-accent) 40%, transparent); background: var(--workspace-accent-soft); }
  .settings-empty, .about-settings p { margin: 4px 0 0; color: var(--workspace-muted); font-size: 8px; line-height: 1.45; }
  .mobile-pairing-action { padding: 10px 0 3px; }
  .pairing-qr { padding: 10px; display: flex; align-items: center; gap: 12px; border: 1px solid var(--workspace-line); border-radius: 10px; background: var(--workspace-subtle); }.pairing-qr img { width: 96px; height: 96px; border-radius: 7px; }.pairing-qr span { display: grid; gap: 3px; }.pairing-qr strong { color: var(--workspace-strong); font: 750 12px var(--lume-font-code, ui-monospace, monospace); }.pairing-qr small { color: var(--workspace-muted); font-size: 7px; }
  .device-card { margin-top: 9px; padding: 9px 10px 3px; border: 1px solid var(--workspace-line); border-radius: 10px; background: var(--workspace-subtle); }.device-card header { display: flex; align-items: center; gap: 8px; }.device-card header span { min-width: 0; display: grid; gap: 2px; flex: 1; }.device-card header button { border: 0; color: #b7605c; background: transparent; font-size: 7px; cursor: pointer; }
  .about-settings { display: grid; gap: 0; }
  .about-update-card { min-width: 0; padding: 13px; display: grid; gap: 9px; border: 1px solid color-mix(in srgb, var(--workspace-accent) 15%, var(--workspace-line)); border-radius: 14px; background: linear-gradient(132deg, color-mix(in srgb, var(--workspace-accent) 6%, var(--workspace-raised)), var(--workspace-subtle)); }
  .about-update-header { min-width: 0; display: grid; grid-template-columns: 38px minmax(0, 1fr) auto; align-items: center; gap: 10px; }
  .about-update-identity { min-width: 0; display: grid; gap: 3px; }
  .about-update-identity > strong { color: var(--workspace-strong); font-size: 13px; font-weight: 790; letter-spacing: -.025em; }
  .about-update-identity > span { display: grid; gap: 1px; }
  .about-update-identity small { color: var(--workspace-muted); font-size: 7px; font-weight: 750; letter-spacing: .08em; text-transform: uppercase; }
  .about-update-identity b { color: var(--workspace-text); font-size: 13px; font-weight: 780; font-variant-numeric: tabular-nums; letter-spacing: -.01em; }
  .about-settings .about-update-action { min-height: 32px; padding: 0 10px; display: inline-flex; align-items: center; justify-content: center; gap: 5px; border: 1px solid color-mix(in srgb, var(--workspace-accent) 25%, var(--workspace-line)); border-radius: 9px; color: var(--workspace-text); background: color-mix(in srgb, var(--workspace-raised) 72%, transparent); font-size: 8px; font-weight: 760; white-space: nowrap; transition: border-color 140ms ease, color 140ms ease, background 140ms ease, transform 140ms ease; }
  .about-settings .about-update-action:hover:not(:disabled) { transform: translateY(-1px); color: var(--workspace-accent); border-color: color-mix(in srgb, var(--workspace-accent) 44%, var(--workspace-line)); background: var(--workspace-raised); }
  .about-settings .about-update-action.primary { color: var(--workspace-raised); border-color: transparent; background: var(--workspace-accent); }
  .about-settings .about-update-action.primary:hover:not(:disabled) { color: var(--workspace-raised); background: color-mix(in srgb, var(--workspace-accent) 84%, var(--workspace-strong)); }
  .about-settings .about-update-action:disabled { opacity: .58; cursor: default; }
  .about-update-card > p { margin: 0; color: var(--workspace-muted); font-size: 8px; line-height: 1.45; }
  .about-update-progress-copy { display: flex; align-items: center; justify-content: space-between; gap: 8px; color: var(--workspace-muted); font-size: 7px; font-weight: 720; letter-spacing: .06em; text-transform: uppercase; }
  .about-update-progress-copy b { color: var(--workspace-accent); font-size: 9px; font-weight: 800; font-variant-numeric: tabular-nums; letter-spacing: 0; }
  .about-update-progress { height: 8px; overflow: hidden; border: 1px solid color-mix(in srgb, var(--workspace-accent) 12%, var(--workspace-line)); border-radius: 999px; background: color-mix(in srgb, var(--workspace-accent) 8%, var(--workspace-raised)); box-shadow: inset 0 1px 2px rgba(0, 0, 0, .08); }
  .about-update-progress > span { position: relative; height: 100%; min-width: 0; display: block; overflow: hidden; border-radius: inherit; background: linear-gradient(90deg, color-mix(in srgb, var(--workspace-accent) 72%, #5598c6), var(--workspace-accent), color-mix(in srgb, var(--workspace-accent) 68%, #c2dfb7)); box-shadow: 0 0 12px color-mix(in srgb, var(--workspace-accent) 28%, transparent); transition: width 220ms cubic-bezier(.16, 1, .3, 1); }
  .about-update-progress > span::after { position: absolute; inset: 0; background: linear-gradient(105deg, transparent 20%, rgba(255, 255, 255, .42) 50%, transparent 80%); content: ""; animation: about-progress-shimmer 1.4s linear infinite; }
  .about-update-progress.indeterminate > span { animation: about-progress-sweep 1.2s cubic-bezier(.4, 0, .2, 1) infinite alternate; }
  @keyframes about-progress-shimmer { from { transform: translateX(-110%); } to { transform: translateX(110%); } }
  @keyframes about-progress-sweep { from { transform: translateX(-85%); } to { transform: translateX(330%); } }
  @media (prefers-reduced-motion: reduce) { .about-update-progress > span, .about-update-progress > span::after { animation: none; transition: none; } }
  .reset-control { display: flex; align-items: center; justify-content: flex-end; gap: 6px; }.reset-control > span { margin-right: auto; color: var(--workspace-muted); font-size: 8px; }.reset-control .danger { color: #b65d59; border-color: rgba(182, 93, 89, .28); }
  .shortcut-scrim { position: fixed; z-index: 60; inset: 0; display: grid; place-items: center; background: rgba(5, 11, 8, .45); backdrop-filter: blur(4px); }
  .shortcut-dialog { width: min(310px, calc(100vw - 36px)); padding: 20px; display: grid; justify-items: center; gap: 15px; border: 1px solid var(--workspace-line); border-radius: 15px; outline: none; color: var(--workspace-text); background: var(--workspace-raised); box-shadow: 0 20px 60px rgba(0, 0, 0, .25); }.shortcut-dialog > strong { color: var(--workspace-strong); font-size: 12px; }.shortcut-dialog > kbd { min-width: 160px; padding: 10px; border: 1px solid var(--workspace-line); border-radius: 8px; color: var(--workspace-accent); background: var(--workspace-subtle); font: 750 10px var(--lume-font-code, ui-monospace, monospace); text-align: center; }.shortcut-dialog > span { display: flex; gap: 7px; }.shortcut-dialog button { min-height: 30px; padding: 0 11px; border: 1px solid var(--workspace-line); border-radius: 8px; color: var(--workspace-muted); background: transparent; font-size: 8px; font-weight: 730; cursor: pointer; }.shortcut-dialog button.primary { color: var(--workspace-raised); background: var(--workspace-accent); }
  .integration-warning-dialog { width: min(440px, calc(100vw - 36px)); padding: 22px; display: grid; justify-items: start; gap: 12px; border: 1px solid var(--workspace-line); border-radius: 16px; color: var(--workspace-text); background: var(--workspace-raised); box-shadow: 0 20px 60px rgba(0, 0, 0, .28); }
  .integration-warning-icon { width: 38px; height: 38px; display: grid; place-items: center; border: 1px solid var(--workspace-line); border-radius: 11px; color: var(--workspace-accent); background: var(--workspace-subtle); }
  .integration-warning-dialog > strong { color: var(--workspace-strong); font-size: 14px; }
  .integration-warning-dialog > p { margin: 0; color: var(--workspace-muted); font-size: 11px; line-height: 1.55; }
  .integration-warning-dialog > span { width: 100%; display: flex; justify-content: flex-end; gap: 7px; padding-top: 4px; }
  .integration-warning-dialog button { min-height: 32px; padding: 0 12px; border: 1px solid var(--workspace-line); border-radius: 8px; color: var(--workspace-muted); background: transparent; font-size: 9px; font-weight: 730; cursor: pointer; }
  .integration-warning-dialog button.primary { border-color: transparent; color: var(--workspace-raised); background: var(--workspace-accent); }
  .header-selectors { min-width: 0; display: flex; align-items: center; gap: 5px; }
  .header-selectors.expanded { flex-wrap: wrap; }
  .header-utilities { margin-left: auto; display: flex; align-items: center; gap: 5px; }
  .header-selectors.expanded .header-utilities { width: 100%; justify-content: flex-end; }
  .header-selectors.expanded { animation: layout-editor-arrive 160ms cubic-bezier(.16, 1, .3, 1) both; }
  .project-picker, .layout-picker { min-width: 0; flex: 1; }
  .project-picker :global(.lume-select), .layout-picker :global(.lume-select) { width: 100%; min-width: 0 !important; }
  .header-control-icon, .header-control-close, .layout-actions button, .layout-name-editor button { width: 29px; height: 29px; padding: 0; display: grid; place-items: center; flex: 0 0 auto; border: 0; border-radius: 8px; color: var(--workspace-muted); background: transparent; cursor: pointer; transition: color 140ms ease, background 140ms ease, transform 160ms cubic-bezier(.16, 1, .3, 1); }
  .header-control-icon:hover, .header-control-close:hover, .layout-actions button:hover:not(:disabled), .layout-name-editor button:hover:not(:disabled) { color: var(--workspace-accent); background: var(--workspace-subtle); }
  .layout-actions { padding-top: 5px; display: flex; align-items: center; gap: 3px; }
  .layout-actions button:disabled, .layout-name-editor button:disabled { opacity: .3; cursor: default; }
  .layout-actions .delete-layout:hover { color: #b96862; background: color-mix(in srgb, #b96862 8%, transparent); }
  .layout-name-editor { height: 34px; margin: 6px 0 0; padding: 2px 3px 2px 8px; display: flex; align-items: center; gap: 5px; border: 1px solid color-mix(in srgb, var(--workspace-accent) 42%, var(--workspace-line)); border-radius: 9px; color: var(--workspace-accent); background: var(--workspace-raised); animation: layout-editor-arrive 160ms cubic-bezier(.16, 1, .3, 1) both; }
  .layout-name-editor input { min-width: 0; flex: 1; border: 0; outline: 0; color: var(--workspace-strong); background: transparent; font-size: 9px; }
  .layout-name-editor button { width: 25px; height: 25px; }
  .session-heading { position: relative; z-index: 20; padding: 13px 15px 8px; display: flex; align-items: center; gap: 7px; color: var(--workspace-muted); }
  .session-heading strong { flex: 1; color: var(--workspace-strong); font-size: 10px; font-weight: 720; }
  .session-heading span { font-size: 8px; font-variant-numeric: tabular-nums; }
  .search-inline { min-width: 0; height: 30px; padding: 0 8px; display: flex; align-items: center; gap: 6px; flex: 1; border: 1px solid var(--workspace-line); border-radius: 8px; color: var(--workspace-faint); background: var(--workspace-raised); animation: layout-editor-arrive 160ms cubic-bezier(.16, 1, .3, 1) both; }
  .search-inline:focus-within { border-color: var(--workspace-accent); }
  .search-inline input { min-width: 0; width: 100%; border: 0; outline: 0; color: var(--workspace-strong); background: transparent; font-size: 9px; }
  .search-inline input::placeholder { color: var(--workspace-faint); }
  .search-inline button { width: 22px; height: 22px; padding: 0; display: grid; place-items: center; flex: 0 0 auto; border: 0; border-radius: 5px; color: var(--workspace-muted); background: transparent; cursor: pointer; }
  .search-inline button:hover { color: var(--workspace-accent); background: var(--workspace-subtle); }
  .search-toggle { width: 26px; height: 26px; padding: 0; display: grid; place-items: center; flex: 0 0 auto; border: 0; border-radius: 7px; color: var(--workspace-muted); background: transparent; cursor: pointer; }
  .search-toggle:hover, .search-toggle.active { color: var(--workspace-accent); background: var(--workspace-subtle); }
  .session-launcher { position: relative; margin-left: auto; }
  .session-launcher > button { width: 26px; height: 26px; padding: 0; display: grid; place-items: center; border: 0; border-radius: 7px; color: var(--workspace-muted); background: transparent; cursor: pointer; transition: color 120ms ease, background-color 120ms ease, transform 120ms cubic-bezier(.16, 1, .3, 1); }
  .session-launcher > button:hover, .session-launcher > button.active { color: var(--workspace-accent); background: var(--workspace-subtle); }
  .session-launcher > button:active { transform: scale(.94); }
  .session-launcher > button :global(svg) { transition: transform 180ms cubic-bezier(.16, 1, .3, 1); }
  .session-launcher > button.active :global(svg) { transform: rotate(45deg); }
  .session-launcher-popover { position: fixed; z-index: 1000; box-sizing: border-box; padding: 10px; overflow-y: auto; border: 1px solid var(--workspace-line); border-radius: 12px; color: var(--workspace-text); background: var(--workspace-raised); box-shadow: 0 16px 42px rgba(8, 18, 13, .19); }
  .session-launcher-popover > strong { display: block; margin: 1px 3px 9px; color: var(--workspace-strong); font-size: 10px; }
  .launcher-agent { border-top: 1px solid var(--workspace-line); }
  .launcher-agent-row { min-height: 44px; display: flex; align-items: center; gap: 5px; }
  .launcher-agent-row > span { min-width: 0; flex: 1; overflow: hidden; color: var(--workspace-strong); font-size: 9px; font-weight: 690; text-overflow: ellipsis; white-space: nowrap; }
  .launcher-agent-row button { box-sizing: border-box; min-width: 64px; min-height: 27px; padding: 0 7px; display: inline-flex; align-items: center; justify-content: center; border: 0; border-radius: 6px; color: var(--workspace-accent); background: var(--workspace-subtle); font-size: 8px; font-weight: 720; cursor: pointer; transition: color 120ms ease, background-color 120ms ease, transform 120ms cubic-bezier(.16, 1, .3, 1); }
  .launcher-agent-row button:not(:disabled):active { transform: scale(.96); }
  .launcher-agent-row button:focus-visible, .launcher-resume-list button:focus-visible { outline: 2px solid color-mix(in srgb, var(--workspace-accent) 48%, transparent); outline-offset: 2px; }
  .launcher-agent-row button:disabled { opacity: .45; cursor: default; }
  .launcher-agent-row button.active { background: var(--workspace-accent-soft); }
  .launcher-agent-row button.loading { color: var(--workspace-accent); background: var(--workspace-accent-soft); opacity: .92; }
  .launcher-button-content { display: inline-flex; align-items: center; justify-content: center; gap: 5px; white-space: nowrap; }
  .launcher-spinner { width: 9px; height: 9px; flex: 0 0 auto; border: 1.4px solid currentColor; border-right-color: transparent; border-radius: 50%; animation: session-launcher-spin 720ms linear infinite; }
  @keyframes session-launcher-spin { to { transform: rotate(360deg); } }
  .launcher-resume-list { max-height: 180px; padding: 0 0 7px 21px; overflow-y: auto; }
  .launcher-resume-list button { width: 100%; min-height: 39px; padding: 5px 7px; display: flex; align-items: center; justify-content: space-between; gap: 8px; border: 0; border-radius: 7px; color: var(--workspace-text); background: transparent; text-align: left; cursor: pointer; transition: background-color 120ms ease, transform 120ms cubic-bezier(.16, 1, .3, 1); }
  .launcher-resume-list button:hover { background: var(--workspace-subtle); }
  .launcher-resume-list button:disabled { opacity: .45; cursor: default; }
  .launcher-resume-list button.loading { background: var(--workspace-accent-soft); opacity: .9; }
  .launcher-session-copy { min-width: 0; display: grid; gap: 2px; }
  .launcher-resume-list strong { overflow: hidden; color: var(--workspace-strong); font-size: 9px; text-overflow: ellipsis; white-space: nowrap; }
  .launcher-resume-list small { overflow: hidden; color: var(--workspace-muted); font-size: 7px; text-overflow: ellipsis; white-space: nowrap; }
  .launcher-loading-note { min-height: 30px; display: flex; align-items: center; gap: 7px; color: var(--workspace-muted); font-size: 8px; }
  .session-launcher-popover p { margin: 8px 3px; color: var(--workspace-muted); font-size: 8px; line-height: 1.45; }
  .session-filters { margin: 0 11px 10px; padding: 3px; display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); border: 1px solid var(--workspace-line); border-radius: 9px; background: color-mix(in srgb, var(--workspace-sidebar) 76%, var(--workspace-bg)); }
  .session-filters button { min-width: 0; height: 25px; padding: 0 5px; overflow: hidden; border: 0; border-radius: 6px; color: var(--workspace-muted); background: transparent; font-size: 8px; font-weight: 700; text-overflow: ellipsis; white-space: nowrap; cursor: pointer; transition: color 140ms ease, background 140ms ease, transform 180ms cubic-bezier(.16, 1, .3, 1); }
  .session-filters button:hover { color: var(--workspace-strong); }
  .session-filters button.active { color: var(--workspace-accent); background: var(--workspace-raised); box-shadow: 0 1px 3px rgba(26, 42, 34, .08); }
  .session-filters button:active { transform: scale(.97); }
  .new-group { margin: -3px 11px 8px; padding: 0 8px; height: 24px; display: flex; align-items: center; gap: 6px; border: 1px dashed var(--workspace-line); border-radius: 8px; color: var(--workspace-muted); background: transparent; font-size: 9px; font-weight: 700; cursor: pointer; }
  .new-group:hover { color: var(--workspace-accent); border-color: color-mix(in srgb, var(--workspace-accent) 45%, var(--workspace-line)); }
  .group-heading { margin: 9px 0 3px; padding: 2px 4px 2px 2px; display: flex; align-items: center; gap: 4px; border-radius: 8px; color: var(--workspace-faint); transition: background 120ms ease; }
  .group-heading { cursor: grab; }
  .group-heading.dragging-section { opacity: .45; }
  .group-heading.reorder-before { box-shadow: 0 -2px 0 var(--workspace-accent); }
  .group-heading.reorder-after { box-shadow: 0 2px 0 var(--workspace-accent); }
  .group-heading.drop-target { background: var(--workspace-accent-soft); outline: 1px dashed var(--workspace-accent); }
  .group-toggle { min-width: 0; height: 24px; padding: 0 6px 0 2px; flex: 1; display: flex; align-items: center; gap: 6px; border: 0; border-radius: 6px; color: var(--workspace-muted); background: transparent; font: inherit; text-align: left; cursor: pointer; }
  .group-toggle strong { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--workspace-strong); font-size: 10px; font-weight: 720; }
  .group-toggle small, .group-label + small { margin-left: auto; font-size: 9px; font-variant-numeric: tabular-nums; }
  .group-chevron { display: grid; transition: transform 160ms ease; }
  .group-heading.collapsed .group-chevron { transform: rotate(-90deg); }
  .group-label { padding: 0 6px; font-size: 9px; font-weight: 720; letter-spacing: .04em; text-transform: uppercase; }
  .group-actions { display: flex; opacity: 0; transition: opacity 120ms ease; }
  .group-heading:hover .group-actions, .group-actions:focus-within { opacity: 1; }
  .group-actions button { width: 22px; height: 22px; padding: 0; display: grid; place-items: center; border: 0; border-radius: 6px; color: var(--workspace-muted); background: transparent; cursor: pointer; }
  .group-actions button:hover { color: var(--workspace-accent); background: var(--workspace-subtle); }
  .group-actions button.confirm { color: #c0554f; background: color-mix(in srgb, #c0554f 14%, transparent); }
  .group-rename { flex: 1; min-width: 0; }
  .group-rename input { box-sizing: border-box; width: 100%; height: 24px; padding: 0 7px; border: 1px solid var(--workspace-accent); border-radius: 6px; outline: 0; color: var(--workspace-strong); background: var(--workspace-raised); font: inherit; font-size: 11px; }
  .session-context-groups { margin: 2px 0; padding: 4px 0; display: grid; border-block: 1px solid var(--workspace-line); }
  .session-context-groups > small { padding: 2px 9px 3px; color: var(--workspace-faint); font-size: 9px; font-weight: 700; text-transform: uppercase; letter-spacing: .05em; }
  .session-context-groups .current { color: var(--workspace-accent); }
  .session-list.releasing { box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--workspace-accent) 35%, transparent); }
  .session-list { min-height: 0; padding: 0 8px 14px; overflow-y: auto; scrollbar-width: thin; scrollbar-color: var(--workspace-scroll-thumb) transparent; }
  .session-row { position: relative; display: flex; align-items: stretch; border-radius: 10px; cursor: grab; transition: background 140ms ease, opacity 140ms ease, transform 180ms cubic-bezier(.16, 1, .3, 1); }
  .session-tree-item { position: relative; min-width: 0; margin-bottom: 2px; border-radius: 10px; transition: background 140ms ease; }
  .session-tree-item.focused { background: var(--workspace-accent-soft); }
  .session-tree-item.focused::before { position: absolute; top: 13px; bottom: 13px; left: 0; width: 1px; border-radius: 1px; background: var(--workspace-accent); content: ""; }
  .session-tree-item.secondary-selected { background: color-mix(in srgb, var(--workspace-accent-soft) 42%, transparent); box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--workspace-accent) 25%, transparent); }
  .session-tree-item.connected { border: 1px solid color-mix(in srgb, var(--workspace-accent) 25%, transparent); background: color-mix(in srgb, var(--workspace-accent-soft) 72%, var(--workspace-sidebar)); box-shadow: none; }
  .session-tree-item.connected-above { border-top: 0; border-top-left-radius: 0; border-top-right-radius: 0; }
  .session-tree-item.connected-below { margin-bottom: 0; border-bottom: 0; border-bottom-left-radius: 0; border-bottom-right-radius: 0; }
  .session-tree-item.connected.focused { background: color-mix(in srgb, var(--workspace-accent-soft) 72%, var(--workspace-sidebar)); }
  .session-row:hover { background: var(--workspace-subtle); }
  .session-row:hover { transform: translateX(2px); }
  .session-row:active { cursor: grabbing; }
  .session-row.dragging { opacity: .48; transform: scale(.98); }
  .session-tree-item.focused .session-row:hover { background: color-mix(in srgb, var(--workspace-accent) 7%, transparent); }
  .session-tree-item.secondary-selected .session-row:hover { background: color-mix(in srgb, var(--workspace-accent) 5%, transparent); }
  .session-select { min-width: 0; min-height: 62px; padding: 9px 4px 9px 10px; display: flex; align-items: flex-start; gap: 9px; flex: 1; border: 0; color: inherit; background: transparent; text-align: left; cursor: inherit; }
  .session-icon { position: relative; width: 34px; height: 34px; display: grid; place-items: center; flex: 0 0 auto; color: var(--workspace-accent); }
  .session-icon :global(.thread-avatar) { transition: transform 230ms cubic-bezier(.16, 1, .3, 1); }
  .session-status-dot { position: absolute; top: 0; right: 0; width: 9px; height: 9px; box-sizing: border-box; border: 2px solid var(--workspace-sidebar); border-radius: 50%; background: #8a9891; opacity: 0; transform: scale(.55); pointer-events: none; transition: opacity 130ms ease, transform 180ms cubic-bezier(.16, 1, .3, 1); }
  .session-copy { min-width: 0; display: grid; gap: 2px; flex: 1; opacity: 1; transform: translateX(0); transition: opacity 120ms ease, transform 180ms cubic-bezier(.16, 1, .3, 1); }
  .session-copy strong, .session-copy small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .session-copy strong { color: var(--workspace-strong); font-size: 10px; font-weight: 720; letter-spacing: -.01em; }
  .session-copy small { color: var(--workspace-muted); font-size: 8px; line-height: 1.3; }
  .session-copy .session-meta { display: flex; align-items: center; gap: 4px; }
  .session-meta span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .session-meta .session-fork { flex: 0 0 auto; min-width: auto; display: inline-flex; color: var(--workspace-accent); }
  .session-environment-marker { position: absolute; right: 8px; bottom: 5px; z-index: 2; }
  .workspace.sidebar-collapsed .session-environment-marker { right: 0; bottom: 0; }
  .session-copy em { display: flex; align-items: center; gap: 5px; color: var(--workspace-muted); font-size: 8px; font-style: normal; font-weight: 650; }
  .session-copy em i { width: 5px; height: 5px; border-radius: 50%; background: #8a9891; }
  .session-copy em.status-running i { background: #4d99cc; }.session-copy em.status-completed i { background: #4daa77; }.session-copy em.status-permission_required i { background: #d6a441; }.session-copy em.status-failed i { background: #c86662; }
  .session-copy em.status-running i { animation: live-pulse 1.8s ease-out infinite; }
  .session-copy em.status-subagents i { background: var(--workspace-accent); }
  .subagent-toggle { min-width: 22px; margin: auto 1px auto auto; display: flex; align-items: center; justify-content: center; gap: 2px; flex: 0 0 auto; color: var(--workspace-muted); font-size: 8px; font-weight: 750; pointer-events: none; }
  .session-select:hover .subagent-toggle, .subagent-toggle.open { color: var(--workspace-accent); }
  .subagent-toggle :global(.lume-icon) { transition: transform 160ms cubic-bezier(.16, 1, .3, 1); }.subagent-toggle.open :global(.lume-icon) { transform: rotate(180deg); }
  .subagent-list-shell { display: grid; grid-template-rows: 0fr; transition: grid-template-rows 180ms cubic-bezier(.16, 1, .3, 1); }.subagent-list-shell.open { grid-template-rows: 1fr; }
  .subagent-list { min-height: 0; margin: 0 8px 3px 22px; overflow: hidden; }
  .subagent-row { position: relative; min-height: 35px; padding: 4px 6px 4px 19px; display: flex; align-items: center; gap: 7px; color: var(--workspace-muted); }
  .subagent-list-shell.open .subagent-row { animation: subagent-enter 210ms cubic-bezier(.16, 1, .3, 1) both; }
  .subagent-branch { position: absolute; top: 0; left: 0; width: 15px; height: 50%; border-bottom: 1px solid var(--workspace-line); border-left: 1px solid var(--workspace-line); border-bottom-left-radius: 8px; }
  .subagent-row:not(:last-child)::after { position: absolute; top: 50%; bottom: 0; left: 0; border-left: 1px solid var(--workspace-line); content: ""; }
  .subagent-avatar { width: 24px; height: 24px; display: grid; place-items: center; flex: 0 0 auto; }
  .subagent-copy { min-width: 0; display: grid; gap: 1px; }.subagent-copy strong { max-width: 165px; overflow: hidden; color: var(--workspace-text); font-size: 9px; font-weight: 690; text-overflow: ellipsis; white-space: nowrap; }.subagent-copy small { color: var(--workspace-faint); font-size: 7px; }.subagent-copy small.status-running { color: #4d99cc; }.subagent-copy small.status-failed { color: #c86662; }
  .internal-heading { margin: 14px 7px 5px; padding-top: 11px; display: flex; align-items: center; gap: 7px; border-top: 1px solid var(--workspace-line); color: var(--workspace-faint); font-size: 8px; font-weight: 750; letter-spacing: .02em; }.internal-heading span { flex: 1; }.internal-heading small { color: var(--workspace-muted); font-size: 8px; }
  .internal-row { min-width: 0; min-height: 49px; padding: 7px 9px; display: flex; align-items: center; gap: 9px; border-radius: 9px; color: var(--workspace-muted); background: var(--workspace-subtle); }.internal-row .session-copy { gap: 3px; }.internal-live { width: 6px; height: 6px; flex: 0 0 auto; border-radius: 50%; background: var(--workspace-accent); }
  .no-results { margin: 36px 20px; color: var(--workspace-muted); font-size: 10px; text-align: center; }
  .session-skeleton { height: 62px; margin-bottom: 4px; padding: 10px; display: flex; gap: 9px; border-radius: 12px; background: var(--workspace-subtle); }
  .session-skeleton i { width: 32px; height: 32px; border-radius: 10px; background: var(--workspace-line); }
  .session-skeleton span { width: 108px; height: 8px; margin-top: 5px; border-radius: 4px; background: var(--workspace-line); }
  .session-context-menu { position: fixed; z-index: 29; width: min(214px, calc(100vw - 16px)); max-height: calc(100vh - 16px); box-sizing: border-box; padding: 8px; display: grid; gap: 7px; overflow-y: auto; border: 1px solid var(--workspace-line); border-radius: 11px; color: var(--workspace-text); background: var(--workspace-raised); box-shadow: 0 12px 36px rgba(6, 19, 11, .2); animation: session-menu-in 130ms cubic-bezier(.16, 1, .3, 1) both; }
  .session-context-menu > strong { padding: 3px 5px 5px; overflow: hidden; color: var(--workspace-strong); font-size: 10px; text-overflow: ellipsis; white-space: nowrap; }
  .session-context-menu p { margin: 0; padding: 0 5px; color: var(--workspace-muted); font-size: 9px; line-height: 1.5; }
  .session-context-rename { display: grid; gap: 7px; }
  .session-context-rename label { padding: 0 2px; color: var(--workspace-muted); font-size: 8px; font-weight: 700; }
  .session-context-rename input { width: 100%; min-width: 0; height: 33px; padding: 0 9px; border: 1px solid var(--workspace-line); border-radius: 8px; outline: 0; color: var(--workspace-strong); background: var(--workspace-subtle); font-size: 10px; }
  .session-context-rename input:focus { border-color: color-mix(in srgb, var(--workspace-accent) 55%, var(--workspace-line)); box-shadow: 0 0 0 2px var(--workspace-accent-soft); }
  .session-context-command, .session-context-actions button { min-height: 32px; padding: 0 9px; border: 0; border-radius: 8px; color: var(--workspace-text); background: var(--workspace-subtle); font-size: 9px; cursor: pointer; }
  .session-context-command { display: flex; align-items: center; gap: 8px; text-align: left; transition: color 130ms ease, background 130ms ease, transform 130ms cubic-bezier(.16, 1, .3, 1); }
  .session-context-command:hover, .session-context-actions button:hover { color: var(--workspace-strong); background: var(--workspace-line); }
  .session-context-command:hover { transform: translateX(1px); }
  .session-context-command.danger-command { color: #b45c58; }
  .session-context-command:disabled { opacity: .55; cursor: wait; transform: none; }
  .session-context-actions { display: flex; gap: 5px; }
  .session-context-actions button { flex: 1; }
  .session-context-actions .danger { color: #b45c58; background: color-mix(in srgb, #b45c58 12%, var(--workspace-raised)); }
  .session-context-actions .danger:hover { background: color-mix(in srgb, #b45c58 20%, var(--workspace-raised)); }
  .session-context-actions .primary { color: var(--workspace-raised); background: var(--workspace-accent); }
  .session-context-actions .primary:hover { color: var(--workspace-raised); background: color-mix(in srgb, var(--workspace-accent) 84%, var(--workspace-strong)); }
  .session-context-actions button:disabled { opacity: .55; cursor: wait; }
  .workspace-stage { position: relative; min-width: 0; min-height: 0; display: grid; grid-template-columns: minmax(0, 1fr) 0px 0px; grid-template-rows: minmax(0, 1fr); overflow: hidden; background: transparent; transition: grid-template-columns 180ms cubic-bezier(.16, 1, .3, 1); }
  .workspace-stage.inspector-open { grid-template-columns: minmax(0, 1fr) clamp(270px, 23vw, 350px) 0px; }
  .workspace-stage.review-open { grid-template-columns: minmax(340px, 38fr) 0px minmax(480px, 62fr); }
  .workspace-stage.review-open.review-wide { grid-template-columns: 0px 0px minmax(0, 1fr); }
  .workspace-stage.review-wide .workbench { visibility: hidden; }
  .workspace-stage.maximized { grid-template-columns: minmax(0, 1fr) 0px 0px; }
  .inspector-shell { min-width: 0; min-height: 0; height: 100%; overflow: hidden; pointer-events: none; }
  .inspector-shell.open { pointer-events: auto; }
  .inspector-content { min-width: 0; width: 100%; height: 100%; }
  .review-shell { min-width: 0; min-height: 0; height: 100%; overflow: hidden; border-left: 0 solid transparent; pointer-events: none; }
  .review-shell.open { border-left-width: 1px; border-left-color: var(--workspace-line); pointer-events: auto; }
  .review-content { min-width: 0; width: 100%; height: 100%; }
  .workbench { position: relative; width: 100%; max-width: 100%; min-width: 0; min-height: 0; isolation: isolate; contain: inline-size; display: grid; grid-template-columns: minmax(0, 1fr); grid-template-rows: minmax(0, 1fr); overflow: hidden; background: var(--workspace-chat-background); }
  .board-chat-layer { display: contents; }
  /* The board covers the chats: they stay mounted (scroll and drafts survive) but stop painting and animating. */
  .board-chat-layer.covered { visibility: hidden; }
  .board-chat-layer.covered :global(*), .board-chat-layer.covered :global(*::before), .board-chat-layer.covered :global(*::after) { animation-play-state: paused !important; }
  .workspace-wallpaper { position: absolute; z-index: -1; inset: 0; width: 100%; height: 100%; background-position: center; background-size: cover; background-repeat: no-repeat; pointer-events: none; }
  .workbench.split { grid-template-columns: minmax(0, 1fr) 7px minmax(0, 1fr); }
  .workbench.resizing { user-select: none; }
  .workbench.drag-active { cursor: move; }
  .workbench.header-relocating { cursor: grabbing; }
  .layout-drop-preview { position: absolute; z-index: 20; top: 8px; bottom: 8px; min-width: 0; padding: 0 7px; box-sizing: border-box; pointer-events: none; }
  .layout-drop-preview::before { position: absolute; inset: 0 7px; border: 1px solid color-mix(in srgb, var(--workspace-accent) 64%, var(--workspace-line)); border-radius: 15px; background: color-mix(in srgb, var(--workspace-accent) 10%, var(--workspace-raised)); box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--workspace-accent) 9%, transparent), 0 10px 34px color-mix(in srgb, var(--workspace-accent) 11%, transparent); content: ""; animation: drop-preview-arrive 150ms cubic-bezier(.16, 1, .3, 1) both; }
  .layout-drop-preview span { position: absolute; top: 50%; left: 50%; min-width: max-content; padding: 6px 9px; display: flex; align-items: center; gap: 6px; border: 1px solid color-mix(in srgb, var(--workspace-accent) 34%, var(--workspace-line)); border-radius: 999px; color: var(--workspace-accent); background: var(--workspace-raised); box-shadow: 0 7px 20px rgba(7, 20, 13, .14); font-size: 8px; font-weight: 760; transform: translate(-50%, -50%); }
  .layout-drop-preview.insert::before { background: color-mix(in srgb, var(--workspace-accent) 17%, var(--workspace-raised)); animation: add-pane-preview 540ms cubic-bezier(.16, 1, .3, 1) both; }
  .layout-drop-preview.insert span { animation: add-label-pulse 1.1s ease-in-out infinite alternate; }
  .layout-drop-preview.replace::before { border-style: dashed; opacity: .72; }
  .layout-drop-preview.move::before { background: color-mix(in srgb, var(--workspace-accent) 8%, var(--workspace-raised)); }
  .session-drag-preview { position: fixed; z-index: 80; top: 0; left: 0; width: 220px; min-height: 58px; box-sizing: border-box; padding: 9px 11px; display: flex; align-items: center; gap: 9px; visibility: hidden; pointer-events: none; border: 1px solid color-mix(in srgb, var(--workspace-accent) 30%, var(--workspace-line)); border-radius: 11px; color: var(--workspace-text); background: var(--workspace-raised); box-shadow: 0 12px 32px rgba(0, 0, 0, .22); animation: drag-card-appear 140ms ease-out both; will-change: transform; }
  .session-drag-preview > span { min-width: 0; display: grid; gap: 3px; flex: 1; }
  .session-drag-preview strong, .session-drag-preview small { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .session-drag-preview strong { color: var(--workspace-strong); font-size: 10px; font-weight: 720; }
  .session-drag-preview small { display: flex; align-items: center; gap: 4px; color: var(--workspace-muted); font-size: 8px; }
  .session-drag-preview > :global(.lume-icon) { flex: 0 0 auto; color: var(--workspace-accent); }
  @keyframes drag-card-appear { from { opacity: .65; } to { opacity: 1; } }
  .workbench.review-focus .pane-divider { width: 0; min-width: 0; visibility: hidden; pointer-events: none; }
  .pane-divider { position: relative; width: 7px; min-width: 7px; padding: 0; border: 0; outline: 0; background: transparent; cursor: col-resize; touch-action: none; }
  .pane-divider::before { position: absolute; inset: 0 3px; background: var(--workspace-line); content: ""; transition: inset 120ms ease, background 120ms ease; }
  .pane-divider:hover::before,
  .pane-divider:focus-visible::before,
  .workbench.resizing .pane-divider::before { inset: 0 2px; background: color-mix(in srgb, var(--workspace-accent) 58%, var(--workspace-line)); }
  .workspace-empty { margin: auto; display: grid; justify-items: center; gap: 10px; color: var(--workspace-muted); text-align: center; }
  .empty-mark { width: 58px; height: 58px; display: grid; place-items: center; color: var(--workspace-accent); }
  .workspace-empty strong { color: var(--workspace-strong); font-size: 17px; letter-spacing: -.03em; }
  .workspace-empty p { max-width: 360px; margin: 0; font-size: 11px; line-height: 1.6; }
  .workspace-loading .empty-mark { animation: loading-breathe 1.4s ease-in-out infinite alternate; }
  .workspace-loading-bar { width: 96px; height: 3px; overflow: hidden; border-radius: 2px; background: var(--workspace-line); }
  .workspace-loading-bar::after { display: block; width: 40%; height: 100%; border-radius: inherit; background: var(--workspace-accent); content: ""; animation: loading-slide 1.1s ease-in-out infinite; }
  @keyframes loading-breathe { from { opacity: .45; transform: scale(.96); } to { opacity: 1; transform: scale(1); } }
  @keyframes loading-slide { from { transform: translateX(-100%); } to { transform: translateX(250%); } }
  .workspace-error { position: fixed; right: 18px; bottom: 18px; max-width: 420px; margin: 0; padding: 10px 12px; border: 1px solid rgba(198, 102, 98, .28); border-radius: 10px; color: #b45c58; background: var(--workspace-pane); font-size: 9px; }
  @keyframes live-pulse { 0%, 45% { box-shadow: 0 0 0 0 rgba(77, 153, 204, .28); } 80%, 100% { box-shadow: 0 0 0 4px rgba(77, 153, 204, 0); } }
  @keyframes session-menu-in { from { opacity: 0; transform: translateY(-4px); } }
  @keyframes subagent-enter { from { opacity: 0; transform: translateY(3px); } }
  @keyframes layout-editor-arrive { from { opacity: 0; transform: translateY(-3px); } }
  @keyframes drop-preview-arrive { from { opacity: 0; transform: scale(.985); } }
  @keyframes add-pane-preview { 0% { opacity: 0; transform: scaleX(.76); } 65% { opacity: 1; transform: scaleX(1.015); } 100% { transform: scaleX(1); } }
  @keyframes add-label-pulse { from { box-shadow: 0 7px 20px rgba(7, 20, 13, .12), 0 0 0 0 color-mix(in srgb, var(--workspace-accent) 20%, transparent); } to { box-shadow: 0 7px 20px rgba(7, 20, 13, .14), 0 0 0 5px transparent; } }
  @media (max-width: 980px) { .workspace:not(.sidebar-collapsed) { grid-template-columns: 216px minmax(0, 1fr); } }
  .workspace.sidebar-collapsed { grid-template-columns: 56px minmax(0, 1fr); }
  .workspace.sidebar-collapsed .brand-header { padding: 2px 3px 7px; border-bottom: 0; }
  .workspace.sidebar-collapsed .brand-top { height: 69px; flex-direction: column; justify-content: center; gap: 5px; }
  .workspace.sidebar-collapsed .brand-mark { width: 27px; height: 27px; }
  .workspace.sidebar-collapsed .sidebar-toggle { width: 27px; height: 27px; }
  .workspace.sidebar-text-hidden .brand-copy,
  .workspace.sidebar-text-hidden .session-copy { opacity: 0; transform: translateX(-6px); }
  .workspace.sidebar-collapsed .brand-copy,
  .workspace.sidebar-collapsed .session-copy { display: none; }
  .workspace.sidebar-collapsed .session-heading > .search-toggle,
  .workspace.sidebar-collapsed .session-heading > strong,
  .workspace.sidebar-collapsed .session-heading > span,
  .workspace.sidebar-collapsed .session-filters { display: none; }
  .workspace.sidebar-collapsed .header-selectors,
  .workspace.sidebar-collapsed .header-utilities { margin: 0; padding: 0; width: 100%; display: flex; flex-direction: column; align-items: center; gap: 3px; }
  .workspace.sidebar-collapsed .header-selectors { padding-top: 10px; }
  .workspace.sidebar-collapsed .session-heading { padding: 5px 0 9px; flex-direction: column; justify-content: center; gap: 0; border-bottom: 1px solid var(--workspace-line); }
  .workspace.sidebar-collapsed .session-launcher { margin: 0; }
  .workspace.sidebar-collapsed .session-list { padding: 7px 3px 12px; }
  .workspace.sidebar-collapsed .session-row { height: 44px; }
  .workspace.sidebar-collapsed .session-select { min-height: 44px; padding: 5px; gap: 0; align-items: center; justify-content: center; }
  .workspace.sidebar-collapsed .session-icon :global(.thread-avatar) { transform: scale(.8); }
  .workspace.sidebar-collapsed .session-status-dot { opacity: 1; transform: scale(1); }
  .workspace.sidebar-collapsed .session-status-dot.status-running { background: #4d99cc; }
  .workspace.sidebar-collapsed .session-status-dot.status-completed { background: #4daa77; }
  .workspace.sidebar-collapsed .session-status-dot.status-permission_required,
  .workspace.sidebar-collapsed .session-status-dot.status-waiting_for_input { background: #d6a441; }
  .workspace.sidebar-collapsed .session-status-dot.status-failed { background: #c86662; }
  .workspace.sidebar-collapsed .session-status-dot.status-subagents { background: var(--workspace-accent); }
  .workspace.sidebar-collapsed .subagent-toggle,
  .workspace.sidebar-collapsed .subagent-list-shell,
  .workspace.sidebar-collapsed .internal-heading span,
  .workspace.sidebar-collapsed .internal-heading small,
  .workspace.sidebar-collapsed .internal-live { display: none; }
  .workspace.sidebar-collapsed .internal-heading { height: 1px; margin: 12px 8px 5px; padding: 0; }
  .workspace.sidebar-collapsed .internal-row { justify-content: center; padding: 7px 0; }
  .workspace.sidebar-collapsed .session-tree-item.focused::before { top: 10px; bottom: 10px; }
  @media (max-width: 1100px) {
    .workspace-stage.inspector-open { grid-template-columns: minmax(0, 1fr) 0px 0px; }
    .inspector-shell { position: absolute; z-index: 34; top: 0; right: 0; bottom: 0; width: min(350px, calc(100% - 44px)); opacity: 0; transform: translateX(20px); transition: opacity 130ms ease, transform 170ms cubic-bezier(.16, 1, .3, 1); }
    .inspector-shell.open { opacity: 1; transform: translateX(0); box-shadow: -16px 0 40px rgba(4, 15, 9, .16); }
    .workspace-stage.review-open { grid-template-columns: minmax(300px, 1fr) 0px minmax(420px, 48vw); }
  }
  @media (max-height: 640px) { .brand-top { height: 46px; }.session-heading { padding-top: 8px; }.session-tree-item { margin-bottom: 0; } }
  @media (prefers-reduced-motion: reduce) { .workspace-loading .empty-mark, .workspace-loading-bar::after { animation: none; } .workspace, .brand-copy, .session-copy, .session-icon :global(.thread-avatar), .session-status-dot, .session-row, .session-tree-item, .subagent-toggle :global(.lume-icon), .subagent-list-shell, .sidebar-toggle, .compact-mode, .settings-button, .session-filters button, .pane-divider::before, .appearance-option, .workspace-switch, .workspace-switch::after, .header-control-icon, .header-control-close, .layout-actions button, .workspace-stage, .session-launcher > button, .session-launcher > button :global(svg), .launcher-agent-row button, .launcher-resume-list button { transition: none; }.launcher-spinner { animation: none !important; }.session-copy em.status-running i, .session-context-menu, .layout-name-editor, .header-selectors.expanded, .layout-drop-preview::before, .layout-drop-preview span, .subagent-row, .session-drag-preview { animation: none; } }
</style>
