import { renderSafeMarkdown } from "./markdown.js";

const inlineReference = /`[^`\n]*`|(!?)\[([^\]\n]*)\]\((<[^>\n]+>|[^)\n]+)\)|\(([^()\n]+)\)/g;
const tokenPattern = /\uE100LUMEFILE(\d+)\uE101/g;
/** @type {Record<string, string>} */
const htmlEscapes = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#039;" };

/** @param {string} value */
function escapeHtml(value) {
  return value.replace(/[&<>"']/g, (character) => htmlEscapes[character]);
}

/** @param {string} raw @param {boolean} parenthesized */
function localFileReference(raw, parenthesized) {
  const target = raw.trim()
    .replace(/\s+["'][^"']*["']\s*$/, "")
    .replace(/^<|>$/g, "")
    .replace(/^file:\/\//i, "")
    .replace(/%20/g, " ");
  if (!target || (/^(?:[a-z][a-z\d+.-]*:|#)/i.test(target) && !/^[a-z]:[\\/]/i.test(target))) return null;
  if (parenthesized && !/[\\/]/.test(target)) return null;

  const path = target.replace(/:\d+(?::\d+)?$/, "");
  const name = path.split(/[\\/]/).pop() ?? "";
  if (!/\.[a-z\d]{1,10}$/i.test(name)) return null;
  const lower = path.replace(/\\/g, "/").toLowerCase();
  if (/(?:^|\/)(?:\.ssh|\.gnupg|\.aws)(?:\/|$)/.test(lower)
    || /^(?:\.env(?:\..*)?|id_rsa|id_ed25519|\.netrc|\.npmrc|\.pypirc)$/i.test(name)
    || /\.(?:pem|key|p12|pfx)$/i.test(name)) return null;
  return { path: target, name };
}

/** @param {string} path @param {string | undefined} workingDirectory */
function openablePath(path, workingDirectory) {
  const withoutLine = path.replace(/:\d+(?::\d+)?$/, "");
  if (/^(?:\/|\\\\|[a-z]:[\\/])/i.test(withoutLine)) return withoutLine;
  if (!workingDirectory) return "";
  const separator = workingDirectory.includes("\\") ? "\\" : "/";
  return `${workingDirectory.replace(/[\\/]+$/, "")}${separator}${withoutLine.replace(/[\\/]/g, separator).replace(/^\.\//, "")}`;
}

/**
 * Decorate local file references after safe Markdown rendering so paths never
 * enter the DOM as unescaped HTML. The icon renderer only receives static
 * local icon assets; text and titles are escaped here.
 * @param {string} source
 * @param {(path: string) => string} renderIcon
 * @param {string} [workingDirectory]
 */
export function renderWorkspaceMarkdownWithFileBadges(source, renderIcon, workingDirectory) {
  /** @type {string[]} */
  const badges = [];
  let inFence = false;
  const marked = String(source ?? "").split("\n").map((line) => {
    if (/^\s*```/.test(line)) {
      inFence = !inFence;
      return line;
    }
    if (inFence) return line;
    return line.replace(inlineReference, (match, image, _label, markdownPath, parenthesizedPath) => {
      if (match.startsWith("`") || image === "!") return match;
      if (badges.length >= 16) return match;
      const reference = localFileReference(markdownPath ?? parenthesizedPath, parenthesizedPath !== undefined);
      if (!reference) return match;
      const path = openablePath(reference.path, workingDirectory);
      const tag = path ? "button" : "span";
      const action = path ? ` type="button" data-local-file="${escapeHtml(path)}"` : "";
      const badge = `<${tag} class="inline-file-badge"${action} title="${escapeHtml(reference.path)}">${renderIcon(reference.name)}<span class="inline-file-name">${escapeHtml(reference.name)}</span></${tag}>`;
      const token = `\uE100LUMEFILE${badges.push(badge) - 1}\uE101`;
      return parenthesizedPath === undefined ? token : `(${token})`;
    });
  }).join("\n");

  return renderSafeMarkdown(marked).replace(tokenPattern, (_, index) => badges[Number(index)] ?? "");
}
