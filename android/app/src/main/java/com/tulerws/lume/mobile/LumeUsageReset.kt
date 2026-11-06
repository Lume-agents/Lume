package com.tulerws.lume.mobile

internal data class LumeUsageResetNotice(val id: String, val body: String)

internal object LumeUsageReset {
    fun notice(type: String, id: String, occurredAt: Long, agent: String, window: String, remaining: Int, language: String, now: Long): LumeUsageResetNotice? {
        if (type != "usage_reset" || id.isBlank() || id.length > 128 || occurredAt <= 0) return null
        if (now - occurredAt > 300_000 || occurredAt - now > 120_000) return null
        val portuguese = language.lowercase().startsWith("pt")
        val agentName = when (agent) {
            "codex" -> "Codex"
            "claude_code" -> "Claude Code"
            "antigravity" -> "Antigravity"
            "gemini" -> "Gemini"
            "opencode" -> "OpenCode"
            "deepseek" -> "DeepSeek"
            "chatgpt" -> "ChatGPT"
            "claude" -> "Claude"
            else -> if (portuguese) "agente" else "agent"
        }
        val label = window.replace(Regex("\\s+"), " ").trim().take(80)
            .ifBlank { if (portuguese) "atual" else "current" }
        val body = if (portuguese) {
            if (remaining == 0) "A janela $label do $agentName reiniciou, mas o limite já está esgotado."
            else "O limite $label do $agentName foi renovado."
        } else {
            if (remaining == 0) "$agentName's $label window reset, but its limit is already exhausted."
            else "$agentName's $label usage limit has reset."
        }
        return LumeUsageResetNotice(id, body)
    }
}
