// LUME_OMP_EXTENSION_OWNER=lume version=1
// Installed by Lume. Forwards Oh My Pi lifecycle and approval events to Lume for observation only.
// It never blocks, rewrites, or answers tool calls; when Lume is unavailable every send is silently dropped.
import { spawn } from "node:child_process";
import type { ExtensionAPI, ExtensionContext } from "@oh-my-pi/pi-coding-agent";

const LUME_EXECUTABLE = "__LUME_EXECUTABLE__";
const SEND_TIMEOUT_MS = 2000;

type LumeEvent = "running" | "completed" | "permission_request" | "session_ended";

function send(payload: string): void {
  try {
    const child = spawn(LUME_EXECUTABLE, ["ingest"], { stdio: ["pipe", "ignore", "ignore"], windowsHide: true });
    child.on("error", () => {});
    child.stdin?.on("error", () => {});
    const timer = setTimeout(() => child.kill(), SEND_TIMEOUT_MS);
    timer.unref?.();
    child.on("exit", () => clearTimeout(timer));
    child.stdin?.end(payload);
    child.unref();
  } catch {
    // Lume is optional for omp; never surface delivery failures.
  }
}

export default function lumeExtension(pi: ExtensionAPI) {
  // Sessions started by Lume already report through the RPC bridge.
  if (process.env.LUME_CONTROLLED === "1") return;

  const forward = (event: LumeEvent, ctx: ExtensionContext, toolName?: string, toolCallId?: string) => {
    // Only the interactive main session is a Lume session; subagents and print runs have no UI.
    if (!ctx.hasUI) return;
    const nativeId = ctx.sessionManager.getSessionId();
    const profile = (ctx.sessionManager.getSessionFile() ?? "").match(/[\\/]profiles[\\/]([^\\/]+)[\\/]agent[\\/]/)?.[1];
    if (!nativeId) return;
    const permission = event === "permission_request"
      ? {
        id: toolCallId || crypto.randomUUID(),
        kind: "tool",
        summary: toolName || "Tool approval",
        resource: toolName || "tool",
        risk: "unknown",
        requestedAt: new Date().toISOString(),
      }
      : undefined;
    send(JSON.stringify({
      event,
      sessionId: `omp:${nativeId}`,
      agent: "omp",
      agentLabel: profile ? `Oh My Pi · ${profile}` : "Oh My Pi",
      source: "cli",
      controlOrigin: "external",
      nativeSessionId: nativeId,
      workingDirectory: ctx.cwd,
      project: ctx.cwd.split(/[\\/]/).filter(Boolean).pop(),
      permissionProfile: permission
        ? {
          mode: "custom",
          label: "Oh My Pi",
          approvalPolicy: "omp",
          canRespondFromLume: false,
          availableActions: ["open_source"],
        }
        : undefined,
      permission,
    }));
  };

  pi.on("agent_start", (_event, ctx) => forward("running", ctx));
  pi.on("agent_end", (_event, ctx) => forward("completed", ctx));
  pi.on("tool_approval_requested", (event, ctx) => forward("permission_request", ctx, event.toolName, event.toolCallId));
  pi.on("tool_approval_resolved", (_event, ctx) => forward("running", ctx));
  pi.on("session_shutdown", (_event, ctx) => forward("session_ended", ctx));
}
