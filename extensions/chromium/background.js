const endpoint = "http://127.0.0.1:43120";
importScripts("shared.js");
const browserName = (async () => {
  if (/Edg\//.test(navigator.userAgent)) return "edge";
  if (navigator.brave?.isBrave && (await navigator.brave.isBrave())) return "brave";
  return "chrome";
})();
const tabSessions = new Map();
const pendingPromptEvents = new Map();

const forwardEvent = (event) =>
  browserName.then((browser) => fetch(`${endpoint}/events`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ ...event, browser }),
  }));

const acknowledgePrompt = (event, promptId, submitted) =>
  browserName.then((browser) => fetch(`${endpoint}/prompt-ack`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      provider: event.provider,
      sessionId: event.sessionId,
      promptId,
      submitted,
      browser,
    }),
  }));

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message?.type === "lume:event") {
    const sourceEvent = message.event;
    const event = globalThis.LumeWebShared.eventForTab(sourceEvent, sender.tab?.id);
    if (Number.isInteger(sender.tab?.id)) {
      const previous = tabSessions.get(sender.tab.id);
      if (previous && previous.sessionId !== event.sessionId) {
        void forwardEvent({
          ...previous,
          state: "closed",
          lastResponse: undefined,
        }).catch(() => {});
        for (const [promptId, pending] of pendingPromptEvents) {
          if (pending.event.sessionId === previous.sessionId) pendingPromptEvents.delete(promptId);
        }
      }
      tabSessions.set(sender.tab.id, event);
    }
    forwardEvent(event)
      .then(async (response) => {
        const result = await response.json().catch(() => ({ ok: response.ok }));
        if (result.focus && sender.tab?.id) {
          await chrome.tabs.update(sender.tab.id, { active: true });
          if (Number.isInteger(sender.tab.windowId)) {
            await chrome.windows.update(sender.tab.windowId, { focused: true });
          }
        }
        const promptId = typeof result.promptId === "string" ? result.promptId : null;
        const prompt = typeof result.prompt === "string" ? result.prompt : null;
        if (promptId && prompt) {
          const pending = pendingPromptEvents.get(promptId);
          if (pending) pending.event = event;
          else pendingPromptEvents.set(promptId, { event, submitted: false });
        }
        const pending = promptId ? pendingPromptEvents.get(promptId) : null;
        let deliveredPrompt = prompt;
        let deliveredPromptId = promptId;
        if (pending?.submitted && promptId) {
          const ackResponse = await acknowledgePrompt(pending.event, promptId, true).catch(() => null);
          const ackResult = await ackResponse?.json().catch(() => ({ confirmed: false }));
          if (ackResult?.confirmed) pendingPromptEvents.delete(promptId);
          deliveredPrompt = null;
          deliveredPromptId = null;
        }
        sendResponse({
          ok: response.ok,
          focus: Boolean(result.focus),
          prompt: deliveredPrompt,
          promptId: deliveredPromptId,
          provider: sourceEvent.provider,
          sessionId: sourceEvent.sessionId,
        });
      })
      .catch(() => sendResponse({ ok: false }));
    return true;
  }
  if (message?.type === "lume:prompt-ack") {
    const pending = typeof message.promptId === "string"
      ? pendingPromptEvents.get(message.promptId)
      : null;
    const currentEvent = Number.isInteger(sender.tab?.id) ? tabSessions.get(sender.tab.id) : null;
    const suppliedEvent =
      typeof message.provider === "string" && typeof message.sessionId === "string"
        ? globalThis.LumeWebShared.eventForTab(
          { provider: message.provider, sessionId: message.sessionId },
          sender.tab?.id,
        )
        : null;
    const event = pending?.event ?? suppliedEvent ?? currentEvent;
    if (!event || typeof message.promptId !== "string") {
      sendResponse({ ok: false });
      return false;
    }
    if (pending) pending.submitted = Boolean(message.submitted);
    acknowledgePrompt(event, message.promptId, Boolean(message.submitted))
      .then(async (response) => {
        const result = await response.json().catch(() => ({ ok: false, confirmed: false }));
        if (result.confirmed) pendingPromptEvents.delete(message.promptId);
        else if (pending && !message.submitted) pending.submitted = false;
        sendResponse({
          ok: response.ok && result.ok === true,
          confirmed: result.confirmed === true,
        });
      })
      .catch(() => sendResponse({ ok: false }));
    return true;
  }
  if (message?.type === "lume:health") {
    fetch(`${endpoint}/health`)
      .then((response) => sendResponse({ ok: response.ok }))
      .catch(() => sendResponse({ ok: false }));
    return true;
  }
  return false;
});

chrome.tabs.onRemoved.addListener((tabId) => {
  const event = tabSessions.get(tabId);
  if (!event) return;
  tabSessions.delete(tabId);
  void forwardEvent({
    ...event,
    state: "closed",
    lastResponse: undefined,
  }).catch(() => {});
  for (const [promptId, pending] of pendingPromptEvents) {
    if (pending.event.sessionId === event.sessionId) pendingPromptEvents.delete(promptId);
  }
});
