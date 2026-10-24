import assert from "node:assert/strict";
import { isOfficialMarketplace, safePluginIconUrl, sortExtensionCatalog } from "../src/lib/extensionCatalog.ts";

const plugin = (id, name, metrics = {}) => ({ id, name, description: "", marketplace: "community", installed: false, ...metrics });
const items = [
  plugin("unknown", "Unknown"),
  plugin("z", "Zulu", { installs: 0, downloads: 999, updatedAt: Date.UTC(2025, 0, 1), rating: 4.9 }),
  plugin("a", "Alpha", { installs: 30, downloads: 50, updatedAt: Date.UTC(2026, 0, 1), rating: 4.2 }),
  plugin("b", "Beta", { downloads: 40 }),
];
const original = structuredClone(items);
const ids = (order) => sortExtensionCatalog(items, order).map((item) => item.id);
assert.deepEqual(ids("popular"), ["b", "a", "z", "unknown"], "missing installs fall back to downloads, but zero installs remain a reported zero");
assert.deepEqual(ids("downloads"), ["z", "a", "b", "unknown"]);
assert.deepEqual(ids("name"), ["a", "b", "unknown", "z"]);
assert.deepEqual(ids("recent"), ["a", "z", "b", "unknown"]);
assert.deepEqual(ids("rating"), ["z", "a", "b", "unknown"]);
assert.deepEqual(items, original, "sorting must not mutate cached catalogs");
const starred = [plugin("unknown", "A"), plugin("zero", "Z", { githubStars: 0 }), plugin("popular", "P", { githubStars: 1200, installs: 1 }), plugin("installed", "I", { installs: 100, githubStars: 2 })];
assert.deepEqual(sortExtensionCatalog(starred, "stars").map((p) => p.id), ["popular", "installed", "zero", "unknown"], "repository stars sort separately, with a known zero before unavailable counts");
assert.deepEqual(sortExtensionCatalog(starred, "popular").map((p) => p.id), ["installed", "popular", "unknown", "zero"], "stars must not be mistaken for install counts");
assert.deepEqual(sortExtensionCatalog([plugin("u", "A"), plugin("zero", "Z", { rating: 0 })], "rating").map((p) => p.id), ["zero", "u"]);

assert.equal(isOfficialMarketplace("claude", "claude-plugins-official"), true);
assert.equal(isOfficialMarketplace("codex", "openai-curated-remote"), true);
assert.equal(isOfficialMarketplace("antigravity", "antigravity-plugins-official"), true);
assert.equal(isOfficialMarketplace("codex", "claude-plugins-official"), false);
assert.equal(isOfficialMarketplace("claude", "community-official"), false);
assert.equal(isOfficialMarketplace("codex", "openai-curated-remote-unofficial"), false);

assert.equal(safePluginIconUrl("https://www.gstatic.com/images/logo.svg"), "https://www.gstatic.com/images/logo.svg");
assert.equal(safePluginIconUrl("https://fctheme.example/logo.png"), "https://fctheme.example/logo.png");
assert.equal(safePluginIconUrl("data:image/png;base64,aGVsbG8="), "data:image/png;base64,aGVsbG8=");
assert.equal(safePluginIconUrl("data:text/html;base64,aGVsbG8="), null);
assert.equal(safePluginIconUrl(`data:image/png;base64,${"A".repeat(140_000)}`), null);
for (const url of ["file:///home/user/.env", "javascript:alert(1)", "http://example.com/logo.png", "https://localhost/icon", "https://127.0.0.1/icon", "https://2130706433/icon", "https://[::1]/icon", "https://192.168.1.2/icon", "https://service.local/icon", "https://user:password@example.com/icon"]) {
  assert.equal(safePluginIconUrl(url), null, url);
}
console.log("Catalog ordering, official source badges and image URL guards passed.");
