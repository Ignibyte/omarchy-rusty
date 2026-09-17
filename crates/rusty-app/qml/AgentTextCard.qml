import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import dev.ignibyte.rusty

// The prose of a conversation: what was asked, what was answered, what the model thought,
// what a turn cost, and the plan it wants approved. Streamed text is plain and becomes
// rich when its block ends (the transcript asks the back end to render it); nothing is
// re-rendered while it is still arriving.
Item {
    id: card
    required property var theme
    required property string kind        // user | text | thinking | notice | result
    required property string text
    required property string html
    required property string state
    required property string meta
    property bool expanded: false
    property bool compact: false
    signal linkClicked(string link)
    signal renderWanted()

    readonly property var info: { try { return JSON.parse(meta || "{}") } catch (e) { return {} } }
    readonly property bool rich: html.length > 0 && kind === "text"

    implicitHeight: box.implicitHeight
    Component.onCompleted: if (kind === "text" && state === "final" && html.length === 0) renderWanted()
    onStateChanged: if (kind === "text" && state === "final" && html.length === 0) renderWanted()

    Rectangle {
        id: box
        x: card.kind === "user" ? Math.round(28 * card.theme.scale) : 0
        width: card.width - x
        implicitHeight: column.implicitHeight + (card.kind === "notice" || card.kind === "result" ? 4 : 14)
        radius: card.theme.radius
        color: card.kind === "user" ? card.theme.active
             : card.kind === "notice" || card.kind === "result" ? "transparent"
             : card.kind === "thinking" ? "transparent" : card.theme.hover
        border.width: card.kind === "result" && card.info.ok === false ? 1 : 0
        border.color: card.theme.red

        ColumnLayout {
            id: column
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.top: parent.top
            anchors.margins: card.kind === "notice" || card.kind === "result" ? 2 : 7
            spacing: 3

            // Thinking: a line that says it happened and how much, opened on a click.
            // The thinking itself does not travel — Claude Code sends the estimate alone —
            // so the line reports that rather than pretending to a transcript.
            RowLayout {
                visible: card.kind === "thinking"
                Layout.fillWidth: true
                spacing: 4
                Icon { name: card.expanded ? "chevron-down" : "chevron-right"; color: card.theme.faint; size: Math.round(12 * card.theme.scale) }
                Text {
                    text: card.text.length > 0 ? "Thought · " + card.text : "Thinking"
                    color: card.theme.faint
                    font.pixelSize: Math.round(10 * card.theme.scale)
                    font.italic: true
                }
                Item { Layout.fillWidth: true }
                TapHandler { onTapped: card.expanded = !card.expanded }
            }

            // A turn's footer: what it cost, how long it took, or why it failed.
            Text {
                visible: card.kind === "result"
                Layout.fillWidth: true
                horizontalAlignment: Text.AlignRight
                text: {
                    const i = card.info
                    const bits = []
                    if (typeof i.durationMs === "number" && i.durationMs > 0) bits.push((i.durationMs / 1000).toFixed(1) + " s")
                    if (typeof i.turns === "number" && i.turns > 0) bits.push(i.turns + (i.turns === 1 ? " turn" : " turns"))
                    if (typeof i.cost === "number" && i.cost > 0) bits.push("$" + i.cost.toFixed(4))
                    return (i.ok === false ? "the turn failed" : "") + (bits.length > 0 ? (i.ok === false ? " · " : "") + bits.join(" · ") : "")
                }
                color: card.info.ok === false ? card.theme.red : card.theme.faint
                font.pixelSize: Math.round(10 * card.theme.scale)
            }

            // The prose itself: plain while it streams and for anything but an answer,
            // rich once the back end has rendered the finished block.
            TextEdit {
                id: plain
                Layout.fillWidth: true
                visible: !card.rich && card.kind !== "thinking" && card.text.length > 0
                text: card.text
                readOnly: true
                selectByMouse: true
                wrapMode: TextEdit.Wrap
                textFormat: TextEdit.PlainText
                color: card.kind === "notice" ? card.theme.faint
                     : card.kind === "thinking" ? card.theme.muted
                     : card.kind === "result" ? card.theme.foreground : card.theme.foreground
                selectionColor: card.theme.accent
                font.italic: card.kind === "thinking"
                font.pixelSize: Math.round((card.kind === "notice" || card.kind === "result" ? 11 : 13) * card.theme.scale)
            }
            Text {
                id: rendered
                Layout.fillWidth: true
                visible: card.rich
                text: card.html
                textFormat: Text.RichText
                wrapMode: Text.Wrap
                color: card.theme.foreground
                font.pixelSize: Math.round(13 * card.theme.scale)
                onLinkActivated: (link) => card.linkClicked(link)
            }
        }
    }
}
