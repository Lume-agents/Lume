import assert from "node:assert/strict";
import { macosAutomationMessage } from "../src/lib/macosAutomation.ts";

assert.equal(
  macosAutomationMessage("Error: MACOS_AUTOMATION_REQUIRED:Permita que o Lume controle o Terminal."),
  "Permita que o Lume controle o Terminal.",
);
assert.equal(
  macosAutomationMessage(new Error("MACOS_AUTOMATION_REQUIRED: Allow Lume to control Terminal.")),
  "Allow Lume to control Terminal.",
);
assert.equal(macosAutomationMessage("O macOS pediu permissão para o Lume controlar o Terminal. Responda ao pedido e tente de novo."), null);
assert.equal(macosAutomationMessage("AGENT_CONNECTION_REQUIRED:Claude Code não está conectado."), null);
console.log("macOS automation test suite passed");
