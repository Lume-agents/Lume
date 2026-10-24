import type { CatalogPlugin } from "./lume";

export type CatalogSort = "popular" | "downloads" | "stars" | "name" | "recent" | "rating";

export function hasCatalogMetric(value: number | null | undefined): value is number {
  return typeof value === "number" && Number.isFinite(value) && value >= 0;
}

/** Installs/downloads and repository stars are distinct sort dimensions, never combined. */
export function sortExtensionCatalog(items: CatalogPlugin[], order: CatalogSort, locale = "en") {
  const names = new Intl.Collator(locale, { numeric: true, sensitivity: "base" });
  const metric = (item: CatalogPlugin) => {
    const value = order === "popular" ? (item.installs ?? item.downloads)
      : order === "downloads" ? item.downloads
      : order === "stars" ? item.githubStars
      : order === "recent" ? item.updatedAt : item.rating;
    return hasCatalogMetric(value) ? value : null;
  };
  return [...items].sort((left, right) => {
    if (order !== "name") {
      const a = metric(left), b = metric(right);
      if (a === null && b !== null) return 1;
      if (a !== null && b === null) return -1;
      if (a !== null && b !== null && a !== b) return b - a;
    }
    return names.compare(left.name, right.name) || names.compare(left.id, right.id);
  });
}

/** Describes the catalog source, not the plugin's author or a security review. No name heuristics. */
export function isOfficialMarketplace(agent: string, marketplace?: string | null): boolean {
  return Boolean(marketplace && ({
    claude: "claude-plugins-official",
    codex: "openai-curated-remote",
    antigravity: "antigravity-plugins-official",
  } as Record<string, string>)[agent] === marketplace);
}

export function safePluginIconUrl(value?: string | null): string | null {
  // Only the native, bounded cache reader emits image data URLs; never permit HTML/file URLs.
  if (value?.startsWith("data:") && value.length <= 140_000) {
    return /^data:image\/(?:png|jpeg|webp|gif|avif|svg\+xml);base64,[A-Za-z0-9+/]+={0,2}$/.test(value) ? value : null;
  }
  if (!value || value.length > 4096) return null;
  try {
    const url = new URL(value);
    if (url.protocol !== "https:" || url.username || url.password) return null;
    const host = url.hostname.replace(/^\[|\]$/g, "").replace(/\.$/, "");
    if (!host.includes(".") && !host.includes(":")) return null;
    if (host === "localhost" || /\.(?:localhost|local|internal)$/.test(host)
      || /^(?:0\.|10\.|127\.|169\.254\.|192\.168\.|172\.(?:1[6-9]|2\d|3[01])\.)/.test(host)
      || (host.includes(":") && /^(?:::|fc|fd|fe[89ab])/i.test(host))) return null;
    return url.href;
  } catch { return null; }
}
