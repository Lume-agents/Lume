// @ts-expect-error TypeScript's bundler mode disallows the explicit source extension used by Node.
import { markedErrorMessage } from "./agentConnection.ts";

export function macosAutomationMessage(error: unknown): string | null {
  return markedErrorMessage(error, "MACOS_AUTOMATION_REQUIRED:");
}
