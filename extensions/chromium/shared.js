(function exposeLumeWebShared(scope) {
  const providerForHost = (host) => {
    const normalized = String(host || "").toLowerCase();
    if (normalized === "claude.ai" || normalized.endsWith(".claude.ai")) return "claude";
    if (normalized === "chat.deepseek.com" || normalized.endsWith(".chat.deepseek.com")) return "deepseek";
    if (normalized === "gemini.google.com" || normalized.endsWith(".gemini.google.com")) return "gemini";
    if (
      normalized === "chatgpt.com"
      || normalized.endsWith(".chatgpt.com")
      || normalized === "chat.openai.com"
      || normalized.endsWith(".chat.openai.com")
    ) return "chatgpt";
    return null;
  };

  const eventForTab = (event, tabId) => {
    if (!event || !Number.isInteger(tabId)) return event;
    return {
      ...event,
      sessionId: `${event.sessionId}.${tabId}`,
    };
  };

  const promptAckRetryState = (submitted, confirmed) => ({
    pending: confirmed !== true,
    submitted: submitted === true && confirmed !== true,
  });

  const normalizePromptText = (value) => String(value ?? "")
    .normalize("NFKC")
    .replace(/\s+/g, " ")
    .trim();

  const promptWasAccepted = (beforeMessages, afterMessages, prompt) => {
    const expected = normalizePromptText(prompt);
    if (!expected) return false;

    const before = (Array.isArray(beforeMessages) ? beforeMessages : [])
      .map(normalizePromptText)
      .filter(Boolean);
    const after = (Array.isArray(afterMessages) ? afterMessages : [])
      .map(normalizePromptText)
      .filter(Boolean);
    const matches = (message) => message.includes(expected);
    const beforeMatches = before.filter(matches).length;
    const afterMatches = after.filter(matches).length;

    if (afterMatches > beforeMatches) return true;
    const latest = after.at(-1) ?? "";
    const previousLatest = before.at(-1) ?? "";
    return matches(latest) && !matches(previousLatest);
  };

  const resolveWebSessionState = ({
    previousState,
    permissionRequired = false,
    hasStopControl = false,
    failed = false,
    stopMissingSince = 0,
    now = Date.now(),
    graceMs = 3_000,
  }) => {
    if (permissionRequired) {
      return { state: "permission_required", stopMissingSince: 0, recheckAfterMs: 0 };
    }
    if (hasStopControl) {
      return { state: "running", stopMissingSince: 0, recheckAfterMs: 0 };
    }
    if (failed) {
      return { state: "failed", stopMissingSince: 0, recheckAfterMs: 0 };
    }
    if (previousState === "running") {
      const startedAt = stopMissingSince || now;
      const elapsed = Math.max(0, now - startedAt);
      if (elapsed < graceMs) {
        return {
          state: "running",
          stopMissingSince: startedAt,
          recheckAfterMs: graceMs - elapsed,
        };
      }
      return { state: "completed", stopMissingSince: 0, recheckAfterMs: 0 };
    }
    if (previousState === "completed") {
      return { state: "completed", stopMissingSince: 0, recheckAfterMs: 0 };
    }
    if (previousState === "failed") {
      return { state: "failed", stopMissingSince: 0, recheckAfterMs: 0 };
    }
    return { state: "waiting_for_input", stopMissingSince: 0, recheckAfterMs: 0 };
  };

  scope.LumeWebShared = Object.freeze({
    providerForHost,
    eventForTab,
    promptAckRetryState,
    promptWasAccepted,
    resolveWebSessionState,
  });
})(globalThis);
