import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import dev.ignibyte.rusty

// The answer to a permission request, as buttons and as keys. Allow always appears only
// when the CLI offered a suggestion to echo back; Deny takes a reason when there is one to
// give, because a denial the agent understands is worth more than a bare refusal.
Item {
    id: decision
    required property var theme
    // The `permission_suggestions` the request carried, if any.
    property var suggestions: []
    property string answered: ""
    readonly property bool canAlways: Array.isArray(suggestions) && suggestions.length > 0
    property bool askingReason: false
    signal decided(bool allow, string extraJson)

    implicitHeight: column.implicitHeight
    function allowOnce() { if (answered.length === 0) decided(true, "{}") }
    function allowAlways() { if (answered.length === 0 && canAlways) decided(true, JSON.stringify({ updatedPermissions: decision.suggestions })) }
    function deny() { if (answered.length === 0) decided(false, "{}") }
    function denyWithReason() { if (answered.length === 0) askingReason = true }
    function sendReason() {
        const why = reason.text.trim()
        askingReason = false
        reason.text = ""
        decided(false, why.length > 0 ? JSON.stringify({ message: why }) : "{}")
    }

    ColumnLayout {
        id: column
        width: decision.width
        spacing: 4
        RowLayout {
            visible: decision.answered.length === 0
            spacing: 6
            Button {
                text: "Allow"
                ToolTip.text: "Allow this once (Y)"
                ToolTip.visible: hovered
                ToolTip.delay: 600
                onClicked: decision.allowOnce()
            }
            Button {
                visible: decision.canAlways
                flat: true
                text: "Allow always"
                ToolTip.text: "Allow this and stop asking for its kind (A)"
                ToolTip.visible: hovered
                ToolTip.delay: 600
                onClicked: decision.allowAlways()
            }
            Button {
                flat: true
                text: "Deny"
                ToolTip.text: "Refuse it (N); Shift+N gives a reason"
                ToolTip.visible: hovered
                ToolTip.delay: 600
                onClicked: decision.deny()
            }
            Button { flat: true; text: "…"; ToolTip.text: "Deny with a reason (Shift+N)"; ToolTip.visible: hovered; ToolTip.delay: 600; onClicked: decision.denyWithReason() }
        }
        RowLayout {
            visible: decision.askingReason && decision.answered.length === 0
            Layout.fillWidth: true
            spacing: 6
            TextField {
                id: reason
                Layout.fillWidth: true
                placeholderText: "Why not — the agent reads this"
                font.pixelSize: Math.round(12 * decision.theme.scale)
                onAccepted: decision.sendReason()
                Keys.onEscapePressed: { decision.askingReason = false; text = "" }
            }
            Button { text: "Deny"; onClicked: decision.sendReason() }
        }
        Text {
            visible: decision.answered.length > 0
            text: decision.answered
            color: decision.theme.faint
            font.pixelSize: Math.round(11 * decision.theme.scale)
        }
    }
}
