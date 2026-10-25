// Pairing links: the QR code shown on the desktop, a lume:// deep link, or an Android intent:// fallback.

export type PairingTarget = {
  gateway: string;
  code: string | null;
  token: string | null;
  deviceId: string | null;
  desktopId: string | null;
};

export function parsePairingTarget(rawValue: string | null | undefined): PairingTarget | null {
  let value = String(rawValue || "").trim();
  if (value.startsWith("intent://")) {
    value = `lume://${value.slice("intent://".length).split("#Intent;", 1)[0]}`;
  }
  try {
    const url = new URL(value);
    const fragment = new URLSearchParams(url.hash.slice(1));
    const read = (key: string) => url.searchParams.get(key) || fragment.get(key);
    const gateway = read("gateway") || (url.protocol === "lume:" ? null : url.origin);
    const code = read("code");
    const token = read("token");
    const deviceId = read("deviceId");
    if (!/^https?:\/\//.test(gateway || "") || (!code && !(token && deviceId))) return null;
    return { gateway: gateway!.replace(/\/+$/, ""), code, token, deviceId, desktopId: read("desktopId") };
  } catch {
    return null;
  }
}

/** The link that opens the installed Android app on the same pairing, falling back to this page. */
export function pairingIntentUrl(target: PairingTarget) {
  const query = new URLSearchParams({ gateway: target.gateway });
  if (target.code) query.set("code", target.code);
  if (target.token && target.deviceId) {
    query.set("token", target.token);
    query.set("deviceId", target.deviceId);
  }
  if (target.desktopId) query.set("desktopId", target.desktopId);
  const fallback = encodeURIComponent(location.href);
  return `intent://pair?${query.toString()}#Intent;scheme=lume;package=com.tulerws.lume.mobile;S.browser_fallback_url=${fallback};end`;
}

export function suggestedDeviceName() {
  if (/iPad/i.test(navigator.userAgent)) return "iPad";
  if (/iPhone/i.test(navigator.userAgent)) return "iPhone";
  if (/Android/i.test(navigator.userAgent)) {
    // The model is in the user agent ("Android 15; Pixel 9"); newer browsers report a bare "K" instead.
    const model = /Android[^;)]*;\s*([^;)]+?)(?:\s+Build\/[^;)]*)?[;)]/i.exec(navigator.userAgent)?.[1]?.trim();
    return model && model !== "K" && !/^(Linux|U)$/i.test(model) ? model : "Android phone";
  }
  return "My phone";
}
