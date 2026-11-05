import { listAgentExtensions, type ExtensionItem } from "$lib/lume";

export type McpPluginIcon = { url: string | null; darkUrl: string | null };

const normalize = (value: string) => value.toLowerCase().replace(/[^a-z0-9]/g, "");
const TTL = 5 * 60_000;
let cache: { at: number; items: Promise<ExtensionItem[]> } | null = null;

function installed() {
  if (!cache || Date.now() - cache.at > TTL) {
    const items = listAgentExtensions()
      .then((agents) => agents.flatMap((agent) => agent.items).filter((item) => item.iconUrl || item.iconDarkUrl))
      .catch(() => [] as ExtensionItem[]);
    cache = { at: Date.now(), items };
  }
  return cache.items;
}

/** The icon of the installed plugin behind an MCP server, when the server is named after one. */
export async function mcpPluginIcon(server: string): Promise<McpPluginIcon | null> {
  const name = normalize(server);
  const found = (await installed()).find((item) => {
    const plugin = normalize(item.name);
    return plugin.length >= 3 && name.includes(plugin);
  });
  return found ? { url: found.iconUrl ?? null, darkUrl: found.iconDarkUrl ?? null } : null;
}
