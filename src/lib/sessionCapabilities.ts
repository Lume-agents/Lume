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

export function sessionCapabilities(session: AgentSession): SessionCapabilities {
  let promptUnavailableReason: PromptUnavailableReason | undefined;
  const promptIsRunning = ["running", "permission_required"].includes(session.status);
  const legacyGeminiCli = session.source !== "web" && session.agent === "gemini";
  if (legacyGeminiCli) {
    promptUnavailableReason = "monitoring_only";
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
    ) {
      promptUnavailableReason = "agent_busy";
    }
  }

  return {
    canPrompt: !promptUnavailableReason,
    promptUnavailableReason,
    canApprove: Boolean(
      session.pendingPermission && session.permissionProfile.canRespondFromLume,
    ),
    canAnswerQuestion: Boolean(session.pendingQuestion),
    canTerminate:
      (!legacyGeminiCli && session.source === "cli" && Boolean(session.processId))
      || (
        session.agent === "codex"
        && session.source === "desktop"
        && session.controlOrigin === "lume"
        && Boolean(session.nativeSessionId?.trim())
      ),
    canOpenSource: session.source === "web" || session.source === "vscode",
    canReadResults: session.results.length > 0 || Boolean(session.lastResponse),
    canAttachImages: session.source !== "web" && session.agent !== "unknown",
    canInterrupt:
      ["running", "permission_required"].includes(session.status)
      && session.controlOrigin === "lume"
      && ["codex", "claude_code"].includes(session.agent)
      && (session.agent === "codex" ? session.source !== "web" : session.source === "cli")
      && Boolean(session.nativeSessionId?.trim()),
    canTakeControl:
      session.controlOrigin === "external"
      && session.source === "cli"
      && session.agent === "codex"
      && Boolean(session.nativeSessionId?.trim())
      && Boolean(session.workingDirectory?.trim())
      && Boolean(session.processId),
    promptDeliveries:
      legacyGeminiCli
        ? []
        : session.agent === "codex"
      && session.source !== "web"
      && session.controlOrigin === "lume"
        ? ["new_turn", "steer", "queue"]
        : ["new_turn"],
  };
}
