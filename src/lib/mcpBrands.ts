import { siAsana, siAtlassian, siCloudflare, siConfluence, siDiscord, siDocker, siFigma, siGithub, siGitlab, siGmail, siGooglecalendar, siGoogledrive, siJira, siLinear, siMongodb, siNotion, siPostgresql, siRedis, siSentry, siStripe, siSupabase, siTrello, siVercel } from "simple-icons";

export type McpBrand = { path: string; color: string | null };

// Brands whose color reads on both surfaces keep it; the near-black ones follow the text color (null).
const brands: Array<[string, { path: string }, string | null]> = [
  ["jira", siJira, "#2684FF"], ["confluence", siConfluence, "#2684FF"], ["atlassian", siAtlassian, "#2684FF"],
  ["github", siGithub, null], ["gitlab", siGitlab, "#FC6D26"], ["figma", siFigma, "#F24E1E"], ["notion", siNotion, null],
  ["linear", siLinear, "#5E6AD2"], ["sentry", siSentry, null], ["stripe", siStripe, "#635BFF"], ["vercel", siVercel, null],
  ["supabase", siSupabase, "#3FCF8E"], ["cloudflare", siCloudflare, "#F38020"], ["postgres", siPostgresql, "#4169E1"],
  ["drive", siGoogledrive, "#4285F4"], ["gmail", siGmail, "#EA4335"], ["calendar", siGooglecalendar, "#4285F4"],
  ["discord", siDiscord, "#5865F2"], ["asana", siAsana, "#F06A6A"], ["trello", siTrello, "#2684FF"], ["docker", siDocker, "#2496ED"],
  ["mongo", siMongodb, "#47A248"], ["redis", siRedis, "#FF4438"],
];

/** The icon of a server Lume knows by name; the product it answered as (Jira) wins over the server (Atlassian). */
export function mcpBrand(server: string, label = ""): McpBrand | null {
  const names = [label, server].map((name) => name.toLowerCase());
  for (const name of names) {
    const found = brands.find(([key]) => name.includes(key));
    if (found) return { path: found[1].path, color: found[2] };
  }
  return null;
}
