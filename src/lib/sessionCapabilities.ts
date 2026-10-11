import type { AgentSession, PromptDelivery } from "$lib/domain";

export type PromptUnavailableReason =
  | "unsupported_agent"
  | "session_not_connected"
  | "working_directory_missing"
  | "agent_busy"
  | "external_session"
  | "monitoring_only";

export interface SessionCapabilities {
  canPrompt: boolean;
  promptUnavailableReason?: PromptUnavailableReason;
  canApprove: boolean;
  canAnswerQuestion: boolean;
  canTerminate: boolean;
  canOpenSource: boolean;
  canReadResults: boolean;
  canAttachImages: boolean;
  canInterrupt: boolean;
  canTakeControl: boolean;
  promptDeliveries: PromptDelivery[];
}

/** A Claude conversation Lume owns: each message is its own run, so they can queue. */
function claudeQueuesInLume(session: AgentSession) {
  return session.agent === "claude_code"
    && session.controlOrigin === "lume"
    && ["cli", "desktop"].includes(session.source);
}

export function sessionCapabilities(session: AgentSession): SessionCapabilities {
  let promptUnavailableReason: PromptUnavailableReason | undefined;
  const promptIsRunning = ["running", "permission_required"].includes(session.status);
  const legacyGeminiCli = session.source !== "web" && session.agent === "gemini";
  const controlledOmp = session.agent === "omp" && session.controlOrigin === "lume";
  const monitoredOmp = session.agent === "omp" && !controlledOmp;
  if (legacyGeminiCli || monitoredOmp) {
    promptUnavailableReason = "monitoring_only";
  } else if (controlledOmp) {
    if (!session.nativeSessionId?.trim()) promptUnavailableReason = "session_not_connected";
    else if (!session.workingDirectory?.trim()) promptUnavailableReason = "working_directory_missing";
  } else if (
    session.source === "web"
    && promptIsRunning
  ) {
    promptUnavailableReason = "agent_busy";
  } else if (session.source !== "web") {
    if (session.agent === "unknown") {
      promptUnavailableReason = "unsupported_agent";
    } else if (!session.nativeSessionId?.trim()) {
      promptUnavailableReason = "session_not_connected";
    } else if (
      session.controlOrigin === "external"
      && ["codex", "claude_code"].includes(session.agent)
    ) {
      promptUnavailableReason = "external_session";
    } else if (session.agent !== "codex" && !session.workingDirectory?.trim()) {
      promptUnavailableReason = "working_directory_missing";
    }
    if (
      !promptUnavailableReason
      && promptIsRunning
      && !(session.agent === "codex" && session.controlOrigin === "lume")
      && !claudeQueuesInLume(session)
    ) {
      promptUnavailableReason = "agent_busy";
    }
  }

  return {
    canPrompt: !promptUnavailableReason,
    promptUnavailableReason,
    canApprove: !monitoredOmp && Boolean(
      session.pendingPermission && session.permissionProfile.canRespondFromLume,
    ),
    canAnswerQuestion: !monitoredOmp && Boolean(session.pendingQuestion),
    canTerminate:
      (!legacyGeminiCli && session.source === "cli" && Boolean(session.processId))
      || (
        ["codex", "opencode", "antigravity", "claude_code", "omp"].includes(session.agent)
        && (session.source === "desktop" || (session.source === "cli" && !session.processId))
        && session.controlOrigin === "lume"
        && Boolean(session.nativeSessionId?.trim())
      ),
    canOpenSource: session.source === "web" || session.source === "vscode",
    canReadResults: session.results.length > 0 || Boolean(session.lastResponse),
    canAttachImages: session.source !== "web" && session.agent !== "unknown" && !monitoredOmp,
    canInterrupt:
      ["running", "permission_required"].includes(session.status)
      && session.controlOrigin === "lume"
      && (controlledOmp || ["codex", "claude_code"].includes(session.agent))
      && (session.agent === "codex" ? session.source !== "web" : ["cli", "desktop"].includes(session.source))
      && Boolean(session.nativeSessionId?.trim()),
    canTakeControl:
      session.controlOrigin === "external"
      && session.source === "cli"
      && ["codex", "omp"].includes(session.agent)
      && Boolean(session.nativeSessionId?.trim())
      && Boolean(session.workingDirectory?.trim())
      && (session.agent === "omp" || Boolean(session.processId)),
    promptDeliveries:
      legacyGeminiCli || monitoredOmp
        ? []
        : controlledOmp
          ? ["new_turn", "steer", "queue"]
          : session.agent === "codex"
      && session.source !== "web"
      && session.controlOrigin === "lume"
        ? ["new_turn", "steer", "queue"]
        // For Claude, "steer" is send now: stop the running message and send the queued one.
        : claudeQueuesInLume(session)
          ? ["new_turn", "queue", "steer"]
          : ["new_turn"],
  };
}
