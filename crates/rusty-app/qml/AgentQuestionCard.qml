import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import dev.ignibyte.rusty

// The agent asking rather than guessing: `AskUserQuestion` as chips, and `ExitPlanMode` as
// a plan with three ways out. Both arrive as permission requests, so both are answered by
// allowing with the input the agent will read — the questions with their answers, the plan
// as it stands — or by denying with what to do instead.
Item {
    id: card
    required property var theme
    required property string kind       // question | plan
    required property string input
    required property string html       // the plan rendered, when the back end has it
    required property string answered
    signal decided(bool allow, string inputJson, string extraJson)
    signal acceptEdits()
    signal renderWanted()

    readonly property var args: { try { return JSON.parse(card.input || "{}") } catch (e) { return {} } }
    readonly property var questions: Array.isArray(card.args.questions) ? card.args.questions : []
    readonly property string plan: card.args.plan || ""
    // answers[question] = a label, a list of labels, or what was typed
    property var answers: ({})
    property string other: ""
    property int otherFor: -1

    implicitHeight: box.implicitHeight
    Component.onCompleted: if (kind === "plan" && plan.length > 0 && html.length === 0) renderWanted()

    function pick(qi, label, multi) {
        const q = card.questions[qi]
        if (!q) return
        const a = card.answers
        if (multi) {
            const had = Array.isArray(a[q.question]) ? a[q.question] : (a[q.question] ? [a[q.question]] : [])
            const at = had.indexOf(label)
            if (at >= 0) had.splice(at, 1); else had.push(label)
            a[q.question] = had
        } else {
            a[q.question] = label
        }
        card.answers = a
        answersChanged()
    }
    function chosen(qi, label) {
        const q = card.questions[qi]
        if (!q) return false
        const a = card.answers[q.question]
        return Array.isArray(a) ? a.indexOf(label) >= 0 : a === label
    }
    function complete() {
        for (let i = 0; i < card.questions.length; i++) {
            const a = card.answers[card.questions[i].question]
            if (a === undefined || (Array.isArray(a) && a.length === 0) || a === "") return false
        }
        return card.questions.length > 0
    }
    function submit() {
        if (card.answered.length > 0 || !card.complete()) return
        card.decided(true, JSON.stringify({ questions: card.questions, answers: card.answers }), "{}")
    }
    function useOther(qi) {
        const text = card.other.trim()
        if (text.length === 0) return
        card.pick(qi, text, false)
        card.other = ""
        card.otherFor = -1
    }

    Rectangle {
        id: box
        width: card.width
        implicitHeight: column.implicitHeight + 16
        radius: card.theme.radius
        color: card.theme.panel3
        border.width: card.answered.length > 0 ? 0 : 1
        border.color: card.theme.accent

        ColumnLayout {
            id: column
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.top: parent.top
            anchors.margins: 8
            spacing: 6

            Text {
                text: card.kind === "plan" ? "The agent has a plan" : "The agent is asking"
                color: card.theme.accent
                font.family: card.theme.termFont
                font.pixelSize: Math.round(10 * card.theme.scale)
                font.letterSpacing: 0.5
            }

            // A plan, rendered when the back end has it, plain until then.
            Text {
                visible: card.kind === "plan" && card.html.length > 0
                Layout.fillWidth: true
                text: card.html
                textFormat: Text.RichText
                wrapMode: Text.Wrap
                color: card.theme.foreground
                font.pixelSize: Math.round(13 * card.theme.scale)
            }
            TextEdit {
                visible: card.kind === "plan" && card.html.length === 0
                Layout.fillWidth: true
                text: card.plan
                readOnly: true
                selectByMouse: true
                wrapMode: TextEdit.Wrap
                textFormat: TextEdit.PlainText
                color: card.theme.foreground
                selectionColor: card.theme.accent
                font.pixelSize: Math.round(13 * card.theme.scale)
            }

            // The questions, each a line and its chips.
            Repeater {
                model: card.kind === "question" ? card.questions.length : 0
                delegate: ColumnLayout {
                    id: question
                    required property int index
                    readonly property var q: card.questions[index] || ({})
                    Layout.fillWidth: true
                    spacing: 4
                    Text {
                        Layout.fillWidth: true
                        text: (question.q.header ? question.q.header + ": " : "") + (question.q.question || "")
                        color: card.theme.foreground
                        font.pixelSize: Math.round(12 * card.theme.scale)
                        wrapMode: Text.Wrap
                    }
                    Flow {
                        Layout.fillWidth: true
                        spacing: 6
                        Repeater {
                            model: Array.isArray(question.q.options) ? question.q.options.length : 0
                            delegate: Rectangle {
                                id: chip
                                required property int index
                                readonly property var option: question.q.options[index] || ({})
                                readonly property bool on: card.chosen(question.index, chip.option.label || "")
                                radius: card.theme.radius
                                color: chip.on ? card.theme.active : card.theme.panel2
                                border.width: 1
                                border.color: chip.on ? card.theme.accent : card.theme.line
                                implicitWidth: label.implicitWidth + 16
                                implicitHeight: label.implicitHeight + 10
                                Text {
                                    id: label
                                    anchors.centerIn: parent
                                    text: chip.option.label || ""
                                    color: chip.on ? card.theme.bright : card.theme.foreground
                                    font.pixelSize: Math.round(12 * card.theme.scale)
                                }
                                ToolTip.text: chip.option.description || ""
                                ToolTip.visible: hover.hovered && (chip.option.description || "").length > 0
                                ToolTip.delay: 500
                                HoverHandler { id: hover }
                                TapHandler { onTapped: if (card.answered.length === 0) card.pick(question.index, chip.option.label || "", question.q.multiSelect === true) }
                            }
                        }
                        Rectangle {
                            radius: card.theme.radius
                            color: card.theme.panel2
                            border.width: 1
                            border.color: card.theme.line
                            implicitWidth: otherLabel.implicitWidth + 16
                            implicitHeight: otherLabel.implicitHeight + 10
                            Text {
                                id: otherLabel
                                anchors.centerIn: parent
                                text: "Other…"
                                color: card.theme.muted
                                font.pixelSize: Math.round(12 * card.theme.scale)
                            }
                            TapHandler { onTapped: if (card.answered.length === 0) card.otherFor = question.index }
                        }
                    }
                    RowLayout {
                        visible: card.otherFor === question.index
                        Layout.fillWidth: true
                        spacing: 6
                        TextField {
                            Layout.fillWidth: true
                            text: card.other
                            placeholderText: "Your own answer"
                            font.pixelSize: Math.round(12 * card.theme.scale)
                            onTextChanged: card.other = text
                            onAccepted: card.useOther(question.index)
                        }
                        Button { text: "Use"; onClicked: card.useOther(question.index) }
                    }
                }
            }

            RowLayout {
                visible: card.answered.length === 0
                spacing: 6
                Button {
                    visible: card.kind === "question"
                    text: "Answer"
                    enabled: card.complete()
                    onClicked: card.submit()
                }
                Button {
                    visible: card.kind === "plan"
                    text: "Approve"
                    onClicked: card.decided(true, card.input, "{}")
                }
                Button {
                    visible: card.kind === "plan"
                    flat: true
                    text: "Approve, accept edits"
                    ToolTip.text: "Approve and let the edits through without asking for each"
                    ToolTip.visible: hovered
                    ToolTip.delay: 600
                    onClicked: { card.decided(true, card.input, "{}"); card.acceptEdits() }
                }
                Button {
                    visible: card.kind === "plan"
                    flat: true
                    text: "Keep planning"
                    onClicked: card.decided(false, card.input, JSON.stringify({ message: "Not yet — keep planning." }))
                }
            }
            Text {
                visible: card.answered.length > 0
                text: card.answered
                color: card.theme.faint
                font.pixelSize: Math.round(11 * card.theme.scale)
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
        }
    }
}
