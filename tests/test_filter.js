/* Smoke test for web/filter.js.
 *
 * Run with:    node tests/test_filter.js
 *
 * Verifies that:
 *   - Internal records (system / event / tool / developer roles, and the
 *     known internal kinds) are classified correctly.
 *   - Chat records (user+message, assistant+message, user+attachment) are
 *     classified correctly.
 *   - <environment_context>...</environment_context> blocks are stripped
 *     from Codex user messages.
 *   - The partition helper hides everything we expect it to hide.
 *
 * Fixtures below mirror real shapes observed in this repo's Codex and
 * Claude Code history (see tests/test_ingest.py output). Both layouts are
 * exercised so a regression in either parser surfaces here.
 */
"use strict";

const path = require("path");
const Filter = require(path.resolve(__dirname, "..", "web", "filter.js"));

let pass = 0;
let fail = 0;

function eq(actual, expected, label) {
    const a = JSON.stringify(actual);
    const e = JSON.stringify(expected);
    if (a === e) {
        pass++;
        console.log("  ok  " + label);
    } else {
        fail++;
        console.log("  FAIL " + label);
        console.log("       expected: " + e);
        console.log("       actual:   " + a);
    }
}
function assert(cond, label) {
    if (cond) { pass++; console.log("  ok  " + label); }
    else      { fail++; console.log("  FAIL " + label); }
}

/* -------------------------------------------------------------------- */
/* Fixtures - taken straight from the parsers' observed output shapes.   */
/* -------------------------------------------------------------------- */

// Codex session_meta (role=system, kind=session_meta).
const codexSessionMeta = {
    ordinal: 0, role: "system", kind: "session_meta",
    text: "", content_json: "{}",
    raw_json: "{}",
};

// Codex event_msg (role=event, kind=event:task_started).
const codexEvent = {
    ordinal: 1, role: "event", kind: "event:task_started",
    text: "", content_json: "{}", raw_json: "{}",
};

// Codex world_state (role=system, kind=world_state).
const codexWorldState = {
    ordinal: 2, role: "system", kind: "world_state",
    text: "", content_json: "{}", raw_json: "{}",
};

// Codex turn_context (role=system, kind=turn_context).
const codexTurnContext = {
    ordinal: 3, role: "system", kind: "turn_context",
    text: "", content_json: "{}", raw_json: "{}",
};

// Codex token_usage (role=system, kind=token_usage).
const codexTokenUsage = {
    ordinal: 4, role: "system", kind: "token_usage",
    text: "", content_json: "{}", raw_json: "{}",
};

// Codex developer message (role=developer, kind=message) - this is the
// giant <skills_instructions> + <permissions> system prompt.
const codexDeveloper = {
    ordinal: 5, role: "developer", kind: "message",
    text: "<skills_instructions>\n## Skills\n...</skills_instructions>\n<permissions>...</permissions>",
    content_json: "{}", raw_json: "{}",
};

// Codex user message whose text starts with <environment_context>.
const codexUserEnvCtx = {
    ordinal: 6, role: "user", kind: "message",
    text: "<environment_context>\n  <cwd>D:\\NJU</cwd>\n  <shell>powershell</shell>\n  <current_date>2026-10-05</current_date>\n</environment_context>\n\n\u73b0\u5728 Web UI \u7684\u4f1a\u8bdd\u8be6\u60c5\u53ef\u8bfb\u6027\u6bd4\u8f83\u5dee...",
    content_json: "{}", raw_json: "{}",
};

// Codex assistant reasoning - chain of thought, should be hidden by default.
const codexReasoning = {
    ordinal: 7, role: "assistant", kind: "reasoning",
    text: "\u7528\u6237\u8ba9\u6211\u4f18\u5316 Web UI\u3002",
    content_json: "{}", raw_json: "{}",
};

// Codex assistant function_call - tool invocation.
const codexFunctionCall = {
    ordinal: 8, role: "assistant", kind: "function_call",
    text: "", content_json: "{}", raw_json: "{}",
};

// Codex tool function_call_output - tool result.
const codexToolResult = {
    ordinal: 9, role: "tool", kind: "function_call_output",
    text: "", content_json: "{}", raw_json: "{}",
};

// Codex assistant text reply.
const codexAssistantReply = {
    ordinal: 10, role: "assistant", kind: "message",
    text: "\u5148\u67e5\u770b\u73b0\u6709 web/app.js \u7684 renderDetailBody\u3002",
    content_json: "{}", raw_json: "{}",
};

// Codex user follow-up with code fence.
const codexUserWithCode = {
    ordinal: 11, role: "user", kind: "message",
    text: "\u8bf7\u4fee\u6539\u4ee5\u4e0b\u4ee3\u7801\uff1a\n```js\nfunction hi(){return 1}\n```",
    content_json: "{}", raw_json: "{}",
};

