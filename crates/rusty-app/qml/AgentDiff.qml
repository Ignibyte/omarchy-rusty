import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import dev.ignibyte.rusty

// What an edit would change, as lines: a gutter character carries the meaning (`+`, `-`,
// a space) so the diff reads the same in a skin whose palette is one hue, and the tint
// behind it is a wash over the card rather than a colour of its own. Long diffs stop at
// `cap` with a line saying how many are left, because a card is not a file view.
Item {
    id: diff
    required property var theme
    // [{kind: "same"|"add"|"del", text}] from Assistant.diff.
    property var rows: []
    property int cap: 24
    property bool expanded: false
    readonly property int shown: expanded ? rows.length : Math.min(rows.length, cap)
    readonly property int hidden: rows.length - shown

    implicitHeight: column.implicitHeight
    implicitWidth: column.implicitWidth

    function glyphFor(kind) { return kind === "add" ? "+" : kind === "del" ? "−" : " " }
    function colorFor(kind) {
        const t = JSON.parse(diff.theme.tokens || "{}")
        return kind === "add" ? (t.green || diff.theme.alive) : kind === "del" ? (t.red || diff.theme.red) : diff.theme.muted
    }

    ColumnLayout {
        id: column
        width: diff.width
        spacing: 0
        Repeater {
            model: diff.shown
            delegate: Rectangle {
                id: line
                required property int index
                readonly property var row: diff.rows[index] || ({ kind: "same", text: "" })
                Layout.fillWidth: true
                implicitHeight: body.implicitHeight + 2
                color: row.kind === "same" ? "transparent" : Qt.alpha(diff.colorFor(row.kind), 0.13)
                RowLayout {
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: 6
                    Text {
                        text: diff.glyphFor(line.row.kind)
                        color: diff.colorFor(line.row.kind)
                        font.family: diff.theme.termFont
                        font.pixelSize: Math.round(11 * diff.theme.scale)
                        Layout.leftMargin: 6
                        Layout.preferredWidth: Math.round(10 * diff.theme.scale)
                    }
                    Text {
                        id: body
                        text: line.row.text.length > 0 ? line.row.text : " "
                        color: line.row.kind === "same" ? diff.theme.muted : diff.theme.foreground
                        font.family: diff.theme.termFont
                        font.pixelSize: Math.round(11 * diff.theme.scale)
                        wrapMode: Text.WrapAnywhere
                        Layout.fillWidth: true
                        Layout.rightMargin: 6
                    }
                }
            }
        }
        Text {
            visible: diff.hidden > 0
            text: "… " + diff.hidden + (diff.hidden === 1 ? " more line" : " more lines") + " — click to show"
            color: diff.theme.faint
            font.pixelSize: Math.round(10 * diff.theme.scale)
            Layout.topMargin: 4
            Layout.leftMargin: 6
            TapHandler { onTapped: diff.expanded = true }
        }
    }
}
