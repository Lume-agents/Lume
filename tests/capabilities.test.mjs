import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { renderSafeMarkdown } from "../src/lib/markdown.js";

const capability = JSON.parse(readFileSync(new URL("../src-tauri/capabilities/default.json", import.meta.url), "utf8"));
const identifiers = capability.permissions.map((permission) => (typeof permission === "string" ? permission : permission.identifier));

// Autostart, notifications and global shortcuts are only driven from Rust, which the
// capability gate never checks. Granting them would only widen what the webview can do.
for (const plugin of ["autostart", "notification", "global-shortcut"]) {
  assert.deepEqual(identifiers.filter((identifier) => identifier.startsWith(`${plugin}:`)), [], `${plugin} must stay out of the webview`);
}

// The bundled URL sets ship tel: and make the scope drift away from what the app opens.
assert.equal(identifiers.includes("opener:default"), false);
assert.equal(identifiers.includes("opener:allow-default-urls"), false);

// allow-open-url without a scope rejects every URL (the plugin allows nothing by default).
const openUrl = capability.permissions.filter((permission) => typeof permission === "object" && permission.identifier === "opener:allow-open-url");
assert.equal(openUrl.length, 1, "opener:allow-open-url must be declared once, with a URL scope");
assert.equal(identifiers.filter((identifier) => identifier === "opener:allow-open-url").length, 1);
const allowedUrls = (openUrl[0].allow ?? []).map((entry) => entry.url);
assert.ok(allowedUrls.length > 0, "opener:allow-open-url needs at least one allowed URL");
assert.deepEqual(openUrl[0].deny ?? [], []);

/** @param {string} url */
const scopeAllows = (url) => allowedUrls.some((pattern) => {
  assert.ok(pattern.endsWith("*") && !pattern.slice(0, -1).includes("*"), `unsupported URL pattern ${pattern}`);
  return url.startsWith(pattern.slice(0, -1));
});
/** @param {string} url */
const markdownLinks = (url) => renderSafeMarkdown(`[link](${url})`).includes("<a href=");

// Every link the sanitizer renders must open, and the scope must not allow anything it drops.
const probes = [
  "https://github.com/tulerws/Lume",
  "http://localhost:1420/",
  "mailto:someone@example.com",
  "tel:+5511999999999",
  "javascript:alert(1)",
  "file:///etc/passwd",
  "data:text/html,hi",
  "ftp://example.com/file",
  "vscode://file/tmp/x",
];
for (const url of probes) {
  assert.equal(scopeAllows(url), markdownLinks(url), `opener scope and markdown sanitizer disagree on ${url}`);
}
assert.deepEqual(probes.filter(scopeAllows), ["https://github.com/tulerws/Lume", "http://localhost:1420/", "mailto:someone@example.com"]);

console.log("capabilities test suite passed");
