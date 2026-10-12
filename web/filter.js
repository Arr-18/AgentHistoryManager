/* AgentHistoryManager - message classifier / sanitiser
 *
 * Pure helpers used by the UI to decide which messages to show in the default
 * chat view, which are "internal" infrastructure records (system prompts,
 * environment context, reasoning, tool calls, token usage, lifecycle events),
 * and how to render markdown-lite code fences inside a chat bubble.
 *
 * Loaded both by the browser (UMD-style global: window.AHMFilter) and by the
 * Node smoke test (module.exports). No DOM, no globals other than what the
 * factory explicitly accepts.
 */
(function (root, factory) {
    "use strict";
    if (typeof module === "object" && module.exports) {
        module.exports = factory();
    } else {
        root.AHMFilter = factory();
    }
})(typeof self !== "undefined" ? self : this, function () {
    "use strict";

    /* Roles that are always infrastructure, never a user/assistant dialog turn. */
    var INTERNAL_ROLES = {
        system: true,
        event: true,
        tool: true,
        developer: true
    };

    /* Kinds that may legitimately appear under user/assistant roles but still
     * represent infrastructure rather than dialog (the model's chain of
     * thought, tool invocations, lifecycle metadata, etc.). */
    var INTERNAL_KINDS = {
        reasoning: true,
        function_call: true,
        function_call_output: true,
        tool_use: true,
        token_usage: true,
        session_meta: true,
        world_state: true,
        turn_context: true,
        ai_title: true,
        permission_mode: true,
        file_history_snapshot: true,
        last_prompt: true,
        // Codex uses kinds like "event:task_started"; "event:*" with anything
        // not in the dialog set is internal by definition.
        "event:task_started": true,
        "event:task_complete": true,
        "event:token_count": true,
        "event:item_completed": true,
        "event:thread_settings_applied": true,
        "system:turn_duration": true
    };

    function _role(m) { return (m && m.role) || ""; }
    function _kind(m) { return (m && m.kind) || ""; }

    /* True iff this message should be hidden in the default chat view. */
    function isInternalMessage(m) {
        if (!m) return true;
        var role = _role(m);
        var kind = _kind(m);
        if (INTERNAL_ROLES[role]) return true;
        if (INTERNAL_KINDS[kind]) return true;
        // Any role we do not explicitly recognise as dialog (user/assistant)
        // is treated as internal so unknown future parser output does not leak
        // into the chat view.
        if (role !== "user" && role !== "assistant") return true;
        return false;
    }

    /* True iff this message is a real dialog turn that should appear as a
     * chat bubble (user message / assistant message). Attachments are part
     * of the user turn so we keep them too. */
    function isChatMessage(m) {
        if (!m) return false;
        var role = _role(m);
        var kind = _kind(m);
        if (role !== "user" && role !== "assistant") return false;
        if (INTERNAL_KINDS[kind]) return false;
        if (kind === "message") return true;
        // Attachments ride along with the user turn and should not be hidden.
        if (role === "user" && kind === "attachment") return true;
        return false;
    }

    /* Strip Codex-style <environment_context>...</environment_context> blocks
     * from a string. Returns { text, stripped } where stripped is the raw
     * blocks that were removed (useful for diagnostics). */
    var ENV_RE = /<environment_context>[\s\S]*?<\/environment_context>/g;
    function stripEnvironmentContext(text) {
        if (!text || typeof text !== "string") {
            return { text: text || "", stripped: [] };
        }
        var stripped = [];
        var match;
        ENV_RE.lastIndex = 0;
        while ((match = ENV_RE.exec(text)) !== null) {
            stripped.push(match[0]);
        }
        var cleaned = text.replace(ENV_RE, "").replace(/^\s*\n/, "").trim();
        return { text: cleaned, stripped: stripped };
    }

    /* Convenience: text suitable for a chat bubble - the message text with
     * the environment_context preamble removed. */
    function cleanChatText(m) {
        if (!m) return "";
        var stripped = stripEnvironmentContext(m.text || "").text;
        if (stripped) return stripped;
        return m.text || "";
    }

    /* Detect fenced code blocks (```lang\n...\n```). Returns an array of
     * { lang, code, start, end } describing each block. */
    function findCodeFences(text) {
        if (!text || typeof text !== "string") return [];
        var re = /```([a-zA-Z0-9_+\-#.]*)\n([\s\S]*?)```/g;
        var out = [];
        var m;
        while ((m = re.exec(text)) !== null) {
            out.push({
                lang: m[1] || "",
                code: m[2].replace(/\n$/, ""),
                start: m.index,
                end: m.index + m[0].length
            });
        }
        return out;
    }

    function hasCodeFence(text) {
        if (!text || typeof text !== "string") return false;
        return /```[\s\S]*?```/.test(text);
    }

    /* Classify a list of messages against showInternal. Returns
     *   { visible: [...], hidden: [...] }
     * where visible is what the renderer should display and hidden is just
     * the count for the banner. Order is preserved. */
    function partition(messages, showInternal) {
        var visible = [];
        var hidden = [];
        for (var i = 0; i < messages.length; i++) {
            var m = messages[i];
            if (showInternal || isChatMessage(m)) {
                visible.push(m);
            } else if (isInternalMessage(m)) {
                hidden.push(m);
            } else {
                // Unknown shape - treat as internal so the chat view stays clean.
                hidden.push(m);
            }
        }
        return { visible: visible, hidden: hidden };
    }

    return {
        INTERNAL_ROLES: INTERNAL_ROLES,
        INTERNAL_KINDS: INTERNAL_KINDS,
        isInternalMessage: isInternalMessage,
        isChatMessage: isChatMessage,
        stripEnvironmentContext: stripEnvironmentContext,
        cleanChatText: cleanChatText,
        findCodeFences: findCodeFences,
        hasCodeFence: hasCodeFence,
        partition: partition
    };
});
