import assert from "node:assert/strict";
import { usageResetNotice } from "../mobile/src/usageReset.ts";

const now = 1_800_000_000_000;
const event = { type: "usage_reset", eventId: "reset-1", occurredAt: now, agent: "claude_code", windowLabel: "5h" };
assert.deepEqual(usageResetNotice(event, "pt-BR", now), { id: "reset-1", title: "Lume", body: "O limite 5h do Claude Code foi renovado." });
assert.match(usageResetNotice({ ...event, agent: "codex", windowLabel: "7d" }, "en", now).body, /Codex.*7d.*reset/);
assert.match(usageResetNotice({ ...event, remainingPercent: 0 }, "pt-BR", now).body, /reiniciou, mas o limite já está esgotado/);
assert.equal(usageResetNotice({ ...event, type: "sessions_changed" }, "pt-BR", now), null);
assert.equal(usageResetNotice({ ...event, occurredAt: now - 300_001 }, "pt-BR", now), null);
assert.equal(usageResetNotice({ ...event, eventId: "" }, "pt-BR", now), null);
console.log("Mobile usage reset notifications passed.");
