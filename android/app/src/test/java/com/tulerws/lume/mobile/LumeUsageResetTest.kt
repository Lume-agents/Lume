package com.tulerws.lume.mobile

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class LumeUsageResetTest {
    private val now = 1_800_000_000_000L

    @Test fun localizedWindowAndAgentAreShown() {
        val notice = LumeUsageReset.notice("usage_reset", "reset-1", now, "claude_code", "5h", 100, "pt", now)
        assertEquals("O limite 5h do Claude Code foi renovado.", notice?.body)
        assertEquals("Codex's 7d usage limit has reset.", LumeUsageReset.notice("usage_reset", "reset-2", now, "codex", "7d", 100, "en", now)?.body)
        assertEquals("A janela 5h do Claude Code reiniciou, mas o limite já está esgotado.", LumeUsageReset.notice("usage_reset", "reset-3", now, "claude_code", "5h", 0, "pt", now)?.body)
    }

    @Test fun ignoresStaleInvalidAndUnrelatedEvents() {
        assertNull(LumeUsageReset.notice("sessions_changed", "reset-1", now, "codex", "5h", 100, "pt", now))
        assertNull(LumeUsageReset.notice("usage_reset", "", now, "codex", "5h", 100, "pt", now))
        assertNull(LumeUsageReset.notice("usage_reset", "reset-1", now - 300_001, "codex", "5h", 100, "pt", now))
    }
}
