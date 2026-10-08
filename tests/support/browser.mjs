// Finds a Chromium for the browser tests: LUME_TEST_BROWSER, the Puppeteer cache or the runner's Chrome.
import assert from "node:assert/strict";
import { existsSync, readdirSync, mkdtempSync } from "node:fs";
import { join } from "node:path";
import { homedir, tmpdir } from "node:os";

const cache = join(homedir(), ".cache/puppeteer/chrome-headless-shell");
const puppeteerPlatform = process.platform === "win32" ? "win64" : process.platform === "darwin" ? (process.arch === "arm64" ? "mac-arm64" : "mac-x64") : "linux64";
const puppeteerExecutable = process.platform === "win32" ? "chrome-headless-shell.exe" : "chrome-headless-shell";
const cached = existsSync(cache)
  ? readdirSync(cache).sort().reverse().map((version) => join(cache, version, `chrome-headless-shell-${puppeteerPlatform}`, puppeteerExecutable))
  : [];
const browserCandidates = [
  process.env.LUME_TEST_BROWSER,
  process.env.CHROME_BIN,
  process.env.CHROME_PATH,
  process.env.CHROMIUM_BIN,
  process.env.PUPPETEER_EXECUTABLE_PATH,
  ...cached,
  ...(process.platform === "win32"
    ? [
        join(process.env.ProgramFiles || "C:\\Program Files", "Google/Chrome/Application/chrome.exe"),
        join(process.env["ProgramFiles(x86)"] || "C:\\Program Files (x86)", "Google/Chrome/Application/chrome.exe"),
        join(process.env.LOCALAPPDATA || "", "Google/Chrome/Application/chrome.exe"),
      ]
    : process.platform === "darwin"
      ? [
          "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
          "/Applications/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
          join(homedir(), "Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
        ]
      : [
          "/usr/bin/google-chrome",
          "/usr/bin/google-chrome-stable",
          "/opt/google/chrome/chrome",
          "/usr/bin/chromium",
          "/usr/bin/chromium-browser",
          "/snap/bin/chromium",
        ]),
];
export const executable = browserCandidates.find((candidate) => candidate && existsSync(candidate));
assert.ok(
  executable,
  `Chromium not found. Set LUME_TEST_BROWSER to its executable. Searched: ${browserCandidates.filter(Boolean).join(", ")}`,
);

/** Arguments that start the browser headless with a throwaway profile (needed for remote debugging on CI). */
export function browserArguments() {
  return ["--headless=new", "--no-sandbox", "--disable-dev-shm-usage", "--disable-gpu", "--no-first-run", "--disable-background-networking", `--user-data-dir=${mkdtempSync(join(tmpdir(), "lume-ui-profile-"))}`, "--remote-debugging-port=0", "about:blank"];
}
