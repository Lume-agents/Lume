# Agent sources

How Lume detects and controls each agent. The [README](../README.md) has the short version.

## Supported sources

| Source | Detection | Direct permission actions |
| --- | --- | --- |
| External Claude Code CLI | Processes and hooks | Monitoring; explicit transfer resumes the same conversation through Lume |
| External Codex CLI and VS Code | Processes, rollouts, and hooks | Monitoring; explicit transfer resumes the same thread through Lume |
| Codex sessions controlled by Lume | Local App Server | Prompts, queue, steer, approvals, model, and reasoning effort |
| Claude Code sessions controlled by Lume | Official Claude CLI and hooks | Prompts, supported permission actions, model, and reasoning effort |
| Antigravity CLI | `agy` processes and CLI-only hooks | Hook activity/status and resume of the latest indexed conversation per workspace; the full transcript stays in the CLI; wildcard `PreToolUse` automatically allows all tools, including when Lume is unavailable; disable/remove the hook to restore native approvals; prompt interruption and queue/steer are not exposed |
| Oh My Pi (`omp`) | CLI processes/session files; controlled sessions use omp RPC | Controlled prompts, steer/queue, interrupt, model/thinking, approval decisions (allow once/deny), questions and compaction; external sessions can be taken over only after the terminal omp is closed; no plan mode |
| Gemini CLI (legacy enterprise/API use) | Process detection | Monitoring only; Lume does not send prompts, resume, terminate it, or install hooks in Gemini's shared settings |
| ChatGPT, Claude, DeepSeek, and Gemini web | Chromium Companion | Prompts while the page is waiting for input, status, final response, and opening the matching tab; confirmed prompt delivery requires Companion protocol v2; queue/steer while running is not supported |

Lume only displays actions supported by the current session. It never simulates an approval that the source integration cannot perform. The session hub shows only observable information exposed by the agent, App Server, or hooks. Private model reasoning is never available.

Google is transitioning individual Gemini CLI users to Antigravity CLI. Lume therefore treats Antigravity as a separate agent instead of renaming Gemini: `agy` uses its own lifecycle events and conversation store, while the legacy Gemini integration remains available for enterprise, Google Cloud, and API-key workflows. Antigravity hooks are installed in the CLI-only `~/.gemini/antigravity-cli/settings.json`, not the shared `~/.gemini/config/hooks.json`. Enabling the integration installs a wildcard `PreToolUse` hook that automatically allows every Antigravity CLI tool call; Lume warns about this before activation. Its fail-open wrapper also returns `allow` if the Lume executable is missing or the hook command fails, so native approval prompts do not return until the hook is disabled or removed from Antigravity CLI settings. The hook is CLI-only and does not configure Antigravity IDE or Gemini Code Assist. On startup, Lume removes its old shared Antigravity hook entry, preserves unrelated hooks, and saves a backup. Hooks from older Lume versions are left disconnected until the user reconnects and confirms automatic tool approval; restart already-open Antigravity sessions to reload the updated configuration. Gemini Code Assist for VS Code is a separate integration surface: Lume does not claim direct control of its chats or configure its tools. The legacy Gemini CLI uses `~/.gemini/settings.json`, which Google also documents for Gemini Code Assist tool/MCP configuration, so Lume now monitors that CLI by process only and removes only its own old hook entries from the shared file (with a backup), preserving user tools, MCP servers, and third-party hooks. Restart VS Code and open Gemini sessions after migration.

Process detection works independently of hooks. Antigravity hook delivery on Windows and WSL still has open upstream reliability reports, so those environments may show the `agy` session before live tool events begin arriving.

## Connect agent sources

After connecting Codex for the first time, run `/hooks` inside Codex and trust the **Lume** hook. Codex requires this confirmation for new or modified local hooks.
For Oh My Pi, process and session-file monitoring works without connecting the integration. **Settings → Oh My Pi → Connect** installs Lume's optional extension for live status. Sessions can be opened in Lume's controlled RPC mode or resumed in a terminal with `omp --resume`; profile sessions are preserved. Lume-controlled sessions support approvals with **Allow once** or **Deny**, never persistent approval, and have no plan mode. External sessions can be taken over only after the terminal omp is closed.
Oh My Pi transcripts remain in omp's JSONL files. Lume reads them into memory while monitoring and does not store a second transcript copy in its database.


To install the browser Companion:

1. Open **Settings → Browsers → Open folder** in Lume.
2. Visit `chrome://extensions` or the equivalent page in Edge or Brave.
3. Enable developer mode.
4. Load the Companion folder as an unpacked extension.

The Companion distinguishes ChatGPT, Claude, DeepSeek, and Gemini. It sends the agent type, state, sanitized title, source, a local tab identifier, and the final response displayed by the selected chat. Prompts submitted through Lume travel only over the local connection to the selected tab, and conversation content is not persisted in Lume history.