// Claude Code system turn_duration.
const claudeTurnDuration = {
    ordinal: 0, role: "system", kind: "system:turn_duration",
    text: "", content_json: "{}", raw_json: "{}",
};

// Claude Code system permission_mode.
const claudePermissionMode = {
    ordinal: 1, role: "system", kind: "permission_mode",
    text: "", content_json: "{}", raw_json: "{}",
};

// Claude Code system ai_title.
const claudeAiTitle = {
    ordinal: 2, role: "system", kind: "ai_title",
    text: "", content_json: "{}", raw_json: "{}",
};

// Claude Code system file_history_snapshot.
const claudeFileHistory = {
    ordinal: 3, role: "system", kind: "file_history_snapshot",
    text: "", content_json: "{}", raw_json: "{}",
};

// Claude Code system last_prompt.
const claudeLastPrompt = {
    ordinal: 4, role: "system", kind: "last_prompt",
    text: "", content_json: "{}", raw_json: "{}",
};

// Claude Code user message (plain text).
const claudeUser = {
    ordinal: 5, role: "user", kind: "message",
    text: "\u73b0\u5728\u5b66\u4e60 Rust\uff0c\u8bf7\u5e2e\u6211\u5b9e\u73b0 guessing_game\u3002",
    content_json: "{}", raw_json: "{}",
};

// Claude Code user attachment.
const claudeUserAttachment = {
    ordinal: 6, role: "user", kind: "attachment",
    text: "Cargo.toml", content_json: "{}", raw_json: "{}",
};

// Claude Code assistant tool_use (only tool blocks, no text).
const claudeAssistantToolUse = {
    ordinal: 7, role: "assistant", kind: "tool_use",
    text: "", content_json: "{}", raw_json: "{}",
};

// Claude Code assistant message with text + thinking blocks.
const claudeAssistantReply = {
    ordinal: 8, role: "assistant", kind: "message",
    text: "\u5148\u770b\u770b Cargo.toml\u3002",
    content_json: "{}", raw_json: "{}",
};

/* -------------------------------------------------------------------- */
/* Tests                                                                */
/* -------------------------------------------------------------------- */

console.log("\n# isInternalMessage / isChatMessage");

// Codex infrastructure records should all be internal.
assert(Filter.isInternalMessage(codexSessionMeta),   "codex system/session_meta -> internal");
assert(Filter.isInternalMessage(codexEvent),        "codex event/event:task_started -> internal");
assert(Filter.isInternalMessage(codexWorldState),   "codex system/world_state -> internal");
assert(Filter.isInternalMessage(codexTurnContext),  "codex system/turn_context -> internal");
assert(Filter.isInternalMessage(codexTokenUsage),   "codex system/token_usage -> internal");
assert(Filter.isInternalMessage(codexDeveloper),    "codex developer/message -> internal");
assert(Filter.isInternalMessage(codexReasoning),    "codex assistant/reasoning -> internal");
assert(Filter.isInternalMessage(codexFunctionCall), "codex assistant/function_call -> internal");
assert(Filter.isInternalMessage(codexToolResult),   "codex tool/function_call_output -> internal");

// Codex dialog records should be chat (not internal).
assert(!Filter.isInternalMessage(codexUserEnvCtx),     "codex user/message -> NOT internal");
assert(!Filter.isInternalMessage(codexAssistantReply), "codex assistant/message -> NOT internal");
assert(!Filter.isInternalMessage(codexUserWithCode),   "codex user/message with code -> NOT internal");

assert( Filter.isChatMessage(codexUserEnvCtx),     "codex user/message -> chat");
assert( Filter.isChatMessage(codexAssistantReply), "codex assistant/message -> chat");
assert( Filter.isChatMessage(codexUserWithCode),   "codex user/message with code -> chat");
assert(!Filter.isChatMessage(codexReasoning),      "codex assistant/reasoning -> NOT chat");
assert(!Filter.isChatMessage(codexToolResult),     "codex tool/function_call_output -> NOT chat");
assert(!Filter.isChatMessage(codexFunctionCall),   "codex assistant/function_call -> NOT chat");
assert(!Filter.isChatMessage(codexDeveloper),      "codex developer/message -> NOT chat");

// Claude Code infrastructure records should all be internal.
assert(Filter.isInternalMessage(claudeTurnDuration),    "claude system/system:turn_duration -> internal");
assert(Filter.isInternalMessage(claudePermissionMode),  "claude system/permission_mode -> internal");
assert(Filter.isInternalMessage(claudeAiTitle),         "claude system/ai_title -> internal");
assert(Filter.isInternalMessage(claudeFileHistory),     "claude system/file_history_snapshot -> internal");
assert(Filter.isInternalMessage(claudeLastPrompt),      "claude system/last_prompt -> internal");
assert(Filter.isInternalMessage(claudeAssistantToolUse),"claude assistant/tool_use -> internal");

