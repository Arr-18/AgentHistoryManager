/* AgentHistoryManager \u2014 vanilla JS frontend
 *
 * Single page app with three zones: source nav, session list, message detail.
 * All state lives in module-level variables; no framework needed.
 *
 * Message classification + sanitisation lives in ``filter.js`` and is loaded
 * as a UMD global (window.AHMFilter). The default chat view shows only real
 * user / assistant dialog turns (kind === "message"); the "Show internal
 * records" toggle reveals reasoning, tool calls, system events, raw JSON, etc.
 */
(() => {
    "use strict";

    const API = "/api";
    const SOURCES = ["all", "codex", "claude_code"];
    const PAGE_SIZE = 50;
    const Filter = (typeof window !== "undefined" && window.AHMFilter) || null;

    if (!Filter) {
        // Loading order bug \u2014 static/app.js was loaded without filter.js.
        console.error("AHMFilter missing: web/filter.js must be loaded before app.js");
    }

    // ---------- state ----------
    const state = {
        activeSource: "all",
        search: "",
        cwdFilter: "",
        dateFrom: "",
        dateTo: "",
        offset: 0,
        total: 0,
        sessions: [],
        sources: [],
        activeSession: null,        // {source, id}
        showInternal: false,        // when false -> chat view; true -> raw view
        busy: false,
        // Cached messages so toggling "Show internal records" does not require
        // re-fetching from the API.
        cachedMessages: [],         // array of full message objects
        cachedVisible: [],          // chat-filtered subset
        cachedHiddenCount: 0,
    };

    // ---------- DOM refs ----------
    const $ = (id) => document.getElementById(id);
    const dom = {
        sourceList: $("sourceList"),
        sessionList: $("sessionList"),
        search: $("search"),
        cwdFilter: $("cwdFilter"),
        dateFrom: $("dateFrom"),
        dateTo: $("dateTo"),
        prevPage: $("prevPage"),
        nextPage: $("nextPage"),
        pagerInfo: $("pagerInfo"),
        sessionCount: $("sessionCount"),
        agentStats: $("agentStats"),
        detailHeader: $("detailHeader"),
        detailSummary: $("detailSummary"),
        detailBody: $("detailBody"),
        detailMeta: $("detailMeta"),
        showInternal: $("showInternal"),
        statusSource: $("statusSource"),
        statusSession: $("statusSession"),
        statusCount: $("statusCount"),
        statusLastSync: $("statusLastSync"),
        btnSync: $("btnSync"),
        btnFull: $("btnFull"),
        btnRefresh: $("btnRefresh"),
        toasts: $("toasts"),
    };

    // ---------- helpers ----------
    const escapeHtml = (s) => {
        if (s === null || s === undefined) return "";
        return String(s)
            .replace(/&/g, "&amp;")
            .replace(/</g, "&lt;")
            .replace(/>/g, "&gt;")
            .replace(/"/g, "&quot;")
            .replace(/'/g, "&#39;");
    };

    const truncate = (s, n) => {
        if (!s) return "";
        return s.length > n ? s.slice(0, n - 1) + "\u2026" : s;
    };

    const fmtDate = (iso) => {
        if (!iso) return "-";
        try {
            const d = new Date(iso);
            if (isNaN(d.getTime())) return iso;
            const pad = (n) => String(n).padStart(2, "0");
            return d.getFullYear() + "-" + pad(d.getMonth() + 1) + "-" + pad(d.getDate()) +
                " " + pad(d.getHours()) + ":" + pad(d.getMinutes());
        } catch (e) { return iso; }
    };

    const fmtNum = (n) => {
        if (n === null || n === undefined) return "0";
        return Number(n).toLocaleString();
    };

    const debounce = (fn, ms) => {
        let t = null;
        return (...args) => {
            clearTimeout(t);
            t = setTimeout(() => fn(...args), ms);
        };
    };

    const toast = (msg, kind = "") => {
        const div = document.createElement("div");
        div.className = "toast " + (kind || "");
        div.textContent = msg;
        dom.toasts.appendChild(div);
        setTimeout(() => div.remove(), 3500);
    };

    async function api(path, opts) {
        const url = API + path;
        const init = Object.assign({ headers: {} }, opts || {});
        const r = await fetch(url, init);
        if (!r.ok) {
            const txt = await r.text().catch(() => "");
            throw new Error(`${r.status} ${r.statusText} :: ${txt.slice(0, 200)}`);
        }
        return r.json();
    }

    // ---------- rendering ----------

    function renderSources() {
        const items = [];
        const all = state.sources.reduce(
            (acc, s) => { acc.s += s.session_count; acc.m += s.message_count; return acc; },
            { s: 0, m: 0 },
        );
        items.push({ key: "all", label: "All", count: all.s });

        for (const s of state.sources) {
            items.push({ key: s.name, label: prettySource(s.name), count: s.session_count });
        }

        dom.sourceList.innerHTML = "";
        dom.agentStats.textContent = `${all.s} sessions \u00b7 ${fmtNum(all.m)} msgs`;
        for (const it of items) {
            const li = document.createElement("li");
            if (it.key === state.activeSource) li.classList.add("active");
            li.innerHTML = `<span>${escapeHtml(it.label)}</span><span class="pill">${it.count}</span>`;
            li.addEventListener("click", () => {
                state.activeSource = it.key;
                state.offset = 0;
                renderSources();
                loadSessions();
            });
            dom.sourceList.appendChild(li);
        }
    }

    function prettySource(s) {
        if (s === "codex") return "Codex CLI";
        if (s === "claude_code") return "Claude Code";
        return s;
    }

    function renderSessions() {
        dom.sessionList.innerHTML = "";
        if (!state.sessions.length) {
            const li = document.createElement("li");
            li.style.color = "var(--text-muted)";
            li.style.fontStyle = "italic";
            li.textContent = "No sessions match the current filters.";
            dom.sessionList.appendChild(li);
        } else {
            for (const s of state.sessions) {
                dom.sessionList.appendChild(renderSessionItem(s));
            }
        }
        const start = state.sessions.length ? state.offset + 1 : 0;
        const end = state.offset + state.sessions.length;
        dom.sessionCount.textContent = `${state.total} total`;
        dom.pagerInfo.textContent = `${start}\u2026${end} of ${state.total}`;
        dom.prevPage.disabled = state.offset <= 0;
        dom.nextPage.disabled = end >= state.total;
    }

    function renderSessionItem(s) {
        const li = document.createElement("li");
        const active = state.activeSession && state.activeSession.source === s.source && state.activeSession.id === s.id;
        if (active) li.classList.add("active");
        const badgeCls = s.source === "codex" ? "codex" : "claude_code";
        const tokens = (s.input_tokens || 0) + (s.output_tokens || 0) + (s.reasoning_tokens || 0);
        li.innerHTML = `
            <div class="row-1">
                <span class="badge ${badgeCls}">${escapeHtml(prettySource(s.source))}</span>
                <span class="muted small">${fmtDate(s.started_at)}</span>
            </div>
            <div class="meta">${escapeHtml(truncate(s.cwd || "", 64))}</div>
            <div class="preview">${escapeHtml(truncate(stripPreview(s.preview), 200))}</div>
            <div class="meta">
                ${s.message_count} msgs \u00b7 ${s.user_message_count} in / ${s.assistant_message_count} out \u00b7
                ${fmtNum(tokens)} tokens
            </div>
        `;
        li.addEventListener("click", () => {
            state.activeSession = { source: s.source, id: s.id };
            renderSessions();
            loadDetail();
        });
        return li;
    }

    function stripPreview(text) {
        if (!text) return "";
        // Drop the giant environment_context block from Codex previews for legibility.
        const m = text.match(/<environment_context>[\s\S]*?<\/environment_context>/);
        if (m) {
            const after = text.slice(m.index + m[0].length).trim();
            if (after) return after;
        }
        return text;
    }

    function renderDetailHeader(d) {
        const tokens = (d.input_tokens || 0) + (d.output_tokens || 0) + (d.reasoning_tokens || 0);
        dom.detailMeta.innerHTML = `
            <div class="title">${escapeHtml(prettySource(d.source))} \u00b7 ${escapeHtml(truncate(d.cwd || "(no cwd)", 80))}</div>
            <div class="meta-row">
                <span>id: ${escapeHtml(d.id)}</span>
                <span>model: ${escapeHtml(d.model || "-")}</span>
                <span>cli: ${escapeHtml(d.cli_version || "-")}</span>
                <span>started: ${escapeHtml(fmtDate(d.started_at))}</span>
                <span>ended: ${escapeHtml(fmtDate(d.ended_at))}</span>
                <span>msgs: ${d.message_count}</span>
                <span>tokens: ${fmtNum(tokens)}</span>
            </div>
        `;
    }

    /* "Showing X of Y" summary bar above the message list. Hidden when there
     * are no messages yet or when the raw view already shows everything. */
    function renderDetailSummary(total, shown, hidden) {
        if (!total) {
            dom.detailSummary.hidden = true;
            dom.detailSummary.textContent = "";
            return;
        }
        if (state.showInternal || hidden === 0) {
            dom.detailSummary.hidden = true;
            dom.detailSummary.textContent = "";
            return;
        }
        const kindSummary = summariseHiddenKinds(state.cachedMessages, state.cachedVisible);
        dom.detailSummary.hidden = false;
        dom.detailSummary.innerHTML =
            `Showing <strong>${shown}</strong> of ${total} records \u00b7 ` +
            `<strong>${hidden}</strong> internal records hidden ` +
            `<span class="muted small">(${escapeHtml(kindSummary)})</span>`;
    }

    function summariseHiddenKinds(all, visibleSet) {
        const counts = {};
        const visible = new Set(visibleSet);
        for (const m of all) {
            if (visible.has(m)) continue;
            const key = (m.role || "?") + ":" + (m.kind || "?");
            counts[key] = (counts[key] || 0) + 1;
        }
        const entries = Object.entries(counts).sort((a, b) => b[1] - a[1]);
        if (!entries.length) return "";
        // Keep the top 4 buckets and aggregate the rest.
        const top = entries.slice(0, 4).map(([k, c]) => `${c} ${k}`);
        const rest = entries.length > 4 ? entries.slice(4).reduce((s, [, c]) => s + c, 0) : 0;
        const tail = rest > 0 ? `, +${rest} more` : "";
        return top.join(", ") + tail;
    }

    function renderDetailBody() {
        dom.detailBody.innerHTML = "";
        const total = state.cachedMessages.length;
        if (!total) {
            const empty = document.createElement("div");
            empty.className = "empty-state";
            empty.innerHTML = `<div class="empty-glyph">\u00b7</div><div>This session has no messages to display.</div>`;
            dom.detailBody.appendChild(empty);
            renderDetailSummary(0, 0, 0);
            return;
        }

        // Decide which messages to render based on the toggle + filter helpers.
        let visible;
        let hiddenCount;
        if (state.showInternal) {
            visible = state.cachedMessages;
            hiddenCount = 0;
        } else if (Filter) {
            const part = Filter.partition(state.cachedMessages, false);
            visible = part.visible;
            hiddenCount = part.hidden.length;
        } else {
            // Defensive fallback: if filter.js failed to load, just show
            // user/assistant messages with kind === "message".
            visible = state.cachedMessages.filter(
                (m) => (m.role === "user" || m.role === "assistant") && m.kind === "message",
            );
            hiddenCount = total - visible.length;
        }
        state.cachedVisible = visible;
        state.cachedHiddenCount = hiddenCount;

        if (!visible.length) {
            const empty = document.createElement("div");
            empty.className = "empty-state";
            empty.innerHTML = `<div class="empty-glyph">\u00b7</div>` +
                `<div>This session has no chat messages.</div>` +
                (hiddenCount > 0 && !state.showInternal
                    ? `<div class="muted small" style="margin-top:6px">Toggle <em>Show internal records</em> to inspect the ${hiddenCount} infrastructure record(s).</div>`
                    : "");
            dom.detailBody.appendChild(empty);
            renderDetailSummary(total, 0, hiddenCount);
            return;
        }

        // Wrap the rendered messages in a centered column. The raw view
        // also uses .msg max-width so it stays consistent.
        const stream = document.createElement("div");
        stream.className = state.showInternal ? "chat-stream raw" : "chat-stream";
        for (const m of visible) {
            const node = state.showInternal
                ? renderRawMessage(m)
                : renderChatMessage(m);
            if (node) stream.appendChild(node);
        }
        dom.detailBody.appendChild(stream);

        renderDetailSummary(total, visible.length, hiddenCount);
    }

    /* ------------------------------------------------------------------
     * Chat view (default) - user/assistant bubbles, code blocks, etc.
     * ------------------------------------------------------------------ */

    function renderChatMessage(m) {
        const role = m.role || "";
        const wrap = document.createElement("div");
        wrap.className = "chat-row chat-" + (role === "user" ? "user" : "assistant");
        wrap.dataset.role = role;
        wrap.dataset.kind = m.kind || "";

        const who = document.createElement("div");
        who.className = "chat-meta";
        const roleLabel = role === "user" ? "You" : "Assistant";
        let metaHtml = `<span class="chat-who">${escapeHtml(roleLabel)}</span>`;
        if (m.timestamp) metaHtml += `<span class="chat-ts">${escapeHtml(fmtDate(m.timestamp))}</span>`;
        if (m.model && role === "assistant") {
            metaHtml += `<span class="chat-model">${escapeHtml(m.model)}</span>`;
        }
        who.innerHTML = metaHtml;
        wrap.appendChild(who);

        const bubble = document.createElement("div");
        bubble.className = "chat-bubble";

        const text = Filter ? Filter.cleanChatText(m) : (m.text || "");
        if (!text || !text.trim()) {
            const ph = document.createElement("div");
            ph.className = "chat-empty muted";
            ph.textContent = role === "assistant" ? "(no text in this turn)" : "(empty message)";
            bubble.appendChild(ph);
        } else if (Filter && Filter.hasCodeFence(text)) {
            renderMarkdownLite(bubble, text);
        } else {
            renderPlainText(bubble, text);
        }
        wrap.appendChild(bubble);
        return wrap;
    }

    function renderPlainText(container, text) {
        const lines = text.split("\n");
        for (let i = 0; i < lines.length; i++) {
            const p = document.createElement("p");
            if (!lines[i]) {
                p.innerHTML = "&nbsp;";
            } else {
                renderInlineSpans(p, lines[i]);
            }
            container.appendChild(p);
        }
    }

    function renderInlineSpans(parent, text) {
        // Inline code spans (`...`) and simple emphasis (**...**).
        const re = /(`[^`\n]+`|\*\*[^*]+\*\*)/g;
        let last = 0;
        let m;
        while ((m = re.exec(text)) !== null) {
            if (m.index > last) {
                parent.appendChild(document.createTextNode(text.slice(last, m.index)));
            }
            const tok = m[0];
            if (tok.startsWith("`")) {
                const code = document.createElement("code");
                code.textContent = tok.slice(1, -1);
                parent.appendChild(code);
            } else if (tok.startsWith("**")) {
                const b = document.createElement("strong");
                b.textContent = tok.slice(2, -2);
                parent.appendChild(b);
            }
            last = m.index + tok.length;
        }
        if (last < text.length) {
            parent.appendChild(document.createTextNode(text.slice(last)));
        }
    }

    /* Tiny markdown-lite renderer: splits the text on ```...``` fences and
     * emits either <pre><code> blocks or <p> paragraphs for the prose parts. */
    function renderMarkdownLite(container, text) {
        const re = /```([a-zA-Z0-9_+\-#.]*)\n([\s\S]*?)```/g;
        let last = 0;
        let m;
        while ((m = re.exec(text)) !== null) {
            if (m.index > last) {
                renderPlainText(container, text.slice(last, m.index));
            }
            const pre = document.createElement("pre");
            pre.className = "chat-codeblock";
            const code = document.createElement("code");
            const lang = m[1] || "";
            if (lang) code.className = "lang-" + lang;
            code.textContent = m[2].replace(/\n$/, "");
            pre.appendChild(code);
            container.appendChild(pre);
            last = m.index + m[0].length;
        }
        if (last < text.length) {
            renderPlainText(container, text.slice(last));
        }
    }

    /* ------------------------------------------------------------------
     * Raw view (Show internal records) - existing card layout.
     * ------------------------------------------------------------------ */

    function renderRawMessage(m) {
        const wrap = document.createElement("div");
        const role = m.role || "event";
        wrap.className = `msg role-${role} ${m.kind || ""}`;
        if (m.kind === "reasoning") wrap.classList.add("reasoning");
        if (m.kind === "function_call") wrap.classList.add("function_call");
        if (m.kind === "function_call_output") wrap.classList.add("function_call_output");

        const head = document.createElement("div");
        head.className = "head";
        head.innerHTML = `
            <span class="role-pill">${escapeHtml(role)}</span>
            <span class="kind-pill">${escapeHtml(m.kind || "")}</span>
            ${m.model ? `<span class="model-tag">${escapeHtml(m.model)}</span>` : ""}
            <span class="ts">${escapeHtml(fmtDate(m.timestamp))}</span>
        `;
        wrap.appendChild(head);

        const body = document.createElement("div");
        body.className = "body";

        const content = m.content || {};
        const rendered = renderContent(content, m.kind, role);
        if (typeof rendered === "string") {
            body.textContent = rendered;
        } else {
            body.appendChild(rendered);
        }

        if (m.text) {
            const txt = document.createElement("div");
            txt.style.marginTop = "8px";
            txt.style.paddingTop = "8px";
            txt.style.borderTop = "1px dashed var(--border)";
            txt.style.color = "var(--text-muted)";
            txt.style.whiteSpace = "pre-wrap";
            txt.textContent = m.text;
            body.appendChild(txt);
        }

        const details = document.createElement("details");
        details.innerHTML = `<summary>Raw JSON</summary>`;
        const pre = document.createElement("pre");
        pre.textContent = JSON.stringify(m, null, 2);
        details.appendChild(pre);
        body.appendChild(details);

        wrap.appendChild(body);
        return wrap;
    }

    function renderContent(content, kind, role) {
        if (Array.isArray(content)) {
            const frag = document.createElement("div");
            for (const block of content) {
                frag.appendChild(renderBlock(block));
            }
            return frag;
        }
        if (content && typeof content === "object") {
            if (content.tool_use) return renderBlock({ type: "tool_use", ...content.tool_use });
            if (content.tool_result) return renderBlock({ type: "tool_result", ...content.tool_result });
            const pre = document.createElement("pre");
            pre.textContent = JSON.stringify(content, null, 2);
            return pre;
        }
        return String(content ?? "");
    }

    function renderBlock(b) {
        if (!b || typeof b !== "object") {
            const span = document.createElement("span");
            span.textContent = String(b ?? "");
            return span;
        }
        if (b.type === "text" || b.type === "input_text" || b.type === "output_text") {
            const div = document.createElement("div");
            div.textContent = b.text || "";
            return div;
        }
        if (b.type === "thinking") {
            const div = document.createElement("div");
            div.style.fontStyle = "italic";
            div.style.color = "var(--text-muted)";
            div.textContent = "[thinking] " + (b.thinking || b.text || "");
            return div;
        }
        if (b.type === "tool_use") {
            const div = document.createElement("div");
            div.innerHTML = `<strong>tool_use</strong>: ${escapeHtml(b.name || "")}<br/>`;
            const pre = document.createElement("pre");
            try {
                pre.textContent = JSON.stringify(typeof b.input === "string" ? safeParse(b.input) : b.input, null, 2);
            } catch (e) { pre.textContent = b.input || ""; }
            div.appendChild(pre);
            return div;
        }
        if (b.type === "tool_result") {
            const div = document.createElement("div");
            div.innerHTML = `<strong>tool_result</strong>:<br/>`;
            const pre = document.createElement("pre");
            const out = typeof b.content === "string" ? b.content : JSON.stringify(b.content, null, 2);
            pre.textContent = out || "";
            div.appendChild(pre);
            return div;
        }
        const pre = document.createElement("pre");
        pre.textContent = JSON.stringify(b, null, 2);
        return pre;
    }

    function safeParse(s) { try { return JSON.parse(s); } catch (e) { return s; } }

    function renderStatusBar() {
        dom.statusSource.textContent = `Source: ${state.activeSource === "all" ? "All" : prettySource(state.activeSource)}`;
        if (state.activeSession) {
            const s = state.sessions.find(x => x.source === state.activeSession.source && x.id === state.activeSession.id);
            dom.statusSession.textContent = `Session: ${s ? truncate(s.id, 12) : state.activeSession.id.slice(0, 12)}`;
        } else {
            dom.statusSession.textContent = "Session: -";
        }
        dom.statusCount.textContent = `Messages: ${state.sessions.length ? state.sessions[0].message_count : 0}`;
    }

    // ---------- data loading ----------

    async function loadStats() {
        try {
            const stats = await api("/stats");
            state.sources = stats.sources || [];
            if (stats.last_sync_at) {
                dom.statusLastSync.textContent = "Last sync: " + fmtDate(stats.last_sync_at);
            }
            renderSources();
        } catch (e) {
            console.error(e);
            toast("Failed to load stats: " + e.message, "error");
        }
    }

    const reloadSessions = debounce(async () => {
        state.offset = 0;
        await loadSessions();
    }, 250);

    async function loadSessions() {
        if (state.busy) return;
        state.busy = true;
        try {
            const params = new URLSearchParams();
            if (state.activeSource !== "all") params.set("source", state.activeSource);
            if (state.search) params.set("search", state.search);
            if (state.cwdFilter) params.set("cwd", state.cwdFilter);
            if (state.dateFrom) params.set("from", state.dateFrom);
            if (state.dateTo) params.set("to", state.dateTo);
            params.set("offset", String(state.offset));
            params.set("limit", String(PAGE_SIZE));
            const data = await api("/sessions?" + params.toString());
            state.total = data.total;
            state.sessions = data.items || [];
            renderSessions();
            renderStatusBar();
        } catch (e) {
            console.error(e);
            toast("Failed to load sessions: " + e.message, "error");
        } finally {
            state.busy = false;
        }
    }

    async function loadDetail() {
        if (!state.activeSession) return;
        const { source, id } = state.activeSession;
        try {
            // Always ask for the full message list so toggling between chat and
            // raw views is a pure client-side operation.
            const detail = await api(`/sessions/${encodeURIComponent(source)}/${encodeURIComponent(id)}`);
            const msgs = await api(`/sessions/${encodeURIComponent(source)}/${encodeURIComponent(id)}/messages?include_events=true`);
            state.cachedMessages = msgs.items || [];
            renderDetailHeader(detail);
            renderDetailBody();
            dom.statusCount.textContent = `Messages: ${msgs.count}`;
            renderStatusBar();
            dom.statusSession.textContent = `Session: ${id.slice(0, 12)}\u2026`;
        } catch (e) {
            console.error(e);
            toast("Failed to load session: " + e.message, "error");
        }
    }

    async function doSync() {
        toast("Syncing\u2026");
        try {
            const r = await api("/ingest/sync", { method: "POST" });
            const total = (r.summary?.codex?.added || 0) + (r.summary?.codex?.updated || 0)
                + (r.summary?.claude_code?.added || 0) + (r.summary?.claude_code?.updated || 0);
            toast(`Sync done (${total} sessions touched)`, "ok");
            await loadStats();
            await loadSessions();
            if (state.activeSession) await loadDetail();
        } catch (e) {
            toast("Sync failed: " + e.message, "error");
        }
    }

    async function doFull() {
        toast("Re-ingesting everything\u2026");
        try {
            const r = await api("/ingest/full", { method: "POST" });
            const total = (r.summary?.codex?.added || 0) + (r.summary?.codex?.updated || 0)
                + (r.summary?.claude_code?.added || 0) + (r.summary?.claude_code?.updated || 0);
            toast(`Full rescan done (${total} sessions)`, "ok");
            await loadStats();
            await loadSessions();
            if (state.activeSession) await loadDetail();
        } catch (e) {
            toast("Full rescan failed: " + e.message, "error");
        }
    }

    // ---------- events ----------

    dom.search.addEventListener("input", (e) => {
        state.search = e.target.value;
        reloadSessions();
    });
    dom.cwdFilter.addEventListener("input", (e) => {
        state.cwdFilter = e.target.value;
        reloadSessions();
    });
    dom.dateFrom.addEventListener("change", (e) => { state.dateFrom = e.target.value; reloadSessions(); });
    dom.dateTo.addEventListener("change", (e) => { state.dateTo = e.target.value; reloadSessions(); });

    dom.prevPage.addEventListener("click", () => {
        if (state.offset > 0) {
            state.offset = Math.max(0, state.offset - PAGE_SIZE);
            loadSessions();
        }
    });
    dom.nextPage.addEventListener("click", () => {
        if (state.offset + state.sessions.length < state.total) {
            state.offset += PAGE_SIZE;
            loadSessions();
        }
    });

    dom.showInternal.addEventListener("change", () => {
        state.showInternal = dom.showInternal.checked;
        renderDetailBody();
    });

    dom.btnSync.addEventListener("click", doSync);
    dom.btnFull.addEventListener("click", doFull);
    dom.btnRefresh.addEventListener("click", async () => {
        await loadStats();
        await loadSessions();
        if (state.activeSession) await loadDetail();
    });

    async function loadHistoryPaths() {
        const container = $("historyPaths");
        try {
            const paths = await api("/history-paths");
            // Keep a path being edited intact during periodic status refreshes.
            if (container.contains(document.activeElement)) return;
            container.replaceChildren();
            for (const entry of paths) {
                const row = document.createElement("div");
                row.className = "history-path-row";
                const label = document.createElement("label");
                label.textContent = prettySource(entry.source);
                const input = document.createElement("input");
                input.type = "text";
                input.value = entry.path;
                input.setAttribute("aria-label", prettySource(entry.source) + " 历史目录绝对路径");
                input.title = entry.path;
                const status = document.createElement("div");
                status.className = "small path-status " + entry.status;
                status.textContent = entry.message;
                const button = document.createElement("button");
                button.type = "button";
                button.className = "btn path-save";
                button.textContent = "保存路径";
                button.addEventListener("click", async () => {
                    button.disabled = true;
                    try {
                        await api("/history-paths", {method:"POST", headers:{"Content-Type":"application/json"}, body:JSON.stringify({source:entry.source,path:input.value})});
                        button.blur();
                        await loadHistoryPaths();
                        toast("路径已保存，正在同步…");
                        await doSync();
                    } catch (e) { toast("保存路径失败: " + e.message, "error"); }
                    finally { button.disabled = false; }
                });
                row.append(label, input, status, button);
                container.appendChild(row);
            }
        } catch (e) { container.textContent = "目录检测失败: " + e.message; }
    }
    // ---------- bootstrap ----------
    (async () => {
        await loadHistoryPaths();
        await loadStats();
        await loadSessions();
        setInterval(() => { loadStats(); loadHistoryPaths(); }, 30000);
    })();
})();
