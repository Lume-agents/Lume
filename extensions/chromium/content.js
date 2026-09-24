(() => {
  const host = location.hostname;
  const provider = globalThis.LumeWebShared?.providerForHost(host);
  if (!provider) return;
  let lastState = "";
  let lastPath = "";
  let lastResponseSignature = "";
  let submittingPromptId = "";
  let unconfirmedPromptId = "";
  let unconfirmedPromptIdentity = null;
  let unconfirmedPromptSubmitted = false;
  let unconfirmedPromptText = "";
  let unconfirmedPromptBaselineMessages = [];
  let acknowledgingPromptId = "";
  let retryUnconfirmedPromptAt = 0;
  let promptRetryDelay = 3_000;
  let promptRetryTimer;
  let runningStopMissingSince = 0;
  let runningStateRecheckTimer;
  let timer;

  const visible = (element) => {
    const rect = element.getBoundingClientRect();
    const style = getComputedStyle(element);
    return rect.width > 0 && rect.height > 0 && style.visibility !== "hidden";
  };

  const buttonText = () =>
    [...document.querySelectorAll("button")]
      .filter(visible)
      .slice(-80)
      .map((button) => `${button.textContent ?? ""} ${button.getAttribute("aria-label") ?? ""}`.trim().toLowerCase());

  const detectState = (previousState = lastState) => {
    const buttons = buttonText();
    const permissionDialog = [...document.querySelectorAll('[role="dialog"], [data-state="open"]')]
      .filter(visible)
      .some((dialog) => {
        const text = dialog.textContent?.toLowerCase() ?? "";
        const actions = [...dialog.querySelectorAll("button")]
          .filter(visible)
          .map((button) => `${button.textContent ?? ""} ${button.getAttribute("aria-label") ?? ""}`.trim().toLowerCase());
        const explicitPermission =
          /permission required|approval required|allow once|run command|permissão necessária|aprovação necessária|permitir uma vez|executar comando/.test(text);
        const hasAllow = actions.some((action) =>
          /^(allow|approve|permitir|aprovar|aceitar)( once| this time)?$/.test(action),
        );
        const hasDeny = actions.some((action) =>
          /^(deny|decline|reject|recusar|negar)$/.test(action),
        );
        return explicitPermission || (hasAllow && hasDeny);
      });
    const runningSelectors = [
      'button[data-testid*="stop"]',
      'button[aria-label*="Stop"]',
      'button[aria-label*="Parar"]',
      '[data-testid="stop-button"]',
    ];
    const hasStopControl =
      runningSelectors.some((selector) => [...document.querySelectorAll(selector)].some(visible)) ||
      buttons.some((text) => text === "stop" || text === "parar");

    const alerts = [...document.querySelectorAll('[role="alert"]')]
      .filter(visible)
      .map((alert) => alert.textContent?.toLowerCase() ?? "")
      .join(" ");
    const result = globalThis.LumeWebShared.resolveWebSessionState({
      previousState,
      permissionRequired: permissionDialog,
      hasStopControl,
      failed: /failed|something went wrong|erro|falhou/.test(alerts),
      stopMissingSince: runningStopMissingSince,
      graceMs: 3_000,
    });
    runningStopMissingSince = result.stopMissingSince;
    if (result.recheckAfterMs > 0) {
      if (!runningStateRecheckTimer) {
        runningStateRecheckTimer = setTimeout(() => {
          runningStateRecheckTimer = null;
          report(true);
        }, result.recheckAfterMs);
      }
    } else if (runningStateRecheckTimer) {
      clearTimeout(runningStateRecheckTimer);
      runningStateRecheckTimer = null;
    }
    return result.state;
  };

  const hash = (value) => {
    let result = 2166136261;
    for (const char of value) {
      result ^= char.charCodeAt(0);
      result = Math.imul(result, 16777619);
    }
    return (result >>> 0).toString(36);
  };

  const cleanTitle = () =>
    document.title
      .replace(/\s*[|·-]\s*(ChatGPT|Claude|DeepSeek|Gemini).*$/i, "")
      .trim()
      .slice(0, 100) || "Sessão web";

  const finalResponse = () => {
    const selectors = provider === "chatgpt"
      ? ['[data-message-author-role="assistant"]']
      : provider === "claude"
        ? ['[data-testid="assistant-message"]', '.font-claude-response']
        : provider === "deepseek"
          ? ['[data-role="assistant"] .ds-markdown', '.ds-markdown', '[class*="ds-markdown"]']
          : ['model-response .markdown', 'model-response', '.model-response-text'];
    const candidates = selectors.flatMap((selector) => [...document.querySelectorAll(selector)]);
    const response = candidates
      .filter((element) => element.textContent?.trim())
      .at(-1)
      ?.textContent?.trim();
    return response?.slice(0, 32768);
  };

  const visibleUserMessages = (composer = null) => {
    const selectors = {
      chatgpt: ['[data-message-author-role="user"]'],
      claude: ['[data-testid*="user-message"]'],
      deepseek: ['[data-role="user"]', '[class*="user-message"]'],
      gemini: ["user-query", '[data-testid*="user-query"]'],
    }[provider] ?? [];
    const root = document.querySelector("main, [role='main']") ?? document;
    const candidates = new Set(
      selectors.flatMap((selector) => [...root.querySelectorAll(selector)]),
    );
    const topLevelMessages = [...candidates].filter((element) =>
      ![...candidates].some((other) =>
        other !== element && other.contains(element),
      )
      && (!composer || (
        element !== composer
        && !element.contains(composer)
        && !composer.contains(element)
      ))
      && visible(element),
    );

    return topLevelMessages
      .map((element) => (element.innerText ?? element.textContent ?? "").trim())
      .filter(Boolean)
      .slice(-20);
  };

  const submitPrompt = async (text) => {
    const baselineMessages = visibleUserMessages();
    const candidates = [
      ...document.querySelectorAll(
        'textarea, [contenteditable="true"][role="textbox"], [contenteditable="true"].ProseMirror, [contenteditable="true"][data-lexical-editor="true"]',
      ),
    ].filter((element) => visible(element) && !element.disabled);
    const composer = candidates.at(-1);
    if (!composer) {
      return { accepted: false, uncertain: false, baselineMessages };
    }

    composer.focus();
    if (composer instanceof HTMLTextAreaElement || composer instanceof HTMLInputElement) {
      const prototype = composer instanceof HTMLTextAreaElement
        ? HTMLTextAreaElement.prototype
        : HTMLInputElement.prototype;
      const setter = Object.getOwnPropertyDescriptor(prototype, "value")?.set;
      setter?.call(composer, text);
    } else {
      composer.textContent = text;
    }
    composer.dispatchEvent(
      new InputEvent("input", { bubbles: true, inputType: "insertText", data: text }),
    );
    composer.dispatchEvent(new Event("change", { bubbles: true }));
    await new Promise((resolve) => setTimeout(resolve, 90));

    const composerText = () => composer instanceof HTMLTextAreaElement || composer instanceof HTMLInputElement
      ? composer.value
      : composer.innerText ?? composer.textContent ?? "";
    const waitForSubmission = async () => {
      let composerChanged = false;
      for (let attempt = 0; attempt < 15; attempt += 1) {
        await new Promise((resolve) => setTimeout(resolve, 100));
        if (globalThis.LumeWebShared.promptWasAccepted(
          baselineMessages,
          visibleUserMessages(composer),
          text,
        )) {
          return { accepted: true, uncertain: false, baselineMessages };
        }
        composerChanged ||= !composer.isConnected || composerText().trim() !== text.trim();
      }
      const state = detectState(lastState);
      return {
        accepted: false,
        uncertain: composerChanged || state === "running" || state === "permission_required",
        baselineMessages,
      };
    };

    const scope = composer.closest("form") ?? document;
    const sendButton = [...scope.querySelectorAll("button")]
      .filter((button) => visible(button) && !button.disabled)
      .find((button) => {
        const label = `${button.textContent ?? ""} ${button.getAttribute("aria-label") ?? ""} ${button.dataset.testid ?? ""}`.toLowerCase();
        return /(^|\s)(send|enviar|submit|enviar mensagem|send message)(\s|$)/.test(label);
      });
    if (sendButton) {
      sendButton.click();
      return waitForSubmission();
    }
    if (scope instanceof HTMLFormElement) {
      scope.requestSubmit();
      return waitForSubmission();
    }
    composer.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", code: "Enter", bubbles: true }),
    );
    return waitForSubmission();
  };

  const acknowledgePrompt = async (promptId, submitted, identity = null) => {
    const response = await chrome.runtime.sendMessage({
      type: "lume:prompt-ack",
      promptId,
      submitted,
      provider: identity?.provider,
      sessionId: identity?.sessionId,
    }).catch(() => null);
    return response?.ok === true && response.confirmed === true;
  };

  const clearUnconfirmedPrompt = () => {
    unconfirmedPromptId = "";
    unconfirmedPromptIdentity = null;
    unconfirmedPromptSubmitted = false;
    unconfirmedPromptText = "";
    unconfirmedPromptBaselineMessages = [];
    retryUnconfirmedPromptAt = 0;
    promptRetryDelay = 3_000;
    if (promptRetryTimer) clearTimeout(promptRetryTimer);
    promptRetryTimer = null;
  };

  const schedulePromptRetry = () => {
    if (promptRetryTimer) clearTimeout(promptRetryTimer);
    retryUnconfirmedPromptAt = Date.now() + promptRetryDelay;
    promptRetryTimer = setTimeout(() => {
      promptRetryTimer = null;
      report(true);
    }, promptRetryDelay);
    promptRetryDelay = Math.min(promptRetryDelay * 2, 30_000);
  };

  const confirmSubmittedPrompt = async (promptId) => {
    if (acknowledgingPromptId === promptId) return;
    if (!globalThis.LumeWebShared.promptWasAccepted(
      unconfirmedPromptBaselineMessages,
      visibleUserMessages(),
      unconfirmedPromptText,
    )) {
      schedulePromptRetry();
      return;
    }
    acknowledgingPromptId = promptId;
    try {
      const confirmed = await acknowledgePrompt(promptId, true, unconfirmedPromptIdentity);
      if (unconfirmedPromptId !== promptId) return;
      const retryState = globalThis.LumeWebShared.promptAckRetryState(true, confirmed);
      if (!retryState.pending) clearUnconfirmedPrompt();
      else schedulePromptRetry();
    } finally {
      if (acknowledgingPromptId === promptId) acknowledgingPromptId = "";
    }
  };

  const report = (force = false) => {
    if (unconfirmedPromptSubmitted) {
      if (Date.now() >= retryUnconfirmedPromptAt) {
        void confirmSubmittedPrompt(unconfirmedPromptId);
      }
    }
    const path = location.pathname;
    const state = detectState(path === lastPath ? lastState : "");
    const lastResponse = state === "completed" ? finalResponse() : undefined;
    const responseSignature = lastResponse ? hash(lastResponse) : "";
    if (!force && state === lastState && path === lastPath && responseSignature === lastResponseSignature) return;
    lastState = state;
    lastPath = path;
    lastResponseSignature = responseSignature;
    void chrome.runtime.sendMessage({
      type: "lume:event",
      event: {
        provider,
        protocolVersion: 2,
        sessionId: hash(`${provider}:${path}`),
        title: cleanTitle(),
        origin: location.origin,
        state,
        lastResponse,
      },
    }).then(async (response) => {
      if (!response?.prompt || response.promptId === submittingPromptId) return;
      if (
        response.promptId === unconfirmedPromptId
        && (unconfirmedPromptSubmitted || Date.now() < retryUnconfirmedPromptAt)
      ) return;
      submittingPromptId = response.promptId || "";
      const promptIdentity = { provider: response.provider, sessionId: response.sessionId };
      const submission = await submitPrompt(response.prompt);
      if (response.promptId) {
        const confirmed = submission.accepted
          ? await acknowledgePrompt(response.promptId, true, promptIdentity)
          : (await acknowledgePrompt(response.promptId, false, promptIdentity), false);
        const retryState = globalThis.LumeWebShared.promptAckRetryState(
          submission.accepted || submission.uncertain,
          confirmed,
        );
        if (!retryState.pending) {
          clearUnconfirmedPrompt();
        } else {
          unconfirmedPromptId = response.promptId;
          unconfirmedPromptIdentity = promptIdentity;
          unconfirmedPromptText = response.prompt;
          unconfirmedPromptBaselineMessages = submission.baselineMessages;
          unconfirmedPromptSubmitted = retryState.submitted;
          schedulePromptRetry();
        }
      }
      submittingPromptId = "";
    }).catch(() => {});
  };

  const schedule = () => {
    clearTimeout(timer);
    timer = setTimeout(report, 450);
  };
  new MutationObserver(schedule).observe(document.documentElement, {
    childList: true,
    subtree: true,
    attributes: true,
    attributeFilter: ["aria-label", "data-state", "disabled"],
  });
  window.addEventListener("popstate", schedule);
  window.addEventListener("focus", () => report(true));
  window.addEventListener("pageshow", () => report(true));
  document.addEventListener("visibilitychange", () => report(true));
  setInterval(() => report(true), 15_000);
  schedule();
})();
