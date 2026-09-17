import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import dev.ignibyte.rusty

// An Agent tab: one session of Claude Code, read as a conversation. The tab holds the
// client; the process lives in the session's host, so closing the window or switching tabs
// changes nothing about the work. A tab with no session starts nothing until the first
// message, so opening one costs no process.
Item {
    id: page
    required property var backend
    required property var theme
    required property var registry
    required property var folders
    property string sessionId: ""
    property string cwd: ""
    property bool isCurrent: false
    property bool windowActive: true
    property string savedPrefs: "{}"
    property string branch: ""
    property string firstPrompt: ""
    property int queued: 0
    property real lastCost: 0
    property int lastTurns: 0
    property string tabTitle: ""
    signal unread()
    signal attention(string message)
    signal sessionBound(string id)
    signal prefsEdited(string json)
    signal titleWanted(string title)
    signal openFile(string path)

    Assistant { id: assistant }

    readonly property var prefs: { try { return JSON.parse(page.savedPrefs || "{}") } catch (e) { return {} } }
    readonly property string shortCwd: {
        const home = page.theme.homeDir
        return page.cwd.indexOf(home) === 0 ? "~" + page.cwd.slice(home.length) : page.cwd
    }
    readonly property string stateWord: {
        if (!assistant.available) return "no claude"
        if (assistant.attachedState === "attaching") return "attaching"
        if (assistant.attachedState === "detached") return page.sessionId.length > 0 ? "detached" : "new session"
        if (transcript.pendingCount > 0) return "needs input · Ctrl+."
        if (assistant.busy) return "working"
        if (assistant.childState === "none") return "sleeping"
        return "ready"
    }

    function focusComposer() { composer.focusInput() }
    function focusPending() { transcript.focusPending() }
    // What a scene uses to photograph a decision taken, and the palette to answer without
    // reaching for the mouse.
    function answerPending(allow) { return transcript.answerPending(allow) }
    function interrupt() { assistant.interrupt() }
    function stopSession() { assistant.stop() }
    function newSession() { page.sessionId = ""; assistant.detach(); transcript.clear() }
    function savePrefs(extra) {
        const p = page.prefs
        for (const k in extra) p[k] = extra[k]
        page.prefsEdited(JSON.stringify(p))
    }
    // The branch, read the way the explorer reads a root's: from the repository itself,
    // never written, and quietly absent when there is none.
    function readBranch() {
        page.branch = ""
        if (page.cwd.length === 0) return
        let head = ""
        try { head = page.folders.readText(page.cwd + "/.git/HEAD") } catch (e) { return }
        const at = head.indexOf("ref: refs/heads/")
        if (at >= 0) page.branch = head.slice(at + 16).trim()
    }
    function createSession(text) {
        page.firstPrompt = text
        assistant.create(JSON.stringify({
            cwd: page.cwd,
            title: page.tabTitle.length > 0 ? page.tabTitle : page.shortCwd,
            permissionMode: page.prefs.mode || "default",
            model: page.prefs.model || "",
            strictMcp: false,
            allowedTools: "none",
            systemPrompt: "",
            mcpUrl: page.backend.url,
            idleTimeout: 1800,
            resume: ""
        }))
    }
    function sendText(text) {
        if (assistant.attachedState !== "attached") { page.createSession(text); return }
        if (!assistant.send(text)) { transcript.push({ kind: "notice", text: "The session's host did not take the message." }); return }
        // Claude Code queues a message written while a turn runs; the footer counts them.
        if (assistant.busy) page.queued = page.queued + 1
    }

    Component.onCompleted: {
        page.readBranch()
        if (page.sessionId.length > 0) assistant.attach(page.sessionId)
    }
    onIsCurrentChanged: if (isCurrent) Qt.callLater(page.focusComposer)

    Connections {
        target: assistant
        ignoreUnknownSignals: true
        function onCreated(id, error) {
            if (id.length === 0) { transcript.push({ kind: "notice", text: "The session could not be started: " + error }); page.firstPrompt = ""; return }
            page.sessionId = id
            page.sessionBound(id)
            page.registry.refresh()
            assistant.attach(id)
        }
        function onReplayDone() {
            if (page.firstPrompt.length > 0) { const text = page.firstPrompt; page.firstPrompt = ""; assistant.send(text) }
        }
        function onStarted(sessionId) { page.registry.refresh(); page.readBranch() }
        function onTurnDone(ok, cost, turns, text, durationMs) {
            page.queued = Math.max(0, page.queued - 1)
            page.lastCost = cost
            page.lastTurns = turns
            page.registry.refresh()
            if (!page.isCurrent || !page.windowActive) page.attention(ok ? "The agent finished" : "The turn failed")
        }
        function onPermissionAsked(requestId, tool, input, description, meta) {
            if (!page.isCurrent || !page.windowActive) page.attention("Claude asks to use " + tool)
        }
        function onModeChanged(mode) { page.savePrefs({ mode: mode }) }
        function onExited(code, reason, message) { if (reason !== "idle" && reason !== "stop") page.unread() }
        function onHostExited(message) { page.unread() }
        function onUserMessage(text) { page.unread() }
        function onTextFinal(text) { page.unread() }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // What this session is: where it runs, on what, and what it is doing.
        RowLayout {
            Layout.fillWidth: true
            Layout.margins: 10
            spacing: 8
            Text {
                text: page.tabTitle.length > 0 ? page.tabTitle : "Agent"
                color: page.theme.foreground
                font.pixelSize: Math.round(13 * page.theme.scale)
            }
            Text {
                Layout.fillWidth: true
                text: page.shortCwd + (page.branch.length > 0 ? "  (" + page.branch + ")" : "")
                color: page.theme.faint
                font.family: page.theme.termFont
                font.pixelSize: Math.round(10 * page.theme.scale)
                elide: Text.ElideMiddle
            }
            Text {
                text: page.stateWord
                color: transcript.pendingCount > 0 ? page.theme.gold : assistant.busy ? page.theme.accent : page.theme.faint
                font.pixelSize: Math.round(10 * page.theme.scale)
                font.letterSpacing: 0.5
            }
            Button {
                flat: true
                text: "New"
                visible: assistant.available && page.sessionId.length > 0
                ToolTip.text: "Leave this session and start another here"
                ToolTip.visible: hovered
                ToolTip.delay: 600
                onClicked: page.newSession()
            }
        }
        Rectangle { Layout.fillWidth: true; height: 1; color: page.theme.line }

        AgentTranscript {
            id: transcript
            Layout.fillWidth: true
            Layout.fillHeight: true
            assistant: assistant
            backend: page.backend
            theme: page.theme
            showThinking: page.prefs.thinking === true
            onFocusComposer: page.focusComposer()
            onOpenPath: (p) => page.openFile(p)
            onOpenLink: (link) => { if (link.indexOf("http") === 0) Qt.openUrlExternally(link) }
            onAcceptEditsWanted: assistant.changeMode("acceptEdits")
            onActivity: page.unread()
        }

        // The empty state says what this tab is for and what it will cost.
        Text {
            visible: transcript.count === 0 && assistant.available
            Layout.fillWidth: true
            Layout.margins: 12
            text: page.sessionId.length > 0
                  ? "Continuing this session."
                  : "Ask Claude Code to work in " + page.shortCwd + ". The session runs in a unit of its own, so it keeps going when Rusty is closed. Enter sends. Shift+Tab cycles the permission mode."
            color: page.theme.faint
            font.pixelSize: Math.round(12 * page.theme.scale)
            wrapMode: Text.Wrap
        }

        AgentComposer {
            id: composer
            Layout.fillWidth: true
            theme: page.theme
            available: assistant.available
            busy: assistant.busy
            attachedState: assistant.attachedState
            hostLive: page.sessionId.length > 0 && page.registry.alive(page.sessionId)
            mode: assistant.permissionMode
            model: assistant.model
            queued: page.queued
            cost: page.lastCost
            turns: page.lastTurns
            placeholder: "Message Claude in " + page.shortCwd + " — Enter sends, Shift+Enter breaks a line"
            history: transcript.userTexts()
            onSend: (text) => page.sendText(text)
            onInterrupt: assistant.interrupt()
            onModeChosen: (mode) => { assistant.changeMode(mode); page.savePrefs({ mode: mode }) }
            onModelChosen: (model) => { assistant.changeModel(model); page.savePrefs({ model: model }) }
            onStart: page.sessionId.length > 0 ? assistant.attach(page.sessionId) : page.focusComposer()
            onEscapeIdle: transcript.focusPending()
            onFocusTranscript: transcript.focusPending()
        }
    }
}
