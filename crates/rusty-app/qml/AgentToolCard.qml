import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import dev.ignibyte.rusty

// One tool call, read the way the tool is used: a path for a read, a command for a shell,
// a diff for an edit, a pattern for a search, a server and a tool for anything of Rusty's
// own. The result lands in the same card, and a call waiting on a decision carries it.
Item {
    id: card
    required property var theme
    required property string name
    required property string input      // the tool's input as JSON
    required property string result
    required property bool isError
    required property string state      // streaming | done | pending
    required property string meta
    required property string answered
    property bool expanded: false
    property bool compact: false
    signal openPath(string path)
    signal decided(bool allow, string extraJson)

    readonly property var args: { try { return JSON.parse(card.input || "{}") } catch (e) { return {} } }
    readonly property var info: { try { return JSON.parse(card.meta || "{}") } catch (e) { return {} } }
    readonly property bool pending: state === "pending"
    readonly property string short: {
        const n = card.name
        if (n.startsWith("mcp__")) { const p = n.split("__"); return p.length > 2 ? p[1] + " · " + p.slice(2).join("__") : n }
        return n
    }
    readonly property string summary: {
        const a = card.args
        switch (card.name) {
        case "Bash": return a.description || a.command || ""
        case "Read": case "Write": case "Edit": case "NotebookEdit": return card.base(a.file_path || a.notebook_path || "")
        case "Grep": case "Glob": return (a.pattern || "") + (a.path ? " in " + card.base(a.path) : "")
        case "WebFetch": return a.url || ""
        case "WebSearch": return a.query || ""
        case "Task": case "Agent": return a.description || ""
        default: return ""
        }
    }
    readonly property var diffRows: {
        if (card.name !== "Edit") return []
        try { return JSON.parse(card.info.diff || "[]") } catch (e) { return [] }
    }

    // A tool's input carries only the fields that tool takes, so a missing one reads as
    // `undefined` here rather than as an empty string.
    function base(path) { const p = path || ""; return p.length > 0 ? p.slice(p.lastIndexOf("/") + 1) : "" }
    function lineCount(text) { const t = text || ""; return t.length === 0 ? 0 : t.split("\n").length }

    implicitHeight: box.implicitHeight
    Rectangle {
        id: box
        width: card.width
        implicitHeight: column.implicitHeight + 14
        radius: card.theme.radius
        color: card.theme.hover
        border.width: card.pending ? 1 : 0
        border.color: card.theme.gold

        ColumnLayout {
            id: column
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.top: parent.top
            anchors.margins: 7
            spacing: 4

            // The header: the tool, what it is about, and whether it is waiting.
            RowLayout {
                Layout.fillWidth: true
                spacing: 6
                Text {
                    text: card.pending ? "⏵" : card.state === "streaming" ? "◌" : "⚙"
                    color: card.pending ? card.theme.gold : card.theme.muted
                    font.pixelSize: Math.round(10 * card.theme.scale)
                }
                Text {
                    text: card.short
                    color: card.pending ? card.theme.gold : card.theme.muted
                    font.family: card.theme.termFont
                    font.pixelSize: Math.round(10 * card.theme.scale)
                    font.letterSpacing: 0.5
                }
                Text {
                    Layout.fillWidth: true
                    text: card.summary
                    color: card.theme.faint
                    font.pixelSize: Math.round(10 * card.theme.scale)
                    elide: Text.ElideMiddle
                }
                Icon {
                    visible: card.result.length > 0 || Object.keys(card.args).length > 0
                    name: card.expanded ? "chevron-down" : "chevron-right"
                    color: card.theme.faint
                    size: Math.round(12 * card.theme.scale)
                }
                TapHandler { onTapped: card.expanded = !card.expanded }
            }

            // A shell command, always shown: it is the thing being decided.
            Text {
                visible: card.name === "Bash" && (card.args.command || "").length > 0
                Layout.fillWidth: true
                text: "$ " + (card.args.command || "")
                color: card.theme.foreground
                font.family: card.theme.termFont
                font.pixelSize: Math.round(11 * card.theme.scale)
                wrapMode: Text.WrapAnywhere
            }

            // A path, clickable when it is a file on this machine.
            Text {
                visible: ["Read", "Write", "Edit", "NotebookEdit"].indexOf(card.name) >= 0 && (card.args.file_path || card.args.notebook_path || "").length > 0
                Layout.fillWidth: true
                text: card.args.file_path || card.args.notebook_path || ""
                color: card.theme.link
                font.family: card.theme.termFont
                font.pixelSize: Math.round(10 * card.theme.scale)
                elide: Text.ElideLeft
                TapHandler { onTapped: card.openPath(card.args.file_path || card.args.notebook_path || "") }
            }

            AgentDiff {
                visible: card.name === "Edit" && card.diffRows.length > 0
                Layout.fillWidth: true
                theme: card.theme
                rows: card.diffRows
                cap: card.compact ? 12 : 24
            }

            Text {
                visible: card.name === "Write" && (card.args.content || "").length > 0
                Layout.fillWidth: true
                text: card.expanded ? card.args.content : card.lineCount(card.args.content) + (card.lineCount(card.args.content) === 1 ? " line" : " lines")
                color: card.theme.muted
                font.family: card.theme.termFont
                font.pixelSize: Math.round(11 * card.theme.scale)
                wrapMode: Text.WrapAnywhere
            }

            // Anything else: its input as JSON, on demand.
            Text {
                visible: card.expanded && ["Bash", "Read", "Write", "Edit", "NotebookEdit"].indexOf(card.name) < 0 && Object.keys(card.args).length > 0
                Layout.fillWidth: true
                text: JSON.stringify(card.args, null, 1)
                color: card.theme.muted
                font.family: card.theme.termFont
                font.pixelSize: Math.round(10 * card.theme.scale)
                wrapMode: Text.WrapAnywhere
            }

            // What the tool answered.
            Text {
                visible: card.result.length > 0
                Layout.fillWidth: true
                text: card.expanded || card.lineCount(card.result) <= 3
                      ? (card.isError ? "↳ " : "↳ ") + card.result
                      : "↳ " + card.lineCount(card.result) + " lines" + (card.isError ? " · error" : "")
                color: card.isError ? card.theme.red : card.theme.muted
                font.family: card.theme.termFont
                font.pixelSize: Math.round(11 * card.theme.scale)
                wrapMode: Text.WrapAnywhere
                TapHandler { onTapped: card.expanded = !card.expanded }
            }

            AgentDecision {
                visible: card.pending || card.answered.length > 0
                Layout.fillWidth: true
                theme: card.theme
                suggestions: card.info.permission_suggestions || []
                answered: card.answered
                // A waiting call has no output yet, so the row's expansion is what asks
                // for a reason to refuse (Shift+N, or the card's own button).
                askingReason: card.pending && card.expanded
                onDecided: (allow, extra) => card.decided(allow, extra)
            }
        }
    }
}
