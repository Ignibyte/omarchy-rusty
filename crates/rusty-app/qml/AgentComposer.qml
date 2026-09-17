import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import dev.ignibyte.rusty

// What is typed to a session, and what is in force while it runs: Enter sends, Shift+Enter
// breaks a line, Escape stops a turn, Up walks back through what was asked before, and
// Shift+Tab cycles the permission mode as the CLI's own key does. A message sent while the
// agent is working is queued by Claude Code, and the footer says how many are waiting.
Item {
    id: composer
    required property var theme
    property bool available: true
    property bool busy: false
    property string attachedState: "detached"
    property bool hostLive: false
    property string mode: "default"
    property string model: ""
    property int queued: 0
    property real cost: 0
    property int turns: 0
    property string placeholder: "Message Claude — Enter sends, Shift+Enter breaks a line"
    property var history: []
    property int maxLines: 8
    // Where Up has walked to; -1 is the live draft.
    property int recallAt: -1
    property string draft: ""
    readonly property var modes: ["default", "acceptEdits", "plan", "bypassPermissions"]
    readonly property var models: ["sonnet", "opus", "haiku", "fable"]
    signal send(string text)
    signal interrupt()
    signal modeChosen(string mode)
    signal modelChosen(string model)
    signal start()
    signal escapeIdle()
    signal focusTranscript()

    function focusInput() { input.forceActiveFocus() }
    function clear() { input.text = ""; recallAt = -1; draft = "" }
    function submit() {
        const text = input.text.trim()
        if (text.length === 0) return
        composer.send(text)
        composer.clear()
    }
    function recall(back) {
        const h = composer.history
        if (h.length === 0) return
        if (back) {
            if (composer.recallAt < 0) { composer.draft = input.text; composer.recallAt = h.length - 1 }
            else if (composer.recallAt > 0) composer.recallAt--
            else return
            input.text = h[composer.recallAt]
        } else {
            if (composer.recallAt < 0) return
            if (composer.recallAt < h.length - 1) { composer.recallAt++; input.text = h[composer.recallAt] }
            else { composer.recallAt = -1; input.text = composer.draft }
        }
        input.cursorPosition = input.text.length
    }
    function cycleMode() {
        const at = composer.modes.indexOf(composer.mode)
        composer.modeChosen(composer.modes[(at + 1) % composer.modes.length])
    }
    function shortModel(name) {
        if (name.length === 0) return "default"
        const known = ["sonnet", "opus", "haiku", "fable"]
        for (const k of known) if (name.indexOf(k) >= 0) return k
        return name
    }

    implicitHeight: column.implicitHeight
    ColumnLayout {
        id: column
        width: composer.width
        spacing: 0

        Rectangle { Layout.fillWidth: true; height: 1; color: composer.theme.line }

        RowLayout {
            Layout.fillWidth: true
            Layout.margins: 8
            spacing: 6
            visible: composer.available

            ScrollView {
                Layout.fillWidth: true
                Layout.maximumHeight: Math.round(composer.maxLines * 18 * composer.theme.scale)
                TextArea {
                    id: input
                    placeholderText: composer.attachedState === "detached" && composer.hostLive ? "The session is not attached — Reconnect, or send to start again"
                                   : composer.placeholder
                    wrapMode: TextEdit.Wrap
                    font.pixelSize: Math.round(13 * composer.theme.scale)
                    Keys.onPressed: (event) => {
                        if ((event.key === Qt.Key_Return || event.key === Qt.Key_Enter) && !(event.modifiers & Qt.ShiftModifier)) { composer.submit(); event.accepted = true }
                        else if (event.key === Qt.Key_Escape) { if (composer.busy) composer.interrupt(); else composer.escapeIdle(); event.accepted = true }
                        else if (event.key === Qt.Key_Backtab || (event.key === Qt.Key_Tab && (event.modifiers & Qt.ShiftModifier))) { composer.cycleMode(); event.accepted = true }
                        else if (event.key === Qt.Key_Tab) { composer.focusTranscript(); event.accepted = true }
                        else if (event.key === Qt.Key_Up && input.cursorPosition === 0) { composer.recall(true); event.accepted = true }
                        else if (event.key === Qt.Key_Down && composer.recallAt >= 0 && input.cursorPosition === input.text.length) { composer.recall(false); event.accepted = true }
                    }
                }
            }
            Button {
                text: composer.busy ? "Stop" : "Send"
                enabled: composer.busy || input.text.trim().length > 0
                onClicked: composer.busy ? composer.interrupt() : composer.submit()
            }
        }

        // What is in force, and what the session costs so far.
        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: 8
            Layout.rightMargin: 8
            Layout.bottomMargin: 6
            spacing: 8
            visible: composer.available

            Chip {
                text: composer.mode === "default" ? "asks first" : composer.mode
                danger: composer.mode === "bypassPermissions"
                tip: "The permission mode (Shift+Tab cycles it)"
                onClicked: modeMenu.popup()
                Menu {
                    id: modeMenu
                    Repeater {
                        model: composer.modes
                        delegate: MenuItem {
                            required property string modelData
                            text: modelData === "default" ? "default — ask before a write"
                                : modelData === "acceptEdits" ? "acceptEdits — let edits through"
                                : modelData === "plan" ? "plan — read and propose, change nothing"
                                : "bypassPermissions — ask nothing (dangerous)"
                            onTriggered: composer.modeChosen(modelData)
                        }
                    }
                }
            }
            Chip {
                text: composer.shortModel(composer.model)
                tip: "The model this session uses"
                onClicked: modelMenu.popup()
                Menu {
                    id: modelMenu
                    Repeater {
                        model: composer.models
                        delegate: MenuItem {
                            required property string modelData
                            text: modelData
                            onTriggered: composer.modelChosen(modelData)
                        }
                    }
                }
            }
            Text {
                visible: composer.queued > 0
                text: composer.queued + " queued"
                color: composer.theme.gold
                font.pixelSize: Math.round(10 * composer.theme.scale)
            }
            Item { Layout.fillWidth: true }
            Text {
                visible: composer.turns > 0 || composer.cost > 0
                text: (composer.turns > 0 ? composer.turns + (composer.turns === 1 ? " turn" : " turns") : "")
                    + (composer.cost > 0 ? (composer.turns > 0 ? " · " : "") + "$" + composer.cost.toFixed(3) : "")
                color: composer.theme.faint
                font.pixelSize: Math.round(10 * composer.theme.scale)
            }
            Button {
                visible: composer.attachedState === "detached"
                flat: true
                text: composer.hostLive ? "Reconnect" : "Start"
                onClicked: composer.start()
            }
        }

        // Nothing to talk to: say what is missing rather than offer a dead field.
        Text {
            visible: !composer.available
            Layout.fillWidth: true
            Layout.margins: 12
            text: "Claude Code is not installed, or systemd-run is missing: a session needs both on PATH. The terminal tabs still run any agent on the machine."
            color: composer.theme.muted
            font.pixelSize: Math.round(12 * composer.theme.scale)
            wrapMode: Text.Wrap
        }
    }

    component Chip: Rectangle {
        id: chip
        property string text: ""
        property string tip: ""
        property bool danger: false
        signal clicked()
        radius: composer.theme.radius
        color: composer.theme.panel2
        border.width: 1
        border.color: chip.danger ? composer.theme.red : composer.theme.line
        implicitWidth: chipLabel.implicitWidth + 14
        implicitHeight: chipLabel.implicitHeight + 8
        Text {
            id: chipLabel
            anchors.centerIn: parent
            text: chip.text
            color: chip.danger ? composer.theme.red : composer.theme.muted
            font.pixelSize: Math.round(10 * composer.theme.scale)
        }
        ToolTip.text: chip.tip
        ToolTip.visible: chipHover.hovered && chip.tip.length > 0
        ToolTip.delay: 600
        HoverHandler { id: chipHover }
        TapHandler { onTapped: chip.clicked() }
    }
}
