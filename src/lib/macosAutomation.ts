const marker = "MACOS_AUTOMATION_REQUIRED:";

export function macosAutomationMessage(error: unknown): string | null {
  const message = String(error).replace(/^Error:\s*/, "");
  if (!message.includes(marker)) return null;
  return message.slice(message.indexOf(marker) + marker.length).trim();
}