// Claude Code dialog records should be chat.
assert(Filter.isChatMessage(claudeUser),               "claude user/message -> chat");
assert(Filter.isChatMessage(claudeUserAttachment),     "claude user/attachment -> chat");
assert(Filter.isChatMessage(claudeAssistantReply),     "claude assistant/message -> chat");
assert(!Filter.isChatMessage(claudeAssistantToolUse),  "claude assistant/tool_use -> NOT chat");

console.log("\n# stripEnvironmentContext");

// Without an env block, text passes through (trimmed).
const noEnv = Filter.stripEnvironmentContext("hello\n\nworld\n");
eq(noEnv.text, "hello\n\nworld", "plain text survives");

// With a single env block at the top, the rest is kept.
const envTop = Filter.stripEnvironmentContext(
    "<environment_context>\n  <cwd>D:\\NJU</cwd>\n</environment_context>\n\n\u73b0\u5728 Web UI \u7684\u4f1a\u8bdd\u8be6\u60c5\u53ef\u8bfb\u6027\u6bd4\u8f83\u5dee\u3002",
);
eq(envTop.stripped.length, 1, "env block recorded as stripped");
assert(envTop.text.indexOf("<environment_context>") === -1, "env block removed from text");
assert(envTop.text.indexOf("<cwd>") === -1, "inner env content also removed");
assert(envTop.text.indexOf("\u73b0\u5728 Web UI") >= 0, "user text after env preserved");

// env block in the middle - both parts preserved.
const envMid = Filter.stripEnvironmentContext(
    "before <environment_context>\nfoo\n</environment_context> after",
);
assert(envMid.text.indexOf("before") >= 0, "text before env kept");
assert(envMid.text.indexOf("after") >= 0, "text after env kept");
assert(envMid.text.indexOf("foo") === -1, "env content removed");

// cleanChatText on the real Codex user fixture.
const cleaned = Filter.cleanChatText(codexUserEnvCtx);
assert(cleaned.indexOf("<environment_context>") === -1,
    "cleanChatText strips env context from Codex user message");
assert(cleaned.indexOf("<cwd>") === -1, "cleanChatText drops env inner content");
assert(cleaned.indexOf("\u73b0\u5728 Web UI") >= 0,
    "cleanChatText keeps user request text");

console.log("\n# findCodeFences / hasCodeFence");

assert(Filter.hasCodeFence("```js\nfoo\n```"), "hasCodeFence detects fence");
assert(!Filter.hasCodeFence("plain text"), "hasCodeFence rejects plain text");

const fences = Filter.findCodeFences("before\n```python\nprint(1)\n```\nmiddle\n```\nraw\n```\nend");
eq(fences.length, 2, "two fences found");
eq(fences[0].lang, "python", "first fence lang");
eq(fences[0].code, "print(1)", "first fence code");
eq(fences[1].lang, "", "second fence lang empty");
eq(fences[1].code, "raw", "second fence code");

console.log("\n# partition (chat view hides everything internal)");

const allMessages = [
    codexSessionMeta, codexEvent, codexWorldState, codexTurnContext,
    codexTokenUsage, codexDeveloper, codexUserEnvCtx, codexReasoning,
    codexFunctionCall, codexToolResult, codexAssistantReply, codexUserWithCode,
    claudeTurnDuration, claudePermissionMode, claudeAiTitle, claudeFileHistory,
    claudeLastPrompt, claudeUser, claudeUserAttachment, claudeAssistantToolUse,
    claudeAssistantReply,
];

const chat = Filter.partition(allMessages, false);
eq(chat.visible.length, 6, "chat view keeps exactly 6 dialog turns");
eq(chat.hidden.length, 15, "chat view hides 15 internal records");

// Visible items should all be user+message, assistant+message, or user+attachment.
const visibleRoles = chat.visible.map((m) => m.role + "/" + m.kind).sort();
eq(visibleRoles, [
    "assistant/message",
    "assistant/message",
    "user/attachment",
    "user/message",
    "user/message",
    "user/message",
], "visible items are exactly the dialog turns");

// Raw view shows everything.
const raw = Filter.partition(allMessages, true);
eq(raw.visible.length, allMessages.length, "raw view keeps all records");
eq(raw.hidden.length, 0, "raw view hides nothing");

console.log("\n# Defensive defaults");

// Null / undefined messages are treated as internal (defensive).
assert(Filter.isInternalMessage(null), "null -> internal");
assert(Filter.isInternalMessage(undefined), "undefined -> internal");
assert(!Filter.isChatMessage(null), "null -> NOT chat");

// Unknown future role (e.g. "moderator") should be internal so a new parser
// cannot accidentally leak records into the chat view.
assert(Filter.isInternalMessage({ role: "moderator", kind: "message" }),
    "unknown role -> internal (defensive)");

console.log("\n# Summary");
console.log("  passed: " + pass);
console.log("  failed: " + fail);
process.exit(fail === 0 ? 0 : 1);
