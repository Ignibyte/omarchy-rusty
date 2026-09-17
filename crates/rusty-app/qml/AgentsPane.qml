import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import dev.ignibyte.rusty

// The sessions on this machine: what is running, what is asleep, and what each one was
// about. The list is the registry's, refreshed as its directory changes, so a session
// started from a terminal shows here too.
Item {
    id: pane
    required property var theme
    required property var registry
    property string filter: ""
    signal openSession(string id, string cwd, string title, bool inNewTab)
    signal newSession()
    signal stopSession(string id)

    readonly property var rows: {
        let all = []
        try { all = JSON.parse(pane.registry.sessions || "[]") } catch (e) { all = [] }
        const q = pane.filter.trim().toLowerCase()
        if (q.length === 0) return all
        return all.filter(function (r) {
            return (r.title || "").toLowerCase().indexOf(q) >= 0 || (r.cwd || "").toLowerCase().indexOf(q) >= 0 || (r.id || "").indexOf(q) >= 0
        })
    }
    function focusList() { list.forceActiveFocus() }
    function base(path) { const at = path.lastIndexOf("/"); return at >= 0 ? path.slice(at + 1) : path }
    function dotFor(row) {
        return row.state === "waiting" ? pane.theme.gold
             : row.state === "working" ? pane.theme.accent
             : row.alive ? pane.theme.alive : pane.theme.faint
    }
    function wordFor(row) {
        return row.state === "waiting" ? "needs input" : row.state === "working" ? "working"
             : row.alive ? "running" : "stopped"
    }
    function openRow(index, inNewTab) {
        const r = pane.rows[index]
        if (r) pane.openSession(r.id, r.cwd || "", r.title || "", inNewTab)
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 8
        spacing: 6

        RowLayout {
            Layout.fillWidth: true
            spacing: 6
            TextField {
                id: filterField
                Layout.fillWidth: true
                placeholderText: "Filter sessions"
                font.pixelSize: Math.round(12 * pane.theme.scale)
                onTextChanged: pane.filter = text
                Keys.onDownPressed: list.forceActiveFocus()
            }
            Button { flat: true; text: "New"; ToolTip.text: "Start a session here"; ToolTip.visible: hovered; ToolTip.delay: 600; onClicked: pane.newSession() }
        }

        Text {
            visible: pane.rows.length === 0
            Layout.fillWidth: true
            text: pane.filter.length > 0 ? "No session matches." : "No sessions yet. A session is a conversation with Claude Code that keeps running when Rusty is closed."
            color: pane.theme.faint
            font.pixelSize: Math.round(12 * pane.theme.scale)
            wrapMode: Text.Wrap
        }

        ListView {
            id: list
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: pane.rows.length
            spacing: 2
            keyNavigationEnabled: true
            ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }
            Keys.onPressed: (event) => {
                if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) { pane.openRow(list.currentIndex, event.modifiers & Qt.ControlModifier); event.accepted = true }
                else if (event.key === Qt.Key_N) { pane.newSession(); event.accepted = true }
                else if (event.key === Qt.Key_Slash) { filterField.forceActiveFocus(); event.accepted = true }
            }
            delegate: Rectangle {
                id: row
                required property int index
                readonly property var session: pane.rows[index] || ({})
                width: list.width
                implicitHeight: body.implicitHeight + 10
                radius: pane.theme.radius
                color: list.currentIndex === index ? pane.theme.active : "transparent"
                ColumnLayout {
                    id: body
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.leftMargin: 6
                    anchors.rightMargin: 6
                    spacing: 1
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 6
                        Rectangle {
                            width: Math.round(7 * pane.theme.scale)
                            height: width
                            radius: width / 2
                            color: pane.dotFor(row.session)
                        }
                        Text {
                            Layout.fillWidth: true
                            text: row.session.title || "Agent"
                            color: pane.theme.foreground
                            font.pixelSize: Math.round(12 * pane.theme.scale)
                            elide: Text.ElideRight
                        }
                        Text {
                            text: pane.wordFor(row.session)
                            color: pane.theme.faint
                            font.pixelSize: Math.round(9 * pane.theme.scale)
                        }
                    }
                    Text {
                        Layout.fillWidth: true
                        text: pane.base(row.session.cwd || "") + (row.session.model ? " · " + row.session.model : "")
                        color: pane.theme.faint
                        font.family: pane.theme.termFont
                        font.pixelSize: Math.round(9 * pane.theme.scale)
                        elide: Text.ElideMiddle
                    }
                }
                TapHandler { onTapped: { list.currentIndex = row.index; pane.openRow(row.index, false) } }
                TapHandler {
                    acceptedButtons: Qt.RightButton
                    onTapped: { list.currentIndex = row.index; rowMenu.popup() }
                }
                Menu {
                    id: rowMenu
                    MenuItem { text: "Open"; onTriggered: pane.openRow(row.index, false) }
                    MenuItem { text: "Open in a new tab"; onTriggered: pane.openRow(row.index, true) }
                    MenuItem { text: "Stop the session"; enabled: row.session.alive === true; onTriggered: pane.stopSession(row.session.id) }
                }
            }
        }
    }
}
