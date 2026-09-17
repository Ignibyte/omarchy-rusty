import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// The Settings tab. Two halves: what this machine gives the app (theme, font, scheme,
// tabs file, back end), and the settings rusty-mcp stores, editable in place.
Item {
    id: page
    required property var backend
    required property var theme
    required property var terminals

    property var settings: []
    property string notice: ""
    // The palette's command list, rendered as the Hotkeys table with the terminal keys.
    property var commands: []
    // The skins the theme offers, and the choice going back to the window.
    readonly property var skinChoices: { try { return JSON.parse(page.theme.choices) } catch (e) { return [] } }
    signal selectSkin(string source, string name)
    signal setScanlines(bool on)
    signal setTextSize(int px)
    readonly property var terminalKeys: [
        { name: "Terminal: Close terminal tab", keys: "Ctrl+Shift+W" },
        { name: "Terminal: Previous or next tab while a terminal has focus", keys: "Ctrl+PgUp / Ctrl+PgDn" },
        { name: "Terminal: Move the tab left or right", keys: "Ctrl+Shift+PgUp / Ctrl+Shift+PgDn" }
    ]
    // The Agent tab's keys live on the cards and the composer, not on the window, so the
    // palette does not list them.
    readonly property var agentKeys: [
        { name: "Agent: Send the message", keys: "Enter (Shift+Enter breaks a line)" },
        { name: "Agent: Stop the running turn", keys: "Escape in the composer" },
        { name: "Agent: Cycle the permission mode", keys: "Shift+Tab" },
        { name: "Agent: The message asked before", keys: "Up at the start of an empty line" },
        { name: "Agent: Leave the composer for the cards", keys: "Tab" },
        { name: "Agent: Allow what is asked", keys: "Y on the card" },
        { name: "Agent: Allow it and stop asking", keys: "A on the card" },
        { name: "Agent: Refuse it", keys: "N on the card (Shift+N to give a reason)" },
        { name: "Agent: Open or close a card", keys: "Enter or Space on the card" },
        { name: "Agent: Back to the latest", keys: "Ctrl+End" }
    ]
    function hotkeyRows(filter) {
        const q = filter.trim().toLowerCase()
        const rows = commands.map(function (c) { return { name: c.name, keys: c.keys || "" } }).concat(terminalKeys).concat(agentKeys)
        return q.length === 0 ? rows : rows.filter(function (r) { return r.name.toLowerCase().indexOf(q) >= 0 || r.keys.toLowerCase().indexOf(q) >= 0 })
    }

    // Every setting the back end reads, with what it means and what it defaults to.
    readonly property var known: [
        { key: "brain_vault_path", about: "The brain vault folder (an Obsidian vault). Restart the service after changing it.", fallback: "~/.rusty/brain" },
        { key: "notes_path", about: "The notes folder the notes tools use; inside the vault unless you point it elsewhere (rusty-cli notes adopt moves an older folder in).", fallback: "<vault>/notes" },
        { key: "embedding_provider", about: "auto (Ollama when it answers), ollama, openai (needs openai_api_key in Secrets), or off.", fallback: "auto" },
        { key: "embedding_model", about: "Overrides the provider's default model (nomic-embed-text, text-embedding-3-small).", fallback: "provider default" },
        { key: "ollama_url", about: "Where Ollama listens.", fallback: "http://127.0.0.1:11434" },
        { key: "pin_timeout_minutes", about: "How long an unlock of the Secrets tab lasts.", fallback: "5" },
        { key: "skills_enabled", about: "Whether the skills store is served to agents.", fallback: "true" },
        { key: "skills_path", about: "Where skills live (active/ and staging/).", fallback: "~/.rusty/skills" },
        { key: "brain_auto_enrich", about: "Enrich captured pages automatically.", fallback: "false" },
        { key: "default_workflow", about: "The default agent workflow name.", fallback: "deep" }
    ]
    function storedValue(key) { const e = settings.find(s => s.key === key); return e ? e.value : "" }
    function others() { const names = known.map(k => k.key); return settings.filter(s => names.indexOf(s.key) < 0) }

    function refresh() { backend.call("settings_list", "{}") }
    function setValue(key, value) {
        if (key.trim().length === 0) return
        backend.call("setting_set", JSON.stringify({ key: key.trim(), value: value }))
    }

    Connections {
        target: page.backend
        function onResult(id, tool, json, ok) {
            if (tool !== "settings_list" && tool !== "setting_set") return
            if (!ok) { page.notice = tool + ": " + json; return }
            page.notice = ""
            if (tool === "settings_list") page.settings = JSON.parse(json)
            else page.refresh()
        }
        function onDataChanged() { page.refresh() }
    }
    Component.onCompleted: if (backend.connected) refresh()

    Flickable {
        anchors.fill: parent
        anchors.margins: 32
        contentHeight: column.implicitHeight
        clip: true
        ColumnLayout {
            id: column
            width: parent.width
            spacing: 12

            Text { text: "Settings"; color: page.theme.foreground; font.pixelSize: Math.round(22 * page.theme.scale); font.bold: true }

            Text { text: "This machine"; color: page.theme.foreground; opacity: 0.6; font.pixelSize: Math.round(12 * page.theme.scale); font.bold: true; Layout.topMargin: 8 }
            Text {
                text: page.theme.facts
                    + "\ntabs file: " + page.terminals.tabsPath
                    + "\nback end: " + page.backend.url + " (" + page.backend.status + ")"
                color: page.theme.foreground; opacity: 0.75; font.pixelSize: Math.round(13 * page.theme.scale)
                wrapMode: Text.WrapAnywhere; Layout.fillWidth: true
            }
            Text { text: "Click an agent in the top bar, or choose one under the tab strip's +, to open it in a new tab; right-click an agent for the agent pane · drag a tab or a task's handle to reorder"; color: page.theme.foreground; opacity: 0.5; font.pixelSize: Math.round(12 * page.theme.scale); wrapMode: Text.WordWrap; Layout.fillWidth: true }
            Button { text: "Re-read theme"; onClicked: page.theme.reload() }
            RowLayout {
                spacing: 10
                Text { text: "Text size"; color: page.theme.foreground; opacity: 0.75; font.pixelSize: Math.round(13 * page.theme.scale) }
                SpinBox { from: 12; to: 18; value: page.theme.baseSize; editable: false; onValueModified: page.setTextSize(value) }
                Text { text: "pixels; Ctrl with plus, minus or zero does the same"; color: page.theme.foreground; opacity: 0.5; font.pixelSize: Math.round(12 * page.theme.scale); wrapMode: Text.WordWrap; Layout.fillWidth: true }
            }

            Text { text: "Skin"; color: page.theme.foreground; opacity: 0.6; font.pixelSize: Math.round(12 * page.theme.scale); font.bold: true; Layout.topMargin: 16 }
            Flow {
                Layout.fillWidth: true
                spacing: 6
                Repeater {
                    model: page.skinChoices
                    delegate: Rectangle {
                        required property var modelData
                        readonly property bool on: modelData.source === page.theme.source && (modelData.source === "omarchy" || modelData.name === page.theme.themeName)
                        width: chipText.implicitWidth + 20
                        height: 24
                        radius: page.theme.radius
                        color: on ? page.theme.active : page.theme.panel3
                        border.width: 1
                        border.color: on ? page.theme.accent : page.theme.line
                        Text { id: chipText; anchors.centerIn: parent; text: modelData.title; color: on ? page.theme.bright : page.theme.muted; font.pixelSize: Math.round(10 * page.theme.scale) }
                        HoverHandler { cursorShape: Qt.PointingHandCursor }
                        TapHandler { onTapped: page.selectSkin(modelData.source, modelData.name) }
                    }
                }
            }
            CheckBox { text: "CRT scanlines"; checked: page.theme.scanlines; onToggled: page.setScanlines(checked) }
            Text {
                text: "A skin of your own is a file in ~/.config/rusty/themes/<name>.toml: a [colors] table with bg, text and accent at least (panel, panel2, panel3, line, line_bright, muted, bright, accent_soft, gold, alive and red as you like), and an optional [type] table with font and radius. The face applies at the next launch."
                color: page.theme.foreground; opacity: 0.55; font.pixelSize: Math.round(11 * page.theme.scale); wrapMode: Text.WordWrap; Layout.fillWidth: true
            }

            Text { text: "Hotkeys"; color: page.theme.foreground; opacity: 0.6; font.pixelSize: Math.round(12 * page.theme.scale); font.bold: true; Layout.topMargin: 16 }
            TextField { id: keyFilter; Layout.preferredWidth: 420; placeholderText: "Filter hotkeys…" }
            Repeater {
                model: page.hotkeyRows(keyFilter.text)
                delegate: RowLayout {
                    required property var modelData
                    Layout.fillWidth: true
                    spacing: 12
                    Text { text: modelData.name; color: page.theme.foreground; font.pixelSize: Math.round(13 * page.theme.scale); elide: Text.ElideRight; Layout.preferredWidth: 440 }
                    Rectangle {
                        visible: modelData.keys.length > 0
                        radius: 4
                        color: page.theme.surface
                        border.color: page.theme.line
                        implicitWidth: keyText.implicitWidth + 14
                        implicitHeight: 22
                        Text { id: keyText; anchors.centerIn: parent; text: modelData.keys; color: page.theme.foreground; font.pixelSize: Math.round(12 * page.theme.scale); font.family: page.theme.termFont }
                    }
                    Text { visible: modelData.keys.length === 0; text: "Blank"; color: page.theme.faint; font.pixelSize: Math.round(12 * page.theme.scale) }
                }
            }

            Text { text: "Settings rusty-mcp reads"; color: page.theme.foreground; opacity: 0.6; font.pixelSize: Math.round(12 * page.theme.scale); font.bold: true; Layout.topMargin: 16 }
            Text { visible: !page.backend.connected; text: page.backend.status; color: page.theme.foreground; opacity: 0.6; font.pixelSize: Math.round(13 * page.theme.scale) }
            Repeater {
                model: page.known
                delegate: ColumnLayout {
                    required property var modelData
                    Layout.fillWidth: true
                    spacing: 2
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 12
                        Text { text: modelData.key; color: page.theme.foreground; font.pixelSize: Math.round(14 * page.theme.scale); font.family: page.theme.termFont; Layout.preferredWidth: 220; elide: Text.ElideRight }
                        TextField {
                            id: knownField
                            Layout.preferredWidth: 420
                            text: page.storedValue(modelData.key)
                            placeholderText: modelData.fallback
                            onAccepted: if (text !== page.storedValue(modelData.key)) page.setValue(modelData.key, text)
                        }
                        Text { text: knownField.text !== page.storedValue(modelData.key) ? "Enter saves" : (page.storedValue(modelData.key).length === 0 ? "default" : ""); color: page.theme.accent; font.pixelSize: Math.round(11 * page.theme.scale) }
                    }
                    Text { text: modelData.about; color: page.theme.foreground; opacity: 0.55; font.pixelSize: Math.round(11 * page.theme.scale); Layout.leftMargin: 232; wrapMode: Text.WordWrap; Layout.fillWidth: true }
                }
            }

            Text { visible: page.others().length > 0; text: "Other stored keys"; color: page.theme.foreground; opacity: 0.6; font.pixelSize: Math.round(12 * page.theme.scale); font.bold: true; Layout.topMargin: 12 }
            Repeater {
                model: page.others()
                delegate: RowLayout {
                    required property var modelData
                    Layout.fillWidth: true
                    spacing: 12
                    Text { text: modelData.key; color: page.theme.foreground; font.pixelSize: Math.round(14 * page.theme.scale); font.family: page.theme.termFont; Layout.preferredWidth: 220; elide: Text.ElideRight }
                    TextField {
                        id: valueField
                        Layout.preferredWidth: 420
                        text: modelData.value
                        placeholderText: modelData.value === "•••" ? "hidden; type a new value to replace it" : ""
                        onAccepted: if (text !== modelData.value && text !== "•••") page.setValue(modelData.key, text)
                    }
                    Text { text: valueField.text !== modelData.value ? "Enter saves" : ""; color: page.theme.accent; font.pixelSize: Math.round(11 * page.theme.scale) }
                }
            }
            RowLayout {
                Layout.fillWidth: true
                Layout.topMargin: 8
                spacing: 12
                TextField { id: newKey; Layout.preferredWidth: 220; placeholderText: "new key" }
                TextField { id: newValue; Layout.preferredWidth: 420; placeholderText: "value"; onAccepted: { page.setValue(newKey.text, text); newKey.text = ""; text = "" } }
                Button { text: "Set"; onClicked: { page.setValue(newKey.text, newValue.text); newKey.text = ""; newValue.text = "" } }
            }
            Text { text: page.notice; visible: page.notice.length > 0; color: page.theme.accent; font.pixelSize: Math.round(12 * page.theme.scale); wrapMode: Text.WordWrap; Layout.fillWidth: true }
        }
    }
}
