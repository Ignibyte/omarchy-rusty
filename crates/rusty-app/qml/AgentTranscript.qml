import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import dev.ignibyte.rusty

// The conversation itself, shared by the Agent tab and the pane beside a note: one row per
// thing said or done, written by one `push` so the model's roles never drift, and one card
// per kind. A replayed conversation and a live one come through the same handlers, because
// the host echoes what it writes as well as what it reads.
Item {
    id: transcript
    required property var assistant
    required property var backend
    required property var theme
    // The pane is narrow: cards stack and diffs are shorter.
    property bool compact: false
    property bool showThinking: false
    // The view follows the tail until the reader scrolls away.
    property bool follow: true
    property bool replaying: false
    readonly property int count: chat.count
    property int pendingCount: 0
    signal openPath(string path)
    signal openLink(string link)
    signal focusComposer()
    signal activity()
    signal acceptEditsWanted()

    function push(row) {
        chat.append({
            uid: String(++uidSeq), kind: row.kind || "notice", name: row.name || "",
            text: row.text || "", html: "", extra: row.extra || "", input: row.input || "",
            meta: row.meta || "{}", result: "", isError: false, answered: "",
            state: row.state || "final", expanded: false
        })
        activity()
    }
    function clear() { chat.clear(); pendingCount = 0; streamAt = -1; streamBuf = "" }
    function rowsOfKind(kind) { const out = []; for (let i = 0; i < chat.count; i++) if (chat.get(i).kind === kind) out.push(i); return out }
    function userTexts() { return transcript.rowsOfKind("user").map(function (i) { return chat.get(i).text }) }
    function lastPending() { for (let i = chat.count - 1; i >= 0; i--) if (chat.get(i).state === "pending") return i; return -1 }
    // Answer the newest request; false when there is nothing to answer yet.
    function answerPending(allow) {
        const i = transcript.lastPending()
        if (i < 0) return false
        transcript.answer(i, allow, "", "{}")
        return true
    }
    function focusPending() { const i = transcript.lastPending(); if (i >= 0) { list.currentIndex = i; list.forceActiveFocus(); list.positionViewAtIndex(i, ListView.Contain) } }
    function jumpToLatest() { transcript.follow = true; list.positionViewAtEnd() }
    function byToolUse(id) {
        if (id.length === 0) return -1
        for (let i = chat.count - 1; i >= 0; i--) { const r = chat.get(i); if (r.extra === id && (r.kind === "tool" || r.kind === "question" || r.kind === "plan")) return i }
        return -1
    }
    function requestOf(row) { try { return JSON.parse(row.meta || "{}").request_id || "" } catch (e) { return "" } }
    function setRow(i, key, value) { if (i >= 0 && i < chat.count) chat.setProperty(i, key, value) }
    function mergeMeta(i, extra) {
        if (i < 0) return
        let m = {}
        try { m = JSON.parse(chat.get(i).meta || "{}") } catch (e) { m = {} }
        for (const k in extra) m[k] = extra[k]
        chat.setProperty(i, "meta", JSON.stringify(m))
    }
    // The card of a tool call the agent is asking to make, or a standalone permission row.
    function kindForTool(name) { return name === "AskUserQuestion" ? "question" : name === "ExitPlanMode" ? "plan" : "tool" }

    property int uidSeq: 0
    // Deltas arrive faster than a text layout: they are gathered and written once a frame.
    property int streamAt: -1
    property string streamBuf: ""
    function flushStream() {
        if (transcript.streamAt < 0) return
        transcript.setRow(transcript.streamAt, "text", transcript.streamBuf)
    }
    Timer { id: flusher; interval: 40; onTriggered: transcript.flushStream() }
    function appendDelta(text) {
        const i = transcript.rowsOfKind("text").pop()
        if (i === undefined) { transcript.push({ kind: "text", text: text, state: "streaming" }); transcript.streamAt = chat.count - 1; transcript.streamBuf = text }
        else {
            if (i !== transcript.streamAt) { transcript.streamAt = i; transcript.streamBuf = chat.get(i).text }
            transcript.streamBuf += text
        }
        if (!flusher.running) flusher.start()
    }
    function endStream() { flusher.stop(); transcript.flushStream(); transcript.streamAt = -1; transcript.streamBuf = "" }

    ListModel { id: chat }

    // Markdown comes from the back end, as every other rendered page does; a row asks for
    // it when its block is complete and its card is built, so a long replay renders only
    // what is read.
    property var renders: ({})
    function style() {
        const t = JSON.parse(transcript.theme.tokens || "{}")
        return {
            text: theme.foreground, muted: theme.muted, link: theme.link, unresolved: theme.faint,
            accent: theme.accent, code: theme.code, code_bg: theme.codeBg, mono: theme.termFont,
            mark_bg: t.mark || theme.accent, line: theme.line, tag: theme.tag,
            red: t.red, green: t.green, yellow: t.yellow, blue: t.blue, magenta: t.magenta, cyan: t.cyan,
            headings: [t.h1, t.h2, t.h3, t.h4, t.h5, t.h6], size: Math.round(13 * transcript.theme.scale),
            bright: theme.bright, gold: theme.gold, alive: theme.alive, accent_soft: theme.accentSoft,
            panel3: theme.panel3, line_bright: theme.lineBright, marks: true, code_head: true
        }
    }
    function renderRow(uid, markdown) {
        if (!backend.connected || markdown.length === 0 || markdown.length > 24000) return
        const id = backend.call("brain_render", JSON.stringify({ slug: "", markdown: markdown, style: transcript.style() }))
        const r = transcript.renders; r[id] = uid; transcript.renders = r
    }
    function rowOfUid(uid) { for (let i = 0; i < chat.count; i++) if (chat.get(i).uid === uid) return i; return -1 }
    Connections {
        target: transcript.backend
        function onResult(id, tool, json, ok) {
            const uid = transcript.renders[id]
            if (uid === undefined) return
            const r = transcript.renders; delete r[id]; transcript.renders = r
            if (!ok) return
            let html = ""
            try { const parsed = JSON.parse(json); html = typeof parsed.html === "string" ? parsed.html : "" } catch (e) { return }
            transcript.setRow(transcript.rowOfUid(uid), "html", html)
        }
    }

    Connections {
        target: transcript.assistant
        ignoreUnknownSignals: true
        function onUserMessage(text) { transcript.endStream(); transcript.push({ kind: "user", text: text }) }
        function onBlockStarted(kind, name, id) {
            if (kind === "text") { transcript.endStream(); transcript.push({ kind: "text", text: "", state: "streaming" }) }
            else if (kind === "thinking") { transcript.endStream(); transcript.push({ kind: "thinking", text: "", state: "streaming" }) }
            else if (kind === "tool_use") transcript.push({ kind: transcript.kindForTool(name), name: name, extra: id, state: "streaming" })
        }
        function onTextDelta(text) { transcript.appendDelta(text) }
        function onTextFinal(text) {
            transcript.endStream()
            const rows = transcript.rowsOfKind("text")
            const i = rows.length > 0 ? rows[rows.length - 1] : -1
            if (i >= 0 && chat.get(i).state !== "final") { transcript.setRow(i, "text", text); transcript.setRow(i, "state", "final") }
            else transcript.push({ kind: "text", text: text, state: "final" })
        }
        function onThinkingTokens(estimated) {
            const rows = transcript.rowsOfKind("thinking")
            if (rows.length === 0) return
            const i = rows[rows.length - 1]
            if (chat.get(i).state === "streaming") transcript.setRow(i, "text", estimated + " tokens")
        }
        function onToolInput(id, name, input) {
            let i = transcript.byToolUse(id)
            if (i < 0) { transcript.push({ kind: transcript.kindForTool(name), name: name, extra: id, state: "streaming" }); i = chat.count - 1 }
            transcript.setRow(i, "input", input)
            transcript.setRow(i, "name", name)
            if (chat.get(i).state === "streaming") transcript.setRow(i, "state", "done")
            if (name === "Edit") {
                let a = {}
                try { a = JSON.parse(input) } catch (e) { a = {} }
                if (typeof a.old_string === "string" && typeof a.new_string === "string") {
                    transcript.mergeMeta(i, { diff: transcript.assistant.diff(a.old_string, a.new_string) })
                }
            }
        }
        function onToolResult(id, text, isError) {
            const i = transcript.byToolUse(id)
            if (i < 0) { transcript.push({ kind: "tool", name: "result", text: "", state: "done" }); transcript.setRow(chat.count - 1, "result", text); transcript.setRow(chat.count - 1, "isError", isError); return }
            transcript.setRow(i, "result", text)
            transcript.setRow(i, "isError", isError)
            if (chat.get(i).state !== "pending") transcript.setRow(i, "state", "done")
        }
        function onPermissionAsked(requestId, tool, input, description, meta) {
            let m = {}
            try { m = JSON.parse(meta || "{}") } catch (e) { m = {} }
            m.request_id = requestId
            let i = transcript.byToolUse(m.tool_use_id || "")
            if (i < 0) { transcript.push({ kind: transcript.kindForTool(tool), name: tool, extra: m.tool_use_id || requestId, input: input, state: "pending" }); i = chat.count - 1 }
            if (chat.get(i).input.length === 0) transcript.setRow(i, "input", input)
            transcript.setRow(i, "state", "pending")
            transcript.mergeMeta(i, m)
            transcript.pendingCount = transcript.pendingCount + 1
            if (!transcript.replaying) transcript.focusPending()
        }
        function onAnswered(requestId, allowed) { transcript.settle(requestId, allowed ? "Allowed" : "Denied") }
        function onExpired(requestId) { transcript.settle(requestId, "Expired — the process went before this was answered") }
        function onTurnDone(ok, cost, turns, text, durationMs) {
            transcript.endStream()
            if (!ok && text.length > 0) transcript.push({ kind: "notice", text: text })
            transcript.push({ kind: "result", meta: JSON.stringify({ ok: ok, cost: cost, turns: turns, durationMs: durationMs }) })
        }
        function onNotice(text) { transcript.push({ kind: "notice", text: text }) }
        function onExited(code, reason, message) {
            if (reason === "idle" || reason === "stop") return
            transcript.push({ kind: "notice", text: "Claude Code " + message + ". The next message starts it again." })
        }
        function onHostExited(message) { transcript.push({ kind: "notice", text: "The session's host is not reachable: " + message }) }
        function onReplayDone() { transcript.replaying = false; transcript.jumpToLatest() }
        function onAttached(id) { transcript.replaying = true }
    }
    function settle(requestId, word) {
        for (let i = chat.count - 1; i >= 0; i--) {
            const r = chat.get(i)
            if (r.state === "pending" && transcript.requestOf(r) === requestId) {
                transcript.setRow(i, "state", "done")
                transcript.setRow(i, "answered", word)
                transcript.pendingCount = Math.max(0, transcript.pendingCount - 1)
                return
            }
        }
    }
    function answer(index, allow, inputJson, extraJson) {
        const row = chat.get(index)
        if (!row || row.answered.length > 0) return
        transcript.assistant.answer(transcript.requestOf(row), allow, inputJson.length > 0 ? inputJson : row.input, extraJson)
        transcript.focusComposer()
    }

    ListView {
        id: list
        anchors.fill: parent
        clip: true
        model: chat
        spacing: 6
        topMargin: 8
        bottomMargin: 8
        cacheBuffer: 2000
        keyNavigationEnabled: true
        highlightMoveDuration: 0
        ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }
        // Follow the tail while the reader is at it; a scroll away stops it and the pill
        // brings it back. `positioning` keeps the programmatic jump from reading as one.
        property bool positioning: false
        function tail() { if (!transcript.follow || transcript.replaying) return; positioning = true; positionViewAtEnd(); positioning = false }
        onCountChanged: Qt.callLater(tail)
        onContentHeightChanged: Qt.callLater(tail)
        onMovementEnded: if (!positioning) transcript.follow = atYEnd
        onFlickEnded: if (!positioning) transcript.follow = atYEnd
        WheelHandler { onWheel: (event) => { if (event.angleDelta.y > 0) transcript.follow = false } }

        Keys.onPressed: (event) => {
            const row = list.currentIndex >= 0 && list.currentIndex < chat.count ? chat.get(list.currentIndex) : null
            if (!row) return
            if (row.state === "pending" && row.answered.length === 0) {
                if (event.key === Qt.Key_Y) { transcript.answer(list.currentIndex, true, "", "{}"); event.accepted = true; return }
                if (event.key === Qt.Key_N && (event.modifiers & Qt.ShiftModifier)) { transcript.setRow(list.currentIndex, "expanded", true); event.accepted = true; return }  // the card opens its reason field
                if (event.key === Qt.Key_N) { transcript.answer(list.currentIndex, false, "", "{}"); event.accepted = true; return }
                if (event.key === Qt.Key_A) {
                    let m = {}
                    try { m = JSON.parse(row.meta || "{}") } catch (e) { m = {} }
                    const s = m.permission_suggestions
                    if (Array.isArray(s) && s.length > 0) { transcript.answer(list.currentIndex, true, "", JSON.stringify({ updatedPermissions: s })); event.accepted = true; return }
                }
            }
            if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter || event.key === Qt.Key_Space) { transcript.setRow(list.currentIndex, "expanded", !row.expanded); event.accepted = true }
            else if (event.key === Qt.Key_Escape) { transcript.focusComposer(); event.accepted = true }
            else if (event.key === Qt.Key_End && (event.modifiers & Qt.ControlModifier)) { transcript.jumpToLatest(); event.accepted = true }
        }

        delegate: Item {
            id: rowItem
            required property int index
            required property string uid
            required property string kind
            required property string name
            required property string text
            required property string html
            required property string extra
            required property string input
            required property string meta
            required property string result
            required property bool isError
            required property string answered
            required property string state
            required property bool expanded
            width: list.width - Math.round(16 * transcript.theme.scale)
            x: Math.round(8 * transcript.theme.scale)
            height: loader.item ? loader.item.implicitHeight + (focused ? 4 : 0) : 0
            readonly property bool focused: list.currentIndex === index && list.activeFocus

            Rectangle {
                anchors.fill: parent
                anchors.margins: -2
                visible: rowItem.focused
                color: "transparent"
                border.width: 1
                border.color: transcript.theme.accent
                radius: transcript.theme.radius
            }
            Loader {
                id: loader
                width: parent.width
                sourceComponent: rowItem.kind === "tool" ? toolCard
                               : rowItem.kind === "question" || rowItem.kind === "plan" ? askCard
                               : textCard
            }
            Component {
                id: textCard
                AgentTextCard {
                    width: loader.width
                    theme: transcript.theme
                    kind: rowItem.kind
                    text: rowItem.text
                    html: rowItem.html
                    state: rowItem.state
                    meta: rowItem.meta
                    compact: transcript.compact
                    expanded: rowItem.kind === "thinking" ? (rowItem.expanded || transcript.showThinking) : rowItem.expanded
                    onExpandedChanged: transcript.setRow(rowItem.index, "expanded", expanded)
                    onRenderWanted: transcript.renderRow(rowItem.uid, rowItem.text)
                    onLinkClicked: (link) => transcript.openLink(link)
                }
            }
            Component {
                id: toolCard
                AgentToolCard {
                    width: loader.width
                    theme: transcript.theme
                    name: rowItem.name
                    input: rowItem.input
                    result: rowItem.result
                    isError: rowItem.isError
                    state: rowItem.state
                    meta: rowItem.meta
                    answered: rowItem.answered
                    compact: transcript.compact
                    expanded: rowItem.expanded
                    onExpandedChanged: transcript.setRow(rowItem.index, "expanded", expanded)
                    onOpenPath: (p) => transcript.openPath(p)
                    onDecided: (allow, extra) => transcript.answer(rowItem.index, allow, "", extra)
                }
            }
            Component {
                id: askCard
                AgentQuestionCard {
                    width: loader.width
                    theme: transcript.theme
                    kind: rowItem.kind
                    input: rowItem.input
                    html: rowItem.html
                    answered: rowItem.answered
                    onRenderWanted: {
                        let a = {}
                        try { a = JSON.parse(rowItem.input || "{}") } catch (e) { a = {} }
                        if (typeof a.plan === "string") transcript.renderRow(rowItem.uid, a.plan)
                    }
                    onDecided: (allow, inputJson, extra) => transcript.answer(rowItem.index, allow, inputJson, extra)
                    onAcceptEdits: transcript.acceptEditsWanted()
                }
            }
        }
    }

    // Back at the tail in one click, for a conversation read from the middle.
    Rectangle {
        visible: !transcript.follow && chat.count > 0
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.margins: 12
        radius: transcript.theme.radius
        color: transcript.theme.panel3
        border.width: 1
        border.color: transcript.theme.accent
        implicitWidth: jump.implicitWidth + 16
        implicitHeight: jump.implicitHeight + 10
        Text {
            id: jump
            anchors.centerIn: parent
            text: "↓ latest"
            color: transcript.theme.accent
            font.pixelSize: Math.round(11 * transcript.theme.scale)
        }
        TapHandler { onTapped: transcript.jumpToLatest() }
    }
}
