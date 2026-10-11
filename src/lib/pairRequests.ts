import type { PendingPairRequest } from "./domain";
import type { SystemBannerItem } from "./SystemBannerStack.svelte";

export const PAIR_REQUEST_EVENT = "lume://node-pair-request";

/** Long enough to read and act on, short enough not to linger after the request expires. */
const MAX_BANNER_MS = 45_000;

export function spacedPairCode(code: string): string {
  return code.length === 6 ? `${code.slice(0, 3)} ${code.slice(3)}` : code;
}

export function pairRequestBanner(
  request: PendingPairRequest,
  language: "en" | "pt-BR",
  now: number,
  onDismiss: () => void,
): SystemBannerItem {
  const code = spacedPairCode(request.code);
  const message = language === "pt-BR"
    ? `${request.displayName} quer ver os agentes deste computador. Código ${code}. Confirme em Configurações → Computadores remotos.`
    : `${request.displayName} wants to see this computer's agents. Code ${code}. Confirm in Settings → Remote computers.`;
  return {
    id: `pair-request:${request.requestId}`,
    message,
    tone: "warning",
    duration: Math.max(8_000, Math.min(MAX_BANNER_MS, request.expiresAt - now)),
    onDismiss,
  };
}
