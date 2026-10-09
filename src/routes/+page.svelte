<script lang="ts">
  import { dev } from "$app/environment";
  import { onMount, setContext, tick } from "svelte";
  import { flip } from "svelte/animate";
  import { groupSessions, readSidebarGroups, sidebarGroupsKey, writeSidebarGroups, type SidebarGroupState } from "$lib/sidebarGroups";
  import { cubicOut } from "svelte/easing";
  import { fade, fly, slide } from "svelte/transition";
  import { getVersion } from "@tauri-apps/api/app";
  import { emit, listen } from "@tauri-apps/api/event";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { relaunch } from "@tauri-apps/plugin-process";
  import { check, type Update } from "@tauri-apps/plugin-updater";
  import QRCode from "qrcode";
  import {
    availableMonitors,
    getCurrentWindow,
    LogicalSize,
    primaryMonitor,
  } from "@tauri-apps/api/window";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import BrandIcon from "$lib/BrandIcon.svelte";
  import AccentColorPicker from "$lib/AccentColorPicker.svelte";
  import { ensureCustomFonts } from "$lib/fonts";
  import { appearanceAttributes, appearanceThemes, darkBases, lightBases } from "$lib/appearance";
  import LumeLogo from "$lib/LumeLogo.svelte";
  import OrbResponse from "$lib/OrbResponse.svelte";
  import LumeMascot from "$lib/LumeMascot.svelte";
  import {
    clampOrbPosition,
    compactPositionForPanel,
    expandedPositionForOrb,
    freeOrbDock,
    orbCornerRadii,
    orbDockAtPosition,
    orbDockPressure,
    orbEdgePressure,
    pinOrbPosition,
    snapOrbPosition,
    type OrbDock,
    type OrbPosition,
  } from "$lib/orbDocking";
  import LumeSelect from "$lib/LumeSelect.svelte";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import { sessionLauncherTransition } from "$lib/sessionLauncherTransition";
  import CodexCliAssociationDialog from "$lib/CodexCliAssociationDialog.svelte";
  import AgentConnectionDialog from "$lib/AgentConnectionDialog.svelte";
  import MacosAutomationDialog from "$lib/MacosAutomationDialog.svelte";
  import { agentConnectionMessage } from "$lib/agentConnection";
  import { macosAutomationMessage } from "$lib/macosAutomation";
  import RemoteComputers from "$lib/RemoteComputers.svelte";
  import { collectAgentAlerts } from "$lib/agentAlerts";
  import { usageAlertDismissals } from "$lib/usageAlertDismissals";
  import { playTone as playNotificationTone, type ToneKind } from "$lib/notificationTones";
  import SystemBannerStack, { type SystemBannerItem } from "$lib/SystemBannerStack.svelte";
  import { SYSTEM_BANNER_CONTEXT, type SystemBannerNotice, type SystemBannerReporter } from "$lib/systemBannerContext";
  import { animatedDisclosure } from "$lib/animatedDisclosure";
  import OrbNavigationIcon from "$lib/OrbNavigationIcon.svelte";
  import { TerminalOpeningTimeoutError, waitForTerminalWindow } from "$lib/terminalOpening";
  import { createSurfaceSizeQueue, settleSurfaceSize } from "$lib/surfaceSizing";
  import StartupModeChooser from "$lib/StartupModeChooser.svelte";
  import ThreadAvatar from "$lib/ThreadAvatar.svelte";
  import WorkspaceHeaderIcon from "$lib/WorkspaceHeaderIcon.svelte";
  import WorkspaceInspector from "$lib/WorkspaceInspector.svelte";
  import { displayText, localize } from "$lib/i18n";
  import {
    clipboardHasImage,
    clipboardMayContainImage,
    collectClipboardImages,
    createImagePreview,
    prepareClipboardImage,
  } from "$lib/imageAttachments";
  import { sessionCapabilities } from "$lib/sessionCapabilities";
  import { resolveLiveResumableSession, resolveTerminalSession } from "$lib/sessionIdentity";
  import { stripInternalAgentMetadata } from "$lib/markdown.js";
  import type {
    AgentKind,
    AgentSession,
    CompanionStatus,
    ExternalAgentPlugin,
    IntegrationDiagnostic,
    IntegrationStatus,
    MobileGatewayStatus,
    MobilePairingOffer,
    MobileScope,
    PairedDevice,
    PermissionAction,
    Preferences,
    PromptAttachmentInput,
    QuestionAnswer,
    ResumableSession,
    SessionStatus,
    TerminalWindowState,
    WhiteboardLayout,
    WorkflowRun,
  } from "$lib/domain";
  import { demoSessions } from "$lib/demo";
  import {
    configureIntegration,
    configureVscode,
    beginMobilePairing,
    answerQuestion,
    diagnoseIntegration,
    disableMobileGateway,
    decidePermission,
    defaultPreferences,
    loadDisplayBackend,
    loadResumableSessions,
    loadIntegrationStatuses,
    loadMobileGatewayStatus,
    loadOverlayPosition,
    loadPairedDevices,
    loadPreferences,
    loadWorkflowRun,
    loadSessions,
    loadTerminalWindows,
    loadExternalPlugins,
    openSessionSource,
    openTerminalWindow,
    openWorkspaceWindow,
    loadVscodeStatus,
    moveOverlay,
    resizeOverlaySurface,
    activateOverlayWindow,
    reportOverlayGeometry,
    installExternalPlugin,
    interruptPrompt,
    removeExternalPlugin,
    readLocalImageDataUrl,
    refreshAgentRateLimits,
    rebindWorkflowSession,
    renameSession,
    canLinkCodexCli,
    isUnidentifiedCodexCli,
    revealBrowserCompanion,
    revokePairedDevice,
    launchAgentSession,
    savePreferences,
    watchShortcutRegistrationError,
    restoreTerminalLayout,
    setTerminalWorkflowEnabled,
    setTerminalWindowsVisible,
    steerQueuedPrompt,
    submitPrompt,
    takePendingShortcutAction,
    terminateSession,
    enableMobileGateway,
    setPairedDeviceScopes,
    revealPluginDirectory,
    type DisplayBackend,
  } from "$lib/lume";

  type View = "sessions" | "board" | "history" | "settings";
  type ShellStatus = SessionStatus | "idle";
  type ShortcutAction = "open" | "palette" | "new-session" | "whiteboard" | "workspace";
  type CompanionUpdateEvent = { mobileVersion: string };
  type ShortcutPreferenceKey =
    | "openShortcut"
    | "globalShortcut"
    | "newSessionShortcut"
    | "whiteboardShortcut"
    | "workspaceShortcut";
  type MonitorOption = { id: string; label: string };
  type UpdateState =
    | "idle"
    | "checking"
    | "up_to_date"
    | "available"
    | "downloading"
    | "ready"
    | "error";

  const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  const runtimePlatform = typeof navigator === "undefined"
    ? ""
    : `${navigator.userAgent} ${navigator.platform}`.toLowerCase();
  const isLinux = runtimePlatform.includes("linux") || runtimePlatform.includes("x11");
  const compactSize = { width: 78, height: 44 };
  const expandedWidth = 392;
  const expandedMaxHeight = 560;
  const devMobileDeviceId = "lume-mobile-dev-preview";
  const devMobileDevice: PairedDevice = {
    id: devMobileDeviceId,
    name: "Lume Mobile",
    createdAt: 0,
    scopes: ["monitor", "prompt", "approve"],
  };
  const expandedPanelMaxHeight = 544;

  let expanded = $state(!isTauri);
  let contentVisible = $state(!isTauri);
  let morphing = $state<"opening" | "closing" | null>(null);
  let morphProgress = $state(isTauri ? 0 : 1);
  let morphWidth = $state(compactSize.width);
  let morphHeight = $state(compactSize.height);
  let measuringPanel = $state(false);
  let expandedHeight = $state(expandedMaxHeight);
  let startupChooserOpen = $state(false);
  let view = $state<View>("sessions");
  let sessions = $state<AgentSession[]>(isTauri ? [] : structuredClone(demoSessions));
  let visibleAgentSessions = $derived(sessions.filter((session) => !isUnidentifiedCodexCli(session)));
  let preferences = $state<Preferences>({ ...defaultPreferences });
  let monitors = $state<MonitorOption[]>([]);
  let integrations = $state<IntegrationStatus[]>([]);
  let vscodeStatus = $state<CompanionStatus>({
    installed: false,
    configured: false,
    detail: "Verificando…",
  });
  let selectedId = $state<string | null>(null);
  // The Workspace's agent groups, shown here too. They are kept in localStorage, shared by both windows.
  let sidebarGroupState = $state<SidebarGroupState>({ groups: [], assignments: {}, order: [] });
  const orbSections = $derived(groupSessions(visibleAgentSessions, sidebarGroupState));
  function toggleOrbGroup(id: string) {
    sidebarGroupState = readSidebarGroups();
    sidebarGroupState = { ...sidebarGroupState, groups: sidebarGroupState.groups.map((group) => group.id === id ? { ...group, collapsed: !group.collapsed } : group) };
    writeSidebarGroups(sidebarGroupState);
  }
  $effect(() => {
    sidebarGroupState = readSidebarGroups();
    const sync = (event: StorageEvent) => { if (event.key === sidebarGroupsKey || event.key === null) sidebarGroupState = readSidebarGroups(); };
    window.addEventListener("storage", sync);
    return () => window.removeEventListener("storage", sync);
  });
  let inspectorSessionId = $state<string | null>(null);
  let orbInspectorSection = $state<"session" | "repository">("session");
  let permissionError = $state<string | null>(null);
  let questionSelections = $state<Record<string, string>>({});
  let savingSettings = $state(false);
  let configuringIntegration = $state<IntegrationStatus["kind"] | null>(null);
  let diagnosingIntegration = $state<IntegrationStatus["kind"] | null>(null);
  let integrationDiagnostics = $state<Partial<Record<IntegrationStatus["kind"], IntegrationDiagnostic>>>({});
  let configuringVscode = $state(false);
  let launcherOpen = $state(false);
  let launching = $state<IntegrationStatus["kind"] | null>(null);
  let launchingSessionId = $state<string | null>(null);
  let launchingPhase = $state<"choosing" | "opening" | null>(null);
  let launchError = $state<string | null>(null);
  let connectionAgent = $state<IntegrationStatus["kind"] | null>(null);
  let automationRequired = $state(false);
  let connectionMessage = $state("");
  let resumeAgent = $state<IntegrationStatus["kind"] | null>(null);
  let resumableSessions = $state<ResumableSession[]>([]);
  let loadingResumeAgent = $state<IntegrationStatus["kind"] | null>(null);
  let browserCompanionPath = $state<string | null>(null);
  let settingsMessage = $state<string | null>(null);
  let settingsMessageIsError = $state(false);
  let resetConfirming = $state(false);
  let resettingSettings = $state(false);
  let composerSessionId = $state<string | null>(null);
  let composerPrompt = $state("");
  let composerAttachments = $state<PromptAttachmentInput[]>([]);
  let composerMessage = $state<string | null>(null);
  let composerSending = $state(false);
  let steeringQueuedActivityId = $state<string | null>(null);
  let terminateConfirmId = $state<string | null>(null);
  let terminatingSessionId = $state<string | null>(null);
  let interruptingSessionId = $state<string | null>(null);
  let sessionActionMessage = $state<string | null>(null);
  let sessionActionMessageIsError = $state(false);
  let renamingSessionId = $state<string | null>(null);
  let cliAssociationSessionId = $state<string | null>(null);
  let cliContextMenu = $state<{ sessionId: string; x: number; y: number } | null>(null);
  let cliContextMenuNode = $state<HTMLDivElement | null>(null);
  let cliContextTrigger: HTMLButtonElement | null = null;

  function openCliContextMenu(session: AgentSession, trigger: HTMLButtonElement, x: number, y: number) {
    if (!canLinkCodexCli(session)) return;
    cliContextTrigger = trigger;
    cliContextMenu = {
      sessionId: session.id,
      x: Math.max(8, Math.min(x, window.innerWidth - 204)),
      y: Math.max(8, Math.min(y, window.innerHeight - 54)),
    };
    void tick().then(() => cliContextMenuNode?.querySelector<HTMLButtonElement>("button")?.focus());
  }

  $effect(() => {
    if (!cliContextMenu) return;
    const dismiss = () => { cliContextMenu = null; };
    const outside = (event: PointerEvent) => {
      if (event.target instanceof Node && !cliContextMenuNode?.contains(event.target)) dismiss();
    };
    const key = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        dismiss();
        cliContextTrigger?.focus();
      } else if (event.key === "Tab") dismiss();
    };
    window.addEventListener("pointerdown", outside);
    window.addEventListener("keydown", key);
    window.addEventListener("resize", dismiss);
    window.addEventListener("scroll", dismiss, true);
    return () => {
      window.removeEventListener("pointerdown", outside);
      window.removeEventListener("keydown", key);
      window.removeEventListener("resize", dismiss);
      window.removeEventListener("scroll", dismiss, true);
    };
  });
  let renameDraft = $state("");
  let renameError = $state<string | null>(null);
  let renamingSession = $state(false);
  let copiedResultId = $state<string | null>(null);
  let selectedProfileKey = $state<string | null>(null);
  let terminalWindows = $state<TerminalWindowState[]>([]);
  let workflowModeChanging = $state(false);
  let workflowSettingsOpen = $state(false);
  let workflowSettingsSaving = $state(false);
  let workflowRunStates = $state<Record<string, WorkflowRun | null>>({});
  let workflowRebindingStepId = $state<string | null>(null);
  let openingTerminal = $state<string | null>(null);
  let terminalOpenError = $state<string | null>(null);
  let terminalMessage = $state<string | null>(null);
  let terminalMessageIsError = $state(false);
  let layoutName = $state("");
  let selectedLayoutId = $state<string | null>(null);
  let restoringLayout = $state(false);
  let externalPlugins = $state<ExternalAgentPlugin[]>([]);
  let installingPlugin = $state(false);
  let pluginMessage = $state<string | null>(null);
  let pluginMessageIsError = $state(false);
  let componentBanners = $state<SystemBannerItem[]>([]);
  let navigationActivation = $state({ view: "", count: 0 });
  let paletteOpen = $state(false);
  let paletteTrigger: HTMLElement | null = null;
  let shortcutEditorKey = $state<ShortcutPreferenceKey | null>(null);
  let shortcutDraft = $state("");
  let shortcutEditorError = $state<string | null>(null);
  let shortcutRegistrationError = $state<string | null>(null);
  let paletteQuery = $state("");
  let paletteIndex = $state(0);
  let overlayPosition = $state({ x: 0, y: 12 });
  let compactAnchorPosition: { x: number; y: number } | null = null;
  let overlayReady = $state(false);
  let monitorBounds = $state({ x: 0, y: 0, width: 1920, height: 1080, scale: 1 });
  let overlayWorkArea = $state({ x: 0, y: 0, width: 1920, height: 1080 });
  let orbDock = $state<OrbDock>(freeOrbDock());
  let orbSettling = $state(false);
  let orbSettleRevision = 0;
  let orbSettleTarget: OrbPosition | null = null;
  let displayBackend = $state<DisplayBackend>("native");
  let dragging = $state(false);
  const orbPressure = $derived(dragging && !expanded && displayBackend !== "native-gnome"
    ? orbEdgePressure(overlayPosition, compactSize, overlayWorkArea, monitorBounds.scale)
    : orbDockPressure(orbDock));
  const orbRadii = $derived(orbCornerRadii(orbPressure));
  const orbDockName = $derived([orbDock.vertical, orbDock.horizontal].filter(Boolean).join("-") || "free");
  const orbSurfaceStyle = $derived([
    `--orb-radius: ${orbRadii.map((radius) => `${radius}px`).join(" ")}`,
    `--orb-scale-x: ${1 - (orbPressure.left + orbPressure.right) * 0.08}`,
    `--orb-scale-y: ${1 - (orbPressure.top + orbPressure.bottom) * 0.10}`,
    `--orb-origin-x: ${orbPressure.left ? "0%" : orbPressure.right ? "100%" : "50%"}`,
    `--orb-origin-y: ${orbPressure.top ? "0%" : orbPressure.bottom ? "100%" : "50%"}`,
    `--orb-content-x: ${(orbPressure.right - orbPressure.left) * compactSize.width * 0.04}px`,
    `--orb-content-y: ${(orbPressure.bottom - orbPressure.top) * compactSize.height * 0.05}px`,
  ].join("; "));
  const panelRadius = $derived(orbRadii
    .map((radius) => `${radius + (21 - radius) * morphProgress}px`).join(" "));
  let mascotAwake = $state(false);
  let mascotSleepTimer: ReturnType<typeof setTimeout> | undefined;
  let appVersion = $state("0.4.0");
  let updateState = $state<UpdateState>("idle");
  let availableVersion = $state<string | null>(null);
  let updateDetail = $state("Updates are checked automatically.");
  let updateProgress = $state<number | null>(null);
  let dismissedAgentAlertIds = $state<string[]>([]);
  let rateLimitRefreshRequested = false;
  let antigravityRateLimitRefreshRequested = false;
  let pendingUpdate: Update | null = null;
  let suppressCompactToggle = false;
  let dragState: {
    pointerId: number;
    startX: number;
    startY: number;
    originX: number;
    originY: number;
    scale: number;
    target: HTMLElement;
    compact: boolean;
    originDock: OrbDock;
  } | null = null;
  let pendingOverlayMove: { x: number; y: number } | null = null;
  let overlayMoveTask: Promise<void> | null = null;
  let systemDark = $state(false);
  let mobileStatus = $state<MobileGatewayStatus | null>(null);
  let pairedDevices = $state<PairedDevice[]>(dev ? [devMobileDevice] : []);
  let newMobileDevice = $state<PairedDevice | null>(null);
  const knownMobileDeviceIds = new Set<string>();
  let mobileDevicesInitialized = false;
  let pairingOffer = $state<MobilePairingOffer | null>(null);
  let pairingQr = $state<string | null>(null);
  let mobileBusy = $state(false);
  let mobileMessage = $state<string | null>(null);
  let mobileMessageIsError = $state(false);
  const mobileApkUrl = "https://github.com/Lume-agents/Lume/releases/latest/download/Lume-Mobile.apk";
  const startupRouteKey = "lume:startup-mode-routed:v1";
  let openingWorkspace = $state(false);
  let workspaceOpenError = $state<string | null>(null);

  function tr(english: string, portuguese: string) {
    return localize(preferences.language, english, portuguese);
  }

  async function showWorkspaceOpenFailure(reason: unknown) {
    workspaceOpenError = String(reason).replace(/^Error:\s*/, "");
    startupChooserOpen = false;
    if (!expanded) await toggleExpanded().catch(() => undefined);
  }

  async function showWorkspaceWindow(): Promise<boolean> {
    if (openingWorkspace) return false;
    openingWorkspace = true;
    workspaceOpenError = null;
    try {
      await openWorkspaceWindow();
      return true;
    } catch (reason) {
      await showWorkspaceOpenFailure(reason);
      return false;
    } finally {
      openingWorkspace = false;
    }
  }

  async function routeStartupMode() {
    if (!isTauri || sessionStorage.getItem(startupRouteKey)) return;
    sessionStorage.setItem(startupRouteKey, "true");
    if (preferences.startupMode === "workspace") {
      await showWorkspaceWindow();
      return;
    }
    if (preferences.startupMode === "ask") {
      startupChooserOpen = true;
      if (!expanded) await toggleExpanded();
    }
  }

  async function chooseStartupMode(mode: "orb" | "workspace", remember: boolean) {
    try {
      if (remember && !(await updatePreference("startupMode", mode))) return;
      if (mode === "workspace") {
        if (await showWorkspaceWindow()) startupChooserOpen = false;
        return;
      }
      if (expanded) await toggleExpanded();
      startupChooserOpen = false;
    } catch (error) {
      settingsMessageIsError = true;
      settingsMessage = String(error).replace(/^Error:\s*/, "");
    }
  }

  const systemBanners = $derived.by<SystemBannerItem[]>(() => {
    if (!expanded) return [];
    const items: Array<SystemBannerItem | null> = [
      ...componentBanners,
      permissionError ? { id: "permission-error", message: permissionError, tone: "error", onDismiss: () => (permissionError = null) } : null,
      launchError ? { id: "launch-error", message: launchError, tone: "error", onDismiss: () => (launchError = null) } : null,
      composerMessage ? { id: "composer-error", message: composerMessage, tone: "error", onDismiss: () => (composerMessage = null) } : null,
      sessionActionMessage ? { id: "session-message", message: sessionActionMessage, tone: sessionActionMessageIsError ? "error" : "info", onDismiss: () => (sessionActionMessage = null) } : null,
      renameError ? { id: "rename-error", message: renameError, tone: "error", onDismiss: () => (renameError = null) } : null,
      shortcutEditorError && shortcutEditorError !== settingsMessage ? { id: "shortcut-error", message: shortcutEditorError, tone: "error", onDismiss: () => (shortcutEditorError = null) } : null,
      shortcutRegistrationError ? {
        id: "shortcut-registration-error",
        message: `${tr("Global shortcuts could not be registered. Review them in Settings.", "Não foi possível registrar os atalhos globais. Revise-os nas Configurações.")} ${shortcutRegistrationError}`,
        tone: "warning",
        onDismiss: () => (shortcutRegistrationError = null),
      } : null,
      terminalMessage ? { id: "terminal-message", message: terminalMessage, tone: terminalMessageIsError ? "error" : "info", onDismiss: () => (terminalMessage = null) } : null,
      settingsMessage ? { id: "settings-message", message: settingsMessage, tone: settingsMessageIsError ? "error" : "success", onDismiss: () => {
        if (shortcutEditorError === settingsMessage) shortcutEditorError = null;
        settingsMessage = null;
      } } : null,
      pluginMessage ? { id: "plugin-message", message: pluginMessage, tone: pluginMessageIsError ? "error" : "info", onDismiss: () => (pluginMessage = null) } : null,
      mobileMessage ? { id: "mobile-message", message: mobileMessage, tone: mobileMessageIsError ? "error" : "success", onDismiss: () => (mobileMessage = null) } : null,
    ];
    const visibleItems = items.filter((item): item is SystemBannerItem => item !== null);
    if (workspaceOpenError) visibleItems.unshift({
      id: "workspace-open-error",
      message: workspaceOpenError,
      tone: "error",
      onDismiss: () => { workspaceOpenError = null; },
    });
    if (terminalOpenError) visibleItems.unshift({
      id: "terminal-open-error",
      message: terminalOpenError,
      tone: "error",
      onDismiss: () => (terminalOpenError = null),
    });
    for (const alert of collectAgentAlerts(sessions, preferences.language, Date.now(), { usageScope: "active" })) {
      if (dismissedAgentAlertIds.includes(alert.id) || $usageAlertDismissals.includes(alert.id)) continue;
      visibleItems.push({
        id: alert.id,
        message: alert.message,
        tone: alert.tone,
        duration: alert.duration,
        onDismiss: () => dismissAgentAlert(alert.id),
      });
    }
    return visibleItems;
  });

  function reportPanelBanner(notice: SystemBannerNotice) {
    componentBanners = [
      {
        ...notice,
        onDismiss: () => {
          componentBanners = componentBanners.filter((item) => item.id !== notice.id);
          notice.onDismiss?.();
        },
      },
      ...componentBanners.filter((item) => item.id !== notice.id),
    ].slice(0, 4);
  }
  setContext<SystemBannerReporter>(SYSTEM_BANNER_CONTEXT, reportPanelBanner);

  function activateNavigation(nextView: typeof view) {
    navigationActivation = { view: nextView, count: navigationActivation.count + 1 };
    void openView(nextView).catch((reason) => reportPanelBanner({
      id: "panel-navigation-error",
      message: String(reason).replace(/^Error:\s*/, ""),
      tone: "error",
    }));
  }

  function dismissAgentAlert(id: string) {
    usageAlertDismissals.dismiss(id);
    if (dismissedAgentAlertIds.includes(id)) return;
    dismissedAgentAlertIds = [...dismissedAgentAlertIds, id].slice(-200);
  }

  function withDevMobileDevice(devices: PairedDevice[]) {
    return dev && !devices.some((device) => device.id === devMobileDeviceId)
      ? [devMobileDevice, ...devices]
      : devices;
  }

  function applyPairedDevices(devices: PairedDevice[], announceNew: boolean) {
    const realDevices = devices.filter((device) => device.id !== devMobileDeviceId);
    const addedDevice = announceNew
      ? [...realDevices]
          .sort((left, right) => right.createdAt - left.createdAt)
          .find((device) => !knownMobileDeviceIds.has(device.id))
      : undefined;
    for (const device of realDevices) knownMobileDeviceIds.add(device.id);
    pairedDevices = withDevMobileDevice(realDevices);
    mobileDevicesInitialized = true;
    if (addedDevice) newMobileDevice = addedDevice;
  }

  function shown(value: string) {
    return displayText(preferences.language, value);
  }

  function sessionDisplayName(session: AgentSession) {
    return session.sessionName?.trim() || session.project?.trim() || session.agentLabel;
  }

  function sessionDirectoryName(session: AgentSession) {
    const directory = session.workingDirectory?.trim().replace(/[\\/]+$/, "");
    return directory?.split(/[\\/]/).pop() || session.project?.trim() || session.agentLabel;
  }

  function currentExpandedSize() {
    return { width: expandedWidth, height: expandedHeight };
  }

  function applyExpandedHeight(nextHeight: number, resizeWindow: boolean) {
    const clampedHeight = Math.min(
      expandedMaxHeight,
      Math.max(compactSize.height, Math.ceil(nextHeight)),
    );
    if (clampedHeight === expandedHeight) return;
    const previousSize = currentExpandedSize();
    const anchor = compactAnchorPosition ?? compactPositionFromExpanded(overlayPosition, previousSize);
    expandedHeight = clampedHeight;
    if (resizeWindow && isTauri && expanded && !morphing) {
      compactAnchorPosition = anchor;
      const target = currentExpandedSize();
      const position = expandedPositionFromCompact(anchor, target);
      overlayPosition = position;
      void Promise.all([
        setOverlaySurfaceSize(target.width, target.height),
        moveOverlay(position.x, position.y, false, preferences.monitorId),
      ]).then(() => settleOverlaySize(target)).catch(() => undefined);
    }
  }

  // X11 applies resizes asynchronously, so the window can end up at a size from the middle of
  // the open/close animation and cut the panel. Check the real sizes and ask again if needed.
  let overlaySettleToken = 0;
  async function settleOverlaySize(target: { width: number; height: number }) {
    if (!isTauri || !isLinux) return;
    const token = ++overlaySettleToken;
    await settleSurfaceSize({
      target,
      shouldStop: () => token !== overlaySettleToken || morphing !== null,
      wait: (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds)),
      measure: async () => {
        const window = getCurrentWindow();
        const [outer, scale] = await Promise.all([window.outerSize(), window.scaleFactor()]);
        return [
          { width: globalThis.innerWidth, height: globalThis.innerHeight },
          { width: outer.width / scale, height: outer.height / scale },
        ];
      },
      apply: () => setOverlaySurfaceSize(target.width, target.height, true),
      onMismatch: ({ attempt, measured }) => {
        const sizes = measured.map((size) => `${Math.round(size.width)}x${Math.round(size.height)}`).join(" / ");
        void reportOverlayGeometry(`esperado ${target.width}x${target.height}, viewport/janela ${sizes} (tentativa ${attempt + 1})`).catch(() => undefined);
      },
    });
  }

  function observePanelSize(node: HTMLElement) {
    let resizeFrame: number | null = null;

    const syncHeight = (resizeWindow: boolean) => {
      applyExpandedHeight(node.offsetHeight, resizeWindow);
    };

    const observer = new ResizeObserver(() => {
      if (!expanded || morphing) return;
      if (resizeFrame !== null) cancelAnimationFrame(resizeFrame);
      resizeFrame = requestAnimationFrame(() => {
        resizeFrame = null;
        syncHeight(true);
      });
    });

    observer.observe(node);
    syncHeight(false);

    return {
      destroy() {
        observer.disconnect();
        if (resizeFrame !== null) cancelAnimationFrame(resizeFrame);
      },
    };
  }

  let launcherSurfaceSync = 0;
  async function syncLauncherSurface(open: boolean) {
    const sync = ++launcherSurfaceSync;
    await tick();
    if (sync !== launcherSurfaceSync || !expanded || morphing) return;
    const panel = document.querySelector<HTMLElement>(".panel");
    if (!panel) return;
    applyExpandedHeight(open ? expandedMaxHeight : panel.offsetHeight, true);
  }

  $effect(() => {
    const open = launcherOpen;
    if (expanded && !morphing) void syncLauncherSurface(open);
  });

  const effectiveDark = $derived(preferences.darkMode ?? systemDark);
  const appearance = $derived(appearanceAttributes(preferences));
  $effect(() => { void ensureCustomFonts(preferences); });
  const darkWorkspaceOpacity = $derived(
    preferences.workspaceDarkBackgroundColor
      ? preferences.workspaceDarkBackgroundOpacity
      : preferences.workspaceBackgroundColor
        ? preferences.workspaceBackgroundOpacity
        : preferences.workspaceDarkBackgroundOpacity
  );
  $effect(() => {
    const root = document.documentElement;
    root.dataset.theme = effectiveDark ? "dark" : "light";
    root.dataset.appearance = appearance.theme;
    if (appearance.accentCss) {
      root.style.setProperty("--lume-accent", appearance.accentCss);
      root.style.setProperty("--lume-accent-strong", appearance.accentCss);
    } else {
      root.style.removeProperty("--lume-accent");
      root.style.removeProperty("--lume-accent-strong");
    }
  });
  const activeCount = $derived(
    visibleAgentSessions.filter((session) =>
      ["running", "permission_required", "waiting_for_input"].includes(session.status),
    ).length,
  );
  function needsAttention(session: AgentSession) {
    return Boolean(session.pendingPermission || session.pendingQuestion)
      || ["permission_required", "waiting_for_input", "failed"].includes(session.status);
  }

  const inspectedSession = $derived(
    visibleAgentSessions.find((session) => session.id === inspectorSessionId)
      ?? visibleAgentSessions.find(needsAttention)
      ?? visibleAgentSessions.find((session) => session.status === "running")
      ?? visibleAgentSessions[0]
      ?? null,
  );
  const detectedProjects = $derived.by(() => {
    const projects = new Map<string, string>();
    for (const [key, profile] of Object.entries(preferences.projectProfiles)) {
      if (profile.label) projects.set(key, profile.label);
    }
    for (const session of visibleAgentSessions) {
      projects.set(projectKey(session.workingDirectory ?? session.project), session.project);
    }
    return Array.from(projects, ([key, label]) => ({ key, label })).sort((left, right) =>
      left.label.localeCompare(right.label),
    );
  });
  const selectedProject = $derived(
    detectedProjects.find((project) => project.key === selectedProfileKey),
  );
  const selectedProjectProfile = $derived(
    selectedProfileKey ? preferences.projectProfiles[selectedProfileKey] : undefined,
  );

  const shellStatus = $derived.by<ShellStatus>(() => {
    if (visibleAgentSessions.length === 0) return "idle";
    if (visibleAgentSessions.some((session) => session.status === "permission_required")) {
      return "permission_required";
    }
    if (visibleAgentSessions.some((session) => session.status === "failed")) return "failed";
    if (visibleAgentSessions.some((session) => session.status === "running")) return "running";
    if (visibleAgentSessions.some((session) => session.status === "completed")) return "completed";
    if (visibleAgentSessions.some((session) => session.status === "waiting_for_input")) {
      return "waiting_for_input";
    }
    return "idle";
  });

  onMount(() => {
    const colorScheme = window.matchMedia("(prefers-color-scheme: dark)");
    const syncSystemTheme = (event: MediaQueryListEvent | MediaQueryList) => {
      systemDark = event.matches;
    };
    const finishOverlayDragFromWindow = (event: PointerEvent) => {
      if (!dragState || dragState.pointerId !== event.pointerId) return;
      void endOverlayDrag(event, dragState.compact);
    };
    syncSystemTheme(colorScheme);
    colorScheme.addEventListener("change", syncSystemTheme);
    window.addEventListener("keydown", handleAppShortcut);
    window.addEventListener("pointerdown", focusOverlayOnPointerDown, true);
    window.addEventListener("pointerup", finishOverlayDragFromWindow, true);
    window.addEventListener("pointercancel", finishOverlayDragFromWindow, true);
    let disposed = false;
    let stopListening: (() => void) | undefined;
    let stopTerminalListening: (() => void) | undefined;
    let stopShortcutListening: (() => void) | undefined;
    let stopPreferencesListening: (() => void) | undefined;
    let stopCompanionUpdateListening: (() => void) | undefined;
    let stopMobileDeviceListening: (() => void) | undefined;
    let stopWorkspaceFailureListening: (() => void) | undefined;
    let stopShortcutStatusListening: (() => void) | undefined;
    if (isTauri) {
      void watchShortcutRegistrationError((error) => {
        if (!disposed) shortcutRegistrationError = error;
      }).then((stop) => {
        if (disposed) stop();
        else stopShortcutStatusListening = stop;
      }).catch(() => undefined);
      void listen<string>("lume://workspace-open-failed", ({ payload }) => {
        if (!disposed) void showWorkspaceOpenFailure(payload);
      }).then((stop) => {
        if (disposed) stop();
        else stopWorkspaceFailureListening = stop;
      }).catch(() => undefined);
    }
    let pollTimer: ReturnType<typeof setInterval> | undefined;
    let updateTimer: ReturnType<typeof setInterval> | undefined;
    let resumeRefreshTimer: ReturnType<typeof setTimeout> | undefined;
    let sessionRefreshTimer: ReturnType<typeof setTimeout> | undefined;
    const refreshAfterResume = () => {
      void refreshSessions(false);
      if (resumeRefreshTimer) clearTimeout(resumeRefreshTimer);
      resumeRefreshTimer = setTimeout(() => void refreshSessions(false), 2_500);
    };
    const refreshWhenVisible = () => {
      if (document.visibilityState === "visible") refreshAfterResume();
    };

    window.addEventListener("focus", refreshAfterResume);
    window.addEventListener("pageshow", refreshAfterResume);
    document.addEventListener("visibilitychange", refreshWhenVisible);

    updateTimer = setInterval(() => void checkForUpdates(), 6 * 60 * 60 * 1_000);

    void (async () => {
      const [nextPreferences, nextDisplayBackend] = await Promise.all([
        loadPreferences(),
        loadDisplayBackend(),
      ]);
      if (disposed) return;
      preferences = nextPreferences;
      displayBackend = nextDisplayBackend;
      try {
        overlayPosition = await loadOverlayPosition();
        overlayReady = true;
      } catch {
        if (preferences.overlayX !== undefined && preferences.overlayY !== undefined) {
          overlayPosition = { x: preferences.overlayX, y: preferences.overlayY };
          overlayReady = true;
        }
      }
      await loadMonitorOptions();
      await positionWindow();
      overlayReady = true;
      await routeStartupMode();

      const [nextSessions, nextIntegrations, nextVscodeStatus, nextPlugins, nextTerminals] = await Promise.all([
        loadSessions(),
        loadIntegrationStatuses(),
        loadVscodeStatus(),
        loadExternalPlugins(),
        loadTerminalWindows().catch(() => []),
      ]);
      if (disposed) return;
      sessions = nextSessions;
      if (!rateLimitRefreshRequested && sessions.some((session) => session.agent === "codex")) {
        rateLimitRefreshRequested = true;
        void refreshAgentRateLimits("codex")
          .then(() => refreshSessions(false))
          .catch(() => undefined);
      }
      if (!antigravityRateLimitRefreshRequested && sessions.some((session) => session.agent === "antigravity")) {
        antigravityRateLimitRefreshRequested = true;
        void refreshAgentRateLimits("antigravity")
          .then(() => refreshSessions(false))
          .catch(() => undefined);
      }
      selectedProfileKey = detectedProjects[0]?.key ?? null;
      void initializeUpdater();
      integrations = nextIntegrations;
      vscodeStatus = nextVscodeStatus;
      externalPlugins = nextPlugins;
      terminalWindows = nextTerminals;
      selectedLayoutId = preferences.whiteboardLayouts[0]?.id ?? null;
      layoutName = preferences.whiteboardLayouts[0]?.name ?? "";
      selectedId =
        sessions.find((session) => session.status === "permission_required")?.id ?? null;

      if (isTauri) {
        stopMobileDeviceListening = await listen<PairedDevice>(
          "lume://mobile-device-paired",
          ({ payload }) => {
            applyPairedDevices([
              ...pairedDevices.filter(
                (device) =>
                  device.id !== devMobileDeviceId && device.id !== payload.id,
              ),
              payload,
            ], true);
          },
        );
        try {
          applyPairedDevices(await loadPairedDevices(), false);
        } catch {
          // Mobile settings still expose the connection error when opened.
        }
        stopListening = await listen("lume://sessions-changed", () => {
          if (sessionRefreshTimer) clearTimeout(sessionRefreshTimer);
          sessionRefreshTimer = setTimeout(() => {
            sessionRefreshTimer = undefined;
            void refreshSessions(true);
          }, 120);
        });
        stopTerminalListening = await listen("lume://terminal-windows-changed", () => {
          void refreshTerminalWindows();
        });
        stopShortcutListening = await listen<ShortcutAction>("lume://shortcut", ({ payload }) => {
          void runShortcutAction(payload);
        });
        stopPreferencesListening = await listen<Preferences>("lume://preferences-changed", ({ payload }) => {
          preferences = payload;
        });
        stopCompanionUpdateListening = await listen<CompanionUpdateEvent>(
          "lume://companion-update-check",
          ({ payload }) => {
            void handleCompanionUpdateRequest(payload);
          },
        );
        const pendingShortcut = await takePendingShortcutAction();
        if (pendingShortcut) void runShortcutAction(pendingShortcut);
        pollTimer = setInterval(() => void refreshSessions(false), 15_000);
      }
    })();

    return () => {
      disposed = true;
      stopListening?.();
      stopTerminalListening?.();
      stopShortcutListening?.();
      stopPreferencesListening?.();
      stopCompanionUpdateListening?.();
      stopMobileDeviceListening?.();
      stopWorkspaceFailureListening?.();
      stopShortcutStatusListening?.();
      colorScheme.removeEventListener("change", syncSystemTheme);
      window.removeEventListener("focus", refreshAfterResume);
      window.removeEventListener("pageshow", refreshAfterResume);
      document.removeEventListener("visibilitychange", refreshWhenVisible);
      window.removeEventListener("keydown", handleAppShortcut);
      window.removeEventListener("pointerdown", focusOverlayOnPointerDown, true);
      window.removeEventListener("pointerup", finishOverlayDragFromWindow, true);
      window.removeEventListener("pointercancel", finishOverlayDragFromWindow, true);
      if (pollTimer) clearInterval(pollTimer);
      if (updateTimer) clearInterval(updateTimer);
      if (resumeRefreshTimer) clearTimeout(resumeRefreshTimer);
      if (sessionRefreshTimer) clearTimeout(sessionRefreshTimer);
      if (mascotSleepTimer) clearTimeout(mascotSleepTimer);
      orbSettleRevision += 1;
      if (pendingUpdate) void pendingUpdate.close();
    };
  });

  async function initializeUpdater() {
    if (!isTauri) {
      updateState = "up_to_date";
      return;
    }

    try {
      appVersion = await getVersion();
    } catch {
      // Keep the package version as fallback.
    }
    await checkForUpdates();
  }

  async function checkForUpdates(): Promise<Update | null> {
    if (
      !isTauri ||
      updateState === "checking" ||
      updateState === "downloading" ||
      updateState === "ready"
    ) return pendingUpdate;
    updateState = "checking";
    updateDetail = tr("Checking for a new version…", "Procurando uma nova versão…");
    updateProgress = null;

    try {
      const nextUpdate = await check({
        timeout: 15_000,
        headers: {
          "Cache-Control": "no-cache",
          Pragma: "no-cache",
        },
      });
      if (pendingUpdate && pendingUpdate !== nextUpdate) await pendingUpdate.close();
      pendingUpdate = nextUpdate;
      availableVersion = nextUpdate?.version ?? null;
      if (nextUpdate) {
        updateState = "available";
        updateDetail = tr(
          `Version ${nextUpdate.version} is ready to download.`,
          `A versão ${nextUpdate.version} está pronta para baixar.`,
        );
      } else {
        updateState = "up_to_date";
        updateDetail = tr("You are using the latest version.", "Você está usando a versão mais recente.");
      }
      return nextUpdate;
    } catch (error) {
      updateState = "error";
      updateDetail = tr(
        "Could not check for updates right now. Try again shortly.",
        "Não foi possível verificar agora. Tente novamente em instantes.",
      );
      reportPanelBanner({ id: "update-check-error", message: `${updateDetail} ${String(error).replace(/^Error:\s*/, "")}`, tone: "error" });
      return null;
    }
  }

  async function handleUpdateButton() {
    if (updateState === "checking" || updateState === "downloading" || updateState === "ready") {
      return;
    }
    await checkForUpdates();
  }

  async function handleInstallUpdate() {
    if (updateState !== "available") return;
    const latestUpdate = await checkForUpdates();
    if (latestUpdate && updateState === "available") await installAvailableUpdate();
  }

  async function handleCompanionUpdateRequest(payload: CompanionUpdateEvent) {
    for (let attempt = 0; updateState === "checking" && attempt < 40; attempt += 1) {
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    if (!["downloading", "ready"].includes(updateState)) {
      await checkForUpdates();
    }
    if (!["available", "downloading", "ready"].includes(updateState)) return;
    if (!expanded) await toggleExpanded();
    await openView("settings");
    await tick();
    document.querySelector<HTMLElement>("[data-update-card]")?.scrollIntoView({
      behavior: "smooth",
      block: "center",
    });
    updateDetail = tr(
      `Lume Mobile ${payload.mobileVersion} found this desktop update.`,
      `O Lume Mobile ${payload.mobileVersion} encontrou esta atualização do desktop.`,
    );
  }

  async function installAvailableUpdate() {
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
    } catch (error) {
      updateState = "error";
      updateDetail = tr(
        "The update could not be installed. Try again.",
        "A atualização não pôde ser instalada. Tente novamente.",
      );
      reportPanelBanner({ id: "update-install-error", message: `${updateDetail} ${String(error).replace(/^Error:\s*/, "")}`, tone: "error" });
      updateProgress = null;
    }
  }

  const announcedUsageAlerts = new Set<string>();
  let usageAlertsSeeded = false;

  async function refreshSessions(withSound: boolean) {
    const next = await loadSessions();
    // A usage alert that was not there before gets its own chime (the first load only records them).
    const freshUsageAlert = collectAgentAlerts(next, preferences.language, Date.now(), { usageScope: "active" })
      .filter((alert) => alert.usage && !announcedUsageAlerts.has(alert.id) && !$usageAlertDismissals.includes(alert.id));
    if (!usageAlertsSeeded || withSound) freshUsageAlert.forEach((alert) => announcedUsageAlerts.add(alert.id));
    if (usageAlertsSeeded && withSound && freshUsageAlert.length && preferences.soundEnabled) playTone("usage");
    usageAlertsSeeded = true;
    if (withSound && preferences.soundEnabled) {
      const soundKey = (session: AgentSession) =>
        session.nativeSessionId
          ? `${session.agent}:${session.nativeSessionId}`
          : session.id;
      const previous = new Map(visibleAgentSessions.map((session) => [soundKey(session), session.status]));
      const played = new Set<string>();
      for (const session of next.filter((item) => !isUnidentifiedCodexCli(item))) {
        if (!projectSoundEnabled(session)) continue;
        const key = soundKey(session);
        const previousStatus = previous.get(key);
        if (previousStatus === session.status) continue;
        if (session.status === "permission_required" && !played.has(`permission:${key}`)) {
          played.add(`permission:${key}`);
          playTone("permission");
        }
        if (
          session.status === "completed" &&
          (previousStatus === "running" || previousStatus === "permission_required") &&
          !played.has(`completed:${key}`)
        ) {
          played.add(`completed:${key}`);
          playTone("completed");
        }
        if (session.status === "failed" && previousStatus && !played.has(`failed:${key}`)) {
          played.add(`failed:${key}`);
          playTone("failed");
        }
      }
    }
    sessions = next;
  }

  async function loadMonitorOptions() {
    if (!isTauri) return;
    try {
      const found = await availableMonitors();
      monitors = found.map((monitor, index) => ({
        id: monitor.name ?? `monitor-${index}`,
        label: monitor.name ?? `Monitor ${index + 1}`,
      }));
    } catch {
      monitors = [];
    }
  }

  async function positionWindow(resetPosition = false) {
    if (!isTauri) return;
    try {
      const target = expanded ? currentExpandedSize() : compactSize;
      await setOverlaySurfaceSize(target.width, target.height);

      const found = await availableMonitors();
      const configured = preferences.monitorId
        ? found.find((monitor, index) =>
            (monitor.name ?? `monitor-${index}`) === preferences.monitorId,
          )
        : undefined;
      const monitor = configured ?? (await primaryMonitor());
      if (!monitor) return;
      const scale = monitor.scaleFactor || 1;
      monitorBounds = {
        x: monitor.position.x,
        y: monitor.position.y,
        width: monitor.size.width,
        height: monitor.size.height,
        scale,
      };
      const workArea = monitor.workArea ?? { position: monitor.position, size: monitor.size };
      overlayWorkArea = {
        x: workArea.position.x - monitor.position.x,
        y: workArea.position.y - monitor.position.y,
        width: workArea.size.width,
        height: workArea.size.height,
      };
      if (!overlayReady || resetPosition) {
        overlayPosition = {
          x:
            preferences.overlayX ??
            Math.max(0, Math.round((monitor.size.width - target.width * scale) / 2)),
          y:
            preferences.overlayY ??
            (isLinux ? Math.round(44 * scale) : 12),
        };
        overlayReady = true;
      }
      if (expanded && compactAnchorPosition) {
        compactAnchorPosition = pinOrbPosition(
          compactAnchorPosition, resetPosition ? freeOrbDock() : orbDock,
          compactSize, overlayWorkArea, scale,
        );
        orbDock = orbDockAtPosition(compactAnchorPosition, compactSize, overlayWorkArea, scale);
        overlayPosition = expandedPositionFromCompact(compactAnchorPosition, target);
      } else {
        overlayPosition = clampOverlayPosition(overlayPosition.x, overlayPosition.y, target);
        if (!expanded) {
          overlayPosition = pinOrbPosition(
            overlayPosition, resetPosition ? freeOrbDock() : orbDock,
            compactSize, overlayWorkArea, scale,
          );
          orbDock = orbDockAtPosition(overlayPosition, compactSize, overlayWorkArea, scale);
        }
      }
      await moveOverlay(overlayPosition.x, overlayPosition.y, false, preferences.monitorId);
    } catch {
      // Alguns compositores Wayland ignoram posicionamento solicitado pelo cliente.
    }
  }

  async function toggleExpanded() {
    if (suppressCompactToggle) {
      suppressCompactToggle = false;
      return;
    }
    if (!overlayReady) {
      await positionWindow();
      if (!overlayReady) return;
    }
    if (morphing) return;
    if (orbSettling) {
      interruptOrbSettlement();
      await waitForOverlayMoves();
    }
    const opening = !expanded;
    let expandedTarget = currentExpandedSize();

    void setTerminalWindowsVisible(opening).catch((error) => {
      terminalMessageIsError = true;
      terminalMessage = String(error).replace(/^Error:\s*/, "");
    });
    morphing = opening ? "opening" : "closing";
    contentVisible = false;
    if (opening) {
      expanded = true;
      measuringPanel = true;
      morphProgress = 1;
      await tick();
      const panel = document.querySelector<HTMLElement>(".panel");
      if (panel) {
        expandedHeight = Math.min(
          expandedMaxHeight,
          Math.max(compactSize.height, Math.ceil(panel.offsetHeight)),
        );
      }
      expandedTarget = currentExpandedSize();
      measuringPanel = false;
      morphProgress = 0;
      morphWidth = compactSize.width;
      morphHeight = compactSize.height;
      await tick();
    } else {
      morphWidth = expandedTarget.width;
      morphHeight = expandedTarget.height;
    }

    const compactTargetPosition = opening
      ? { ...overlayPosition }
      : compactAnchorPosition ??
        compactPositionFromExpanded(overlayPosition, expandedTarget);
    const expandedTargetPosition = expandedPositionFromCompact(
      compactTargetPosition,
      expandedTarget,
    );

    compactAnchorPosition = compactTargetPosition;
    await animateCapsule(
      opening,
      expandedTarget,
      compactTargetPosition,
      expandedTargetPosition,
    );

    if (opening) {
      contentVisible = true;
    } else {
      expanded = false;
      morphProgress = 0;
      await tick();
      compactAnchorPosition = null;
      selectedId = null;
      view = "sessions";
      launcherOpen = false;
    }
    morphing = null;
    if (opening) {
      // Content can finish mounting while the morph is in progress, when the
      // resize observer is intentionally paused. Re-measure once it is visible.
      await tick();
      const panel = document.querySelector<HTMLElement>(".panel");
      if (panel) applyExpandedHeight(panel.offsetHeight, true);
    }
    void settleOverlaySize(opening ? currentExpandedSize() : compactSize);
  }

  async function animateCapsule(
    opening: boolean,
    expandedTarget: { width: number; height: number },
    compactTargetPosition: { x: number; y: number },
    expandedTargetPosition: { x: number; y: number },
  ) {
    const from = opening ? compactSize : expandedTarget;
    const to = opening ? expandedTarget : compactSize;
    const fromPosition = opening ? compactTargetPosition : expandedTargetPosition;
    const toPosition = opening ? expandedTargetPosition : compactTargetPosition;

    if (!isTauri) {
      morphProgress = opening ? 1 : 0;
      overlayPosition = { ...toPosition };
      return;
    }

    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const duration = reducedMotion ? 1 : opening ? 300 : 260;
    const startedAt = performance.now();

    await new Promise<void>((resolve) => {
      const frame = async (now: number) => {
        const linear = Math.min(1, (now - startedAt) / duration);
        const eased = morphEase(linear);
        morphProgress = opening ? eased : 1 - eased;
        if (opening && eased >= 0.38) contentVisible = true;

        await applyCapsuleGeometry({
          width: Math.round(from.width + (to.width - from.width) * eased),
          height: Math.round(from.height + (to.height - from.height) * eased),
          x: Math.round(fromPosition.x + (toPosition.x - fromPosition.x) * eased),
          y: Math.round(fromPosition.y + (toPosition.y - fromPosition.y) * eased),
        });

        if (linear < 1) {
          requestAnimationFrame((next) => void frame(next));
        } else {
          resolve();
        }
      };
      requestAnimationFrame((now) => void frame(now));
    });

    await applyCapsuleGeometry({
      width: to.width,
      height: to.height,
      x: toPosition.x,
      y: toPosition.y,
    });
    morphProgress = opening ? 1 : 0;
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  }

  async function applyCapsuleGeometry(geometry: {
    width: number;
    height: number;
    x: number;
    y: number;
  }) {
    morphWidth = geometry.width;
    morphHeight = geometry.height;
    await tick();

    const tasks: Promise<unknown>[] = [
      setOverlaySurfaceSize(geometry.width, geometry.height, true),
    ];
    if (geometry.x !== overlayPosition.x || geometry.y !== overlayPosition.y) {
      tasks.push(moveOverlay(geometry.x, geometry.y, false, preferences.monitorId));
    }
    await Promise.allSettled(tasks);
    overlayPosition = { x: geometry.x, y: geometry.y };
  }

  const queueOverlaySurfaceSize = createSurfaceSizeQueue(async ({ width, height, syncLinuxSurface }) => {
    const size = new LogicalSize(width, height);
    if (isLinux && (displayBackend !== "native-gnome" || syncLinuxSurface)) {
      // Release the old WebView minimum before shrinking its GTK parent.
      await getCurrentWebview().setSize(size);
      await resizeOverlaySurface(width, height);
    }
    await getCurrentWindow().setSize(size);
  });

  function setOverlaySurfaceSize(width: number, height: number, syncLinuxSurface = false) {
    return queueOverlaySurfaceSize({ width, height, syncLinuxSurface });
  }

  function morphEase(value: number) {
    return value < 0.5
      ? 4 * value * value * value
      : 1 - Math.pow(-2 * value + 2, 3) / 2;
  }

  function clampOverlayPosition(
    x: number,
    y: number,
    target = expanded ? currentExpandedSize() : compactSize,
  ) {
    return clampOrbPosition({ x, y }, target, overlayWorkArea, monitorBounds.scale);
  }

  function expandedPositionFromCompact(
    compactPosition: { x: number; y: number },
    target = currentExpandedSize(),
  ) {
    return expandedPositionForOrb(
      compactPosition, compactSize, target, overlayWorkArea, monitorBounds.scale, orbDock,
    );
  }

  function compactPositionFromExpanded(
    expandedPosition: { x: number; y: number },
    source = currentExpandedSize(),
  ) {
    return compactPositionForPanel(
      expandedPosition, source, compactSize, overlayWorkArea, monitorBounds.scale,
    );
  }

  // `detail` of a pointerdown is not a click count in every webview (WebView2 reports 0), so a double
  // press is also recognized from two presses close in time and place.
  let lastOverlayPress = { at: 0, x: 0, y: 0 };
  function isOverlayDoublePress(event: PointerEvent) {
    const double = event.detail === 2
      || (event.timeStamp - lastOverlayPress.at < 420
        && Math.hypot(event.clientX - lastOverlayPress.x, event.clientY - lastOverlayPress.y) < 6);
    lastOverlayPress = double ? { at: 0, x: 0, y: 0 } : { at: event.timeStamp, x: event.clientX, y: event.clientY };
    return double;
  }

  function beginOverlayDrag(event: PointerEvent, compact = false) {
    if (!isTauri || !overlayReady || event.button !== 0 || morphing) return;
    bringOverlayToFront();
    if (!compact && (event.target as HTMLElement).closest("button, input, select, textarea, .system-banner-stack")) {
      return;
    }
    if (!compact && isOverlayDoublePress(event)) {
      event.preventDefault();
      void toggleExpanded();
      return;
    }
    if (displayBackend === "native-gnome") {
      orbDock = freeOrbDock();
      dragging = true;
      void getCurrentWindow()
        .startDragging()
        .catch(() => undefined)
        .finally(() => {
          setTimeout(() => {
            dragging = false;
          }, 120);
        });
      return;
    }
    interruptOrbSettlement();
    const target = event.currentTarget as HTMLElement;
    target.setPointerCapture(event.pointerId);
    dragState = {
      pointerId: event.pointerId,
      startX: event.screenX,
      startY: event.screenY,
      originX: overlayPosition.x,
      originY: overlayPosition.y,
      scale: monitorBounds.scale,
      target,
      compact,
      originDock: { ...orbDock },
    };
    dragging = false;
  }

  // The listener must not pass its event as `force`.
  function focusOverlayOnPointerDown() {
    void bringOverlayToFront();
  }

  async function bringOverlayToFront(force = false) {
    if (!isTauri) return;
    // A click on a window that already has the keyboard needs no request.
    if (!force && document.hasFocus()) return;
    // In the XWayland fallback the window manager decides who has the keyboard (the terminal
    // behind the Orb keeps it otherwise), so ask the way a user's action asks. Elsewhere,
    // or if that is not available, the ordinary focus request is used.
    const activated = isLinux ? await activateOverlayWindow().catch(() => false) : false;
    if (!activated) await getCurrentWindow().setFocus().catch(() => undefined);
  }

  function wakeMascot() {
    if (shellStatus !== "idle") return;
    mascotAwake = true;
    if (mascotSleepTimer) clearTimeout(mascotSleepTimer);
    mascotSleepTimer = setTimeout(() => {
      mascotAwake = false;
      mascotSleepTimer = undefined;
    }, 1_600);
  }

  function moveOverlayDrag(event: PointerEvent) {
    if (!dragState || dragState.pointerId !== event.pointerId) return;
    const dx = (event.screenX - dragState.startX) * dragState.scale;
    const dy = (event.screenY - dragState.startY) * dragState.scale;
    if (!dragging && Math.hypot(dx, dy) < 3) return;
    if (!dragging) orbDock = freeOrbDock();
    dragging = true;
    event.preventDefault();
    overlayPosition = clampOverlayPosition(
      dragState.originX + dx,
      dragState.originY + dy,
    );
    queueOverlayMove(overlayPosition.x, overlayPosition.y);
  }

  function queueOverlayMove(x: number, y: number) {
    pendingOverlayMove = { x, y };
    if (overlayMoveTask) return;
    overlayMoveTask = (async () => {
      while (pendingOverlayMove) {
        const next = pendingOverlayMove;
        pendingOverlayMove = null;
        await moveOverlay(next.x, next.y, false, preferences.monitorId);
      }
    })()
      .catch(() => undefined)
      .finally(() => {
        overlayMoveTask = null;
        if (pendingOverlayMove) queueOverlayMove(pendingOverlayMove.x, pendingOverlayMove.y);
      });
  }

  async function waitForOverlayMoves() {
    while (overlayMoveTask) await overlayMoveTask;
  }

  function interruptOrbSettlement() {
    if (!orbSettling) return;
    orbSettleRevision += 1;
    orbSettling = false;
    if (orbSettleTarget) {
      overlayPosition = { ...orbSettleTarget };
      queueOverlayMove(overlayPosition.x, overlayPosition.y);
      orbSettleTarget = null;
    }
  }

  async function settleOrbPosition(position: OrbPosition) {
    const origin = { ...overlayPosition };
    const revision = ++orbSettleRevision;
    if (origin.x === position.x && origin.y === position.y) {
      queueOverlayMove(position.x, position.y);
      await waitForOverlayMoves();
      return;
    }
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const duration = reducedMotion ? 0 : 160;
    const startedAt = performance.now();
    orbSettleTarget = position;
    orbSettling = true;
    await new Promise<void>((resolve) => {
      const frame = (now: number) => {
        if (revision !== orbSettleRevision) { resolve(); return; }
        const progress = duration === 0 ? 1 : Math.min(1, (now - startedAt) / duration);
        const eased = 1 - Math.pow(1 - progress, 3);
        overlayPosition = {
          x: Math.round(origin.x + (position.x - origin.x) * eased),
          y: Math.round(origin.y + (position.y - origin.y) * eased),
        };
        queueOverlayMove(overlayPosition.x, overlayPosition.y);
        if (progress < 1) requestAnimationFrame(frame);
        else {
          orbSettling = false;
          orbSettleTarget = null;
          resolve();
        }
      };
      requestAnimationFrame(frame);
    });
    await waitForOverlayMoves();
  }

  async function endOverlayDrag(event: PointerEvent, compact = false) {
    if (!dragState || dragState.pointerId !== event.pointerId) return;
    const completedDrag = dragState;
    dragState = null;
    const target = completedDrag.target;
    if (target.hasPointerCapture(event.pointerId)) target.releasePointerCapture(event.pointerId);
    if (!dragging) return;
    dragging = false;
    if (event.type === "pointercancel" || event.type === "lostpointercapture") {
      overlayPosition = { x: completedDrag.originX, y: completedDrag.originY };
      orbDock = completedDrag.originDock;
      queueOverlayMove(overlayPosition.x, overlayPosition.y);
      return;
    }
    if (compact) suppressCompactToggle = true;
    const drop = snapOrbPosition(overlayPosition, compactSize, overlayWorkArea, monitorBounds.scale);
    const persistedPosition = expanded
      ? compactPositionFromExpanded(overlayPosition)
      : drop.position;
    orbDock = expanded
      ? orbDockAtPosition(persistedPosition, compactSize, overlayWorkArea, monitorBounds.scale)
      : drop.dock;
    compactAnchorPosition = expanded ? persistedPosition : null;
    preferences = {
      ...preferences,
      overlayX: Math.round(persistedPosition.x),
      overlayY: Math.round(persistedPosition.y),
    };
    if (expanded) {
      queueOverlayMove(overlayPosition.x, overlayPosition.y);
      await waitForOverlayMoves();
      await savePreferences(preferences);
    } else {
      await Promise.all([settleOrbPosition(drop.position), savePreferences(preferences)]);
    }
  }

  function openSession(session: AgentSession) {
    inspectorSessionId = session.id;
    selectedId = selectedId === session.id ? null : session.id;
    if (selectedId !== session.id) composerSessionId = null;
    permissionError = null;
    terminateConfirmId = null;
    sessionActionMessage = null;
  }

  function revealScrollbarWhileScrolling(node: HTMLElement) {
    let hideTimer: ReturnType<typeof setTimeout> | undefined;
    const onScroll = () => {
      node.classList.add("is-scrolling");
      if (hideTimer) clearTimeout(hideTimer);
      hideTimer = setTimeout(() => node.classList.remove("is-scrolling"), 700);
    };
    node.addEventListener("scroll", onScroll, { passive: true });
    return {
      destroy() {
        node.removeEventListener("scroll", onScroll);
        if (hideTimer) clearTimeout(hideTimer);
      },
    };
  }

  function beginSessionRename(session: AgentSession) {
    void bringOverlayToFront(true);
    renamingSessionId = session.id;
    renameDraft = sessionDisplayName(session);
    renameError = null;
    void focusOrbField(".session-name-editor input", true);
  }

  async function focusOrbField(selector: string, selectText = false) {
    await tick();
    const field = document.querySelector<HTMLInputElement | HTMLTextAreaElement>(selector);
    if (!field?.isConnected) return;
    field.focus({ preventScroll: true });
    if (selectText && field instanceof HTMLInputElement) field.select();
  }

  function cancelSessionRename() {
    renamingSessionId = null;
    renameDraft = "";
    renameError = null;
  }

  async function saveSessionRename(session: AgentSession) {
    if (renamingSession) return;
    const requested = renameDraft.trim();
    if (!requested) {
      renameError = tr("Enter a name for this session.", "Digite um nome para esta sessão.");
      return;
    }
    renamingSession = true;
    renameError = null;
    try {
      const finalName = isTauri
        ? await renameSession(session.id, requested)
        : uniqueLocalSessionName(requested, session.id);
      sessions = sessions.map((item) =>
        item.id === session.id ? { ...item, sessionName: finalName } : item
      );
      cancelSessionRename();
    } catch (error) {
      renameError = String(error).replace(/^Error:\s*/, "");
    } finally {
      renamingSession = false;
    }
  }

  function uniqueLocalSessionName(requested: string, sessionId: string) {
    const used = new Set(
      sessions
        .filter((session) => session.id !== sessionId)
        .map((session) => sessionDisplayName(session).toLocaleLowerCase())
    );
    if (!used.has(requested.toLocaleLowerCase())) return requested;
    for (let suffix = 2; ; suffix += 1) {
      const candidate = `${requested} (${suffix})`;
      if (!used.has(candidate.toLocaleLowerCase())) return candidate;
    }
  }

  function canSubmitToSession(session: AgentSession) {
    return sessionCapabilities(session).canPrompt;
  }

  function canContinueSession(session: AgentSession) {
    return (
      ["completed", "failed", "waiting_for_input"].includes(session.status)
      || (
        session.status === "running"
        && sessionCapabilities(session).promptDeliveries.includes("steer")
      )
    );
  }

  function pendingQueuedPrompts(session: AgentSession) {
    return session.activities
      .filter((activity) =>
        ["queued_prompt", "codex_queued_prompt"].includes(activity.kind)
        && activity.status === "waiting"
      )
      .sort((left, right) => left.createdAt - right.createdAt);
  }

  function toggleSessionComposer(session: AgentSession) {
    const opening = composerSessionId !== session.id;
    composerSessionId = opening ? session.id : null;
    composerPrompt = "";
    composerAttachments = [];
    composerMessage = null;
    if (opening) {
      void bringOverlayToFront(true);
      void focusOrbField(".inline-composer textarea");
    }
  }

  async function sendSessionPrompt(session: AgentSession) {
    const prompt = composerPrompt.trim();
    if ((!prompt && composerAttachments.length === 0) || composerSending) return;
    composerSending = true;
    composerMessage = null;
    try {
      const delivery = session.status === "running" ? "queue" : "new_turn";
      if (isTauri) {
        await submitPrompt(session.id, prompt, composerAttachments, delivery);
      }
      sessions = sessions.map((item) =>
        item.id === session.id
          ? {
              ...item,
              status: "running",
              statusLabel: delivery === "queue" ? "Prompt queued" : "Prompt sent by Lume",
              lastResponse: delivery === "new_turn" ? undefined : item.lastResponse,
            }
          : item,
      );
      composerPrompt = "";
      composerAttachments = [];
      composerSessionId = delivery === "queue" ? session.id : null;
      if (isTauri) await refreshSessions(false);
    } catch (error) {
      composerMessage = String(error).replace(/^Error:\s*/, "");
    } finally {
      composerSending = false;
    }
  }

  async function steerSessionQueuedPrompt(session: AgentSession) {
    const queuedPrompt = pendingQueuedPrompts(session)[0];
    if (!queuedPrompt || queuedPrompt.kind !== "queued_prompt" || steeringQueuedActivityId) return;
    steeringQueuedActivityId = queuedPrompt.id;
    composerMessage = null;
    try {
      if (isTauri) {
        await steerQueuedPrompt(session.id, queuedPrompt.id);
        await refreshSessions(false);
      }
      composerMessage = tr(
        "Queued prompt steered into the current task.",
        "Prompt da fila enviado para a tarefa atual.",
      );
    } catch (error) {
      composerMessage = String(error).replace(/^Error:\s*/, "");
      if (isTauri) await refreshSessions(false).catch(() => undefined);
    } finally {
      steeringQueuedActivityId = null;
    }
  }

  function handleSessionComposerKeydown(event: KeyboardEvent, session: AgentSession) {
    const nextQueuedPrompt = pendingQueuedPrompts(session)[0];
    if (
      event.key !== "Tab"
      || event.shiftKey
      || event.isComposing
      || nextQueuedPrompt?.kind !== "queued_prompt"
    ) return;
    event.preventDefault();
    void steerSessionQueuedPrompt(session);
  }

  async function inlineImagePreview(path: string) {
    return createImagePreview(
      await readLocalImageDataUrl(path),
      preferences.language,
    );
  }

  function removeComposerImage(index: number) {
    composerAttachments = composerAttachments.filter((_, current) => current !== index);
  }

  async function pasteSessionImages(
    event: ClipboardEvent,
    session: AgentSession,
  ) {
    if (!clipboardHasImage(event) && !clipboardMayContainImage(event)) return;
    event.preventDefault();
    composerMessage = null;
    const capabilities = sessionCapabilities(session);
    if (
      composerSending ||
      !canContinueSession(session) ||
      !capabilities.canPrompt ||
      !capabilities.canAttachImages
    ) {
      composerMessage = tr(
        "Images can only be attached when this session is ready for a prompt.",
        "Imagens só podem ser anexadas quando esta sessão estiver pronta para um prompt.",
      );
      return;
    }
    try {
      const { files, paths } = await collectClipboardImages(event, preferences.language);
      const available = 4 - composerAttachments.length;
      const prepared: PromptAttachmentInput[] = [];
      for (const [index, file] of files.slice(0, available).entries()) {
        prepared.push(await prepareClipboardImage(file, index, preferences.language));
      }
      for (const path of paths.slice(0, available - prepared.length)) {
        prepared.push({
          name: path.split(/[\\/]/).pop() || "image",
          mimeType: "",
          path,
          previewDataUrl: await inlineImagePreview(path),
        });
      }
      composerAttachments = [...composerAttachments, ...prepared];
    } catch (error) {
      composerMessage = String(error).replace(/^Error:\s*/, "");
    }
  }

  function canTerminateSession(session: AgentSession) {
    return sessionCapabilities(session).canTerminate;
  }

  function canInterruptSession(session: AgentSession) {
    return sessionCapabilities(session).canInterrupt;
  }

  async function interruptSessionPrompt(session: AgentSession) {
    if (!canInterruptSession(session) || interruptingSessionId) return;
    interruptingSessionId = session.id;
    sessionActionMessage = null;
    try {
      if (isTauri) await interruptPrompt(session.id);
      await refreshSessions(false);
    } catch (error) {
      sessionActionMessageIsError = true;
      sessionActionMessage = String(error).replace(/^Error:\s*/, "");
    } finally {
      interruptingSessionId = null;
    }
  }

  async function copyResult(resultId: string, response: string) {
    try {
      await navigator.clipboard.writeText(response);
      copiedResultId = resultId;
      setTimeout(() => {
        if (copiedResultId === resultId) copiedResultId = null;
      }, 1_500);
    } catch (error) {
      copiedResultId = null;
      sessionActionMessageIsError = true;
      sessionActionMessage = tr("Could not copy the response.", "Não foi possível copiar a resposta.") + ` ${String(error).replace(/^Error:\s*/, "")}`;
    }
  }

  async function terminateAgent(session: AgentSession) {
    if (!canTerminateSession(session) || terminatingSessionId) return;
    if (terminateConfirmId !== session.id) {
      terminateConfirmId = session.id;
      sessionActionMessage = null;
      return;
    }
    terminatingSessionId = session.id;
    sessionActionMessage = null;
    try {
      if (isTauri) await terminateSession(session.id);
      terminateConfirmId = null;
      await refreshSessions(false);
    } catch (error) {
      sessionActionMessageIsError = true;
      sessionActionMessage = String(error).replace(/^Error:\s*/, "");
    } finally {
      terminatingSessionId = null;
    }
  }

  async function refreshTerminalWindows() {
    terminalWindows = await loadTerminalWindows();
  }

  async function togglePanelWorkflowMode(enabled: boolean) {
    if (workflowModeChanging) return;
    workflowModeChanging = true;
    terminalMessage = null;
    try {
      await updatePreference("workflowEnabled", enabled);
      if (isTauri) terminalWindows = await setTerminalWorkflowEnabled(enabled);
    } catch (error) {
      terminalMessageIsError = true;
      terminalMessage = String(error).replace(/^Error:\s*/, "");
    } finally {
      workflowModeChanging = false;
    }
  }

  async function toggleWorkflowSettings() {
    workflowSettingsOpen = !workflowSettingsOpen;
    if (!workflowSettingsOpen || !isTauri) return;
    await refreshTerminalWindows();
    const entries = await Promise.all(preferences.workflowGroups.map(async (group) => {
      try {
        return [group.id, await loadWorkflowRun(group.id)] as const;
      } catch {
        return [group.id, null] as const;
      }
    }));
    workflowRunStates = Object.fromEntries(entries);
  }

  function workflowSessionKey(session: AgentSession) {
    return session.nativeSessionId?.trim() || session.id;
  }

  function missingWorkflowSteps() {
    const connected = new Set(visibleAgentSessions.map(workflowSessionKey));
    return preferences.workflowGroups
      .filter((group) => {
        const run = workflowRunStates[group.id];
        return Boolean(run && !["completed", "cancelled"].includes(run.status));
      })
      .flatMap((group) => {
        const runStepIds = new Set(workflowRunStates[group.id]?.steps.map((step) => step.stepId));
        return group.steps
          .filter((step) => runStepIds.has(step.id) && !connected.has(step.sessionNativeId))
          .map((step, index) => ({ group, step, index }));
      });
  }

  function workflowReplacementSessions(groupId: string, stepId: string) {
    const group = preferences.workflowGroups.find((item) => item.id === groupId);
    const occupied = new Set(
      group?.steps
        .filter((step) => step.id !== stepId)
        .map((step) => step.sessionNativeId) ?? [],
    );
    return visibleAgentSessions.filter((session) => !occupied.has(workflowSessionKey(session)));
  }

  async function updateWorkflowSetting<K extends keyof Preferences["workflowSettings"]>(
    key: K,
    value: Preferences["workflowSettings"][K],
  ) {
    if (workflowSettingsSaving) return;
    workflowSettingsSaving = true;
    try {
      await updatePreference("workflowSettings", {
        ...preferences.workflowSettings,
        [key]: value,
      });
    } finally {
      workflowSettingsSaving = false;
    }
  }

  async function replaceWorkflowSession(
    workflowId: string,
    stepId: string,
    sessionNativeId: string,
  ) {
    if (!sessionNativeId || workflowRebindingStepId) return;
    workflowRebindingStepId = stepId;
    terminalMessage = null;
    try {
      await rebindWorkflowSession(workflowId, stepId, sessionNativeId);
      preferences = await loadPreferences();
      if (isTauri) void emit("lume://preferences-changed", preferences);
      terminalMessageIsError = false;
      terminalMessage = tr(
        "Workflow agent replaced. Retry the step if it was interrupted.",
        "Agente do workflow substituído. Tente a etapa novamente se ela foi interrompida.",
      );
    } catch (error) {
      terminalMessageIsError = true;
      terminalMessage = String(error).replace(/^Error:\s*/, "");
    } finally {
      workflowRebindingStepId = null;
    }
  }

  async function openTerminal(session: AgentSession) {
    if (openingTerminal) return;
    openingTerminal = session.id;
    terminalOpenError = null;
    terminalMessage = null;
    try {
      if (isTauri) {
        const label = await openTerminalWindow(session.id);
        terminalWindows = await waitForTerminalWindow(label, loadTerminalWindows);
      } else {
        terminalMessageIsError = false;
        terminalMessage = tr(
          `${sessionDisplayName(session)} opens in a separate window.`,
          `${sessionDisplayName(session)} abre em uma janela separada.`,
        );
      }
    } catch (error) {
      terminalOpenError = error instanceof TerminalOpeningTimeoutError
        ? tr("The terminal did not finish loading. Try opening it again.", "O terminal não terminou de carregar. Tente abri-lo novamente.")
        : String(error).replace(/^Error:\s*/, "");
    } finally {
      openingTerminal = null;
    }
  }

  async function saveCurrentLayout() {
    terminalMessageIsError = false;
    try {
      await refreshTerminalWindows();
    } catch (error) {
      terminalMessageIsError = true;
      terminalMessage = String(error).replace(/^Error:\s*/, "");
      return;
    }
    if (terminalWindows.length === 0) {
      terminalMessage = tr("Open at least one terminal before saving a layout.", "Abra ao menos um terminal antes de salvar um layout.");
      return;
    }
    const name = layoutName.trim() || tr("My layout", "Meu layout");
    const id = selectedLayoutId && preferences.whiteboardLayouts.some((layout) => layout.id === selectedLayoutId)
      ? selectedLayoutId
      : `layout-${Date.now().toString(36)}`;
    const layout: WhiteboardLayout = {
      id,
      name,
      terminals: terminalWindows.flatMap((terminal) => {
        const session = resolveTerminalSession(terminal, sessions);
        return session
          ? [{
              agent: session.agent,
              agentLabel: session.agentLabel,
              project: session.project,
              source: session.source,
              x: terminal.x,
              y: terminal.y,
              width: terminal.width,
              height: terminal.height,
              groupId: terminal.groupId,
              monitorId: terminal.monitorId,
            }]
          : [];
      }),
    };
    const layouts = preferences.whiteboardLayouts.some((item) => item.id === id)
      ? preferences.whiteboardLayouts.map((item) => item.id === id ? layout : item)
      : [...preferences.whiteboardLayouts, layout];
    if (!await updatePreference("whiteboardLayouts", layouts)) return;
    selectedLayoutId = id;
    layoutName = name;
    terminalMessage = tr("Whiteboard layout saved.", "Layout do whiteboard salvo.");
  }

  async function restoreSavedLayout(layout: WhiteboardLayout) {
    if (restoringLayout) return;
    restoringLayout = true;
    terminalMessageIsError = false;
    terminalMessage = null;
    const used = new Set<string>();
    const entries: Array<{
      sessionId: string;
      x: number;
      y: number;
      width: number;
      height: number;
      groupId?: string;
      monitorId?: string;
    }> = [];
    try {
      for (const slot of layout.terminals) {
        const session = sessions.find((item) =>
          !used.has(item.id) &&
          item.agent === slot.agent &&
          (item.agent !== "unknown" || item.agentLabel === slot.agentLabel) &&
          item.project === slot.project &&
          item.source === slot.source,
        );
        if (!session) continue;
        used.add(session.id);
        await openTerminalWindow(session.id);
        entries.push({
          sessionId: session.id,
          x: slot.x,
          y: slot.y,
          width: slot.width,
          height: slot.height,
          groupId: slot.groupId,
          monitorId: slot.monitorId,
        });
      }
      if (entries.length === 0) {
        terminalMessage = tr("No open session matches this layout.", "Nenhuma sessão aberta corresponde a este layout.");
        return;
      }
      terminalWindows = await restoreTerminalLayout(entries);
      selectedLayoutId = layout.id;
      layoutName = layout.name;
      terminalMessage = tr(`Restored ${entries.length} terminals.`, `${entries.length} terminais restaurados.`);
    } catch (error) {
      terminalMessageIsError = true;
      terminalMessage = String(error).replace(/^Error:\s*/, "");
    } finally {
      restoringLayout = false;
    }
  }

  async function deleteSavedLayout(id: string) {
    const previous = preferences;
    const profiles = Object.fromEntries(
      Object.entries(preferences.projectProfiles).map(([key, profile]) => [
        key,
        profile.whiteboardLayoutId === id
          ? { ...profile, whiteboardLayoutId: undefined }
          : profile,
      ]),
    );
    preferences = {
      ...preferences,
      whiteboardLayouts: preferences.whiteboardLayouts.filter((layout) => layout.id !== id),
      projectProfiles: profiles,
    };
    try {
      await savePreferences(preferences);
    } catch (error) {
      preferences = previous;
      terminalMessageIsError = true;
      terminalMessage = String(error).replace(/^Error:\s*/, "");
      return;
    }
    selectedLayoutId = preferences.whiteboardLayouts[0]?.id ?? null;
    layoutName = preferences.whiteboardLayouts.find((layout) => layout.id === selectedLayoutId)?.name ?? "";
  }

  function terminalIsOpen(session: AgentSession) {
    return terminalWindows.some(
      (terminal) => resolveTerminalSession(terminal, sessions)?.id === session.id,
    );
  }

  let permissionPending = $state<{ sessionId: string; action: PermissionAction } | null>(null);

  async function handlePermission(session: AgentSession, action: PermissionAction) {
    const permission = session.pendingPermission;
    if (!permission || permissionPending) return;
    permissionError = null;
    permissionPending = { sessionId: session.id, action };
    try {
      await resolvePermissionAction(session, permission, action);
    } finally {
      permissionPending = null;
    }
  }

  async function resolvePermissionAction(session: AgentSession, permission: NonNullable<AgentSession["pendingPermission"]>, action: PermissionAction) {

    if (action === "open_source") {
      try {
        await openSessionSource(session.id);
      } catch (error) {
        permissionError = String(error).replace(/^Error:\s*/, "");
      }
      return;
    }

    try {
      if (isTauri) {
        await decidePermission(session.id, permission.id, action);
        await refreshSessions(false);
      } else {
        sessions = sessions.map((item) =>
          item.id === session.id
            ? {
                ...item,
                status: "running",
                statusLabel: action === "deny" ? "Permission denied" : "Continuing task",
                pendingPermission: undefined,
              }
            : item,
        );
      }
      selectedId = null;
    } catch (error) {
      permissionError = String(error).replace(/^Error:\s*/, "");
    }
  }

  async function handleQuestionOption(
    session: AgentSession,
    questionId: string,
    value: string,
  ) {
    const request = session.pendingQuestion;
    if (!request) return;
    const selections = {
      ...questionSelections,
      [`${request.id}:${questionId}`]: value,
    };
    questionSelections = selections;
    const answers: QuestionAnswer[] = request.questions
      .map((question) => ({
        questionId: question.id,
        answers: selections[`${request.id}:${question.id}`]
          ? [selections[`${request.id}:${question.id}`]]
          : [],
      }))
      .filter((answer) => answer.answers.length > 0);
    if (answers.length !== request.questions.length) return;
    permissionError = null;
    try {
      await answerQuestion(session.id, request.id, answers);
      questionSelections = {};
      await refreshSessions(false);
    } catch (error) {
      permissionError = String(error).replace(/^Error:\s*/, "");
    }
  }

  async function refreshMobileSettings() {
    if (!isTauri) return;
    try {
      const [status, devices] = await Promise.all([
        loadMobileGatewayStatus(),
        loadPairedDevices(),
      ]);
      mobileStatus = status;
      applyPairedDevices(devices, mobileDevicesInitialized);
    } catch (error) {
      mobileMessageIsError = true;
      mobileMessage = String(error).replace(/^Error:\s*/, "");
    }
  }

  async function toggleMobileAccess() {
    if (!isTauri || mobileBusy) return;
    mobileBusy = true;
    mobileMessage = null;
    pairingOffer = null;
    pairingQr = null;
    try {
      mobileStatus = mobileStatus?.networkReachable
        ? await disableMobileGateway()
        : await enableMobileGateway();
      if (mobileStatus.networkReachable) {
        pairingOffer = await beginMobilePairing();
        pairingQr = await QRCode.toDataURL(pairingOffer.payload, {
          width: 208,
          margin: 4,
          errorCorrectionLevel: "M",
          color: { dark: "#14241d", light: "#ffffff" },
        });
      }
      mobileMessageIsError = false;
    } catch (error) {
      mobileMessageIsError = true;
      mobileMessage = String(error).replace(/^Error:\s*/, "");
    } finally {
      mobileBusy = false;
    }
  }

  async function createMobilePairing() {
    if (!isTauri || mobileBusy) return;
    mobileBusy = true;
    mobileMessage = null;
    try {
      pairingOffer = await beginMobilePairing();
      pairingQr = await QRCode.toDataURL(pairingOffer.payload, {
        width: 208,
        margin: 4,
        errorCorrectionLevel: "M",
        color: { dark: "#14241d", light: "#ffffff" },
      });
      mobileMessageIsError = false;
    } catch (error) {
      mobileMessageIsError = true;
      mobileMessage = String(error).replace(/^Error:\s*/, "");
    } finally {
      mobileBusy = false;
    }
  }

  async function removePairedDevice(id: string) {
    if (mobileBusy) return;
    mobileBusy = true;
    mobileMessage = null;
    try {
      await revokePairedDevice(id);
      applyPairedDevices(await loadPairedDevices(), false);
      if (newMobileDevice?.id === id) newMobileDevice = null;
      mobileMessageIsError = false;
      mobileMessage = tr("Device access revoked.", "Acesso do dispositivo revogado.");
    } catch (error) {
      mobileMessageIsError = true;
      mobileMessage = String(error).replace(/^Error:\s*/, "");
    } finally {
      mobileBusy = false;
    }
  }

  async function togglePairedDeviceScope(device: PairedDevice, scope: MobileScope) {
    if (mobileBusy || scope === "monitor") return;
    const scopes = device.scopes.includes(scope)
      ? device.scopes.filter((value) => value !== scope)
      : [...device.scopes, scope];
    if (dev && device.id === devMobileDeviceId) {
      pairedDevices = pairedDevices.map((value) =>
        value.id === devMobileDeviceId ? { ...value, scopes } : value,
      );
      mobileMessageIsError = false;
      mobileMessage = tr("Preview permission updated.", "Permissão de demonstração atualizada.");
      return;
    }
    mobileBusy = true;
    mobileMessage = null;
    try {
      await setPairedDeviceScopes(device.id, scopes);
      applyPairedDevices(await loadPairedDevices(), false);
      mobileMessageIsError = false;
      mobileMessage = tr("Device permissions updated.", "Permissões do dispositivo atualizadas.");
    } catch (error) {
      mobileMessageIsError = true;
      mobileMessage = String(error).replace(/^Error:\s*/, "");
    } finally {
      mobileBusy = false;
    }
  }

  async function copyMobileValue(value: string) {
    try {
      await navigator.clipboard.writeText(value);
      mobileMessageIsError = false;
      mobileMessage = tr("Copied.", "Copiado.");
    } catch {
      mobileMessageIsError = true;
      mobileMessage = tr("Could not copy this value.", "Não foi possível copiar este valor.");
    }
  }

  async function reviewNewMobileDevice() {
    const deviceId = newMobileDevice?.id;
    if (!deviceId) return;
    await openView("settings");
    await tick();
    const section = document.querySelector<HTMLDetailsElement>("[data-mobile-access-section]");
    if (section) section.open = true;
    await tick();
    const deviceCard = [...document.querySelectorAll<HTMLElement>("[data-mobile-device-id]")]
      .find((element) => element.dataset.mobileDeviceId === deviceId);
    (deviceCard ?? section)?.scrollIntoView({ behavior: "smooth", block: "center" });
    newMobileDevice = null;
  }

  async function openView(nextView: View) {
    if (
      (nextView === "settings" || nextView === "history") &&
      isTauri &&
      expanded &&
      !morphing &&
      expandedHeight < expandedPanelMaxHeight
    ) {
      const previousSize = currentExpandedSize();
      const anchor =
        compactAnchorPosition ?? compactPositionFromExpanded(overlayPosition, previousSize);
      expandedHeight = expandedPanelMaxHeight;
      compactAnchorPosition = anchor;
      const target = currentExpandedSize();
      const position = expandedPositionFromCompact(anchor, target);
      await Promise.allSettled([
        setOverlaySurfaceSize(target.width, target.height),
        moveOverlay(position.x, position.y, false, preferences.monitorId),
      ]);
      overlayPosition = position;
    }
    view = nextView;
    paletteOpen = false;
    selectedId = null;
    permissionError = null;
    launcherOpen = false;
    composerSessionId = null;
    composerMessage = null;
    terminalMessage = null;
    if (nextView === "board") await refreshTerminalWindows();
    if (nextView === "settings") {
      selectedProfileKey ??= detectedProjects[0]?.key ?? null;
      settingsMessage = null;
      await refreshMobileSettings();
    }
  }

  async function openAgentSettings() {
    await openView("settings");
    await tick();
    const group = document.querySelector<HTMLDetailsElement>("[data-agent-integrations]");
    if (!group) return;
    group.open = true;
    group.querySelector("summary")?.focus({ preventScroll: true });
    group.scrollIntoView({ block: "nearest" });
  }

  type PaletteCommand = { id: string; label: string; detail: string; run: () => void | Promise<void> };

  function paletteCommands(): PaletteCommand[] {
    const commands: PaletteCommand[] = [
      { id: "workspace", label: "Workspace", detail: tr("Open the multi-agent workbench", "Abrir a bancada de múltiplos agentes"), run: async () => { await showWorkspaceWindow(); } },
      { id: "sessions", label: tr("Sessions", "Sessões"), detail: tr("Show active agents", "Mostrar agentes ativos"), run: () => openView("sessions") },
      { id: "whiteboard", label: tr("Terminals", "Terminais"), detail: tr("Open floating terminals", "Abrir terminais flutuantes"), run: () => openView("board") },
      { id: "history", label: tr("Inspector", "Inspector"), detail: tr("Inspect a session and its repository", "Inspecionar uma sessão e seu repositório"), run: () => openView("history") },
      { id: "settings", label: tr("Settings", "Ajustes"), detail: tr("Configure Lume", "Configurar o Lume"), run: () => openView("settings") },
      { id: "new-session", label: tr("New agent session", "Nova sessão de agente"), detail: tr("Open the agent launcher", "Abrir o iniciador de agentes"), run: async () => { await openView("sessions"); launcherOpen = true; } },
    ];
    for (const session of sessions) {
      commands.push({
        id: `session-${session.id}`,
        label: sessionDisplayName(session),
        detail: shown(session.statusLabel),
        run: async () => {
          await openView("sessions");
          selectedId = session.id;
        },
      });
      {
        const open = terminalIsOpen(session);
        commands.push({
          id: `terminal-${session.id}`,
          label: open
            ? tr(`Show ${sessionDisplayName(session)} terminal`, `Mostrar terminal ${sessionDisplayName(session)}`)
            : tr(`Open ${sessionDisplayName(session)} terminal`, `Abrir terminal ${sessionDisplayName(session)}`),
          detail: `${session.project} · ${tr("Chat and changed files", "Chat e arquivos alterados")}`,
          run: async () => {
            await openView("board");
            await openTerminal(session);
          },
        });
      }
      if (canSubmitToSession(session)) {
        commands.push({
          id: `prompt-${session.id}`,
          label: tr(`Send prompt to ${sessionDisplayName(session)}`, `Enviar prompt para ${sessionDisplayName(session)}`),
          detail: session.project,
          run: async () => {
            await openView("sessions");
            selectedId = session.id;
            composerSessionId = session.id;
            composerPrompt = "";
            await tick();
          },
        });
      }
    }
    const query = paletteQuery.trim().toLowerCase();
    return query
      ? commands.filter((command) => `${command.label} ${command.detail}`.toLowerCase().includes(query))
      : commands;
  }

  async function runShortcutAction(action: ShortcutAction) {
    if (action === "workspace") {
      await showWorkspaceWindow();
      return;
    }
    if (action === "palette") {
      await showCommandPalette();
      return;
    }
    if (!expanded) await toggleExpanded();
    if (action === "new-session") {
      await openView("sessions");
      launcherOpen = true;
      return;
    }
    if (action === "whiteboard") {
      await openView("board");
    }
  }

  function shortcutFromEvent(event: KeyboardEvent): string | null {
    if (["Control", "Shift", "Alt", "Meta"].includes(event.key)) return null;
    const modifiers = [
      event.ctrlKey ? "Ctrl" : "",
      event.altKey ? "Alt" : "",
      event.shiftKey ? "Shift" : "",
      event.metaKey ? "Super" : "",
    ].filter(Boolean);
    if (modifiers.length === 0) return null;
    let key = event.code;
    if (key.startsWith("Key")) key = key.slice(3);
    else if (key.startsWith("Digit")) key = key.slice(5);
    if (!key || key === "Unidentified") return null;
    return [...modifiers, key].join("+");
  }

  function shortcutMatches(event: KeyboardEvent, shortcut: string) {
    return shortcutFromEvent(event)?.toLowerCase() === shortcut.toLowerCase();
  }

  function handleAppShortcut(event: KeyboardEvent) {
    if (event.defaultPrevented || event.repeat) return;
    const configured: Array<[ShortcutAction, string]> = [
      ["open", preferences.openShortcut],
      ["palette", preferences.globalShortcut],
      ["new-session", preferences.newSessionShortcut],
      ["whiteboard", preferences.whiteboardShortcut],
      ["workspace", preferences.workspaceShortcut],
    ];
    let action = configured.find(([, shortcut]) => shortcutMatches(event, shortcut))?.[0];
    if (
      !action &&
      (event.ctrlKey || event.metaKey) &&
      event.shiftKey &&
      event.code === "KeyP"
    ) {
      action = "palette";
    }
    if (!action) return;
    event.preventDefault();
    event.stopPropagation();
    void runShortcutAction(action);
  }

  function captureShortcut(event: KeyboardEvent) {
    event.preventDefault();
    event.stopPropagation();
    if (event.key === "Escape") {
      shortcutEditorKey = null;
      return;
    }
    const shortcut = shortcutFromEvent(event);
    if (!shortcut) return;
    shortcutDraft = shortcut;
    shortcutEditorError = null;
  }

  async function openShortcutEditor(key: ShortcutPreferenceKey) {
    shortcutEditorKey = key;
    shortcutDraft = preferences[key];
    shortcutEditorError = null;
    await tick();
    document.querySelector<HTMLElement>("[data-shortcut-capture]")?.focus();
  }

  async function saveShortcut() {
    if (!shortcutEditorKey || !shortcutDraft) return;
    const saved = await updatePreference(shortcutEditorKey, shortcutDraft);
    if (saved) shortcutEditorKey = null;
    else shortcutEditorError = settingsMessage;
  }

  async function showCommandPalette() {
    paletteTrigger = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    void refreshTerminalWindows().catch(() => undefined);
    if (!expanded) await toggleExpanded();
    paletteQuery = "";
    paletteIndex = 0;
    paletteOpen = true;
    await tick();
    document.querySelector<HTMLInputElement>("[data-command-palette]")?.focus();
  }

  async function closeCommandPalette() {
    paletteOpen = false;
    await tick();
    if (paletteTrigger?.isConnected && paletteTrigger !== document.body) paletteTrigger.focus();
    else document.querySelector<HTMLButtonElement>(".palette-button")?.focus();
  }

  async function runPaletteCommand(command: PaletteCommand) {
    paletteOpen = false;
    await command.run();
  }

  function handlePaletteKey(event: KeyboardEvent) {
    const commands = paletteCommands();
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      void closeCommandPalette();
      return;
    }
    if (event.key === "ArrowDown") {
      event.preventDefault();
      paletteIndex = commands.length ? (paletteIndex + 1) % commands.length : 0;
      return;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      paletteIndex = commands.length ? (paletteIndex - 1 + commands.length) % commands.length : 0;
      return;
    }
    if (event.key === "Enter" && commands[paletteIndex]) {
      event.preventDefault();
      void runPaletteCommand(commands[paletteIndex]);
    }
  }

  async function toggleLauncher() {
    launcherOpen = !launcherOpen;
    launchError = null;
    if (!launcherOpen) {
      resumeAgent = null;
      resumableSessions = [];
    }
    if (launcherOpen && integrations.length === 0) {
      integrations = await loadIntegrationStatuses();
    }
  }

  async function startSession(agent: IntegrationStatus["kind"]) {
    if (!isTauri) {
      launcherOpen = false;
      return;
    }
    launching = agent;
    launchingSessionId = null;
    launchingPhase = "choosing";
    launchError = null;
    try {
      const selected = await openDialog({
        directory: true,
        multiple: false,
        title: tr("Project for the new session", "Projeto da nova sessão"),
      });
      if (!selected || Array.isArray(selected)) return;

      launchingPhase = "opening";
      const profile = preferences.projectProfiles[projectKey(selected)];
      await launchAgentSession(
        agent,
        selected,
        false,
        undefined,
        profile?.launchTarget ?? preferences.launchTarget,
        profile?.permissionMode,
        profile?.approvalPolicy,
      );
      launcherOpen = false;
    } catch (error) {
      const connection = agentConnectionMessage(error);
      if (connection) { connectionAgent = agent; connectionMessage = connection; }
      else if (macosAutomationMessage(error)) automationRequired = true;
      else launchError = String(error).replace(/^Error:\s*/, "");
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
    launchError = null;
    try {
      resumableSessions = await loadResumableSessions(agent);
    } catch (error) {
      launchError = String(error).replace(/^Error:\s*/, "");
    } finally {
      loadingResumeAgent = null;
    }
  }

  async function resumeStoredSession(stored: ResumableSession) {
    launching = stored.agent;
    launchingSessionId = stored.id;
    launchingPhase = "opening";
    launchError = null;
    try {
      const liveSession = resolveLiveResumableSession(stored, sessions);
      if (liveSession) {
        await openTerminalWindow(liveSession.id);
        await refreshTerminalWindows();
        launcherOpen = false;
        resumeAgent = null;
        resumableSessions = [];
        return;
      }
      const profile = preferences.projectProfiles[projectKey(stored.workingDirectory)];
      await launchAgentSession(
        stored.agent,
        stored.workingDirectory,
        true,
        stored.id,
        profile?.launchTarget ?? preferences.launchTarget,
      );
      launcherOpen = false;
      resumeAgent = null;
      resumableSessions = [];
    } catch (error) {
      const connection = agentConnectionMessage(error);
      if (connection) { connectionAgent = stored.agent; connectionMessage = connection; }
      else if (macosAutomationMessage(error)) automationRequired = true;
      else launchError = String(error).replace(/^Error:\s*/, "");
    } finally {
      launching = null;
      launchingSessionId = null;
      launchingPhase = null;
    }
  }

  async function toggleIntegration(integration: IntegrationStatus) {
    if (!integration.installed) return;
    const enabling = !integration.configured;
    configuringIntegration = integration.kind;
    settingsMessage = null;
    try {
      await configureIntegration(integration.kind, enabling);
      integrations = await loadIntegrationStatuses();
      settingsMessageIsError = false;
      settingsMessage = enabling
        ? integration.kind === "codex"
          ? tr(
              "Codex connected. Open /hooks in Codex and trust the Lume hook once.",
              "Codex conectado. Abra /hooks no Codex e confie no hook Lume uma vez.",
            )
          : tr(`${integration.label} connected to Lume.`, `${integration.label} conectado ao Lume.`)
        : tr(`${integration.label} disconnected.`, `${integration.label} desconectado.`);
    } catch (error) {
      settingsMessageIsError = true;
      settingsMessage = String(error).replace(/^Error:\s*/, "");
    } finally {
      configuringIntegration = null;
    }
  }

  async function runIntegrationDiagnostic(integration: IntegrationStatus) {
    diagnosingIntegration = integration.kind;
    settingsMessage = null;
    try {
      integrationDiagnostics = {
        ...integrationDiagnostics,
        [integration.kind]: await diagnoseIntegration(integration.kind),
      };
    } catch (error) {
      settingsMessageIsError = true;
      settingsMessage = String(error).replace(/^Error:\s*/, "");
    } finally {
      diagnosingIntegration = null;
    }
  }

  async function toggleVscode() {
    if (!vscodeStatus.installed) return;
    const enabling = !vscodeStatus.configured;
    configuringVscode = true;
    settingsMessage = null;
    try {
      await configureVscode(enabling);
      vscodeStatus = await loadVscodeStatus();
      settingsMessageIsError = false;
      settingsMessage = enabling
        ? tr("Companion installed in VS Code.", "Companion instalado no VS Code.")
        : tr("Companion removed from VS Code.", "Companion removido do VS Code.");
    } catch (error) {
      settingsMessageIsError = true;
      settingsMessage = String(error).replace(/^Error:\s*/, "");
    } finally {
      configuringVscode = false;
    }
  }

  async function openBrowserCompanion() {
    try {
      browserCompanionPath = await revealBrowserCompanion();
    } catch (error) {
      settingsMessageIsError = true;
      settingsMessage = tr(
        "Could not open the extension folder.",
        "Não foi possível abrir a pasta da extensão.",
      ) + ` ${String(error).replace(/^Error:\s*/, "")}`;
    }
  }

  async function addExternalPlugin() {
    if (!isTauri || installingPlugin) return;
    installingPlugin = true;
    pluginMessage = null;
    try {
      const selected = await openDialog({
        multiple: false,
        directory: false,
        title: tr("Install agent detector", "Instalar detector de agente"),
        filters: [{ name: "Lume plugin", extensions: ["json"] }],
      });
      if (!selected || Array.isArray(selected)) return;
      const plugin = await installExternalPlugin(selected);
      externalPlugins = await loadExternalPlugins();
      pluginMessageIsError = false;
      pluginMessage = tr(`${plugin.name} is now monitored.`, `${plugin.name} agora é monitorado.`);
    } catch (error) {
      pluginMessageIsError = true;
      pluginMessage = String(error).replace(/^Error:\s*/, "");
    } finally {
      installingPlugin = false;
    }
  }

  async function uninstallExternalPlugin(id: string) {
    try {
      await removeExternalPlugin(id);
      externalPlugins = await loadExternalPlugins();
      pluginMessageIsError = false;
      pluginMessage = tr("Detector removed.", "Detector removido.");
    } catch (error) {
      pluginMessageIsError = true;
      pluginMessage = String(error).replace(/^Error:\s*/, "");
    }
  }

  async function openPluginFolder() {
    try {
      pluginMessageIsError = false;
      pluginMessage = await revealPluginDirectory();
    } catch (error) {
      pluginMessageIsError = true;
      pluginMessage = String(error).replace(/^Error:\s*/, "");
    }
  }

  async function updatePreference<K extends keyof Preferences>(
    key: K,
    value: Preferences[K],
  ): Promise<boolean> {
    const previous = preferences;
    preferences = { ...preferences, [key]: value };
    if (key === "language") {
      if (updateState === "up_to_date") {
        updateDetail = tr("You are using the latest version.", "Você está usando a versão mais recente.");
      } else if (updateState === "idle") {
        updateDetail = tr("Updates are checked automatically.", "As atualizações são verificadas automaticamente.");
      }
    }
    savingSettings = true;
    try {
      await savePreferences(preferences);
      if (isTauri) void emit("lume://preferences-changed", preferences);
      if (key === "monitorId") await positionWindow();
      return true;
    } catch (error) {
      preferences = previous;
      settingsMessageIsError = true;
      settingsMessage = String(error).replace(/^Error:\s*/, "");
      return false;
    } finally {
      savingSettings = false;
    }
  }

  async function selectAppearanceTheme(theme: Preferences["appearanceTheme"]) {
    const previous = preferences;
    preferences = { ...preferences, appearanceTheme: theme };
    savingSettings = true;
    try {
      await savePreferences(preferences);
      if (isTauri) void emit("lume://preferences-changed", preferences);
    } catch (error) {
      preferences = previous;
      settingsMessageIsError = true;
      settingsMessage = String(error).replace(/^Error:\s*/, "");
    } finally {
      savingSettings = false;
    }
  }

  async function updateAppearancePatch(patch: Partial<Preferences>) {
    const previous = preferences;
    preferences = { ...preferences, ...patch };
    savingSettings = true;
    try {
      await savePreferences(preferences);
      if (isTauri) void emit("lume://preferences-changed", preferences);
    } catch (error) {
      preferences = previous;
      settingsMessageIsError = true;
      settingsMessage = String(error).replace(/^Error:\s*/, "");
    } finally {
      savingSettings = false;
    }
  }

  async function resetSettings() {
    if (!resetConfirming) {
      resetConfirming = true;
      settingsMessage = null;
      return;
    }
    resettingSettings = true;
    settingsMessage = null;
    try {
      preferences = { ...defaultPreferences };
      await savePreferences(preferences);
      if (isTauri) void emit("lume://preferences-changed", preferences);
      selectedLayoutId = null;
      layoutName = "";
      resetConfirming = false;
      await positionWindow();
    } catch (error) {
      settingsMessageIsError = true;
      settingsMessage = String(error).replace(/^Error:\s*/, "");
    } finally {
      resettingSettings = false;
    }
  }

  function projectKey(value: string) {
    const normalized = value.trim().replaceAll("\\", "/").replace(/\/+$/, "");
    const identity = /^[a-z]:/i.test(normalized) ? normalized.toLowerCase() : normalized;
    let hash = 0x811c9dc5;
    for (let index = 0; index < identity.length; index += 1) {
      hash ^= identity.charCodeAt(index);
      hash = Math.imul(hash, 0x01000193);
    }
    return `project-${(hash >>> 0).toString(16).padStart(8, "0")}`;
  }

  function projectSoundEnabled(session: AgentSession) {
    const profile = preferences.projectProfiles[projectKey(session.workingDirectory ?? session.project)];
    return profile?.soundEnabled ?? true;
  }

  async function updateSelectedProjectProfile(
    patch: Partial<Preferences["projectProfiles"][string]>,
  ) {
    if (!selectedProfileKey || !selectedProject) return;
    const current = preferences.projectProfiles[selectedProfileKey] ?? {
      label: selectedProject.label,
      soundEnabled: true,
      launchTarget: undefined,
      monitorId: undefined,
      overlayX: undefined,
      overlayY: undefined,
      permissionMode: undefined,
      approvalPolicy: undefined,
      whiteboardLayoutId: undefined,
      preferredAgents: [],
    };
    await updatePreference("projectProfiles", {
      ...preferences.projectProfiles,
      [selectedProfileKey]: { ...current, ...patch },
    });
  }

  async function captureProfilePosition() {
    const position = expanded
      ? compactPositionFromExpanded(overlayPosition)
      : overlayPosition;
    await updateSelectedProjectProfile({
      overlayX: Math.round(position.x),
      overlayY: Math.round(position.y),
    });
  }

  async function togglePreferredAgent(agent: AgentKind) {
    const current = selectedProjectProfile?.preferredAgents ?? [];
    await updateSelectedProjectProfile({
      preferredAgents: current.includes(agent)
        ? current.filter((item) => item !== agent)
        : [...current, agent],
    });
  }

  function integrationAgentKind(kind: IntegrationStatus["kind"]): AgentKind {
    return kind === "claude" ? "claude_code" : kind;
  }

  async function applySelectedProjectProfile() {
    const profile = selectedProjectProfile;
    if (!profile) return;
    const previous = preferences;
    preferences = {
      ...preferences,
      monitorId: profile.monitorId ?? preferences.monitorId,
      overlayX: profile.overlayX ?? preferences.overlayX,
      overlayY: profile.overlayY ?? preferences.overlayY,
    };
    try {
      await savePreferences(preferences);
    } catch (error) {
      preferences = previous;
      settingsMessageIsError = true;
      settingsMessage = String(error).replace(/^Error:\s*/, "");
      return;
    }
    if (isTauri) void emit("lume://preferences-changed", preferences);
    await positionWindow(true);
    const layout = preferences.whiteboardLayouts.find(
      (item) => item.id === profile.whiteboardLayoutId,
    );
    if (layout) {
      view = "board";
      await restoreSavedLayout(layout);
      if (terminalMessageIsError) return;
    }
    settingsMessageIsError = false;
    settingsMessage = tr("Project profile applied.", "Perfil do projeto aplicado.");
  }

  function launcherIntegrations() {
    const preferred = selectedProjectProfile?.preferredAgents ?? [];
    return integrations
      .filter((integration) => integration.installed && integration.canLaunch)
      .slice()
      .sort((left, right) => {
        const leftIndex = preferred.indexOf(integrationAgentKind(left.kind));
        const rightIndex = preferred.indexOf(integrationAgentKind(right.kind));
        if (leftIndex === rightIndex) return left.label.localeCompare(right.label);
        if (leftIndex < 0) return 1;
        if (rightIndex < 0) return -1;
        return leftIndex - rightIndex;
      });
  }

  function playTone(kind: ToneKind) {
    playNotificationTone(kind, preferences.soundVolume);
  }

  function actionLabel(action: PermissionAction) {
    return {
      allow_once: tr("Allow once", "Permitir uma vez"),
      allow_session: tr("For this session", "Nesta sessão"),
      deny: tr("Deny", "Recusar"),
      open_source: tr("Open source", "Abrir origem"),
    }[action];
  }

  function sourceLabel(session: AgentSession) {
    if (session.source === "web") {
      if (session.sourceApp === "chrome") return "Chrome";
      if (session.sourceApp === "edge") return "Edge";
      if (session.sourceApp === "brave") return "Brave";
      return "Web";
    }
    return { cli: "CLI", vscode: "VS Code", desktop: "Lume" }[session.source];
  }

  function sourceIcon(session: AgentSession) {
    if (session.source === "cli") return "terminal" as const;
    if (session.source === "vscode") return "vscode" as const;
    if (session.source === "web") return session.sourceApp ?? ("browsers" as const);
    return session.source === "desktop" ? ("lume" as const) : ("unknown" as const);
  }

  function relativeTime(timestamp: number) {
    const seconds = Math.max(0, Math.round((Date.now() - timestamp) / 1_000));
    if (seconds < 60) return tr("now", "agora");
    const minutes = Math.floor(seconds / 60);
    if (minutes < 60) return tr(`${minutes} min ago`, `há ${minutes} min`);
    const hours = Math.floor(minutes / 60);
    if (hours < 24) return tr(`${hours} hr ago`, `há ${hours} h`);
    return new Intl.DateTimeFormat(preferences.language === "pt-BR" ? "pt-BR" : "en", { day: "2-digit", month: "short" }).format(
      timestamp,
    );
  }

</script>

<svelte:head>
  <title>Lume</title>
  <meta name="description" content="A discreet local monitor for AI agent sessions." />
</svelte:head>

<main
  data-appearance={appearance.theme}
  class:expanded
  class:dark={effectiveDark}
  class:morphing={morphing !== null}
  class="overlay-shell"
  style={`--panel-radius: ${panelRadius}; --morph-width: ${morphWidth}px; --morph-height: ${morphHeight}px;${appearance.accentCss ? ` --lume-accent: ${appearance.accentCss}; --lume-accent-strong: ${appearance.accentCss};` : ""}${appearance.baseCss ? ` ${appearance.baseCss};` : ""}`}
  onpointermove={wakeMascot}
  aria-label={tr("Lume, agent monitor", "Lume, monitor de agentes")}
>
  {#if cliAssociationSessionId}
    {#key cliAssociationSessionId}
      <CodexCliAssociationDialog sessionId={cliAssociationSessionId} language={preferences.language} dark={effectiveDark} onClose={() => { cliAssociationSessionId = null; }} onLinked={() => refreshSessions(false)} />
    {/key}
  {/if}
  {#if cliContextMenu && expanded && view === "sessions"}
    <div class="cli-context-menu" bind:this={cliContextMenuNode} role="menu" aria-label={tr("Session actions", "Ações da sessão")} tabindex="-1" style:left={`${cliContextMenu.x}px`} style:top={`${cliContextMenu.y}px`} transition:fade={{ duration: 100 }}>
      <button type="button" role="menuitem" onclick={() => {
        cliAssociationSessionId = cliContextMenu?.sessionId ?? null;
        cliContextMenu = null;
      }}>
        <LumeIcon name="split" size={16} />
        <span>{tr("Link conversation", "Vincular conversa")}</span>
      </button>
    </div>
  {/if}
  {#if !expanded}
    <button
      class="lume-orb status-{shellStatus}"
      class:dragging
      class:docked={orbDockName !== "free"}
      class:settling={orbSettling}
      data-orb-dock={orbDockName}
      style={orbSurfaceStyle}
      type="button"
      onclick={toggleExpanded}
      onpointerdown={(event) => beginOverlayDrag(event, true)}
      onpointermove={moveOverlayDrag}
      onpointerup={(event) => endOverlayDrag(event, true)}
      onpointercancel={(event) => endOverlayDrag(event, true)}
      onlostpointercapture={(event) => endOverlayDrag(event, true)}
      aria-label={tr(`Open Lume, ${activeCount} active agents`, `Abrir Lume, ${activeCount} agentes ativos`)}
      aria-describedby={orbDockName === "free" ? undefined : "orb-dock-help"}
    >
      <span class="orb-surface" aria-hidden="true"></span>
      <span class="orb-content">
        <LumeMascot status={shellStatus} awake={mascotAwake || dragging} size={32} />
        <span class="agent-count">{activeCount}</span>
      </span>
      {#if orbDockName !== "free"}
        <span id="orb-dock-help" hidden>{tr(
          "Drag away from the edge to undock. Click to open.",
          "Arraste para longe da borda para desacoplar. Clique para abrir.",
        )}</span>
      {/if}
    </button>
  {:else}
    <section use:observePanelSize class:content-visible={contentVisible} class:morphing class:measuring={measuringPanel} class:palette-open={paletteOpen} class:launcher-open={launcherOpen} class:workflow-settings-open={workflowSettingsOpen} class:onboarding={startupChooserOpen} class="panel">
      {#if startupChooserOpen}
        <div class="startup-chooser-layer">
          <StartupModeChooser language={preferences.language} onChoose={chooseStartupMode} />
        </div>
      {/if}
      <header
        role="banner"
        class:dragging
        class="panel-header"
        onpointerdown={beginOverlayDrag}
        onpointermove={moveOverlayDrag}
        onpointerup={endOverlayDrag}
        onpointercancel={endOverlayDrag}
        onlostpointercapture={endOverlayDrag}
      >
        <div class="brand-lockup">
          <LumeMascot status={shellStatus} awake={mascotAwake || dragging} size={32} />
          <div>
            <strong>Lume</strong>
            <span>{activeCount === 1 ? tr("1 active agent", "1 agente ativo") : tr(`${activeCount} active agents`, `${activeCount} agentes ativos`)}</span>
          </div>
        </div>
        <div class="header-actions">
          <button class:opening={openingWorkspace} class="workspace-button" type="button" title={tr("Open Workspace", "Abrir Workspace")} disabled={openingWorkspace} aria-busy={openingWorkspace} onclick={() => void showWorkspaceWindow()} aria-label={tr("Open Workspace", "Abrir Workspace")}>
            <WorkspaceHeaderIcon name="orb" size={19} />
          </button>
          <button class="palette-button" type="button" title={preferences.globalShortcut} onclick={showCommandPalette} aria-label={tr("Open command palette", "Abrir paleta de comandos")}>
            <svg viewBox="0 0 20 20" aria-hidden="true"><circle cx="8.5" cy="8.5" r="4.5" /><path d="m12 12 4 4" /></svg>
          </button>
          {#if view === "sessions"}
            <button
              class:active={launcherOpen}
              class="add-button"
              type="button"
              onclick={toggleLauncher}
              aria-expanded={launcherOpen}
              aria-controls="session-launcher"
              aria-label={launcherOpen ? tr("Close session launcher", "Fechar iniciador de sessões") : tr("Open or resume session", "Abrir ou retomar sessão")}
            >
              <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M10 5v10M5 10h10" /></svg>
            </button>
          {/if}
          <button class="collapse-button" type="button" onclick={toggleExpanded} aria-label={tr("Collapse", "Recolher")}>
            <svg viewBox="0 0 20 20" aria-hidden="true"><path d="m5.5 8 4.5 4 4.5-4" /></svg>
          </button>
        </div>
        <SystemBannerStack items={systemBanners} contained offset="calc(100% + 8px)" language={preferences.language} dismissLabel={tr("Dismiss", "Fechar")} />
      </header>

      {#if newMobileDevice}
        <aside class="mobile-device-banner" transition:slide={{ duration: 180, easing: cubicOut }}>
          <span class="mobile-device-banner-icon" aria-hidden="true">
            <svg viewBox="0 0 20 20"><rect x="5.5" y="2.5" width="9" height="15" rx="2" /><path d="M8.5 5h3M9 14.5h2" /></svg>
          </span>
          <span>
            <strong>{tr("New phone connected", "Novo celular conectado")}</strong>
            <small>{newMobileDevice.name}</small>
          </span>
          <button type="button" onclick={reviewNewMobileDevice}>
            {tr("Review permissions", "Ver permissões")}
          </button>
        </aside>
      {/if}

      {#if launcherOpen}
        <div id="session-launcher" class="launcher-popover" transition:sessionLauncherTransition={{ duration: 190 }}>
          <div class="launcher-popover-scroll">
            <span class="launcher-title">{tr("Open session", "Abrir sessão")}</span>
            {#each launcherIntegrations() as integration}
              <div class:expanded={resumeAgent === integration.kind} class="launcher-agent">
                <div class="launcher-row">
                  <span class="agent-avatar agent-{integration.kind}"><BrandIcon name={integration.kind} size={17} /></span>
                  <strong>{integration.label}</strong>
                  <button
                    class:loading={launching === integration.kind && launchingSessionId === null}
                    disabled={launching !== null}
                    type="button"
                    onclick={() => startSession(integration.kind)}
                    aria-busy={launching === integration.kind && launchingSessionId === null}
                  >
                    <span class="launcher-button-content">
                      {#if launching === integration.kind && launchingSessionId === null}
                        <span class="launcher-spinner" aria-hidden="true"></span>
                      {/if}
                      <span>
                        {#if launching === integration.kind && launchingSessionId === null}
                          {launchingPhase === "choosing" ? tr("Choose…", "Escolher…") : tr("Opening…", "Abrindo…")}
                        {:else}
                          {tr("New", "Nova")}
                        {/if}
                      </span>
                    </span>
                  </button>
                  {#if integration.kind !== "gemini"}
                    <button
                      class:active={resumeAgent === integration.kind}
                      class:loading={loadingResumeAgent === integration.kind}
                      disabled={launching !== null || loadingResumeAgent !== null}
                      type="button"
                      onclick={() => toggleResumeSessions(integration.kind)}
                      aria-busy={loadingResumeAgent === integration.kind}
                    >
                      <span class="launcher-button-content">
                        {#if loadingResumeAgent === integration.kind}
                          <span class="launcher-spinner" aria-hidden="true"></span>
                        {/if}
                        <span>{loadingResumeAgent === integration.kind ? tr("Loading…", "Buscando…") : tr("Resume", "Retomar")}</span>
                      </span>
                    </button>
                  {/if}
                </div>
                {#if resumeAgent === integration.kind}
                  <div class="resume-session-list" transition:slide={{ duration: 145, easing: cubicOut }}>
                    {#if loadingResumeAgent === integration.kind}
                      <div class="launcher-loading-note" role="status" aria-live="polite" transition:fade={{ duration: 120 }}>
                        <span class="launcher-spinner" aria-hidden="true"></span>
                        <span>{tr("Finding recent sessions…", "Buscando sessões recentes…")}</span>
                      </div>
                    {:else if resumableSessions.length > 0}
                      {#each resumableSessions as stored (stored.id)}
                        <button
                          class:loading={launchingSessionId === stored.id}
                          class="resume-session"
                          disabled={launching !== null}
                          type="button"
                          title={stored.workingDirectory}
                          onclick={() => resumeStoredSession(stored)}
                          aria-busy={launchingSessionId === stored.id}
                        >
                          <span>
                            <strong>{stored.name}</strong>
                            <small>{launchingSessionId === stored.id ? tr("Opening session…", "Abrindo sessão…") : `${stored.source} · ${relativeTime(stored.updatedAt)}`}</small>
                          </span>
                          {#if launchingSessionId === stored.id}
                            <span class="launcher-spinner" aria-hidden="true"></span>
                          {:else}
                            <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M6 5h8v8M13.5 5.5 5 14" /></svg>
                          {/if}
                        </button>
                      {/each}
                    {:else}
                      <p>{tr("No resumable sessions were found.", "Nenhuma sessão retomável foi encontrada.")}</p>
                    {/if}
                  </div>
                {/if}
              </div>
            {:else}
              <p>{tr("No compatible CLI was found.", "Nenhuma CLI compatível foi encontrada.")}</p>
            {/each}
          </div>
        </div>
      {/if}

      {#if workflowSettingsOpen}
        <button class="workflow-settings-dismiss" type="button" aria-label={tr("Close workflow settings", "Fechar configurações do workflow")} onclick={() => (workflowSettingsOpen = false)}></button>
        <section class="workflow-settings-popover" transition:fade={{ duration: 100 }}>
          <div class="workflow-settings-scroll">
          <header>
            <div>
              <strong>{tr("Workflow settings", "Configurações do workflow")}</strong>
              <small>{tr("Global safety limits", "Limites globais de segurança")}</small>
            </div>
            <button type="button" aria-label={tr("Close", "Fechar")} onclick={() => (workflowSettingsOpen = false)}>×</button>
          </header>

          <div class="workflow-setting-grid">
            <label>
              <span>{tr("Transitions", "Transições")}</span>
              <input type="number" min="1" max="100" value={preferences.workflowSettings.maxTransitions} disabled={workflowSettingsSaving} onchange={(event) => void updateWorkflowSetting("maxTransitions", Number(event.currentTarget.value))} />
            </label>
            <label>
              <span>{tr("Attempts per step", "Tentativas por etapa")}</span>
              <input type="number" min="1" max="10" value={preferences.workflowSettings.maxAttemptsPerStep} disabled={workflowSettingsSaving} onchange={(event) => void updateWorkflowSetting("maxAttemptsPerStep", Number(event.currentTarget.value))} />
            </label>
            <label>
              <span>{tr("Timeout (minutes)", "Timeout (minutos)")}</span>
              <input type="number" min="0" max="1440" value={preferences.workflowSettings.stepTimeoutMinutes} disabled={workflowSettingsSaving} onchange={(event) => void updateWorkflowSetting("stepTimeoutMinutes", Number(event.currentTarget.value))} />
            </label>
            <label>
              <span>{tr("Context tokens", "Tokens de contexto")}</span>
              <input type="number" min="1000" max="100000" step="1000" value={preferences.workflowSettings.maxContextTokens} disabled={workflowSettingsSaving} onchange={(event) => void updateWorkflowSetting("maxContextTokens", Number(event.currentTarget.value))} />
            </label>
          </div>

          <label class="workflow-setting-toggle">
            <span>{tr("Approve sensitive handoffs", "Aprovar handoffs sensíveis")}</span>
            <input type="checkbox" checked={preferences.workflowSettings.requireApprovalForSensitiveContext} disabled={workflowSettingsSaving} onchange={(event) => void updateWorkflowSetting("requireApprovalForSensitiveContext", event.currentTarget.checked)} />
            <i aria-hidden="true"></i>
          </label>
          <label class="workflow-setting-toggle">
            <span>{tr("Protect agent rate limits", "Proteger limites dos agentes")}</span>
            <input type="checkbox" checked={preferences.workflowSettings.pauseOnRateLimit} disabled={workflowSettingsSaving} onchange={(event) => void updateWorkflowSetting("pauseOnRateLimit", event.currentTarget.checked)} />
            <i aria-hidden="true"></i>
          </label>
          {#if preferences.workflowSettings.pauseOnRateLimit}
            <label class="workflow-reserve-setting">
              <span>{tr("Minimum remaining", "Reserva mínima")}</span>
              <input type="range" min="0" max="50" step="5" value={preferences.workflowSettings.minimumRateLimitRemainingPercent} disabled={workflowSettingsSaving} onchange={(event) => void updateWorkflowSetting("minimumRateLimitRemainingPercent", Number(event.currentTarget.value))} />
              <strong>{preferences.workflowSettings.minimumRateLimitRemainingPercent}%</strong>
            </label>
          {/if}

          {#if missingWorkflowSteps().length > 0}
            <div class="workflow-missing-sessions">
              <strong>{tr("Missing agents", "Agentes ausentes")}</strong>
              {#each missingWorkflowSteps() as missing (`${missing.group.id}:${missing.step.id}`)}
                <label>
                    <span title={`${missing.step.customRoleLabel || missing.step.role} · ${tr("Step", "Etapa")} ${missing.index + 1}`}>{missing.step.customRoleLabel || missing.step.role} · {missing.index + 1}</span>
                  <LumeSelect
                    ariaLabel={tr("Replacement agent session", "Sessão substituta do agente")}
                    value=""
                    minWidth={168}
                    options={[
                      { value: "", label: workflowRebindingStepId === missing.step.id ? tr("Replacing…", "Substituindo…") : tr("Replace session…", "Substituir sessão…") },
                      ...workflowReplacementSessions(missing.group.id, missing.step.id).map((session) => ({
                        value: workflowSessionKey(session),
                        label: sessionDisplayName(session),
                        description: `${session.agentLabel} · ${session.project}`,
                      })),
                    ]}
                    onValueChange={(value) => void replaceWorkflowSession(missing.group.id, missing.step.id, value)}
                  />
                </label>
              {/each}
            </div>
          {/if}
          </div>
        </section>
      {/if}

      {#if paletteOpen}
        <div class="command-palette-layer" transition:fade={{ duration: 120 }}>
          <button class="command-palette-backdrop" type="button" aria-label={tr("Close command palette", "Fechar paleta de comandos")} onclick={() => void closeCommandPalette()}></button>
          <div class="command-palette" role="dialog" aria-label={tr("Command palette", "Paleta de comandos")}>
            <div class="command-search">
              <svg viewBox="0 0 20 20" aria-hidden="true"><circle cx="8.5" cy="8.5" r="4.5" /><path d="m12 12 4 4" /></svg>
              <input
                data-command-palette
                value={paletteQuery}
                placeholder={tr("Search sessions and commands…", "Buscar sessões e comandos…")}
                oninput={(event) => { paletteQuery = event.currentTarget.value; paletteIndex = 0; }}
                onkeydown={handlePaletteKey}
              />
              <kbd>Esc</kbd>
            </div>
            <div class="command-results">
              {#each paletteCommands() as command, index (command.id)}
                <button class:active={paletteIndex === index} type="button" onmouseenter={() => (paletteIndex = index)} onclick={() => runPaletteCommand(command)}>
                  <span><strong>{command.label}</strong><small>{command.detail}</small></span>
                  <kbd>↵</kbd>
                </button>
              {:else}
                <p>{tr("No matching command.", "Nenhum comando encontrado.")}</p>
              {/each}
            </div>
          </div>
        </div>
      {/if}

      {#if shortcutEditorKey}
        <div class="shortcut-editor-layer" transition:fade={{ duration: 120 }}>
          <button class="shortcut-editor-backdrop" type="button" aria-label={tr("Close shortcut editor", "Fechar editor de atalho")} onclick={() => (shortcutEditorKey = null)}></button>
          <div class="shortcut-editor" role="dialog" aria-modal="true" aria-labelledby="shortcut-editor-title">
            <strong id="shortcut-editor-title">{tr("Press a new shortcut", "Pressione um novo atalho")}</strong>
            <small>{tr("Use one or more modifier keys with another key.", "Use uma ou mais teclas modificadoras com outra tecla.")}</small>
            <button data-shortcut-capture class="shortcut-capture" type="button" onkeydown={captureShortcut}>
              <kbd>{shortcutDraft || tr("Press keys…", "Pressione as teclas…")}</kbd>
            </button>
            <div class="shortcut-editor-actions">
              <button type="button" onclick={() => (shortcutEditorKey = null)}>{tr("Cancel", "Cancelar")}</button>
              <button class="primary" disabled={!shortcutDraft || savingSettings} type="button" onclick={() => void saveShortcut()}>{tr("Save", "Salvar")}</button>
            </div>
          </div>
        </div>
      {/if}

      <div class="panel-content" class:inspector-content={view === "history"}>
        {#if view === "sessions"}
          <div class="session-list" use:revealScrollbarWhileScrolling>
            {#if visibleAgentSessions.length}
              {#each orbSections as section (section.key)}
              {#if section.group || orbSections.length > 1}
                <button class:collapsed={section.group?.collapsed} class="orb-group-heading" type="button" disabled={!section.group} aria-expanded={section.group ? !section.group.collapsed : undefined} onclick={() => section.group && toggleOrbGroup(section.group.id)}>
                  {#if section.group}<span class="orb-group-chevron"><svg viewBox="0 0 20 20" width="11" height="11" aria-hidden="true"><path d="m6 8 4 4 4-4" /></svg></span>{/if}
                  <strong>{section.group?.name ?? tr("No group", "Sem grupo")}</strong><small>{section.sessions.length}</small>
                </button>
              {/if}
              {#if !section.group?.collapsed}
              <div class="orb-group-body" transition:slide={{ duration: 180 }}>
              {#each section.sessions as session (session.id)}
                {@const visibleLastResponse = stripInternalAgentMetadata(session.lastResponse)}
                <article
                  animate:flip={{ duration: 220 }}
                  class:attention={needsAttention(session)}
                  class:selected={selectedId === session.id}
                  class="session-row"
                >
                  <button class="session-summary" type="button" aria-expanded={selectedId === session.id} onclick={() => openSession(session)}
                    oncontextmenu={(event) => {
                      if (!canLinkCodexCli(session)) return;
                      event.preventDefault();
                      openCliContextMenu(session, event.currentTarget, event.clientX, event.clientY);
                    }}
                    onkeydown={(event) => {
                      if (!canLinkCodexCli(session) || !(event.key === "ContextMenu" || (event.shiftKey && event.key === "F10"))) return;
                      event.preventDefault();
                      const rect = event.currentTarget.getBoundingClientRect();
                      openCliContextMenu(session, event.currentTarget, rect.left + rect.width / 2, rect.bottom);
                    }}>
                    <span class="thread-avatar-shell">
                      <ThreadAvatar seed={session.nativeSessionId || session.sessionName || session.id} label={sessionDisplayName(session)} size={32} />
                    </span>
                    <span class="session-copy">
                      <span class="session-title-row">
                        <strong>{sessionDisplayName(session)}</strong>
                        {#if session.controlOrigin === "external"}
                          <span class="source-label">
                            <BrandIcon name={sourceIcon(session)} size={session.source === "web" ? 11 : 9} />
                            {sourceLabel(session)}
                          </span>
                        {/if}
                        {#if session.permissionProfile.approvalsReviewer === "auto_review" && session.permissionProfile.mode !== "full_access"}
                          <span class="access-badge auto-review">
                            <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M6.8.8 2.9 6.3h2.5L4.9 11l4.2-5.7H6.5Z" /></svg>
                            {tr("Auto", "Auto")}
                          </span>
                        {/if}
                        {#if session.permissionProfile.mode === "full_access"}
                          <span class="access-badge full-access">
                            <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M3 5V3.7a3 3 0 0 1 5.6-1.5M2.2 5.2h7.6v5.5H2.2Z" /></svg>
                            {tr("Full access", "Acesso total")}
                          </span>
                        {/if}
                      </span>
                      <span class="project-name" title={session.workingDirectory}>
                        <BrandIcon name={session.agent} size={10} />
                        <span>{sessionDirectoryName(session)}</span>
                      </span>
                      <span class="status-line status-{session.status}">
                        {#if session.status === "running"}
                          <span class="running-dots" aria-hidden="true"><i></i><i></i><i></i></span>
                        {:else}
                          <i></i>
                        {/if}
                        {shown(session.statusLabel)}
                      </span>
                    </span>
                    <svg class="chevron" viewBox="0 0 20 20" aria-hidden="true">
                      <path d="m8 5 5 5-5 5" />
                    </svg>
                  </button>

                  {#if selectedId === session.id}
                    {@const capabilities = sessionCapabilities(session)}
                    {@const queuedPrompts = pendingQueuedPrompts(session)}
                    <div class="session-details" transition:slide={{ duration: 190, easing: cubicOut }}>
                      {#if session.pendingPermission}
                        <div class="permission-block risk-{session.pendingPermission.risk}">
                          <strong>{shown(session.pendingPermission.summary)}</strong>
                          <code>{session.pendingPermission.resource}</code>
                          <div class="permission-actions">
                            {#each session.permissionProfile.availableActions as action}
                              {@const deciding = permissionPending?.sessionId === session.id && permissionPending.action === action}
                              <button
                                class:primary={action === "allow_once"}
                                class:danger={action === "deny"}
                                class:loading={deciding}
                                type="button"
                                disabled={permissionPending !== null}
                                onclick={() => handlePermission(session, action)}
                              >
                                {#if deciding}<i class="permission-spinner" aria-hidden="true"></i>{/if}
                                {actionLabel(action)}
                              </button>
                            {/each}
                          </div>
                        </div>
                      {/if}
                      {#if session.pendingQuestion}
                        <div class="question-block">
                          <span class="eyebrow">{tr("Agent question", "Pergunta do agente")}</span>
                          {#each session.pendingQuestion.questions as question}
                            <section>
                              <strong>{shown(question.question)}</strong>
                              {#if question.options.length}
                                <div class="question-actions">
                                  {#each question.options as option, index}
                                    <button
                                      class:selected={questionSelections[`${session.pendingQuestion.id}:${question.id}`] === option.label}
                                      type="button"
                                      onclick={() => void handleQuestionOption(session, question.id, option.label)}
                                    >
                                      <b>{index + 1}</b> {shown(option.label)}
                                    </button>
                                  {/each}
                                </div>
                              {/if}
                              <small>{question.options.length
                                ? tr("Select an option to answer.", "Selecione uma opção para responder.")
                                : tr("Open the terminal to answer this question.", "Abra o terminal para responder a esta pergunta.")}</small>
                            </section>
                          {/each}
                        </div>
                      {/if}

                      <div class="session-action-bar" aria-label={tr("Session actions", "Ações da sessão")}>
                        <button
                          class="session-action-button"
                          type="button"
                          data-label={tr("Rename session", "Renomear sessão")}
                          aria-label={tr("Rename session", "Renomear sessão")}
                          onclick={() => beginSessionRename(session)}
                        >
                          <svg viewBox="0 0 20 20" aria-hidden="true"><path d="m4 14-.5 2.5L6 16l9-9-2-2-9 9Z"></path><path d="m11.5 6.5 2 2"></path></svg>
                        </button>
                        {#if capabilities.canOpenSource}
                          <button
                            class="session-action-button"
                            type="button"
                            data-label={tr("Open source", "Abrir origem")}
                            aria-label={tr("Open source", "Abrir origem")}
                            onclick={() => openSessionSource(session.id)}
                          >
                            <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M7 5h8v8M14.5 5.5 6 14"></path><path d="M13 15H5V7"></path></svg>
                          </button>
                        {/if}
                        {#if canContinueSession(session) && canSubmitToSession(session)}
                          <button
                            class:active={composerSessionId === session.id}
                            class="session-action-button"
                            type="button"
                            data-label={session.status === "waiting_for_input" ? tr("Send prompt", "Enviar prompt") : tr("Continue", "Continuar")}
                            aria-label={session.status === "waiting_for_input" ? tr("Send prompt", "Enviar prompt") : tr("Continue", "Continuar")}
                            onclick={() => toggleSessionComposer(session)}
                          >
                            <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M4 10h11M11 6l4 4-4 4"></path></svg>
                          </button>
                        {/if}
                        {#if canInterruptSession(session)}
                          <button
                            class="session-action-button warning"
                            disabled={interruptingSessionId === session.id}
                            type="button"
                            data-label={interruptingSessionId === session.id ? tr("Interrupting…", "Interrompendo…") : tr("Interrupt prompt", "Interromper prompt")}
                            aria-label={interruptingSessionId === session.id ? tr("Interrupting…", "Interrompendo…") : tr("Interrupt prompt", "Interromper prompt")}
                            onclick={() => void interruptSessionPrompt(session)}
                          >
                            <svg viewBox="0 0 20 20" aria-hidden="true"><rect x="6" y="6" width="8" height="8" rx="1"></rect></svg>
                          </button>
                        {/if}
                        {#if canTerminateSession(session)}
                          <button
                            class="session-action-button danger"
                            disabled={terminatingSessionId === session.id}
                            type="button"
                            data-label={tr("Stop agent", "Encerrar agente")}
                            aria-label={tr("Stop agent", "Encerrar agente")}
                            onclick={() => void terminateAgent(session)}
                          >
                            <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M10 3v7M5.5 5.5a6 6 0 1 0 9 0"></path></svg>
                          </button>
                        {/if}
                      </div>

                      {#if renamingSessionId === session.id}
                        <form class="session-name-editor" onsubmit={(event) => { event.preventDefault(); void saveSessionRename(session); }}>
                          <input
                            maxlength="80"
                            bind:value={renameDraft}
                            aria-label={tr("Session name", "Nome da sessão")}
                            onkeydown={(event) => {
                              if (event.key === "Escape") {
                                event.preventDefault();
                                cancelSessionRename();
                              }
                            }}
                          />
                          <button class="primary" disabled={renamingSession} type="submit">{tr("Save", "Salvar")}</button>
                          <button disabled={renamingSession} type="button" onclick={cancelSessionRename}>{tr("Cancel", "Cancelar")}</button>
                        </form>
                      {/if}

                      {#if terminateConfirmId === session.id}
                        <div class="terminate-agent-control confirming">
                          <span>{tr("Stop the agent and its running commands?", "Encerrar o agente e os comandos em execução?")}</span>
                          <button type="button" onclick={() => (terminateConfirmId = null)}>{tr("Cancel", "Cancelar")}</button>
                          <button class="danger" disabled={terminatingSessionId === session.id} type="button" onclick={() => void terminateAgent(session)}>
                            {terminatingSessionId === session.id ? tr("Stopping…", "Encerrando…") : tr("Stop", "Encerrar")}
                          </button>
                        </div>
                      {/if}

                      {#if canContinueSession(session) && canSubmitToSession(session) && composerSessionId === session.id}
                          <form
                            class="inline-composer"
                            onpaste={(event) => void pasteSessionImages(event, session)}
                            onsubmit={(event) => {
                              event.preventDefault();
                              void sendSessionPrompt(session);
                            }}
                            transition:slide={{ duration: 160, easing: cubicOut }}
                          >
                            {#if composerAttachments.length}
                              <div class="inline-attachments">
                                {#each composerAttachments as attachment, index}
                                  <span title={attachment.name}>
                                    <img src={attachment.previewDataUrl} alt={attachment.name} />
                                    <button
                                      type="button"
                                      onclick={() => removeComposerImage(index)}
                                      aria-label={tr("Remove image", "Remover imagem")}
                                    >×</button>
                                  </span>
                                {/each}
                              </div>
                            {/if}
                            {#if queuedPrompts[0]?.kind === "codex_queued_prompt"}
                              <div class="inline-queue-tray read-only" role="status">
                                <span class="queue-mark" aria-hidden="true">↳</span>
                                <span class="queue-copy">
                                  <small>{queuedPrompts.length > 1 ? tr(`${queuedPrompts.length} queued prompts`, `${queuedPrompts.length} prompts na fila`) : tr("Queued via Codex CLI", "Na fila pela CLI do Codex")}</small>
                                  <strong>{queuedPrompts[0].detail || tr("Prompt queued in Codex", "Prompt na fila do Codex")}</strong>
                                </span>
                                <span class="queue-shortcut"><small>{tr("Read only", "Somente leitura")}</small></span>
                              </div>
                            {:else if queuedPrompts[0]}
                              <button
                                class="inline-queue-tray"
                                disabled={steeringQueuedActivityId !== null}
                                type="button"
                                onclick={() => void steerSessionQueuedPrompt(session)}
                                aria-label={tr("Steer the next queued prompt now", "Enviar agora o próximo prompt da fila")}
                              >
                                <span class="queue-mark" aria-hidden="true">↳</span>
                                <span class="queue-copy">
                                  <small>{queuedPrompts.length > 1 ? tr(`${queuedPrompts.length} queued prompts`, `${queuedPrompts.length} prompts na fila`) : tr("Queued next", "Próximo na fila")}</small>
                                  <strong>{queuedPrompts[0].detail || tr("Prompt with attached images", "Prompt com imagens anexadas")}</strong>
                                </span>
                                <span class="queue-shortcut">
                                  <kbd>Tab</kbd>
                                  <small>{steeringQueuedActivityId === queuedPrompts[0].id ? tr("Steering…", "Enviando…") : tr("Steer now", "Enviar agora")}</small>
                                </span>
                              </button>
                            {/if}
                            <div class="inline-composer-controls">
                              <textarea
                                bind:value={composerPrompt}
                                onkeydown={(event) => handleSessionComposerKeydown(event, session)}
                                aria-label={tr(`New prompt for ${sessionDisplayName(session)}`, `Novo prompt para ${sessionDisplayName(session)}`)}
                                placeholder={tr("Paste an image or enter the next prompt…", "Cole uma imagem ou digite o próximo prompt…")}
                                rows="2"
                              ></textarea>
                              <button
                                disabled={(!composerPrompt.trim() && composerAttachments.length === 0) || composerSending}
                                type="submit"
                                aria-label={tr("Send prompt", "Enviar prompt")}
                              >
                                <svg viewBox="0 0 20 20" aria-hidden="true"><path d="m4 10 12-6-4 12-2-4zM10 12l2-2" /></svg>
                              </button>
                            </div>
                          </form>
                      {/if}

                      {#if visibleLastResponse}
                        <details class="final-response" open={!needsAttention(session)}>
                          <summary>{needsAttention(session) ? tr("Previous response", "Resposta anterior") : tr("Latest response", "Última resposta")}</summary>
                          <div class="final-response-body">
                            <button
                              class="final-response-copy"
                              type="button"
                              onclick={() => copyResult(`${session.id}-latest`, visibleLastResponse)}
                              aria-label={copiedResultId === `${session.id}-latest` ? tr("Copied", "Copiado") : tr("Copy final response", "Copiar resposta final")}
                              title={copiedResultId === `${session.id}-latest` ? tr("Copied", "Copiado") : tr("Copy", "Copiar")}
                            >
                              {#if copiedResultId === `${session.id}-latest`}
                                <svg viewBox="0 0 20 20" aria-hidden="true"><path d="m5 10 3 3 7-7" /></svg>
                              {:else}
                                <svg viewBox="0 0 20 20" aria-hidden="true"><rect x="7" y="6" width="8" height="9" rx="1.5" /><path d="M12 6V4.5A1.5 1.5 0 0 0 10.5 3h-6A1.5 1.5 0 0 0 3 4.5v7A1.5 1.5 0 0 0 4.5 13H7" /></svg>
                              {/if}
                            </button>
                            <OrbResponse text={visibleLastResponse} language={preferences.language} />
                          </div>
                        </details>
                      {/if}

                    </div>
                  {/if}
                </article>
              {/each}
              </div>
              {/if}
              {/each}
            {:else}
              <div class="empty-state" transition:fade>
                <span class="quiet-orbit" aria-hidden="true"><i></i></span>
                <strong>{tr("No active sessions", "Nenhuma sessão ativa")}</strong>
                <p>{tr("New sessions will appear here automatically.", "Novas sessões aparecerão aqui automaticamente.")}</p>
                <div class="empty-session-actions">
                  <button type="button" onclick={toggleLauncher}>{tr("Open session", "Abrir sessão")}</button>
                  <button type="button" onclick={openAgentSettings}>{tr("Connect an agent", "Conectar agente")}</button>
                </div>
              </div>
            {/if}
          </div>
        {:else if view === "board"}
          <div class="whiteboard" in:fade={{ duration: 150 }}>
            <div class="layout-toolbar">
              <LumeSelect
                ariaLabel={tr("Saved whiteboard layout", "Layout salvo do whiteboard")}
                value={selectedLayoutId ?? ""}
                minWidth={142}
                options={[
                  { value: "", label: tr("New layout", "Novo layout") },
                  ...preferences.whiteboardLayouts.map((layout) => ({ value: layout.id, label: layout.name })),
                ]}
                onValueChange={(value) => {
                  selectedLayoutId = value || null;
                  layoutName = preferences.whiteboardLayouts.find((layout) => layout.id === selectedLayoutId)?.name ?? "";
                }}
              />
              {#if !selectedLayoutId}
                <input bind:value={layoutName} maxlength="48" placeholder={tr("Layout name", "Nome do layout")} />
              {/if}
              <button
                class="layout-action"
                type="button"
                title={selectedLayoutId ? tr("Update layout", "Atualizar layout") : tr("Save new layout", "Salvar novo layout")}
                aria-label={selectedLayoutId ? tr("Update layout", "Atualizar layout") : tr("Save new layout", "Salvar novo layout")}
                onclick={saveCurrentLayout}
              >
                <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M4 3.5h10l2 2v11H4v-13Z" /><path d="M7 3.5v5h6v-5M7 16.5v-5h6v5" /></svg>
              </button>
              {#if selectedLayoutId}
                {@const selectedLayout = preferences.whiteboardLayouts.find((layout) => layout.id === selectedLayoutId)}
                <button
                  class:loading={restoringLayout}
                  class="layout-action"
                  disabled={!selectedLayout || restoringLayout}
                  type="button"
                  title={tr("Restore layout", "Restaurar layout")}
                  aria-label={tr("Restore layout", "Restaurar layout")}
                  onclick={() => selectedLayout && restoreSavedLayout(selectedLayout)}
                >
                  {#if restoringLayout}
                    <span class="layout-spinner" aria-hidden="true"></span>
                  {:else}
                    <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M5.5 6.5H2.8V3.8" /><path d="M3.2 6.2A7 7 0 1 1 3 13" /></svg>
                  {/if}
                </button>
                <button
                  class="layout-action layout-delete"
                  type="button"
                  title={tr("Delete layout", "Excluir layout")}
                  aria-label={tr("Delete layout", "Excluir layout")}
                  onclick={() => selectedLayoutId && deleteSavedLayout(selectedLayoutId)}
                >
                  <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M4 6h12M8 3.5h4L13 6H7l1-2.5ZM6 6l.7 10h6.6L14 6M8.5 9v4.5M11.5 9v4.5" /></svg>
                </button>
              {/if}
            </div>

            <div class:enabled={preferences.workflowEnabled} class="workflow-global-mode">
              <span class:workflow-active={preferences.workflowEnabled} class="workflow-mode-switch" role="group" aria-label={tr("Terminal mode", "Modo dos terminais")}>
                <button
                  class:active={!preferences.workflowEnabled}
                  disabled={workflowModeChanging}
                  type="button"
                  aria-pressed={!preferences.workflowEnabled}
                  onclick={() => void togglePanelWorkflowMode(false)}
                >
                  <svg class="mode-icon" viewBox="0 0 20 20" aria-hidden="true"><path d="M4 10h12" /><circle cx="4" cy="10" r="1.7" /><circle cx="10" cy="10" r="1.7" /><circle cx="16" cy="10" r="1.7" /></svg>
                  {tr("Normal", "Normal")}
                </button>
                <button
                  class:active={preferences.workflowEnabled}
                  disabled={workflowModeChanging}
                  type="button"
                  aria-pressed={preferences.workflowEnabled}
                  onclick={() => void togglePanelWorkflowMode(true)}
                >
                  <svg class="mode-icon" viewBox="0 0 20 20" aria-hidden="true"><path d="M10 4 4 15h12L10 4Z" /><circle cx="10" cy="4" r="1.7" /><circle cx="4" cy="15" r="1.7" /><circle cx="16" cy="15" r="1.7" /></svg>
                  Workflow
                </button>
              </span>
              <button
                class:active={workflowSettingsOpen}
                class="workflow-settings-trigger"
                type="button"
                title={tr("Workflow settings", "Configurações do workflow")}
                aria-label={tr("Workflow settings", "Configurações do workflow")}
                aria-expanded={workflowSettingsOpen}
                onclick={() => void toggleWorkflowSettings()}
              >
                <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M3.5 6h5.5M13 6h3.5M3.5 14h2.5M10 14h6.5" /><circle cx="11" cy="6" r="1.9" /><circle cx="8" cy="14" r="1.9" /></svg>
              </button>
            </div>

            <div class="terminal-picker" use:revealScrollbarWhileScrolling>
              {#each visibleAgentSessions as session (session.id)}
                <div class="terminal-picker-row">
                  <span class="terminal-picker-avatar">
                    <ThreadAvatar seed={session.nativeSessionId || session.sessionName || session.id} label={sessionDisplayName(session)} size={30} />
                  </span>
                  <span class="terminal-picker-copy">
                    <strong>{sessionDisplayName(session)}</strong>
                    <small title={session.workingDirectory}>
                      <BrandIcon name={session.agent} size={9} />
                      <span>{sessionDirectoryName(session)}</span>
                    </small>
                  </span>
                  {#if session.controlOrigin === "external"}
                    <span class="source-label">
                      <BrandIcon name={sourceIcon(session)} size={session.source === "web" ? 11 : 9} />
                      {sourceLabel(session)}
                    </span>
                  {/if}
                  <button
                    disabled={openingTerminal !== null}
                    type="button"
                    title={terminalIsOpen(session) ? tr("Bring this terminal to the front", "Trazer este terminal à frente") : tr("Open separate terminal", "Abrir terminal separado")}
                    onclick={() => openTerminal(session)}
                  >
                    {openingTerminal === session.id ? tr("Opening…", "Abrindo…") : terminalIsOpen(session) ? tr("Show", "Mostrar") : tr("Open", "Abrir")}
                  </button>
                </div>
              {:else}
                <p class="board-empty">{tr("Sessions will appear here when detected.", "As sessões aparecerão aqui quando forem detectadas.")}</p>
              {/each}
            </div>
          </div>
        {:else if view === "history"}
          <div class="inspector-screen" in:fade={{ duration: 150 }}>
            <div class="orb-inspector-content">
              {#if visibleAgentSessions.length}
                <WorkspaceInspector
                  session={inspectedSession}
                  language={preferences.language}
                  variant="orb"
                  showCloseButton={false}
                  sessionOptions={visibleAgentSessions.map((session) => ({
                    value: session.id,
                    label: sessionDisplayName(session),
                    description: `${session.agentLabel} · ${shown(session.statusLabel)}`,
                  }))}
                  onSelectSession={(value) => (inspectorSessionId = value)}
                  bind:section={orbInspectorSection}
                />
              {:else}
                <div class="inspector-no-sessions">
                  <strong>{tr("No agent sessions to inspect", "Nenhuma sessão de agente para inspecionar")}</strong>
                  <p>{tr("Start an agent or connect an integration to see its status and activity here.", "Inicie um agente ou conecte uma integração para acompanhar o estado e a atividade aqui.")}</p>
                  <button type="button" onclick={() => openView("sessions")}>{tr("View sessions", "Ver sessões")}</button>
                </div>
              {/if}
            </div>
          </div>
        {:else}
          <div class="settings" in:fade={{ duration: 150 }}>
            <details use:animatedDisclosure class="settings-section" data-agent-integrations>
              <summary class="settings-section-label">{tr("Agents", "Agentes")}</summary>
              <div class="settings-section-content">
                {#each [
                  { key: "available", label: null, items: integrations.filter((integration) => integration.canLaunch) },
                  { key: "monitoring", label: tr("Monitoring only", "Somente monitoramento"), items: integrations.filter((integration) => !integration.canLaunch) },
                ] as group (group.key)}
                  {#if group.label}<div class="integration-group-label">{group.label}</div>{/if}
                  {#each group.items as integration (integration.kind)}
                    {@const diagnostic = integrationDiagnostics[integration.kind]}
                    <div class="integration-row">
                      <span class="agent-avatar agent-{integration.kind}"><BrandIcon name={integration.kind} size={18} /></span>
                      <div>
                        <strong>{integration.label}</strong>
                        <span>{shown(integration.detail)}</span>
                      </div>
                      <div class="integration-actions">
                        <button
                          class="diagnose-button"
                          disabled={diagnosingIntegration !== null}
                          type="button"
                          onclick={() => runIntegrationDiagnostic(integration)}
                        >{diagnosingIntegration === integration.kind ? "…" : tr("Test", "Testar")}</button>
                        {#if integration.canConfigure}
                          <button
                            class:connected={integration.configured}
                            disabled={!integration.installed || configuringIntegration === integration.kind}
                            type="button"
                            onclick={() => toggleIntegration(integration)}
                          >
                            {configuringIntegration === integration.kind
                              ? "…"
                              : integration.configured
                                ? tr("Connected", "Conectado")
                                : tr("Connect", "Conectar")}
                          </button>
                        {/if}
                      </div>
                    </div>
                    {#if diagnostic}
                      <div class:healthy={diagnostic.healthy} class="diagnostic-card" transition:slide={{ duration: 150, easing: cubicOut }}>
                        {#each diagnostic.checks as check (check.id)}
                          <div class="diagnostic-check status-{check.status}">
                            <i aria-hidden="true"></i>
                            <span><strong>{shown(check.label)}</strong><small>{check.id === "activity" && diagnostic.lastEventAt ? relativeTime(diagnostic.lastEventAt) : shown(check.detail)}</small></span>
                          </div>
                        {/each}
                      </div>
                    {/if}
                  {/each}
                {/each}
              </div>
            </details>
            <details use:animatedDisclosure class="settings-section">
              <summary class="settings-section-label">{tr("External detectors", "Detectores externos")}</summary>
              <div class="settings-section-content">
                {#each externalPlugins as plugin (plugin.id)}
                  <div class="integration-row external-plugin-row">
                    <span class="agent-avatar agent-unknown"><BrandIcon name="unknown" size={17} /></span>
                    <div><strong>{plugin.name}</strong><span>{plugin.executable} · {plugin.id}</span></div>
                    <button type="button" onclick={() => uninstallExternalPlugin(plugin.id)}>{tr("Remove", "Remover")}</button>
                  </div>
                {:else}
                  <p class="profile-empty">{tr("Install a JSON manifest to monitor another CLI process.", "Instale um manifesto JSON para monitorar outro processo CLI.")}</p>
                {/each}
                <div class="plugin-actions">
                  <button disabled={installingPlugin} type="button" onclick={addExternalPlugin}>{installingPlugin ? "…" : tr("Install manifest", "Instalar manifesto")}</button>
                  <button type="button" onclick={openPluginFolder}>{tr("Open folder", "Abrir pasta")}</button>
                </div>
              </div>
            </details>
            <details use:animatedDisclosure class="settings-section">
              <summary class="settings-section-label">Interface</summary>
              <div class="settings-section-content">
                <div class="integration-row">
                  <span class="agent-avatar agent-vscode"><BrandIcon name="vscode" size={19} /></span>
                  <div>
                    <strong>VS Code Companion</strong>
                    <span>{shown(vscodeStatus.detail)}</span>
                  </div>
                  <button
                    class:connected={vscodeStatus.configured}
                    disabled={!vscodeStatus.installed || configuringVscode}
                    type="button"
                    onclick={toggleVscode}
                  >{configuringVscode ? "…" : vscodeStatus.configured ? tr("Connected", "Conectado") : tr("Connect", "Conectar")}</button>
                </div>
                <div class="integration-row browser-row">
                  <span class="agent-avatar agent-browser"><BrandIcon name="browsers" size={21} /></span>
                  <div>
                    <strong>Chrome, Edge & Brave</strong>
                    <span>{tr("Load the folder as an unpacked extension.", "Carregue a pasta como extensão descompactada.")}</span>
                  </div>
                  <button type="button" onclick={openBrowserCompanion}>{tr("Open folder", "Abrir pasta")}</button>
                </div>
                {#if browserCompanionPath}
                  <p class="browser-path" transition:fade>{browserCompanionPath}</p>
                {/if}
              </div>
            </details>
            <details use:animatedDisclosure class="settings-section">
              <summary class="settings-section-label">{tr("Preferences", "Preferências")}</summary>
              <div class="settings-section-content">
            <label class="field-row">
              <span><strong>{tr("Language", "Idioma")}</strong><small>{tr("Lume interface language.", "Idioma da interface do Lume.")}</small></span>
              <LumeSelect
                ariaLabel={tr("Language", "Idioma")}
                value={preferences.language}
                options={[{ value: "en", label: "English" }, { value: "pt-BR", label: "Português" }]}
                onValueChange={(value) => updatePreference("language", value as Preferences["language"])}
              />
            </label>
            <div class="setting-row">
              <div><strong>{tr("Dark mode", "Modo escuro")}</strong><span>{tr("Switch between the light and dark appearance.", "Alterne entre a aparência clara e escura.")}</span></div>
              <label class="switch">
                <input
                  type="checkbox"
                  checked={effectiveDark}
                  onchange={(event) =>
                    updatePreference("darkMode", event.currentTarget.checked)}
                />
                <span></span>
              </label>
            </div>
            <div class="appearance-theme-setting">
              <span><strong>{tr("Surface tint", "Tom das superfícies")}</strong></span>
              <div class="appearance-theme-list">
                {#each appearanceThemes as theme (theme.value)}
                  <button class:active={appearance.theme === theme.value} type="button" title={theme.label} aria-label={theme.label} onclick={() => void selectAppearanceTheme(theme.value)}>
                    <i style:--theme-accent={theme.accent} style:--theme-surface={effectiveDark ? theme.darkSurface : theme.lightSurface}></i>
                  </button>
                {/each}
              </div>
            </div>
            <div class="appearance-theme-setting">
              <span><strong>{tr("Neutral surfaces", "Superfícies neutras")}</strong></span>
              <div class="appearance-theme-list">
                {#if effectiveDark}
                  {#each darkBases as base (base.value)}
                    <button class:active={preferences.darkBase === base.value} type="button" title={tr(base.label, base.labelPt)} aria-label={tr(base.label, base.labelPt)} onclick={() => void updateAppearancePatch({ darkBase: base.value })}>
                      <i style:--theme-accent={appearance.accent ?? "#43b47d"} style:--theme-surface={base.pigments?.surface ?? appearanceThemes.find((theme) => theme.value === appearance.theme)?.darkSurface}></i>
                    </button>
                  {/each}
                {:else}
                  {#each lightBases as base (base.value)}
                    <button class:active={preferences.lightBase === base.value} type="button" title={tr(base.label, base.labelPt)} aria-label={tr(base.label, base.labelPt)} onclick={() => void updateAppearancePatch({ lightBase: base.value })}>
                      <i style:--theme-accent={appearance.accent ?? "#43b47d"} style:--theme-surface={base.pigments?.surface ?? appearanceThemes.find((theme) => theme.value === appearance.theme)?.lightSurface}></i>
                    </button>
                  {/each}
                {/if}
              </div>
            </div>
            <div class="field-row appearance-accent-row">
              <span><strong>{tr("Accent color", "Cor de destaque")}</strong><small>{appearance.accent ?? tr("Following the surface tint", "Seguindo o tom das superfícies")}</small></span>
              <AccentColorPicker value={appearance.accent} opacity={preferences.accentOpacity} readyColors={appearanceThemes.map((theme) => theme.accent)} fallback={appearanceThemes.find((theme) => theme.value === appearance.theme)?.accent ?? "#43b47d"} language={preferences.language} label={tr("Accent color", "Cor de destaque")} onValueChange={(color, opacity) => void updateAppearancePatch({ accentColor: color, accentOpacity: opacity })} onReset={() => void updateAppearancePatch({ accentColor: undefined, accentOpacity: 100 })} />
            </div>
            <div class="field-row appearance-accent-row">
              <span><strong>{tr("Light workspace", "Workspace claro")}</strong><small>{preferences.workspaceLightBackgroundColor ?? tr("Light theme preset", "Preset do tema claro")} · {preferences.workspaceLightBackgroundOpacity}%</small></span>
              <AccentColorPicker value={preferences.workspaceLightBackgroundColor} opacity={preferences.workspaceLightBackgroundOpacity} fallback={appearanceThemes.find((theme) => theme.value === appearance.theme)?.lightSurface ?? "#e9eee8"} readyColors={["#f7f8f4", "#eef2ec", "#e9eee8", "#e9eee3", "#e5edf0", "#ece9f1", "#f0e9e2"]} minimumOpacity={35} language={preferences.language} label={tr("Light workspace background", "Fundo claro do Workspace")} onValueChange={(color, opacity) => void updateAppearancePatch({ workspaceLightBackgroundColor: color, workspaceLightBackgroundOpacity: opacity })} onReset={() => void updateAppearancePatch({ workspaceLightBackgroundColor: undefined, workspaceLightBackgroundOpacity: 96 })} />
            </div>
            <div class="field-row appearance-accent-row">
              <span><strong>{tr("Dark workspace", "Workspace escuro")}</strong><small>{preferences.workspaceDarkBackgroundColor ?? preferences.workspaceBackgroundColor ?? tr("Dark theme preset", "Preset do tema escuro")} · {darkWorkspaceOpacity}%</small></span>
              <AccentColorPicker value={preferences.workspaceDarkBackgroundColor ?? preferences.workspaceBackgroundColor} opacity={darkWorkspaceOpacity} fallback={appearanceThemes.find((theme) => theme.value === appearance.theme)?.darkSurface ?? "#14231c"} readyColors={["#0f1915", "#14231c", "#182116", "#101f28", "#1b1726", "#261a13", "#121916"]} minimumOpacity={35} language={preferences.language} label={tr("Dark workspace background", "Fundo escuro do Workspace")} onValueChange={(color, opacity) => void updateAppearancePatch({ workspaceDarkBackgroundColor: color, workspaceDarkBackgroundOpacity: opacity })} onReset={() => void updateAppearancePatch({ workspaceDarkBackgroundColor: undefined, workspaceDarkBackgroundOpacity: 96, workspaceBackgroundColor: undefined })} />
            </div>
            <div class="field-row">
              <span><strong>{tr("Open Lume as", "Abrir o Lume como")}</strong><small>{tr("Choose the default view for the next launch.", "Escolha a visualização padrão da próxima abertura.")}</small></span>
              <LumeSelect
                ariaLabel={tr("Default startup view", "Visualização inicial padrão")}
                value={preferences.startupMode}
                options={[
                  { value: "ask", label: tr("Always ask", "Perguntar sempre") },
                  { value: "orb", label: "Orb" },
                  { value: "workspace", label: "Workspace" },
                ]}
                minWidth={122}
                onValueChange={(value) => void updatePreference("startupMode", value as Preferences["startupMode"])}
              />
            </div>
            <div class="setting-row">
              <div><strong>{tr("Start with the system", "Iniciar com o sistema")}</strong><span>{tr("Lume stays available in the system tray.", "Lume fica disponível na bandeja.")}</span></div>
              <label class="switch">
                <input
                  type="checkbox"
                  checked={preferences.autostart}
                  onchange={(event) =>
                    updatePreference("autostart", event.currentTarget.checked)}
                />
                <span></span>
              </label>
            </div>
            <div class="setting-row">
              <div><strong>{tr("Desktop pop-up notifications", "Notificações pop-up no desktop")}</strong><span>{tr("Show task and permission alerts outside Lume.", "Mostre alertas de tarefas e permissões fora do Lume.")}</span></div>
              <label class="switch">
                <input
                  type="checkbox"
                  checked={preferences.popupNotificationsEnabled}
                  onchange={(event) =>
                    updatePreference("popupNotificationsEnabled", event.currentTarget.checked)}
                />
                <span></span>
              </label>
            </div>
            <div class="setting-row">
              <div><strong>{tr("Subtle sounds", "Sons sutis")}</strong><span>{tr("Only when a task finishes, fails, requests permission, or usage runs low.", "Apenas ao finalizar, encontrar erro, pedir permissão ou quando o uso estiver acabando.")}</span></div>
              <label class="switch">
                <input
                  type="checkbox"
                  checked={preferences.soundEnabled}
                  onchange={(event) =>
                    updatePreference("soundEnabled", event.currentTarget.checked)}
                />
                <span></span>
              </label>
            </div>
            <div class:disabled={!preferences.soundEnabled} class="setting-row sound-volume-row">
              <div><strong>{tr("Sound volume", "Volume dos sons")}</strong><span>{tr("Adjust notification tones.", "Ajuste o volume dos alertas.")}</span></div>
              <label class="volume-control">
                <input
                  aria-label={tr("Sound volume", "Volume dos sons")}
                  disabled={!preferences.soundEnabled}
                  type="range"
                  min="0"
                  max="100"
                  step="5"
                  value={preferences.soundVolume}
                  oninput={(event) =>
                    updatePreference("soundVolume", Number(event.currentTarget.value))}
                />
                <output>{preferences.soundVolume}%</output>
              </label>
            </div>
            <div class="setting-row">
              <div><strong>{tr("Show over fullscreen", "Sobre tela cheia")}</strong><span>{tr("Keep disabled to avoid videos and games.", "Desativado evita vídeos e jogos.")}</span></div>
              <label class="switch">
                <input
                  type="checkbox"
                  checked={preferences.showOverFullscreen}
                  onchange={(event) =>
                    updatePreference("showOverFullscreen", event.currentTarget.checked)}
                />
                <span></span>
              </label>
            </div>
            <label class="field-row">
              <span><strong>{tr("Monitor", "Monitor")}</strong><small>{tr("The primary display is used by default.", "O principal é usado por padrão.")}</small></span>
              <LumeSelect
                ariaLabel={tr("Monitor", "Monitor")}
                value={preferences.monitorId ?? ""}
                options={[
                  { value: "", label: tr("Primary", "Principal") },
                  ...monitors.map((monitor) => ({ value: monitor.id, label: monitor.label })),
                ]}
                onValueChange={(value) => updatePreference("monitorId", value || undefined)}
              />
            </label>
            <label class="field-row">
              <span><strong>{tr("History", "Histórico")}</strong><small>{tr("Local, sanitized summaries.", "Resumos locais e sanitizados.")}</small></span>
              <LumeSelect
                ariaLabel={tr("History retention", "Retenção do histórico")}
                value={String(preferences.historyRetentionDays)}
                options={[
                  { value: "7", label: tr("7 days", "7 dias") },
                  { value: "30", label: tr("30 days", "30 dias") },
                  { value: "90", label: tr("90 days", "90 dias") },
                ]}
                onValueChange={(value) => updatePreference("historyRetentionDays", Number(value))}
              />
            </label>
            <div class="launch-setting">
              <span><strong>{tr("Open sessions in", "Abrir sessões em")}</strong><small>{tr("Use your usual tool.", "Use sua ferramenta habitual.")}</small></span>
              <div class="segmented" aria-label={tr("Session destination", "Destino das sessões")}>
                {#each [["auto", "Auto"], ["terminal", "Terminal"], ["vscode", "VS Code"]] as option}
                  <button
                    class:active={preferences.launchTarget === option[0]}
                    type="button"
                    onclick={() =>
                      updatePreference("launchTarget", option[0] as Preferences["launchTarget"])}
                  >{option[1]}</button>
                {/each}
              </div>
            </div>
              </div>
            </details>
            <details use:animatedDisclosure class="settings-section">
              <summary class="settings-section-label">{tr("Keyboard shortcuts", "Atalhos de teclado")}</summary>
              <div class="settings-section-content">
                {#each [
                  ["openShortcut", tr("Open Lume", "Abrir o Lume"), tr("Shows and expands the overlay.", "Exibe e expande a sobreposição.")],
                  ["globalShortcut", tr("Command palette", "Paleta de comandos"), tr("Search actions and active agents.", "Busca ações e agentes ativos.")],
                  ["newSessionShortcut", tr("New session", "Nova sessão"), tr("Opens the agent launcher.", "Abre o iniciador de agentes.")],
                  ["whiteboardShortcut", "Whiteboard", tr("Opens the floating terminal hub.", "Abre o hub de terminais flutuantes.")],
                  ["workspaceShortcut", "Workspace", tr("Opens the full multi-agent workspace.", "Abre o workspace completo de múltiplos agentes.")],
                ] as shortcut}
                  <div class="field-row shortcut-row">
                    <span><strong>{shortcut[1]}</strong><small>{shortcut[2]}</small></span>
                    <button
                      class="shortcut-input"
                      type="button"
                      aria-label={shortcut[1]}
                      onclick={() => void openShortcutEditor(shortcut[0] as ShortcutPreferenceKey)}
                    >{preferences[shortcut[0] as ShortcutPreferenceKey]}</button>
                  </div>
                {/each}
              </div>
            </details>
            <details use:animatedDisclosure class="settings-section">
              <summary class="settings-section-label">{tr("Project profiles", "Perfis por projeto")}</summary>
              <div class="settings-section-content">
            {#if detectedProjects.length > 0}
              <label class="field-row">
                <span><strong>{tr("Project", "Projeto")}</strong><small>{tr("Overrides only for this project.", "Ajustes somente para este projeto.")}</small></span>
                <LumeSelect
                  ariaLabel={tr("Project", "Projeto")}
                  value={selectedProfileKey ?? ""}
                  minWidth={145}
                  options={detectedProjects.map((project) => ({ value: project.key, label: project.label }))}
                  onValueChange={(value) => (selectedProfileKey = value)}
                />
              </label>
              <div class="setting-row">
                <div><strong>{tr("Project sounds", "Sons do projeto")}</strong><span>{tr("Allow completion, error, and permission sounds.", "Permite sons de conclusão, erro e permissão.")}</span></div>
                <label class="switch">
                  <input
                    type="checkbox"
                    checked={selectedProjectProfile?.soundEnabled ?? true}
                    onchange={(event) =>
                      updateSelectedProjectProfile({ soundEnabled: event.currentTarget.checked })}
                  />
                  <span></span>
                </label>
              </div>
              <div class="launch-setting project-launch-setting">
                <span><strong>{tr("Session destination", "Destino das sessões")}</strong><small>{tr("Override the global destination.", "Substitui o destino global.")}</small></span>
                <div class="segmented" aria-label={tr("Project session destination", "Destino das sessões do projeto")}>
                  {#each [["", tr("Global", "Global")], ["auto", "Auto"], ["terminal", "Terminal"], ["vscode", "VS Code"]] as option}
                    <button
                      class:active={(selectedProjectProfile?.launchTarget ?? "") === option[0]}
                      type="button"
                      onclick={() =>
                        updateSelectedProjectProfile({ launchTarget: option[0] ? option[0] as Preferences["launchTarget"] : undefined })}
                    >{option[1]}</button>
                  {/each}
                </div>
              </div>
              <label class="field-row">
                <span><strong>{tr("Profile monitor", "Monitor do perfil")}</strong><small>{tr("Where this project should appear.", "Onde este projeto deve aparecer.")}</small></span>
                <LumeSelect
                  ariaLabel={tr("Profile monitor", "Monitor do perfil")}
                  value={selectedProjectProfile?.monitorId ?? ""}
                  options={[
                    { value: "", label: tr("Global", "Global") },
                    ...monitors.map((monitor) => ({ value: monitor.id, label: monitor.label })),
                  ]}
                  onValueChange={(value) => updateSelectedProjectProfile({ monitorId: value || undefined })}
                />
              </label>
              <div class="setting-row">
                <div><strong>{tr("Capsule position", "Posição da cápsula")}</strong><span>{selectedProjectProfile?.overlayX !== undefined ? `${selectedProjectProfile.overlayX}, ${selectedProjectProfile.overlayY}` : tr("Use the global position", "Usar a posição global")}</span></div>
                <button class="profile-action" type="button" onclick={captureProfilePosition}>{tr("Use current", "Usar atual")}</button>
              </div>
              <label class="field-row">
                <span><strong>{tr("Permission preset", "Preset de permissão")}</strong><small>{tr("Applied only when Lume starts a new session.", "Aplicado apenas ao iniciar uma nova sessão pelo Lume.")}</small></span>
                <LumeSelect
                  ariaLabel={tr("Permission preset", "Preset de permissão")}
                  value={selectedProjectProfile?.permissionMode ?? ""}
                  minWidth={145}
                  options={[
                    { value: "", label: tr("Agent default", "Padrão do agente") },
                    { value: "plan", label: "Plan" },
                    { value: "read_only", label: tr("Read only", "Somente leitura") },
                    { value: "workspace_write", label: "Workspace write" },
                    { value: "full_access", label: tr("Full access", "Acesso total"), description: tr("No sandbox", "Sem sandbox") },
                  ]}
                  onValueChange={(value) => updateSelectedProjectProfile({ permissionMode: (value || undefined) as Preferences["projectProfiles"][string]["permissionMode"] })}
                />
              </label>
              <label class="field-row">
                <span><strong>{tr("Approval policy", "Política de aprovação")}</strong><small>{tr("Supported by Codex launch profiles.", "Suportada nos perfis de abertura do Codex.")}</small></span>
                <LumeSelect
                  ariaLabel={tr("Approval policy", "Política de aprovação")}
                  value={selectedProjectProfile?.approvalPolicy ?? ""}
                  options={[
                    { value: "", label: tr("Agent default", "Padrão do agente") },
                    { value: "untrusted", label: "Untrusted" },
                    { value: "on-request", label: "On request" },
                    { value: "never", label: "Never" },
                  ]}
                  onValueChange={(value) => updateSelectedProjectProfile({ approvalPolicy: (value || undefined) as Preferences["projectProfiles"][string]["approvalPolicy"] })}
                />
              </label>
              <label class="field-row">
                <span><strong>Whiteboard</strong><small>{tr("Default saved layout for this project.", "Layout salvo padrão deste projeto.")}</small></span>
                <LumeSelect
                  ariaLabel="Whiteboard"
                  value={selectedProjectProfile?.whiteboardLayoutId ?? ""}
                  options={[
                    { value: "", label: tr("No layout", "Sem layout") },
                    ...preferences.whiteboardLayouts.map((layout) => ({ value: layout.id, label: layout.name })),
                  ]}
                  onValueChange={(value) => updateSelectedProjectProfile({ whiteboardLayoutId: value || undefined })}
                />
              </label>
              <div class="launch-setting preferred-agents-setting">
                <span><strong>{tr("Preferred agents", "Agentes preferidos")}</strong><small>{tr("Shown first in the launcher.", "Aparecem primeiro no iniciador.")}</small></span>
                <div class="agent-preferences">
                  {#each integrations.filter((integration) => integration.canLaunch) as integration (integration.kind)}
                    <button class:active={(selectedProjectProfile?.preferredAgents ?? []).includes(integrationAgentKind(integration.kind))} type="button" onclick={() => togglePreferredAgent(integrationAgentKind(integration.kind))}>
                      <BrandIcon name={integrationAgentKind(integration.kind)} size={14} />{integration.label}
                    </button>
                  {/each}
                </div>
              </div>
              <button class="apply-profile-button" type="button" onclick={applySelectedProjectProfile}>{tr("Apply project profile", "Aplicar perfil do projeto")}</button>
            {:else}
              <p class="profile-empty">{tr("Profiles appear after a project is detected.", "Os perfis aparecem depois que um projeto é detectado.")}</p>
            {/if}
              </div>
            </details>
            <details use:animatedDisclosure class="settings-section" data-remote-nodes-section>
              <summary class="settings-section-label">{tr("Remote computers", "Computadores remotos")}</summary>
              <div class="settings-section-content">
                <RemoteComputers language={preferences.language} dark={effectiveDark} />
              </div>
            </details>
            <details use:animatedDisclosure class="settings-section" data-mobile-access-section>
              <summary class="settings-section-label">{tr("Mobile access", "Acesso mobile")}</summary>
              <div class="settings-section-content">
                <div class="mobile-access-card">
              <div class="mobile-access-header">
                <div>
                  <strong>{tr("Local network gateway", "Gateway da rede local")}</strong>
                  <span>{tr("Only paired devices can read your sessions.", "Apenas dispositivos pareados podem ler suas sessões.")}</span>
                </div>
                <label class="switch">
                  <input
                    type="checkbox"
                    checked={mobileStatus?.networkReachable ?? false}
                    disabled={!isTauri || mobileBusy}
                    onchange={() => void toggleMobileAccess()}
                  />
                  <span></span>
                </label>
              </div>
              {#if !isTauri}
                <p class="mobile-message">{tr("Open the floating Lume desktop app to enable mobile access.", "Abra o aplicativo desktop flutuante do Lume para ativar o acesso mobile.")}</p>
              {/if}
              {#if mobileBusy}
                <p class="mobile-message">{tr("Starting the secure gateway…", "Iniciando o gateway seguro…")}</p>
              {/if}
              {#if mobileStatus?.networkReachable}
                <div class="mobile-address">
                  <span><strong>{tr("Encrypted local", "Local criptografado")}</strong><code>{mobileStatus.address}</code></span>
                  <button type="button" onclick={() => void copyMobileValue(mobileStatus?.address ?? "")}>{tr("Copy", "Copiar")}</button>
                </div>
                <div class="mobile-pair-action">
                  <span>
                    <strong>{tr("Scan once to connect", "Leia uma vez para conectar")}</strong>
                    <small>{tr("The installed app opens automatically. Otherwise, the PWA opens with the APK download option.", "O aplicativo instalado abre automaticamente. Caso contrário, o PWA abre com a opção de baixar o APK.")}</small>
                  </span>
                  <button disabled={mobileBusy} type="button" onclick={() => void createMobilePairing()}>{pairingOffer ? tr("New code", "Novo código") : tr("Show QR", "Mostrar QR")}</button>
                </div>
                {#if pairingOffer && pairingQr}
                  <div class="mobile-pairing" transition:fade={{ duration: 140 }}>
                    <img src={pairingQr} alt={tr("Lume mobile pairing QR code", "QR Code de pareamento mobile do Lume")} />
                    <span>
                      <strong>{tr("One-time code", "Código de uso único")}</strong>
                      <code>{pairingOffer.code}</code>
                      <small>{tr("Expires", "Expira")} {new Date(pairingOffer.expiresAt).toLocaleTimeString()} · {tr("Same Wi-Fi required", "Requer a mesma rede Wi-Fi")}</small>
                    </span>
                  </div>
                {/if}
                <div class="mobile-apk">
                  <span>
                    <strong>{tr("Optional direct APK download", "Download direto opcional do APK")}</strong>
                    <code>{mobileApkUrl}</code>
                    <small>{tr("The same download is offered after scanning the QR code.", "O mesmo download é oferecido depois da leitura do QR Code.")}</small>
                  </span>
                  <button type="button" onclick={() => void copyMobileValue(mobileApkUrl)}>{tr("Copy link", "Copiar link")}</button>
                </div>
              {/if}
                </div>
                {#if pairedDevices.length}
                  <div class="paired-devices">
                    <div class="paired-devices-intro">
                      <strong>{tr("Phone permissions", "Permissões do telefone")}</strong>
                      </div>
                    {#each pairedDevices as device (device.id)}
                      <article class="paired-device-card" data-mobile-device-id={device.id}>
                        <div class="paired-device-header">
                          <span class="paired-device-info">
                            <strong>{device.name}</strong>
                            <small>{dev && device.id === devMobileDeviceId ? tr("Development preview", "Demonstração do ambiente de desenvolvimento") : device.lastSeenAt ? relativeTime(device.lastSeenAt) : tr("Not used yet", "Ainda não utilizado")}</small>
                          </span>
                          {#if dev && device.id === devMobileDeviceId}
                            <span class="preview-badge">{tr("Preview", "Demonstração")}</span>
                          {:else}
                            <button class="revoke-device" disabled={mobileBusy} type="button" onclick={() => void removePairedDevice(device.id)}>{tr("Revoke access", "Revogar acesso")}</button>
                          {/if}
                        </div>
                        <div class="device-permissions">
                          <div class="device-permission">
                            <span class="permission-copy">
                              <strong>{tr("View sessions", "Visualizar sessões")}</strong>
                            </span>
                            <span class="permission-state allowed">{tr("Always allowed", "Sempre permitido")}</span>
                          </div>
                          {#each [
                            {
                              scope: "prompt" as MobileScope,
                              label: tr("Send prompts", "Enviar prompts"),
                            },
                            {
                              scope: "approve" as MobileScope,
                              label: tr("Manage approvals", "Gerenciar aprovações"),
                            },
                            {
                              scope: "terminate" as MobileScope,
                              label: tr("Stop agents", "Encerrar agentes"),
                            },
                          ] as permission}
                            <div class:active={device.scopes.includes(permission.scope)} class="device-permission">
                              <span class="permission-copy">
                                <strong>{permission.label}</strong>
                              </span>
                              <span class="permission-choice">
                                <label class="switch">
                                  <input
                                    aria-label={`${permission.label}: ${device.scopes.includes(permission.scope) ? tr("Allowed", "Permitido") : tr("Not allowed", "Não permitido")}`}
                                    type="checkbox"
                                    checked={device.scopes.includes(permission.scope)}
                                    disabled={mobileBusy}
                                    onchange={() => void togglePairedDeviceScope(device, permission.scope)}
                                  />
                                  <span></span>
                                </label>
                              </span>
                            </div>
                          {/each}
                        </div>
                      </article>
                    {/each}
                  </div>
                {/if}
              </div>
            </details>
            <section class="settings-section settings-section-static">
              <div class="settings-section-label">{tr("About", "Sobre")}</div>
              <div class="settings-section-content">
                <div class="update-card" data-update-card aria-live="polite">
              <div class="update-main">
                <LumeLogo size={30} />
                <div class="update-copy">
                  <strong>Lume</strong>
                  <span>{tr("Version", "Versão")} {appVersion}</span>
                </div>
                {#if updateState === "available"}
                  <button class="update-available" type="button" onclick={handleInstallUpdate}>
                    {tr("Update to", "Atualizar para")} {availableVersion}
                  </button>
                {:else}
                  <button
                    type="button"
                    disabled={updateState === "checking" || updateState === "downloading" || updateState === "ready"}
                    onclick={handleUpdateButton}
                  >
                    {updateState === "checking"
                      ? tr("Checking…", "Verificando…")
                      : updateState === "downloading"
                        ? updateProgress === null
                          ? tr("Downloading…", "Baixando…")
                          : `${updateProgress}%`
                        : updateState === "ready"
                          ? tr("Restarting…", "Reiniciando…")
                          : tr("Check", "Verificar")}
                  </button>
                {/if}
              </div>
              {#if updateState !== "error"}<p>{updateDetail}</p>{/if}
              {#if updateState === "downloading" || updateState === "ready"}
                <div class:indeterminate={updateProgress === null} class="update-progress" aria-hidden="true">
                  <span style:width={`${updateProgress ?? 24}%`}></span>
                </div>
              {/if}
                </div>
              </div>
            </section>
            <details use:animatedDisclosure class="settings-section">
              <summary class="settings-section-label">{tr("Reset", "Redefinir")}</summary>
              <div class="settings-section-content">
                <div class:confirming={resetConfirming} class="reset-settings-control">
                  {#if resetConfirming}
                    <span>{tr("Reset all Lume settings to their defaults?", "Redefinir todas as configurações do Lume para o padrão?")}</span>
                    <button type="button" onclick={() => (resetConfirming = false)}>{tr("Cancel", "Cancelar")}</button>
                    <button class="danger" disabled={resettingSettings} type="button" onclick={() => void resetSettings()}>
                      {resettingSettings ? tr("Resetting…", "Redefinindo…") : tr("Reset", "Redefinir")}
                    </button>
                  {:else}
                    <button type="button" onclick={() => void resetSettings()}>
                      {tr("Reset", "Redefinir")}
                    </button>
                  {/if}
                </div>
              </div>
            </details>
            <span class:visible={savingSettings} class="save-state">{tr("Saving…", "Salvando…")}</span>
          </div>
        {/if}
      </div>

      <footer>
        <button
          class:active={view === "sessions"}
          type="button"
          onclick={() => activateNavigation("sessions")}
          aria-label={tr("Sessions", "Sessões")}
        >
          <OrbNavigationIcon name="sessions" activation={navigationActivation.view === "sessions" ? navigationActivation.count : 0} />
          <span>{tr("Sessions", "Sessões")}</span>
        </button>
        <button
          class:active={view === "board"}
          type="button"
          onclick={() => activateNavigation("board")}
          aria-label={tr("Terminals", "Terminais")}
        >
          <OrbNavigationIcon name="terminals" activation={navigationActivation.view === "board" ? navigationActivation.count : 0} />
          <span>{tr("Terminals", "Terminais")}</span>
        </button>
        <button
          class:active={view === "history"}
          type="button"
          onclick={() => activateNavigation("history")}
          aria-label={tr("Inspector", "Inspector")}
        >
          <OrbNavigationIcon name="inspector" activation={navigationActivation.view === "history" ? navigationActivation.count : 0} />
          <span>{tr("Inspector", "Inspector")}</span>
        </button>
        <button
          class:active={view === "settings"}
          class:has-update={updateState === "available"}
          class:has-mobile-device={newMobileDevice !== null}
          type="button"
          onclick={() => activateNavigation("settings")}
          aria-label={tr("Settings", "Configurações")}
        >
          <OrbNavigationIcon name="settings" activation={navigationActivation.view === "settings" ? navigationActivation.count : 0} />
          <span>{tr("Settings", "Ajustes")}</span>
        </button>
      </footer>
    </section>
  {/if}
  {#if connectionAgent}
    <AgentConnectionDialog agent={connectionAgent} message={connectionMessage} language={preferences.language} onClose={() => { connectionAgent = null; }} />
  {/if}
  {#if automationRequired}
    <MacosAutomationDialog language={preferences.language} onClose={() => { automationRequired = false; }} />
  {/if}
</main>

<style>
  .overlay-shell {
    position: relative;
    width: 100%;
    height: 100%;
    display: flex;
    align-items: flex-start;
    justify-content: flex-start;
  }

  .overlay-shell.morphing {
    clip-path: inset(
      0 max(0px, calc(100% - var(--morph-width)))
      max(0px, calc(100% - var(--morph-height))) 0
      round var(--panel-radius)
    );
  }

  .overlay-shell:not(.expanded) {
    clip-path: inset(0 calc(100% - 78px) calc(100% - 44px) 0);
  }

  button,
  input,
  textarea {
    -webkit-tap-highlight-color: transparent;
  }

  .lume-orb {
    --orb-fill: rgba(249, 251, 250, 0.985);
    --orb-border: rgba(103, 122, 114, 0.2);
    --orb-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.34), inset 0 -5px 12px rgba(43, 64, 55, 0.035);
    position: relative;
    width: 78px;
    height: 44px;
    flex: 0 0 auto;
    padding: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    border: 0;
    border-radius: 0;
    color: #4e7567;
    background: transparent;
    /* Keep one compositing layer across drag and settle on transparent WebKit windows. */
    transform: translateZ(0);
    isolation: isolate;
    cursor: pointer;
    touch-action: none;
  }

  .orb-surface {
    position: absolute;
    inset: 0;
    pointer-events: none;
    border: 1px solid var(--orb-border);
    border-radius: var(--orb-radius, 22px);
    background: var(--orb-fill);
    box-shadow: var(--orb-shadow);
    transform-origin: var(--orb-origin-x, 50%) var(--orb-origin-y, 50%);
    transform: scale(var(--orb-scale-x, 1), var(--orb-scale-y, 1));
    transition: border-radius 180ms cubic-bezier(0.16, 1, 0.3, 1), transform 180ms cubic-bezier(0.16, 1, 0.3, 1), border-color 160ms ease, background-color 160ms ease;
  }

  .orb-content {
    position: relative;
    display: flex;
    align-items: center;
    gap: 8px;
    pointer-events: none;
    transform: translate(var(--orb-content-x, 0px), var(--orb-content-y, 0px));
    transition: transform 180ms cubic-bezier(0.16, 1, 0.3, 1);
  }

  .lume-orb:hover { --orb-border: rgba(79, 116, 99, 0.3); }
  .lume-orb:active { --orb-fill: rgba(245, 249, 247, 0.99); }
  .lume-orb.docked { cursor: grab; }
  .lume-orb.dragging { cursor: grabbing; }
  .lume-orb.dragging .orb-surface,
  .lume-orb.dragging .orb-content { transition-duration: 60ms; }
  .lume-orb:focus-visible { outline: none; }
  .lume-orb:focus-visible .orb-surface { outline: 2px solid currentColor; outline-offset: -3px; }

  .status-permission_required { color: #ae6b24; }
  .status-failed { color: #a84d4d; }
  .status-completed { color: #4f966b; }
  .status-idle { color: #829089; }

  .agent-count {
    min-width: 19px;
    height: 19px;
    padding: 0 5px;
    display: grid;
    place-items: center;
    border-radius: 999px;
    color: #f8faf9;
    background: #30473e;
    font-size: 10px;
    font-weight: 760;
  }

  .panel {
    position: relative;
    width: 100%;
    height: auto;
    max-height: 544px;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border: 1px solid rgba(105, 124, 116, 0.18);
    border-radius: var(--panel-radius);
    color: #26322e;
    background: #f9fbfa;
  }

  .panel.onboarding { min-height: 320px; }
  .panel.onboarding > :not(.startup-chooser-layer) { display: none; }
  .startup-chooser-layer { position: absolute; z-index: 45; inset: 0; }

  .panel.palette-open {
    min-height: 390px;
  }

  .panel.launcher-open,
  .panel.workflow-settings-open {
    overflow: visible;
  }

  .panel.morphing:not(.measuring) {
    width: var(--morph-width);
    height: var(--morph-height);
    flex: 0 0 auto;
    min-height: 0;
    max-height: none;
  }

  .panel.measuring {
    position: absolute;
    width: 392px;
    height: auto;
    max-height: 544px;
    visibility: hidden;
  }

  .panel-content,
  .panel footer,
  .panel .brand-lockup > div,
  .panel .header-actions,
  .panel .launcher-popover,
  .panel .mobile-device-banner {
    transition: opacity 150ms ease;
  }
  .panel:not(.content-visible) .panel-content,
  .panel:not(.content-visible) footer,
  .panel:not(.content-visible) .brand-lockup > div,
  .panel:not(.content-visible) .header-actions,
  .panel:not(.content-visible) .launcher-popover,
  .panel:not(.content-visible) .mobile-device-banner {
    opacity: 0;
    pointer-events: none;
  }

  .panel-header {
    --system-banner-layer: 260;
    position: relative;
    flex: 0 0 auto;
    min-height: 61px;
    padding: 12px 13px 10px 16px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    border-bottom: 1px solid rgba(101, 120, 112, 0.11);
    cursor: grab;
    touch-action: none;
  }

  .panel-header.dragging { cursor: grabbing; }

  .mobile-device-banner {
    min-height: 52px;
    padding: 8px 12px;
    display: grid;
    grid-template-columns: 30px minmax(0, 1fr) auto;
    align-items: center;
    gap: 8px;
    border-bottom: 1px solid rgba(84, 143, 112, 0.16);
    background: rgba(91, 164, 126, 0.075);
  }
  .mobile-device-banner-icon {
    width: 30px;
    height: 30px;
    display: grid;
    place-items: center;
    border-radius: 10px;
    color: #4c8c6c;
    background: rgba(88, 167, 125, 0.12);
  }
  .mobile-device-banner-icon svg { width: 16px; height: 16px; }
  .mobile-device-banner > span:nth-child(2) { min-width: 0; display: grid; gap: 2px; }
  .mobile-device-banner strong { overflow: hidden; color: #315e49; font-size: 9px; text-overflow: ellipsis; white-space: nowrap; }
  .mobile-device-banner small { overflow: hidden; color: #6d887b; font-size: 8px; text-overflow: ellipsis; white-space: nowrap; }
  .mobile-device-banner > button {
    min-height: 27px;
    padding: 0 8px;
    border: 1px solid rgba(75, 137, 105, 0.2);
    border-radius: 8px;
    color: #47745f;
    background: rgba(255, 255, 255, 0.52);
    font-size: 8px;
    font-weight: 750;
    cursor: pointer;
  }
  .mobile-device-banner > button:hover { background: rgba(255, 255, 255, 0.82); }

  .brand-lockup { display: flex; align-items: center; gap: 10px; color: #4e7567; }
  .brand-lockup div { display: grid; gap: 1px; }
  .brand-lockup strong { color: #202d28; font-size: 13px; letter-spacing: -0.01em; }
  .brand-lockup div span { color: #75817c; font-size: 10px; }

  .workspace-button { position: relative; }
  .workspace-button.opening :global(svg) { opacity: .28; transition: opacity 120ms ease; }
  .workspace-button.opening::after { position: absolute; inset: 7px; border: 2px solid color-mix(in srgb, currentColor 22%, transparent); border-top-color: currentColor; border-radius: 50%; content: ""; animation: workspace-opening 760ms linear infinite; }
  @keyframes workspace-opening { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { .workspace-button.opening::after { animation-duration: 2s; } }
  .add-button,
  .workspace-button,
  .palette-button,
  .collapse-button {
    border: 0;
    color: #697872;
    background: transparent;
    cursor: pointer;
  }

  .collapse-button {
    width: 32px;
    height: 32px;
    display: grid;
    place-items: center;
    border-radius: 10px;
  }

  .header-actions { display: flex; align-items: center; gap: 2px; }
  .add-button, .workspace-button, .palette-button { width: 32px; height: 32px; display: grid; place-items: center; border-radius: 10px; }
  .add-button:hover,
  .add-button.active { color: #486d5e; background: rgba(80, 103, 94, 0.07); }
  .add-button svg { transition: transform 180ms cubic-bezier(0.16, 1, 0.3, 1); }
  .add-button.active svg { transform: rotate(45deg); }
  .add-button:active { transform: scale(0.96); }

  .add-button:hover,
  .workspace-button:hover,
  .palette-button:hover,
  .collapse-button:hover { background: rgba(80, 103, 94, 0.07); }

  svg {
    width: 17px;
    height: 17px;
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-linejoin: round;
    stroke-width: 1.65;
  }

  .panel-content { position: relative; max-height: 431px; min-height: 0; flex: 0 1 auto; overflow: hidden; }
  .panel-content.inspector-content { height: 431px; display: flex; flex-direction: column; }
  .launcher-popover { position: absolute; z-index: 4; top: 53px; right: 13px; width: 320px; max-height: calc(100vh - 65px); overflow: hidden; isolation: isolate; border: 1px solid rgba(99, 119, 110, 0.14); border-radius: 14px; background: #fafcfb; background-clip: padding-box; }
  .launcher-popover-scroll { box-sizing: border-box; width: 100%; max-height: calc(100vh - 65px); padding: 10px 11px; overflow-x: hidden; overflow-y: auto; overscroll-behavior: contain; scrollbar-gutter: stable; }
  .launcher-title { display: block; padding: 1px 3px 7px; color: #8c9691; font-size: 9px; font-weight: 750; letter-spacing: 0.06em; text-transform: uppercase; }
  .launcher-row { min-height: 45px; display: flex; align-items: center; gap: 7px; border-top: 1px solid rgba(105, 123, 115, 0.08); }
  .launcher-row .agent-avatar { width: 25px; height: 25px; border-radius: 8px; font-size: 9px; }
  .launcher-row strong { min-width: 0; flex: 1; color: #35423d; font-size: 10px; }
  .launcher-row button { box-sizing: border-box; min-width: 64px; height: 27px; padding: 0 8px; display: inline-flex; align-items: center; justify-content: center; border: 0; border-radius: 7px; color: #60736a; background: rgba(78, 105, 93, 0.055); font-size: 9px; font-weight: 700; cursor: pointer; transition: color 120ms ease, background-color 120ms ease, transform 120ms cubic-bezier(0.16, 1, 0.3, 1); }
  .launcher-row button:hover { background: rgba(78, 105, 93, 0.1); }
  .launcher-row button.active { color: #327a58; background: rgba(57, 139, 96, 0.1); }
  .launcher-row button.loading { color: #327a58; background: rgba(57, 139, 96, 0.1); opacity: 0.92; }
  .launcher-row button:disabled:not(.loading) { opacity: 0.45; cursor: default; }
  .launcher-row button:not(:disabled):active { transform: scale(0.96); }
  .launcher-row button:focus-visible, .resume-session:focus-visible { outline: 2px solid rgba(73, 133, 103, 0.48); outline-offset: 2px; }
  .launcher-button-content { display: inline-flex; align-items: center; justify-content: center; gap: 5px; white-space: nowrap; }
  .launcher-spinner { width: 9px; height: 9px; flex: 0 0 auto; border: 1.4px solid currentColor; border-right-color: transparent; border-radius: 50%; animation: launcher-spin 720ms linear infinite; }
  @keyframes launcher-spin { to { transform: rotate(360deg); } }
  .resume-session-list { padding: 2px 0 7px 32px; display: grid; gap: 3px; overflow: visible; }
  .resume-session-list > p { margin: 8px 2px; color: #89938f; font-size: 9px; }
  .launcher-loading-note { min-height: 30px; display: flex; align-items: center; gap: 7px; color: #718078; font-size: 8px; }
  .resume-session { width: 100%; min-height: 39px; padding: 5px 7px 5px 8px; display: flex; align-items: center; gap: 8px; overflow: hidden; border: 0; border-radius: 9px; color: #51665c; background: rgba(76, 104, 91, 0.045); text-align: left; cursor: pointer; }
  .resume-session:hover { background: rgba(61, 132, 96, 0.09); }
  .resume-session:disabled { opacity: 0.45; cursor: default; }
  .resume-session.loading { color: #327a58; background: rgba(57, 139, 96, 0.1); opacity: 0.9; }
  .resume-session > span { min-width: 0; flex: 1; display: grid; gap: 2px; }
  .resume-session strong, .resume-session small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .resume-session strong { color: #3e5048; font-size: 9px; }
  .resume-session small { color: #89958f; font-size: 7px; }
  .resume-session svg { width: 12px; height: 12px; flex: 0 0 auto; }
  .launcher-popover-scroll > p { margin: 8px 3px; color: #89938f; font-size: 10px; }
  .command-palette-layer { position: absolute; z-index: 12; inset: 0 0 16px; display: grid; place-items: start center; padding-top: 66px; }
  .command-palette-backdrop { position: absolute; inset: 0; width: 100%; border: 0; background: rgba(21, 31, 27, 0.2); backdrop-filter: blur(3px); cursor: default; }
  .command-palette { position: relative; width: calc(100% - 30px); overflow: hidden; border: 1px solid rgba(89, 111, 101, 0.16); border-radius: 15px; background: rgba(250, 252, 251, 0.98); box-shadow: 0 18px 45px rgba(24, 38, 32, 0.24); }
  .command-search { height: 43px; padding: 0 10px; display: flex; align-items: center; gap: 8px; border-bottom: 1px solid rgba(91, 112, 102, 0.1); }
  .command-search svg { width: 15px; height: 15px; flex: 0 0 auto; fill: none; stroke: #6f8179; stroke-width: 1.5; }
  .command-search input { min-width: 0; flex: 1; border: 0; outline: 0; color: #304039; background: transparent; font: inherit; font-size: 10px; }
  .command-palette kbd { padding: 3px 5px; border: 1px solid rgba(92, 112, 103, 0.13); border-radius: 5px; color: #7b8983; background: rgba(80, 105, 94, 0.045); font-family: inherit; font-size: 7px; }
  .command-results { max-height: 265px; padding: 5px; overflow-y: auto; }
  .command-results > button { width: 100%; min-height: 42px; padding: 6px 8px; display: flex; align-items: center; gap: 8px; border: 0; border-radius: 9px; color: inherit; background: transparent; text-align: left; cursor: pointer; }
  .command-results > button.active { background: rgba(78, 109, 95, 0.075); }
  .command-results > button span { min-width: 0; flex: 1; display: grid; gap: 2px; }
  .command-results strong { overflow: hidden; color: #34443d; font-size: 9px; text-overflow: ellipsis; white-space: nowrap; }
  .command-results small, .command-results > p { margin: 0; color: #89958f; font-size: 8px; }
  .shortcut-editor-layer { position: absolute; z-index: 18; inset: 0; display: grid; place-items: center; padding: 18px; }
  .shortcut-editor-backdrop { position: absolute; inset: 0; width: 100%; border: 0; background: rgba(21, 31, 27, 0.24); backdrop-filter: blur(3px); cursor: default; }
  .shortcut-editor { position: relative; width: min(270px, 100%); padding: 16px; display: grid; gap: 8px; border: 1px solid rgba(89, 111, 101, 0.18); border-radius: 14px; background: rgba(250, 252, 251, 0.99); box-shadow: 0 18px 45px rgba(24, 38, 32, 0.26); }
  .shortcut-editor > strong { color: #34443d; font-size: 11px; }
  .shortcut-editor > small { color: #829089; font-size: 8px; line-height: 1.45; }
  .shortcut-capture { height: 44px; margin-top: 3px; border: 1px solid rgba(70, 113, 95, 0.28); border-radius: 10px; outline: 0; color: #3e6153; background: rgba(74, 122, 102, 0.07); cursor: text; }
  .shortcut-capture:focus { border-color: rgba(69, 130, 103, 0.58); box-shadow: 0 0 0 3px rgba(74, 122, 102, 0.1); }
  .shortcut-capture kbd { font: 750 10px var(--lume-font-ui, Inter, sans-serif); }
  .shortcut-editor-actions { margin-top: 4px; display: flex; justify-content: flex-end; gap: 6px; }
  .shortcut-editor-actions button { min-width: 60px; height: 28px; padding: 0 9px; border: 1px solid rgba(82, 105, 95, 0.15); border-radius: 8px; color: #66776e; background: transparent; font-size: 8px; font-weight: 750; cursor: pointer; }
  .shortcut-editor-actions button.primary { color: #fff; border-color: #317e59; background: #317e59; }
  .shortcut-editor-actions button:disabled { opacity: 0.45; cursor: default; }
  .session-list,
  .settings { max-height: 431px; min-height: 0; overflow-x: hidden; overflow-y: auto; overscroll-behavior: contain; scrollbar-gutter: stable; scrollbar-width: thin; scrollbar-color: #cad2ce transparent; }

  .session-list::-webkit-scrollbar,
  .settings::-webkit-scrollbar,
  .terminal-picker::-webkit-scrollbar { width: 5px; background: transparent; }
  .session-list::-webkit-scrollbar-button,
  .settings::-webkit-scrollbar-button,
  .terminal-picker::-webkit-scrollbar-button { width: 0; height: 0; display: none; }
  .session-list::-webkit-scrollbar-track,
  .settings::-webkit-scrollbar-track,
  .terminal-picker::-webkit-scrollbar-track { background: transparent; }
  .session-list::-webkit-scrollbar-thumb,
  .settings::-webkit-scrollbar-thumb,
  .terminal-picker::-webkit-scrollbar-thumb { border-radius: 999px; background: #cad2ce; }

  .session-list { padding: 5px 14px 8px; }
  .orb-group-heading { width: 100%; margin: 8px 0 2px; padding: 3px 4px; display: flex; align-items: center; gap: 6px; border: 0; border-radius: 8px; color: #73807a; background: transparent; font: inherit; font-size: 10px; text-align: left; cursor: pointer; }
  .orb-group-heading:disabled { cursor: default; }
  .orb-group-heading:not(:disabled):hover { background: rgba(76, 104, 92, 0.07); }
  .orb-group-heading strong { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 720; letter-spacing: .02em; }
  .orb-group-heading small { margin-left: auto; font-size: 9px; font-variant-numeric: tabular-nums; }
  .orb-group-chevron { display: grid; transition: transform 160ms ease; }
  .orb-group-heading.collapsed .orb-group-chevron { transform: rotate(-90deg); }
  .orb-group-chevron svg { fill: none; stroke: currentColor; stroke-width: 1.8; stroke-linecap: round; stroke-linejoin: round; }
  .orb-group-body { overflow: hidden; }
  .session-list:not(.is-scrolling),
  .terminal-picker:not(.is-scrolling) { scrollbar-color: transparent transparent; }
  .session-list::-webkit-scrollbar-thumb,
  .terminal-picker::-webkit-scrollbar-thumb { background: transparent; transition: background-color 140ms ease; }
  .session-list.is-scrolling::-webkit-scrollbar-thumb,
  .terminal-picker.is-scrolling::-webkit-scrollbar-thumb { background: #cad2ce; }
  .final-response > summary { padding: 10px; color: inherit; font-size: 11px; font-weight: 650; cursor: pointer; }
  .final-response-body { position: relative; padding: 8px 10px 12px; color: inherit; }
  .final-response-body .final-response-copy { position: relative; top: auto; right: auto; margin: 0 0 5px auto; }
  .empty-session-actions button { min-height: 30px; padding: 5px 10px; border: 1px solid rgba(70, 109, 87, .3); border-radius: 7px; color: inherit; background: rgba(72, 131, 97, .08); font: 650 11px var(--lume-font-ui, Inter, sans-serif); cursor: pointer; }
  .empty-session-actions button:hover { background: rgba(72, 131, 97, .16); }
  .empty-session-actions { display: flex; flex-wrap: wrap; justify-content: center; gap: 8px; margin-top: 14px; }
  .session-details .permission-block, .session-details .question-block { margin: 0 0 14px; padding: 11px; border: 1px solid rgba(166, 122, 49, .28); border-radius: 10px; font-size: 12px; line-height: 1.5; }
  .session-details .permission-block code { max-height: 120px; overflow: auto; text-overflow: clip; white-space: pre-wrap; overflow-wrap: anywhere; }
  .session-details .permission-actions button, .session-details .question-actions button { min-height: 32px; font-size: 11px; }
  .overlay-shell.dark .status-line.status-permission_required, .overlay-shell.dark .status-line.status-waiting_for_input { color: #e0b777; }
  .overlay-shell.dark .status-line.status-running { color: #91bee0; }
  .overlay-shell.dark .status-line.status-completed { color: #9dceb0; }
  .overlay-shell.dark footer button.active { color: #a4dbbc; }


  .session-row {
    border-bottom: 1px solid rgba(105, 123, 115, 0.1);
    transition: background 160ms ease;
  }

  .session-row:last-child { border-bottom: 0; }
  .session-row:hover:not(.attention),
  .session-row.selected:not(.attention) { margin: 0 -6px; padding: 0 6px; border-radius: 12px; background: rgba(76, 104, 92, 0.045); }
  .session-row.attention { margin: 0 -6px; padding: 0 6px; border-radius: 12px; background: linear-gradient(90deg, rgba(183, 111, 36, 0.11), rgba(183, 111, 36, 0.025) 76%, transparent); }

  .session-summary {
    width: 100%;
    min-height: 76px;
    padding: 10px 1px;
    display: flex;
    align-items: center;
    gap: 11px;
    border: 0;
    color: inherit;
    background: transparent;
    text-align: left;
    cursor: pointer;
  }

  .agent-avatar {
    width: 32px;
    height: 32px;
    flex: 0 0 auto;
    display: grid;
    place-items: center;
    border-radius: 10px;
    font-size: 11px;
    font-weight: 780;
    transition: transform 160ms ease;
  }

  .session-summary:hover .agent-avatar { transform: scale(1.04); }
  .thread-avatar-shell { width: 32px; height: 32px; flex: 0 0 auto; transition: transform 160ms ease; }
  .session-summary:hover .thread-avatar-shell { transform: scale(1.04); }
  .agent-codex,
  .agent-chatgpt { color: #202523; background: #edf0ee; }
  .agent-claude,
  .agent-claude_code { color: #d97757; background: #f7ece6; }
  .agent-antigravity { color: #476c5b; background: #e8f1ec; }
  .agent-deepseek { color: #5786fe; background: #edf2ff; }
  .agent-gemini { color: #6e73ca; background: #eef0fb; }
  .agent-vscode { color: #287aa9; background: #edf6fb; }
  .agent-browser { color: #52615a; background: #f1f3f2; }
  .agent-unknown { color: #48534f; background: #e2e7e4; }

  .session-copy { min-width: 0; flex: 1; display: grid; gap: 2px; }
  .session-title-row { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 6px; }
  .session-title-row strong { color: #27342f; font-size: 11px; }
  .source-label { display: inline-flex; align-items: center; gap: 3px; padding: 2px 5px; border-radius: 999px; color: #718079; background: rgba(80, 104, 94, 0.075); font-size: 8px; font-weight: 720; letter-spacing: 0.045em; line-height: 1.25; text-transform: uppercase; }
  .access-badge { padding: 2px 5px; display: inline-flex; align-items: center; gap: 3px; border: 0; border-radius: 999px; font-size: 7px; font-weight: 780; letter-spacing: 0.025em; line-height: 1.25; white-space: nowrap; }
  .access-badge svg { width: 9px; height: 9px; flex: 0 0 auto; fill: none; stroke: currentColor; stroke-width: 1.35; }
  .access-badge.auto-review { color: #315f86; background: #cbdff0; }
  .access-badge.auto-review svg { fill: currentColor; stroke: none; }
  .access-badge.full-access { color: #764c2e; background: #e8ceb1; }
  .project-name { min-width: 0; display: flex; align-items: center; gap: 4px; overflow: hidden; color: #56645e; font-size: 11px; white-space: nowrap; }
  .project-name span { min-width: 0; overflow: hidden; text-overflow: ellipsis; }

  .status-line { display: flex; align-items: center; gap: 5px; color: #7a8580; font-size: 10px; }
  .status-line > i { width: 5px; height: 5px; border-radius: 50%; background: #82908a; }
  .status-line.status-running { color: #4e7faf; }
  .running-dots { height: 8px; display: inline-flex; align-items: center; gap: 2px; }
  .running-dots i { width: 3px; height: 3px; border-radius: 50%; background: #5388bd; animation: status-dot-bounce 900ms ease-in-out infinite; }
  .running-dots i { will-change: transform, opacity; }
  .running-dots i:nth-child(2) { animation-delay: 120ms; }
  .running-dots i:nth-child(3) { animation-delay: 240ms; }
  .status-line.status-permission_required { color: #a46522; }
  .status-line.status-permission_required > i { background: #cb8235; box-shadow: 0 0 0 3px rgba(203, 130, 53, 0.1); }
  .status-line.status-completed { color: #4f966b; }
  .status-line.status-completed > i { background: #59aa78; box-shadow: 0 0 0 3px rgba(89, 170, 120, 0.1); }
  .status-line.status-failed > i { background: #b95454; }
  .status-line.status-waiting_for_input { color: #a87925; }
  .status-line.status-waiting_for_input > i { background: #c99a3f; }

  @keyframes status-dot-bounce {
    0%, 60%, 100% { opacity: 0.48; transform: translateY(1px); }
    30% { opacity: 1; transform: translateY(-2px); }
  }

  .chevron { width: 13px; height: 13px; color: #98a19d; transition: transform 180ms ease; }
  .selected .chevron { transform: rotate(90deg); }

  .session-details { padding: 0 2px 13px 43px; }
  .session-action-bar { position: relative; margin: 0 0 10px; display: flex; flex-wrap: wrap; align-items: center; gap: 5px; }
  .cli-context-menu { position: fixed; z-index: 1200; width: min(196px, calc(100vw - 16px)); box-sizing: border-box; padding: 5px; border-radius: 10px; color: var(--lume-ink-light); background: var(--lume-raised-light); box-shadow: 0 8px 24px rgb(0 0 0 / 18%); }
  .cli-context-menu button { display: flex; align-items: center; gap: 9px; width: 100%; min-height: 34px; padding: 7px 9px; border: 0; border-radius: 6px; color: inherit; background: transparent; font: 550 12px var(--lume-font-ui, Inter, sans-serif); text-align: left; cursor: pointer; }
  .cli-context-menu button:hover, .cli-context-menu button:focus-visible { background: color-mix(in srgb, var(--lume-accent) 12%, transparent); }
  .cli-context-menu button:focus-visible { outline: 2px solid var(--lume-accent-strong); outline-offset: -2px; }
  .overlay-shell.dark .cli-context-menu { color: var(--lume-ink-dark); background: var(--lume-raised-dark); }
  .session-action-button { position: relative; width: 27px; height: 27px; padding: 0; display: grid; place-items: center; border: 1px solid rgba(83, 108, 97, 0.11); border-radius: 8px; color: #65786f; background: rgba(77, 105, 92, 0.035); cursor: pointer; transition: color 130ms ease, background 130ms ease, transform 130ms ease; }
  .session-action-button:hover:not(:disabled),
  .session-action-button.active { color: #3f745d; background: rgba(68, 125, 99, 0.09); transform: translateY(-1px); }
  .session-action-button.warning { color: #a2762f; }
  .session-action-button.danger { color: #9a5c59; }
  .session-action-button:disabled { opacity: 0.42; cursor: default; }
  .session-action-button svg { width: 13px; height: 13px; fill: none; stroke: currentColor; stroke-linecap: round; stroke-linejoin: round; stroke-width: 1.5; }
  .session-action-button::after { position: absolute; z-index: 25; bottom: calc(100% + 5px); left: 50%; max-width: 120px; padding: 4px 6px; content: attr(data-label); opacity: 0; pointer-events: none; border: 1px solid rgba(74, 96, 86, 0.12); border-radius: 6px; color: #52635b; background: rgba(249, 251, 250, 0.98); box-shadow: 0 5px 15px rgba(43, 58, 51, 0.12); font-size: 7px; font-weight: 700; line-height: 1.2; text-align: center; white-space: nowrap; transform: translate(-50%, 3px); transition: opacity 110ms ease, transform 110ms ease; }
  .session-action-button:hover::after,
  .session-action-button:focus-visible::after { opacity: 1; transform: translate(-50%, 0); }
  .session-name-editor { margin: 0 0 9px; display: flex; flex-wrap: wrap; gap: 5px; }
  .session-name-editor input { min-width: 0; flex: 1 1 130px; padding: 6px 8px; border: 1px solid rgba(75, 101, 89, 0.14); border-radius: 7px; color: #314139; background: rgba(255, 255, 255, 0.5); font: inherit; font-size: 10px; outline: none; }
  .session-name-editor input:focus { border-color: rgba(64, 132, 99, 0.42); box-shadow: 0 0 0 2px rgba(64, 132, 99, 0.08); }
  .session-name-editor button { padding: 5px 7px; border: 1px solid rgba(75, 101, 89, 0.12); border-radius: 7px; color: #66746d; background: rgba(75, 101, 89, 0.06); font-size: 9px; }
  .session-name-editor button.primary { color: #fff; background: #3e8e68; }
  .session-name-editor small { flex-basis: 100%; color: #ad5555; font-size: 8px; }
  .permission-block { padding-left: 11px; border-left: 2px solid #d49350; display: grid; gap: 6px; }
  .permission-block > strong { color: #4d3b2a; font-size: 11px; font-weight: 650; line-height: 1.4; }
  .question-block { padding: 9px; display: grid; gap: 8px; border: 1px solid rgba(48, 133, 176, 0.2); border-radius: 9px; background: rgba(48, 133, 176, 0.055); }
  .question-block section { min-width: 0; display: grid; gap: 5px; }
  .question-block section > strong { color: #344b52; font-size: 11px; line-height: 1.4; overflow-wrap: anywhere; }
  .question-block section > small { color: #6c8077; font-size: 9px; }
  .question-actions { display: flex; flex-wrap: wrap; gap: 5px; }
  .question-actions button { min-height: 26px; padding: 0 8px; border: 1px solid rgba(65, 112, 133, 0.16); border-radius: 7px; color: #425a61; background: rgba(255, 255, 255, 0.52); font-size: 9px; font-weight: 700; cursor: pointer; }
  .question-actions button.selected { border-color: rgba(44, 137, 178, 0.42); background: rgba(48, 145, 187, 0.12); }
  .question-actions button b { color: #2f83aa; }
  code { padding: 7px 8px; overflow: hidden; border-radius: 7px; color: #46524d; background: rgba(70, 82, 77, 0.055); font-family: var(--lume-font-code, "SFMono-Regular", Consolas, monospace); font-size: 10px; text-overflow: ellipsis; white-space: nowrap; }

  .permission-actions { margin-top: 2px; display: flex; flex-wrap: wrap; gap: 5px; }
  .permission-actions button {
    min-height: 27px;
    padding: 0 9px;
    border: 1px solid rgba(82, 101, 93, 0.16);
    border-radius: 8px;
    color: #4d5b55;
    background: rgba(255, 255, 255, 0.58);
    font-size: 10px;
    font-weight: 700;
    cursor: pointer;
    transition: transform 130ms ease, background 130ms ease;
  }
  .permission-actions button:hover { transform: translateY(-1px); background: white; }
  .permission-actions button:active { transform: scale(0.97); }
  .permission-actions button { display: inline-flex; align-items: center; gap: 6px; }
  .permission-actions button:disabled:not(.loading) { opacity: .5; }
  .permission-actions button.loading { cursor: progress; }
  .permission-spinner { width: 10px; height: 10px; border: 2px solid currentColor; border-right-color: transparent; border-radius: 50%; animation: permission-spin .7s linear infinite; }
  @keyframes permission-spin { to { transform: rotate(360deg); } }
  .permission-actions button.primary { border-color: #456d5d; color: white; background: #456d5d; }
  .permission-actions button.danger { color: #a54c4c; }
  .integration-note { margin: 0; color: #7c8983; font-size: 10px; line-height: 1.45; }

  .final-response { position: relative; margin: 8px 0 0; padding: 0; border: 1px solid rgba(78, 105, 93, 0.1); border-radius: 10px; background: rgba(73, 102, 89, 0.035); }
  .final-response-copy { position: absolute; top: 6px; right: 6px; width: 24px; height: 24px; padding: 0; display: grid; place-items: center; border: 0; border-radius: 7px; color: #6a7f75; background: transparent; cursor: pointer; }
  .final-response-copy:hover { color: #3f6253; background: rgba(72, 99, 87, 0.08); }
  .final-response-copy svg { width: 13px; height: 13px; }

  .inline-composer { margin-top: 8px; display: flex; flex-direction: column; gap: 6px; }
  .inline-queue-tray { min-width: 0; width: 100%; min-height: 36px; padding: 5px 7px; display: flex; align-items: center; gap: 7px; border: 1px solid rgba(80, 119, 160, 0.13); border-radius: 9px; color: #4f6d83; background: rgba(74, 119, 157, 0.055); text-align: left; cursor: pointer; }
  .inline-queue-tray:hover:not(:disabled) { border-color: rgba(67, 119, 164, 0.24); background: rgba(74, 119, 157, 0.09); }
  .inline-queue-tray.read-only { cursor: default; }
  .inline-queue-tray.read-only:hover { border-color: rgba(80, 119, 160, 0.13); background: rgba(74, 119, 157, 0.055); }
  .inline-queue-tray:disabled { opacity: 0.58; cursor: default; }
  .inline-queue-tray .queue-mark { width: 18px; height: 18px; display: grid; flex: 0 0 auto; place-items: center; border-radius: 5px; color: #477fa9; background: rgba(66, 127, 174, 0.1); font: 800 11px var(--lume-font-ui, Inter, sans-serif); }
  .inline-queue-tray .queue-copy { min-width: 0; flex: 1; display: grid; gap: 1px; }
  .inline-queue-tray .queue-copy small { color: #7790a1; font-size: 7px; font-weight: 760; letter-spacing: 0.035em; text-transform: uppercase; }
  .inline-queue-tray .queue-copy strong { overflow: hidden; color: #4c6576; font-size: 9px; font-weight: 620; text-overflow: ellipsis; white-space: nowrap; }
  .inline-queue-tray .queue-shortcut { display: flex; flex: 0 0 auto; align-items: center; gap: 4px; color: #748b9a; }
  .inline-queue-tray .queue-shortcut kbd { min-width: 24px; padding: 2px 4px; border: 1px solid rgba(75, 106, 127, 0.17); border-bottom-width: 2px; border-radius: 5px; color: #547286; background: rgba(255, 255, 255, 0.48); font: 750 7px var(--lume-font-ui, Inter, sans-serif); text-align: center; }
  .inline-queue-tray .queue-shortcut small { font-size: 7px; font-weight: 650; white-space: nowrap; }
  .inline-composer-controls { display: flex; align-items: flex-end; gap: 6px; }
  .inline-composer textarea { resize: none; outline: none; font: inherit; }
  .inline-composer textarea { min-width: 0; min-height: 52px; flex: 1; padding: 8px 9px; border: 1px solid rgba(85, 109, 99, 0.14); border-radius: 10px; color: #34423c; background: rgba(255, 255, 255, 0.48); font-size: 10px; line-height: 1.4; }
  .inline-composer textarea:focus { border-color: rgba(70, 111, 94, 0.42); box-shadow: 0 0 0 3px rgba(74, 118, 99, 0.06); }
  .inline-composer-controls > button { width: 30px; height: 30px; display: grid; flex: 0 0 auto; place-items: center; border: 0; border-radius: 9px; color: white; background: #496f60; cursor: pointer; transition: transform 140ms ease, opacity 140ms ease; }
  .inline-composer-controls > button:hover:not(:disabled) { transform: translateY(-1px); }
  .inline-composer-controls > button:disabled { opacity: 0.35; cursor: default; }
  .inline-attachments { min-width: 0; display: flex; gap: 6px; overflow-x: auto; }
  .inline-attachments > span { position: relative; width: 44px; height: 44px; flex: 0 0 auto; overflow: hidden; border: 1px solid rgba(85, 109, 99, 0.14); border-radius: 9px; background: rgba(71, 98, 86, 0.05); }
  .inline-attachments img { width: 100%; height: 100%; display: block; object-fit: cover; }
  .inline-attachments button { position: absolute; top: 2px; right: 2px; width: 16px; height: 16px; padding: 0; display: grid; place-items: center; border: 1px solid rgba(255, 255, 255, 0.5); border-radius: 50%; color: white; background: rgba(27, 39, 34, 0.78); font-size: 11px; line-height: 1; cursor: pointer; }
  .terminate-agent-control { margin: 0 0 9px; display: flex; align-items: center; gap: 6px; }
  .terminate-agent-control.confirming { padding: 7px 8px; border: 1px solid rgba(166, 77, 77, 0.13); border-radius: 9px; background: rgba(166, 77, 77, 0.035); }
  .terminate-agent-control.confirming span { min-width: 0; flex: 1; color: #755b57; font-size: 9px; line-height: 1.35; }
  .terminate-agent-control.confirming button { min-height: 24px; padding: 0 7px; border: 1px solid rgba(91, 107, 100, 0.13); border-radius: 7px; color: #627068; background: rgba(255, 255, 255, 0.42); font-size: 8px; font-weight: 700; cursor: pointer; }
  .terminate-agent-control.confirming button.danger { border-color: rgba(166, 77, 77, 0.2); color: #a54c4c; }
  .terminate-agent-control button:disabled { opacity: 0.45; cursor: default; }
  .reset-settings-control { display: flex; align-items: center; gap: 6px; }
  .reset-settings-control > button { min-height: 27px; padding: 0 9px; border: 1px solid rgba(165, 76, 76, 0.45); border-radius: 8px; color: #a54c4c; background: transparent; font-size: 10px; font-weight: 750; cursor: pointer; transition: transform 130ms ease, background 130ms ease; }
  .reset-settings-control > button:hover:not(:disabled) { transform: translateY(-1px); background: rgba(165, 76, 76, 0.1); }
  .reset-settings-control.confirming { padding: 7px 8px; border: 1px solid rgba(165, 76, 76, 0.22); border-radius: 9px; background: rgba(165, 76, 76, 0.05); }
  .reset-settings-control.confirming span { min-width: 0; flex: 1; color: #8a4340; font-size: 9px; font-weight: 650; line-height: 1.35; }
  .reset-settings-control.confirming button { min-height: 24px; padding: 0 7px; border: 1px solid rgba(77, 91, 85, 0.3); border-radius: 7px; color: #4d5b55; background: transparent; font-size: 8px; font-weight: 750; cursor: pointer; transition: background 130ms ease; }
  .reset-settings-control.confirming button:hover:not(:disabled) { background: rgba(77, 91, 85, 0.1); }
  .reset-settings-control.confirming button.danger { border-color: rgba(165, 76, 76, 0.5); color: #a54c4c; }
  .reset-settings-control.confirming button.danger:hover:not(:disabled) { background: rgba(165, 76, 76, 0.12); }
  .reset-settings-control button:disabled { opacity: 0.45; cursor: default; }

  .whiteboard { position: relative; max-height: 431px; min-height: 0; padding: 7px 16px 15px; display: flex; flex-direction: column; overflow: hidden; }






  .whiteboard { --wb-line: var(--lume-line-light); --wb-raised: var(--lume-raised-light); --wb-subtle: var(--lume-subtle-light); --wb-surface: var(--lume-surface-light); --wb-text: var(--lume-ink-light); --wb-strong: var(--lume-ink-strong-light); --wb-muted: var(--lume-ink-muted-light); --wb-accent: var(--lume-accent-strong); --wb-accent-soft: var(--lume-accent-soft-light); --wb-danger: #b0524f; }
  .overlay-shell.dark .whiteboard { --wb-line: var(--lume-line-dark); --wb-raised: var(--lume-raised-dark); --wb-subtle: var(--lume-subtle-dark); --wb-surface: var(--lume-surface-dark); --wb-text: var(--lume-ink-dark); --wb-strong: var(--lume-ink-strong-dark); --wb-muted: var(--lume-ink-muted-dark); --wb-accent: var(--lume-accent); --wb-accent-soft: var(--lume-accent-soft-dark); --wb-danger: #d98a86; }
  .layout-toolbar { padding: 6px 0 8px; display: flex; align-items: center; gap: 6px; }
  .layout-toolbar :global(.lume-select) { min-width: 0 !important; flex: 1 1 0; }
  .layout-toolbar :global(.lume-select-trigger) { min-height: 34px; border-radius: 10px; }
  .layout-toolbar input { min-width: 0; height: 34px; padding: 0 11px; flex: 1 1 0; border: 1px solid var(--wb-line); border-radius: 10px; outline: 0; color: var(--wb-strong); background: var(--wb-raised); font: 500 10px var(--lume-font-ui, Inter, sans-serif); transition: border-color 140ms ease, box-shadow 140ms ease; }
  .layout-toolbar input::placeholder { color: var(--wb-muted); opacity: .75; }
  .layout-toolbar input:focus { border-color: color-mix(in srgb, var(--wb-accent) 55%, var(--wb-line)); box-shadow: 0 0 0 3px var(--wb-accent-soft); }
  .layout-toolbar button { width: 34px; height: 34px; padding: 0; flex: 0 0 34px; display: grid; place-items: center; border: 1px solid var(--wb-line); border-radius: 10px; color: var(--wb-muted); background: var(--wb-raised); cursor: pointer; transition: color 140ms ease, border-color 140ms ease, background 140ms ease, transform 140ms ease; }
  .layout-toolbar button:hover:not(:disabled) { color: var(--wb-accent); border-color: color-mix(in srgb, var(--wb-accent) 40%, var(--wb-line)); background: var(--wb-accent-soft); }
  .layout-toolbar button:active:not(:disabled) { transform: scale(.94); }
  .layout-toolbar button:disabled { opacity: .45; cursor: default; }
  .layout-toolbar button svg { width: 16px; height: 16px; fill: none; stroke: currentColor; stroke-width: 1.5; stroke-linecap: round; stroke-linejoin: round; }
  .layout-toolbar .layout-delete:hover:not(:disabled) { color: var(--wb-danger); border-color: color-mix(in srgb, var(--wb-danger) 40%, var(--wb-line)); background: color-mix(in srgb, var(--wb-danger) 10%, transparent); }
  .workflow-global-mode { position: relative; margin: 0 0 6px; padding-bottom: 8px; display: flex; align-items: center; gap: 6px; border-bottom: 1px solid var(--wb-line); }
  .workflow-mode-switch { position: relative; height: 36px; padding: 3px; flex: 1 1 auto; display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); border: 1px solid var(--wb-line); border-radius: 11px; background: var(--wb-subtle); isolation: isolate; }
  .workflow-mode-switch::before { position: absolute; z-index: 0; top: 3px; bottom: 3px; left: 3px; width: calc((100% - 6px) / 2); border-radius: 8px; content: ""; background: var(--wb-surface); box-shadow: 0 1px 4px color-mix(in srgb, #000 16%, transparent), inset 0 0 0 1px color-mix(in srgb, var(--wb-line) 70%, transparent); transform: translateX(0); transition: transform 240ms cubic-bezier(.2, .85, .2, 1), background 180ms ease; }
  .workflow-mode-switch.workflow-active::before { transform: translateX(100%); background: color-mix(in srgb, var(--wb-accent) 14%, var(--wb-surface)); }
  .workflow-mode-switch button { position: relative; z-index: 1; min-width: 0; padding: 0 8px; display: flex; align-items: center; justify-content: center; gap: 7px; border: 0; border-radius: 8px; color: var(--wb-muted); background: transparent; font: 650 11px var(--lume-font-ui, Inter, sans-serif); cursor: pointer; transition: color 160ms ease, transform 160ms cubic-bezier(.2, .8, .2, 1); }
  .workflow-mode-switch button:hover:not(.active):not(:disabled) { color: var(--wb-strong); }
  .workflow-mode-switch button:active:not(:disabled) { transform: scale(.96); }
  .workflow-mode-switch button.active { color: var(--wb-accent); }
  .workflow-mode-switch button:disabled { opacity: .5; cursor: default; }
  .mode-icon { width: 15px; height: 15px; flex: 0 0 15px; fill: none; stroke: currentColor; stroke-width: 1.4; stroke-linecap: round; stroke-linejoin: round; }
  .mode-icon circle { fill: var(--wb-surface); }
  .workflow-settings-trigger { width: 36px; height: 36px; padding: 0; flex: 0 0 36px; display: grid; place-items: center; border: 1px solid var(--wb-line); border-radius: 11px; color: var(--wb-muted); background: var(--wb-raised); cursor: pointer; transition: color 140ms ease, border-color 140ms ease, background 140ms ease; }
  .workflow-settings-trigger:hover, .workflow-settings-trigger.active { color: var(--wb-accent); border-color: color-mix(in srgb, var(--wb-accent) 40%, var(--wb-line)); background: var(--wb-accent-soft); }
  .workflow-settings-trigger svg { width: 17px; height: 17px; fill: none; stroke: currentColor; stroke-width: 1.5; stroke-linecap: round; }
  .workflow-settings-trigger circle { fill: var(--wb-raised); }
  .layout-spinner { width: 12px; height: 12px; border: 1.5px solid currentColor; border-right-color: transparent; border-radius: 50%; animation: layout-spin 650ms linear infinite; }
  @keyframes layout-spin { to { transform: rotate(360deg); } }



























  .workflow-settings-dismiss { position: fixed; z-index: 238; inset: 0; width: 100%; height: 100%; padding: 0; border: 0; background: transparent; cursor: default; }
  .workflow-settings-popover { position: absolute; z-index: 240; top: 98px; right: 16px; width: min(310px, calc(100% - 32px)); max-height: calc(100vh - 96px); overflow: hidden; isolation: isolate; border: 1px solid rgba(67, 105, 86, 0.18); border-radius: 12px; color: #485b51; background: #f7faf8; background-clip: padding-box; }
  .workflow-settings-scroll { box-sizing: border-box; width: 100%; max-height: calc(100vh - 96px); padding: 11px; display: grid; gap: 9px; overflow-x: hidden; overflow-y: auto; overscroll-behavior: contain; scrollbar-width: thin; }
  .workflow-settings-scroll > header { display: flex; align-items: flex-start; gap: 8px; }
  .workflow-settings-scroll > header div { min-width: 0; flex: 1; display: grid; gap: 2px; }
  .workflow-settings-scroll > header strong { color: #33483d; font-size: 10px; }
  .workflow-settings-scroll > header small { color: #829087; font-size: 7.5px; }
  .workflow-settings-scroll > header button { width: 22px; height: 22px; padding: 0; border: 0; border-radius: 6px; color: #73827a; background: transparent; font-size: 16px; cursor: pointer; }
  .workflow-setting-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 6px; }
  .workflow-setting-grid label { min-width: 0; display: grid; gap: 4px; color: #718078; font-size: 7.5px; font-weight: 700; }
  .workflow-setting-grid input { width: 100%; min-width: 0; height: 29px; padding: 0 7px; border: 1px solid rgba(75, 105, 90, 0.15); border-radius: 7px; outline: 0; color: #3f574a; background: rgba(73, 105, 88, 0.045); font: 750 9px var(--lume-font-ui, Inter, sans-serif); }
  .workflow-setting-grid input:focus { border-color: rgba(50, 143, 96, 0.45); box-shadow: 0 0 0 2px rgba(50, 143, 96, 0.07); }
  .workflow-setting-toggle { min-height: 29px; display: flex; align-items: center; gap: 8px; color: #596d62; font-size: 8.5px; font-weight: 700; cursor: pointer; }
  .workflow-setting-toggle span { min-width: 0; flex: 1; }
  .workflow-setting-toggle input { position: absolute; opacity: 0; pointer-events: none; }
  .workflow-setting-toggle i { width: 27px; height: 15px; padding: 2px; flex: 0 0 auto; border-radius: 9px; background: #c4cec9; transition: background 150ms ease; }
  .workflow-setting-toggle i::before { width: 11px; height: 11px; display: block; border-radius: 50%; content: ""; background: white; box-shadow: 0 1px 3px rgba(34, 55, 44, 0.2); transition: transform 170ms cubic-bezier(.2,.8,.2,1); }
  .workflow-setting-toggle input:checked + i { background: #3e9568; }
  .workflow-setting-toggle input:checked + i::before { transform: translateX(12px); }
  .workflow-reserve-setting { display: grid; grid-template-columns: minmax(0, 1fr) 100px 30px; align-items: center; gap: 7px; color: #65766d; font-size: 8px; font-weight: 700; }
  .workflow-reserve-setting input { width: 100%; accent-color: #3c9266; }
  .workflow-reserve-setting strong { color: #397b59; font-size: 8px; text-align: right; }
  .workflow-missing-sessions { padding-top: 8px; display: grid; gap: 6px; border-top: 1px solid rgba(75, 105, 90, 0.12); }
  .workflow-missing-sessions > strong { color: #9a6c3b; font-size: 8px; text-transform: uppercase; letter-spacing: .05em; }
  .workflow-missing-sessions label { display: grid; grid-template-columns: 74px minmax(0, 1fr); align-items: center; gap: 7px; }
  .workflow-missing-sessions label > span { overflow: hidden; color: #64766c; font-size: 8px; font-weight: 700; text-overflow: ellipsis; text-transform: capitalize; white-space: nowrap; }
  .workflow-missing-sessions :global(.lume-select) { width: 100%; }
  .terminal-picker { min-height: 0; padding: 9px 0 6px; flex: 1 1 auto; overflow-x: hidden; overflow-y: auto; overscroll-behavior: contain; scrollbar-gutter: stable; scrollbar-width: thin; scrollbar-color: #cad2ce transparent; }
  .terminal-picker-row { min-height: 59px; display: flex; align-items: center; gap: 8px; border-bottom: 1px solid rgba(105, 123, 115, 0.09); }
  .terminal-picker-row:last-child { border-bottom: 0; }
  .terminal-picker-avatar { width: 32px; height: 32px; display: grid; flex: 0 0 auto; place-items: center; }
  .terminal-picker-copy { min-width: 0; flex: 1; display: grid; gap: 2px; }
  .terminal-picker-copy strong { color: #35423d; font-size: 10px; }
  .terminal-picker-copy small { min-width: 0; display: flex; align-items: center; gap: 4px; overflow: hidden; color: #89938f; font-size: 9px; white-space: nowrap; }
  .terminal-picker-copy small span { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .terminal-picker-row > button { min-width: 52px; height: 28px; padding: 0 9px; border: 1px solid rgba(82, 105, 95, 0.16); border-radius: 9px; color: #4d6f61; background: rgba(255, 255, 255, 0.38); font-size: 9px; font-weight: 720; cursor: pointer; transition: transform 140ms ease, background 140ms ease; }
  .terminal-picker-row > button:hover:not(:disabled) { transform: translateY(-1px); background: white; }
  .terminal-picker-row > button:disabled { opacity: 0.5; cursor: default; }
  .board-empty { margin: 22px 0; color: #89938f; font-size: 9px; line-height: 1.45; }

  .empty-state { height: 100%; min-height: 260px; display: flex; flex-direction: column; align-items: center; justify-content: center; color: #73807a; text-align: center; }
  .empty-state strong { margin-top: 10px; color: #44524c; font-size: 11px; }
  .empty-state p { max-width: 210px; margin: 4px 0 0; font-size: 10px; line-height: 1.45; }
  .quiet-orbit { width: 31px; height: 31px; display: grid; place-items: center; border: 1px solid #aab6b0; border-radius: 50%; }
  .quiet-orbit i { width: 7px; height: 7px; border-radius: 50%; background: #799186; }

  .inspector-screen { width: 100%; height: 100%; min-height: 0; display: flex; flex-direction: column; overflow: hidden; }
  .orb-inspector-content { min-height: 0; padding: 7px 9px 9px; display: flex; flex: 1 1 auto; flex-direction: column; }
  .orb-inspector-content :global(.workspace-inspector.orb-inspector) { height: auto; min-height: 0; flex: 1 1 auto; }
  .inspector-no-sessions { margin: auto; padding: 16px 12px; display: grid; justify-items: center; gap: 8px; color: #62746a; text-align: center; }
  .inspector-no-sessions strong { font-size: 11px; }
  .inspector-no-sessions p { margin: 0; font-size: 9px; line-height: 1.5; }
  .inspector-no-sessions button { min-height: 29px; padding: 0 9px; border: 1px solid rgba(82, 105, 95, .16); border-radius: 7px; color: #547462; background: transparent; font-size: 9px; font-weight: 700; cursor: pointer; }
  .results-empty { margin: 8px 2px 4px; color: #89938f; font-size: 9px; }

  .settings { padding: 5px 16px 20px; }
  .settings-section-label { padding: 9px 0 5px; color: #929c97; font-size: 9px; font-weight: 750; letter-spacing: 0.07em; text-transform: uppercase; }
  .settings-section-label.preferences-label { padding-top: 17px; }
  .settings-section { border-bottom: 1px solid rgba(105, 123, 115, 0.1); }
  .settings-section > .settings-section-label {
    min-height: 39px;
    padding: 0 2px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    list-style: none;
    cursor: pointer;
    user-select: none;
  }
  .settings-section > .settings-section-label::-webkit-details-marker { display: none; }
  .settings-section > .settings-section-label::after {
    content: "+";
    color: #7d8d85;
    font-size: 15px;
    font-weight: 450;
    letter-spacing: 0;
    transition: color 130ms ease, transform 130ms ease;
  }
  .settings-section[open] > .settings-section-label::after {
    content: "−";
    color: #4f7463;
    transform: rotate(180deg);
  }
  .settings-section > .settings-section-label:hover { color: #66766f; }
  .settings-section-static > .settings-section-label { cursor: default; }
  .settings-section-static > .settings-section-label::after { content: none; }
  .settings-section-static > .settings-section-label:hover { color: #929c97; }
  .settings-section-content { padding: 0 1px 6px; }
  .integration-group-label { padding: 9px 2px 3px; color: #77847e; font-size: 8px; font-weight: 760; letter-spacing: 0.04em; text-transform: uppercase; }
  .integration-group-label:first-child { padding-top: 3px; }
  .integration-row { min-height: 55px; display: flex; align-items: center; gap: 10px; border-bottom: 1px solid rgba(105, 123, 115, 0.1); }
  .integration-row .agent-avatar { width: 28px; height: 28px; border-radius: 9px; font-size: 10px; }
  .integration-row > div:not(.integration-actions) { min-width: 0; flex: 1; display: grid; gap: 2px; }
  .integration-row strong { color: #35423d; font-size: 10px; }
  .integration-row div span { overflow: hidden; color: #89938f; font-size: 9px; text-overflow: ellipsis; white-space: nowrap; }
  .integration-row button { min-width: 63px; height: 27px; padding: 0 8px; border: 1px solid rgba(82, 105, 95, 0.14); border-radius: 8px; color: #577064; background: transparent; font-size: 9px; font-weight: 680; cursor: pointer; transition: background 150ms ease, color 150ms ease, transform 150ms ease; }
  .integration-row button:hover:not(:disabled) { transform: translateY(-1px); background: rgba(82, 112, 99, 0.06); }
  .integration-row button.connected { border-color: transparent; color: #6d7e76; }
  .integration-row button:disabled { cursor: default; opacity: 0.5; }
  .integration-actions { display: flex; align-items: center; gap: 4px; }
  .integration-actions .diagnose-button { min-width: 42px; padding: 0 6px; border-color: transparent; color: #78877f; }
  .diagnostic-card { margin: -1px 0 7px 38px; padding: 7px 8px; display: grid; gap: 6px; border: 1px solid rgba(93, 113, 104, 0.1); border-radius: 9px; background: rgba(75, 103, 90, 0.03); }
  .diagnostic-check { min-width: 0; display: flex; align-items: flex-start; gap: 7px; }
  .diagnostic-check > i { width: 6px; height: 6px; margin-top: 3px; flex: 0 0 auto; border-radius: 50%; background: #789487; }
  .diagnostic-check.status-warning > i { background: #c3933c; }
  .diagnostic-check.status-error > i { background: #bd5c59; }
  .diagnostic-check > span { min-width: 0; display: grid; gap: 1px; }
  .diagnostic-check strong { color: #4c5c55; font-size: 8px; }
  .diagnostic-check small { overflow: hidden; color: #89958f; font-size: 8px; line-height: 1.35; text-overflow: ellipsis; white-space: nowrap; }
  .browser-row button { min-width: 68px; }
  .browser-path { margin: 7px 2px 0; overflow-wrap: anywhere; color: #89938f; font-size: 9px; line-height: 1.4; }
  .plugin-actions { padding-top: 8px; display: flex; gap: 5px; }
  .plugin-actions button, .profile-action, .apply-profile-button { min-height: 27px; padding: 0 8px; border: 1px solid rgba(82, 105, 95, 0.14); border-radius: 8px; color: #577064; background: transparent; font-size: 8px; font-weight: 680; cursor: pointer; }
  .external-plugin-row > button { min-width: 54px; }
  .setting-row,
  .field-row { min-height: 67px; display: flex; align-items: center; justify-content: space-between; gap: 14px; border-bottom: 1px solid rgba(105, 123, 115, 0.1); }
  .setting-row > div,
  .field-row > span,
  .launch-setting > span { display: grid; gap: 3px; }
  .setting-row strong,
  .field-row strong,
  .launch-setting strong { color: #35423d; font-size: 10px; }
  .setting-row div span,
  .field-row small,
  .launch-setting small { color: #89938f; font-size: 9px; font-weight: 400; }

  .switch { position: relative; width: 33px; height: 19px; flex: 0 0 auto; }
  .switch input { position: absolute; opacity: 0; pointer-events: none; }
  .switch span { position: absolute; inset: 0; border-radius: 999px; background: #ccd3cf; cursor: pointer; transition: background 180ms ease; }
  .switch span::after { content: ""; position: absolute; width: 15px; height: 15px; top: 2px; left: 2px; border-radius: 50%; background: white; box-shadow: 0 1px 3px rgba(29, 43, 37, 0.22); transition: transform 180ms cubic-bezier(0.2, 0.8, 0.2, 1); }
  .switch input:checked + span { background: #527c6c; }
  .switch input:checked + span::after { transform: translateX(14px); }
  .switch input:focus-visible + span { outline: 2px solid #83958d; outline-offset: 2px; }
  .sound-volume-row.disabled { opacity: 0.52; }
  .volume-control { width: 116px; display: flex; align-items: center; gap: 7px; }
  .volume-control input { width: 82px; height: 16px; accent-color: #527c6c; cursor: pointer; }
  .volume-control input:disabled { cursor: default; }
  .volume-control output { width: 27px; color: #718078; font-size: 8px; font-variant-numeric: tabular-nums; text-align: right; }
  .appearance-theme-setting { min-height: 72px; padding: 10px 0; display: grid; gap: 9px; border-bottom: 1px solid rgba(105, 123, 115, 0.1); }
  .appearance-theme-setting > span { display: grid; gap: 3px; }
  .appearance-theme-setting strong { color: #35423d; font-size: 10px; }.appearance-theme-setting small { color: #89938f; font-size: 9px; }
  .appearance-theme-list { display: grid; grid-template-columns: repeat(5, minmax(0, 1fr)); gap: 5px; }
  .appearance-theme-list button { height: 32px; padding: 3px; border: 1px solid transparent; border-radius: 8px; background: transparent; cursor: pointer; }
  .appearance-theme-list button:hover, .appearance-theme-list button.active { border-color: color-mix(in srgb, var(--theme-accent) 48%, transparent); background: color-mix(in srgb, var(--theme-accent) 10%, transparent); }
  .appearance-theme-list i { width: 100%; height: 100%; display: block; border: 6px solid var(--theme-surface); border-radius: 6px; background: var(--theme-accent); }

  .field-row :global(.lume-select) { max-width: 145px; }
  .launch-setting { padding: 14px 0 10px; display: grid; gap: 11px; }
  .segmented { padding: 2px; display: grid; grid-template-columns: repeat(3, 1fr); border-radius: 9px; background: rgba(83, 104, 95, 0.07); }
  .segmented button { height: 29px; border: 0; border-radius: 7px; color: #74817b; background: transparent; font-size: 9px; font-weight: 680; cursor: pointer; transition: color 150ms ease, background 150ms ease, box-shadow 150ms ease; }
  .segmented button.active { color: #35473f; background: rgba(255, 255, 255, 0.82); box-shadow: 0 1px 4px rgba(37, 53, 46, 0.1); }
  .project-launch-setting .segmented { grid-template-columns: repeat(4, 1fr); }
  .preferred-agents-setting { border-bottom: 1px solid rgba(105, 123, 115, 0.1); }
  .agent-preferences { display: flex; flex-wrap: wrap; gap: 5px; }
  .agent-preferences button { min-height: 29px; padding: 0 8px; display: inline-flex; align-items: center; gap: 5px; border: 1px solid rgba(83, 107, 97, 0.12); border-radius: 8px; color: #74817b; background: transparent; font-size: 8px; cursor: pointer; }
  .agent-preferences button.active { color: #3f6656; border-color: rgba(72, 114, 96, 0.24); background: rgba(72, 114, 96, 0.07); }
  .apply-profile-button { width: 100%; margin-top: 10px; color: #f6fbf8; border-color: #527c6c; background: #527c6c; }
  .shortcut-input {
    width: 112px;
    padding: 6px 7px;
    border: 1px solid rgba(92, 111, 103, 0.16);
    border-radius: 8px;
    outline: 0;
    color: #607068;
    background: rgba(80, 105, 94, 0.045);
    font-family: inherit;
    font-size: 8px;
    text-align: center;
    cursor: pointer;
  }
  .shortcut-input:focus {
    color: #3e6153;
    border-color: rgba(69, 113, 94, 0.42);
    box-shadow: 0 0 0 2px rgba(74, 122, 102, 0.08);
  }
  .profile-empty { margin: 5px 1px 2px; color: #89938f; font-size: 9px; line-height: 1.45; }
  .mobile-access-card { padding: 11px; display: grid; gap: 9px; border: 1px solid rgba(92, 111, 103, 0.11); border-radius: 13px; background: rgba(84, 111, 99, 0.035); }
  .mobile-access-header,
  .mobile-address,
  .mobile-apk,
  .mobile-pair-action,
  .mobile-pairing { display: flex; align-items: center; gap: 9px; }
  .mobile-access-header > div,
  .mobile-pair-action > span,
  .mobile-apk > span { min-width: 0; flex: 1; display: grid; gap: 2px; }
  .mobile-access-card strong { color: #35423d; font-size: 9px; }
  .mobile-access-card span,
  .mobile-access-card small { color: #89938f; font-size: 8px; line-height: 1.4; }
  .mobile-address,
  .mobile-apk,
  .mobile-pair-action { padding-top: 8px; border-top: 1px solid rgba(92, 111, 103, 0.09); }
  .mobile-address > span { min-width: 0; flex: 1; display: flex; align-items: center; gap: 6px; }
  .mobile-address code { overflow: hidden; color: #53665d; font-size: 8px; text-overflow: ellipsis; white-space: nowrap; }
  .mobile-apk code { overflow: hidden; color: #53665d; font-size: 8px; text-overflow: ellipsis; white-space: nowrap; }
  .mobile-access-card button,
  .paired-devices button { min-height: 25px; padding: 0 7px; border: 1px solid rgba(82, 105, 95, 0.14); border-radius: 7px; color: #577064; background: transparent; font-size: 8px; font-weight: 680; cursor: pointer; }
  .mobile-access-card button:disabled,
  .paired-devices button:disabled { cursor: default; opacity: 0.5; }
  .mobile-pairing { flex-direction: column; align-items: center; padding: 10px; border-radius: 9px; background: rgba(255, 255, 255, 0.5); }
  .mobile-pairing img { width: 208px; max-width: 100%; height: auto; border-radius: 5px; image-rendering: pixelated; }
  .mobile-pairing > span { width: 100%; min-width: 0; display: grid; gap: 5px; text-align: center; }
  .mobile-pairing code { color: #31483e; font-size: 9px; overflow-wrap: anywhere; }
  .mobile-message { margin: 0; color: #61756b; font-size: 8px; line-height: 1.4; }
  .paired-devices { margin-top: 9px; display: grid; gap: 8px; }
  .paired-devices-intro { padding: 1px 2px 3px; }
  .paired-devices-intro strong { color: #35423d; font-size: 9px; }
  .paired-device-card { padding: 10px; border: 1px solid rgba(92, 111, 103, 0.12); border-radius: 12px; background: rgba(84, 111, 99, 0.03); }
  .paired-device-header { display: flex; align-items: center; gap: 9px; }
  .paired-device-info { min-width: 0; flex: 1; display: grid; gap: 2px; }
  .paired-device-info strong { color: #35423d; font-size: 10px; }
  .paired-device-info small { color: #89938f; font-size: 8px; }
  .preview-badge { padding: 4px 6px; border: 1px solid rgba(82, 124, 108, 0.14); border-radius: 999px; color: #527c6c; background: rgba(82, 124, 108, 0.06); font-size: 7px; font-weight: 750; }
  .paired-devices .revoke-device { color: #8a5e5e; border-color: rgba(151, 91, 91, 0.14); }
  .device-permissions { margin-top: 8px; border-top: 1px solid rgba(92, 111, 103, 0.1); }
  .device-permission { min-height: 49px; padding: 7px 1px; display: flex; align-items: center; gap: 9px; border-bottom: 1px solid rgba(92, 111, 103, 0.08); transition: background 140ms ease; }
  .device-permission.active { background: rgba(82, 124, 108, 0.035); }
  .device-permission:last-child { border-bottom: 0; }
  .permission-copy { min-width: 0; flex: 1; display: grid; gap: 2px; }
  .permission-copy strong { color: #42514b; font-size: 9px; }
  .permission-copy small { color: #89938f; font-size: 8px; line-height: 1.35; }
  .permission-choice { display: flex; align-items: center; gap: 7px; }
  .permission-state { max-width: 62px; color: #919b96; font-size: 7px; font-weight: 720; line-height: 1.25; text-align: right; }
  .permission-state.allowed { color: #47745f; }
  .update-card { padding: 12px; border: 1px solid rgba(92, 111, 103, 0.11); border-radius: 13px; background: rgba(84, 111, 99, 0.035); }
  .update-main { display: flex; align-items: center; gap: 9px; }
  .update-copy { min-width: 0; flex: 1; display: grid; gap: 2px; }
  .update-copy strong { color: #35423d; font-size: 10px; }
  .update-copy span,
  .update-card p { color: #89938f; font-size: 9px; }
  .update-card p { margin: 9px 0 0; line-height: 1.4; }
  .update-main button { min-width: 63px; height: 27px; padding: 0 8px; border: 1px solid rgba(82, 105, 95, 0.14); border-radius: 8px; color: #577064; background: transparent; font-size: 9px; font-weight: 680; cursor: pointer; transition: background 150ms ease, transform 150ms ease; }
  .update-main button.update-available { color: #f7fbf9; border-color: #527c6c; background: #527c6c; }
  .update-main button:hover:not(:disabled) { transform: translateY(-1px); background: rgba(82, 112, 99, 0.09); }
  .update-main button.update-available:hover { background: #476f60; }
  .update-main button:disabled { cursor: default; opacity: 0.58; }
  .update-progress { height: 2px; margin-top: 9px; overflow: hidden; border-radius: 999px; background: rgba(82, 112, 99, 0.1); }
  .update-progress span { height: 100%; display: block; border-radius: inherit; background: #5f8ac7; transition: width 180ms ease; }
  .update-progress.indeterminate span { animation: update-slide 1.15s ease-in-out infinite alternate; }
  @keyframes update-slide { from { transform: translateX(-70%); } to { transform: translateX(320%); } }
  .save-state { display: block; color: #87928d; font-size: 9px; text-align: right; opacity: 0; transition: opacity 120ms ease; }
  .save-state.visible { opacity: 1; }

  footer {
    flex: 0 0 auto;
    min-height: 52px;
    padding: 6px 10px 8px;
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    align-items: center;
    border-top: 1px solid rgba(101, 120, 112, 0.11);
  }
  footer button { height: 36px; display: flex; align-items: center; justify-content: center; gap: 5px; border: 0; border-radius: 10px; color: #88928e; background: transparent; font-size: 11px; font-weight: 650; cursor: pointer; transition: color 150ms ease, background 150ms ease; }
  footer button:hover { color: #52615a; background: rgba(76, 100, 90, 0.045); }
  footer button.active { color: #476c5d; }
  footer button :global(.orb-navigation-icon) { width: 18px; height: 18px; }
  footer button.has-update,
  footer button.has-mobile-device { position: relative; }
  footer button.has-update::after { content: ""; position: absolute; top: 5px; right: 14px; width: 5px; height: 5px; border: 2px solid rgba(248, 250, 249, 0.95); border-radius: 50%; background: #5f8ac7; }
  footer button.has-mobile-device::after { content: ""; position: absolute; top: 5px; right: 14px; width: 5px; height: 5px; border: 2px solid rgba(248, 250, 249, 0.95); border-radius: 50%; background: #58a97d; }

  .overlay-shell:not(.dark) .lume-orb {
    --orb-border: rgba(54, 92, 70, 0.38);
    --orb-fill: #e6ead7;
    --orb-shadow: inset 0 0 0 1px rgba(247, 242, 220, 0.62), 0 5px 16px rgba(35, 62, 49, 0.12);
  }
  .overlay-shell:not(.dark) .panel {
    border-color: rgba(48, 86, 64, 0.34);
    background: #dce6d8;
    box-shadow: inset 0 0 0 1px rgba(244, 239, 218, 0.48), 0 10px 28px rgba(35, 58, 48, 0.14);
  }
  .overlay-shell:not(.dark) .panel-header,
  .overlay-shell:not(.dark) footer {
    border-color: rgba(73, 99, 87, 0.18);
    background: #e8e5d2;
  }
  .overlay-shell:not(.dark) .panel-content,
  .overlay-shell:not(.dark) .session-list,
  .overlay-shell:not(.dark) .settings,
  .overlay-shell:not(.dark) .whiteboard { background: #d7e3d5; }
  .overlay-shell:not(.dark) .session-row,
  .overlay-shell:not(.dark) .setting-row,
  .overlay-shell:not(.dark) .field-row,
  .overlay-shell:not(.dark) .terminal-picker-row,
  .overlay-shell:not(.dark) .settings-section { border-color: rgba(73, 99, 87, 0.16); }
  .overlay-shell:not(.dark) .session-row:hover:not(.attention),
  .overlay-shell:not(.dark) .session-row.selected:not(.attention) { background: #c8ddcc; }
  .overlay-shell:not(.dark) .diagnostic-card,
  .overlay-shell:not(.dark) .update-card,
  .overlay-shell:not(.dark) .mobile-access-card,
  .overlay-shell:not(.dark) .paired-device-card {
    border-color: rgba(70, 98, 85, 0.2);
    background: #ebe8d6;
    box-shadow: 0 1px 3px rgba(42, 67, 55, 0.045);
  }



  .overlay-shell:not(.dark) .terminal-picker-row > button,
  .overlay-shell:not(.dark) .shortcut-input,
  .overlay-shell:not(.dark) .inline-composer textarea,
  .overlay-shell:not(.dark) .permission-actions button {
    border-color: rgba(68, 94, 82, 0.22);
    background: #eee9d8;
  }
  .overlay-shell:not(.dark) .final-response {
    border-color: rgba(66, 96, 82, 0.18);
    background: #e2e9d6;
  }
  .overlay-shell:not(.dark) .launcher-popover,
  .overlay-shell:not(.dark) .workflow-settings-popover { background: #e9e7d6; }
  .overlay-shell:not(.dark) .segmented,
  .overlay-shell:not(.dark) .mobile-pairing { background: #cfdccd; }
  .overlay-shell:not(.dark) .segmented button.active { color: #245f43; background: #e9e6d3; }
  .overlay-shell:not(.dark) footer button.active,
  .overlay-shell:not(.dark) .add-button.active { color: #276f4a; background: rgba(55, 137, 89, 0.11); }
  .overlay-shell:not(.dark) .agent-count { background: #2f6749; }

  @media (prefers-reduced-motion: reduce) {
    *, *::before, *::after { animation-duration: 0.01ms !important; animation-iteration-count: 1 !important; transition-duration: 0.01ms !important; }
    .launcher-spinner { animation: none !important; }
  }

  .overlay-shell.dark { color-scheme: dark; }
  .overlay-shell.dark .lume-orb { color: #dfe8e3; --orb-border: rgba(190, 209, 200, 0.13); --orb-fill: #1b221f; }
  .overlay-shell.dark .launcher-popover { color: #dfe8e3; border-color: rgba(190, 209, 200, 0.13); background: #1b221f; }
  .overlay-shell.dark .resume-session { color: #afc0b7; background: rgba(216, 229, 223, 0.035); }
  .overlay-shell.dark .resume-session:hover { background: rgba(101, 180, 141, 0.08); }
  .overlay-shell.dark .resume-session.loading,
  .overlay-shell.dark .launcher-row button.loading { color: #a3d8ba; background: rgba(101, 180, 141, 0.12); }
  .overlay-shell.dark .launcher-loading-note { color: #93a89c; }
  .overlay-shell.dark .resume-session strong { color: #dbe7e1; }
  .overlay-shell.dark .resume-session small { color: #899a91; }
  .overlay-shell.dark .panel { color: #dfe8e3; border-color: rgba(190, 209, 200, 0.13); background: #1b221f; }
  .overlay-shell.dark .mobile-device-banner { border-color: rgba(111, 190, 151, 0.14); background: rgba(88, 167, 125, 0.07); }
  .overlay-shell.dark .mobile-device-banner-icon { color: #87c6a7; background: rgba(88, 167, 125, 0.12); }
  .overlay-shell.dark .mobile-device-banner strong { color: #b7dcc9; }
  .overlay-shell.dark .mobile-device-banner small { color: #8fa99c; }
  .overlay-shell.dark .mobile-device-banner > button { color: #a8d2bc; border-color: rgba(111, 190, 151, 0.18); background: rgba(216, 229, 223, 0.04); }
  .overlay-shell.dark .brand-lockup strong,
  .overlay-shell.dark .session-title-row strong,
  .overlay-shell.dark .terminal-picker-copy strong,
  .overlay-shell.dark .setting-row strong,
  .overlay-shell.dark .integration-row strong,
  .overlay-shell.dark .field-row strong,
  .overlay-shell.dark .launch-setting strong { color: #e3ebe7; }
  .overlay-shell.dark .update-copy strong { color: #e3ebe7; }
  .overlay-shell.dark .launcher-row strong { color: #dfe8e3; }
  .overlay-shell.dark .panel-header,
  .overlay-shell.dark footer,
  .overlay-shell.dark .session-row,
  .overlay-shell.dark .setting-row,
  .overlay-shell.dark .field-row { border-color: rgba(190, 209, 200, 0.09); }
  .overlay-shell.dark .settings-section { border-color: rgba(190, 209, 200, 0.09); }
  .overlay-shell.dark .settings-section[open] > .settings-section-label::after { color: #8eb9a5; }
  .overlay-shell.dark .session-row:hover:not(.attention),
  .overlay-shell.dark .session-row.selected:not(.attention) { background: rgba(198, 218, 208, 0.045); }
  .overlay-shell.dark .inspector-no-sessions { color: #a0b0a7; }
  .overlay-shell.dark .inspector-no-sessions button { border-color: rgba(207, 223, 215, .12); color: #b0c3b8; background: rgba(222, 233, 228, .035); }
  .overlay-shell.dark .session-action-button { color: #9caea5; border-color: rgba(207, 223, 215, 0.1); background: rgba(222, 233, 228, 0.035); }
  .overlay-shell.dark .session-action-button:hover:not(:disabled),
  .overlay-shell.dark .session-action-button.active { color: #9fd0b7; background: rgba(100, 180, 143, 0.09); }
  .overlay-shell.dark .session-action-button.warning { color: #d0aa67; }
  .overlay-shell.dark .session-action-button.danger { color: #d49792; }
  .overlay-shell.dark .session-action-button::after { color: #c7d5ce; border-color: rgba(205, 222, 213, 0.11); background: rgba(28, 40, 34, 0.98); box-shadow: 0 6px 18px rgba(0, 0, 0, 0.24); }
  .overlay-shell.dark .session-name-editor input,
  .overlay-shell.dark .session-name-editor button { color: #c5d0cb; border-color: rgba(207, 223, 215, 0.12); background: rgba(222, 233, 228, 0.04); }
  .overlay-shell.dark .session-name-editor button.primary { color: #f4faf7; background: #397b5c; }
  .overlay-shell.dark .update-card { border-color: rgba(190, 209, 200, 0.09); background: rgba(216, 229, 223, 0.035); }
  .overlay-shell.dark .terminal-picker-copy small { color: #9aaba3; }
  .overlay-shell.dark .project-name { color: #9aaba3; }
  .overlay-shell.dark .mobile-access-card { border-color: rgba(190, 209, 200, 0.09); background: rgba(216, 229, 223, 0.035); }
  .overlay-shell.dark .mobile-access-card strong { color: #dce7e1; }
  .overlay-shell.dark .mobile-access-card span,
  .overlay-shell.dark .mobile-access-card small { color: #aebdb5; }
  .overlay-shell.dark .mobile-apk code { color: #aebdb5; }
  .overlay-shell.dark .mobile-pairing { background: rgba(222, 233, 228, 0.04); }
  .overlay-shell.dark .mobile-access-card button,
  .overlay-shell.dark .paired-devices button { color: #b9c8c0; border-color: rgba(207, 223, 215, 0.12); }
  .overlay-shell.dark .paired-devices-intro strong,
  .overlay-shell.dark .paired-device-info strong,
  .overlay-shell.dark .permission-copy strong { color: #dce7e1; }
  .overlay-shell.dark .paired-device-info small,
  .overlay-shell.dark .permission-copy small { color: #aebdb5; }
  .overlay-shell.dark .paired-device-card { border-color: rgba(190, 209, 200, 0.09); background: rgba(216, 229, 223, 0.03); }
  .overlay-shell.dark .device-permissions,
  .overlay-shell.dark .device-permission { border-color: rgba(190, 209, 200, 0.08); }
  .overlay-shell.dark .device-permission.active { background: rgba(116, 191, 157, 0.035); }
  .overlay-shell.dark .permission-state { color: #91a098; }
  .overlay-shell.dark .permission-state.allowed { color: #91c7ae; }
  .overlay-shell.dark .preview-badge { color: #91c7ae; border-color: rgba(116, 191, 157, 0.16); background: rgba(92, 161, 130, 0.08); }
  .overlay-shell.dark .paired-devices .revoke-device { color: #d19a9a; border-color: rgba(209, 131, 131, 0.16); }
  .overlay-shell.dark .diagnostic-card { border-color: rgba(190, 209, 200, 0.09); background: rgba(216, 229, 223, 0.035); }
  .overlay-shell.dark .diagnostic-check strong { color: #dce7e1; }
  .overlay-shell.dark .diagnostic-check small { color: #aebdb5; }
  .overlay-shell.dark .empty-state strong { color: #c5d0cb; }
  .overlay-shell.dark code,
  .overlay-shell.dark .segmented { color: #bdc8c3; background: rgba(216, 229, 223, 0.06); }
  .overlay-shell.dark .permission-block > strong { color: #e2d0bd; }
  .overlay-shell.dark .question-block { border-color: rgba(83, 165, 204, 0.2); background: rgba(55, 139, 178, 0.07); }
  .overlay-shell.dark .question-block section > strong { color: #d4e2dc; }
  .overlay-shell.dark .question-block section > small { color: #8fa59b; }
  .overlay-shell.dark .question-actions button { color: #c5d7cf; border-color: rgba(178, 210, 224, 0.12); background: rgba(219, 235, 228, 0.045); }
  .overlay-shell.dark .permission-actions button,
  .overlay-shell.dark .shortcut-input,
  .overlay-shell.dark .inline-composer textarea { color: #c5d0cb; border-color: rgba(207, 223, 215, 0.12); background: rgba(222, 233, 228, 0.04); }
  .overlay-shell.dark .inline-queue-tray { color: #a7bdcd; border-color: rgba(125, 166, 199, 0.13); background: rgba(91, 143, 184, 0.065); }
  .overlay-shell.dark .inline-queue-tray:hover:not(:disabled) { border-color: rgba(128, 177, 216, 0.23); background: rgba(91, 143, 184, 0.1); }
  .overlay-shell.dark .inline-queue-tray.read-only:hover { border-color: rgba(125, 166, 199, 0.13); background: rgba(91, 143, 184, 0.065); }
  .overlay-shell.dark .inline-queue-tray .queue-mark { color: #87b8dc; background: rgba(105, 166, 210, 0.11); }
  .overlay-shell.dark .inline-queue-tray .queue-copy small,
  .overlay-shell.dark .inline-queue-tray .queue-shortcut { color: #829daa; }
  .overlay-shell.dark .inline-queue-tray .queue-copy strong { color: #b1c6d2; }
  .overlay-shell.dark .inline-queue-tray .queue-shortcut kbd { color: #9bb8c9; border-color: rgba(169, 197, 214, 0.14); background: rgba(220, 235, 243, 0.055); }
  .overlay-shell.dark .inline-attachments > span { border-color: rgba(207, 223, 215, 0.12); background: rgba(222, 233, 228, 0.04); }
  .overlay-shell.dark .final-response { color: #cad9d0; border-color: rgba(203, 221, 212, .1); background: rgba(210, 230, 220, .035); }
  .overlay-shell.dark .final-response-copy { color: #98aaa1; }
  .overlay-shell.dark .final-response-copy:hover { color: #d1ded7; background: rgba(222, 233, 228, 0.07); }
  .overlay-shell.dark .source-label { color: #9daca5; background: rgba(205, 222, 213, 0.08); }
  .overlay-shell.dark .access-badge.auto-review { color: #b4d3ee; background: #29445d; }
  .overlay-shell.dark .access-badge.full-access { color: #e4b88f; background: #543b29; }
  .overlay-shell.dark .terminal-picker-row,
  .overlay-shell.dark .workflow-missing-sessions { border-color: rgba(190, 209, 200, 0.09); }
  .overlay-shell.dark .terminal-picker-row > button { color: #b7c4be; border-color: rgba(207, 223, 215, 0.12); background: rgba(222, 233, 228, 0.04); }
  .overlay-shell.dark .terminal-picker-row > button:hover:not(:disabled) { background: rgba(222, 233, 228, 0.09); }












  .overlay-shell.dark .workflow-settings-popover { color: #bdcbc4; border-color: rgba(202, 220, 211, 0.12); background: #18221d; box-shadow: 0 18px 44px rgba(0, 0, 0, 0.38); }
  .overlay-shell.dark .workflow-settings-scroll > header strong { color: #d9e5df; }
  .overlay-shell.dark .workflow-settings-scroll > header small,
  .overlay-shell.dark .workflow-setting-grid label,
  .overlay-shell.dark .workflow-reserve-setting { color: #91a39a; }
  .overlay-shell.dark .workflow-setting-grid input { color: #cfddd5; border-color: rgba(205, 222, 213, 0.12); background: rgba(220, 235, 227, 0.045); }
  .overlay-shell.dark .workflow-setting-toggle { color: #b4c4bc; }
  .overlay-shell.dark .workflow-setting-toggle i { background: #46534d; }
  .overlay-shell.dark .workflow-missing-sessions { border-color: rgba(205, 222, 213, 0.1); }
  .overlay-shell.dark .workflow-missing-sessions label > span { color: #9fb0a7; }
  .overlay-shell.dark .permission-actions button:hover { background: rgba(222, 233, 228, 0.09); }
  .overlay-shell.dark .segmented button.active { color: #dfe8e3; background: rgba(214, 229, 221, 0.1); }
  .overlay-shell.dark .command-palette { border-color: rgba(207, 223, 215, 0.13); background: rgba(27, 34, 31, 0.985); }
  .overlay-shell.dark .shortcut-editor { border-color: rgba(207, 223, 215, 0.13); background: rgba(27, 34, 31, 0.99); }
  .overlay-shell.dark .shortcut-editor > strong { color: #dce7e1; }
  .overlay-shell.dark .shortcut-editor > small { color: #91a198; }
  .overlay-shell.dark .shortcut-capture { color: #a9d5be; border-color: rgba(139, 195, 166, 0.22); background: rgba(116, 181, 147, 0.07); }
  .overlay-shell.dark .shortcut-editor-actions button { color: #bdcbc4; border-color: rgba(207, 223, 215, 0.12); }
  .overlay-shell.dark .command-search { border-color: rgba(207, 223, 215, 0.09); }
  .overlay-shell.dark .command-search input,
  .overlay-shell.dark .command-results strong { color: #dce7e1; }
  .overlay-shell.dark .command-results > button.active { background: rgba(213, 229, 221, 0.07); }

  .overlay-shell.dark .plugin-actions button,
  .overlay-shell.dark .profile-action,
  .overlay-shell.dark .agent-preferences button { color: #bdcbc4; border-color: rgba(207, 223, 215, 0.12); background: rgba(222, 233, 228, 0.04); }
  .overlay-shell .switch input:checked + span,
  .overlay-shell .volume-control input,
  .overlay-shell .workflow-setting-toggle.active i { background: var(--lume-accent); accent-color: var(--lume-accent); }
  .overlay-shell .session-action-button:hover:not(:disabled),
  .overlay-shell .session-action-button.active { color: var(--lume-accent-strong); }
  .overlay-shell:not(.dark)[data-appearance] .panel-content,
  .overlay-shell:not(.dark)[data-appearance] .session-list,
  .overlay-shell:not(.dark)[data-appearance] .settings,
  .overlay-shell:not(.dark)[data-appearance] .whiteboard { background: color-mix(in srgb, var(--lume-sidebar-light) 78%, var(--lume-surface-light)); }
  .overlay-shell:not(.dark)[data-appearance] .lume-orb { color: var(--lume-accent-strong); --orb-border: color-mix(in srgb, var(--lume-accent-strong) 30%, transparent); --orb-fill: var(--lume-raised-light); }
  .overlay-shell.dark[data-appearance] .lume-orb { color: var(--lume-accent); --orb-border: color-mix(in srgb, var(--lume-accent) 27%, transparent); --orb-fill: var(--lume-raised-dark); }
  .overlay-shell[data-appearance] .lume-orb:hover { --orb-border: color-mix(in srgb, var(--lume-accent) 52%, transparent); }
  .overlay-shell[data-appearance] .agent-count { background: var(--lume-accent-strong); }
  .overlay-shell.dark[data-appearance] .panel { background: var(--lume-raised-dark); }
  .overlay-shell.dark .appearance-theme-setting strong { color: #e3ebe7; }
  .overlay-shell.dark .appearance-theme-setting small { color: #91a198; }
</style>
