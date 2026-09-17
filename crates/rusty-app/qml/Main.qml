import QtCore
import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import dev.ignibyte.rusty

// The workspace, laid out as Obsidian lays out a vault: a ribbon of actions, the left
// sidebar (files, search), the main area with document tabs, the right sidebar
// (backlinks, outgoing links, outline, an agent), and a status bar. Pages, agent
// terminals and the built-in views (tasks, memory, skills, secrets, settings) are all
// tabs. Everything shown came through rusty-mcp; the app keeps no store of its own.
ApplicationWindow {
    id: win
    visible: true
    width: 1500
    height: 950
    title: currentNote ? currentNote.title + " - brain - Rusty" : "Rusty"
    color: theme.background
    palette.window: theme.background
    palette.windowText: theme.foreground
    palette.base: theme.surface
    palette.alternateBase: theme.background
    palette.text: theme.foreground
    palette.button: theme.hover
    palette.buttonText: theme.foreground
    palette.highlight: theme.accent
    palette.highlightedText: theme.background
    palette.placeholderText: theme.faint
    palette.mid: theme.line
    font.pixelSize: Math.round(14 * theme.scale)

    // The Rust-backed objects. Inline components below reach them as win.theme and so
    // on, because inside a Component scope an unqualified name finds the component's
    // own property of that name first.
    Theme { id: themeObj }
    Terminals { id: terminalsObj }
    Backend { id: backendObj }
    readonly property var theme: themeObj
    readonly property var terminals: terminalsObj
    readonly property var backend: backendObj
    Tools { id: tools }

    // Remembered between runs: the window through Qt's settings, the workspace state
    // through a JSON file the Rust side keeps (see Terminals.loadState).
    Settings {
        id: win_settings
        category: "window"
        property int width: 1500
        property int height: 950
        property int lastTab: 0
    }
    onWidthChanged: if (visible) win_settings.width = width
    onHeightChanged: if (visible) win_settings.height = height
    // The sessions on this machine (TICKET-031); each surface holds its own client.
    // Named `registry` because `agents` is already the agent CLIs found on PATH.
    Agents { id: agentRegistry }
    readonly property var registry: agentRegistry

    QtObject {
        id: ui
        property int lastTab: win_settings.lastTab
        property int leftWidth: 300
        property int rightWidth: 320
        property bool leftOpen: true
        property bool rightOpen: true
        property string leftPane: "files"
        property string rightPane: "backlinks"
        property string expanded: "{}"
        property string paneProgram: ""
        property string graph: "{}"
        property string bookmarks: "[]"
        property string roots: "[]"
        property string theme: ""
        property int textSize: 0
        // The Skills page's split and which of its sections are open, as JSON (TICKET-023).
        property string skillsLayout: ""
        // The agent pane's session id per page, as JSON (TICKET-025).
        property string agentSessions: "{}"
        // The editing mode a page opens in on Ctrl+E: "live" or "source" (TICKET-028).
        property string editMode: "live"
        property bool loaded: false
        onLastTabChanged: win_settings.lastTab = lastTab
        onLeftWidthChanged: save()
        onRightWidthChanged: save()
        onLeftOpenChanged: save()
        onSkillsLayoutChanged: save()
        onAgentSessionsChanged: save()
        onEditModeChanged: save()
        onRightOpenChanged: save()
        onLeftPaneChanged: save()
        onRightPaneChanged: save()
        onExpandedChanged: save()
        onPaneProgramChanged: save()
        onGraphChanged: save()
        onBookmarksChanged: save()
        onThemeChanged: save()
        onTextSizeChanged: save()
        // Written with the `ui.` prefix throughout: an unqualified `rightPane` here would
        // find the sidebar item of that id before this object's property.
        function load() {
            try {
                const s = JSON.parse(terminals.loadState())
                if (typeof s.leftWidth === "number") ui.leftWidth = s.leftWidth
                if (typeof s.rightWidth === "number") ui.rightWidth = s.rightWidth
                if (typeof s.leftOpen === "boolean") ui.leftOpen = s.leftOpen
                if (typeof s.rightOpen === "boolean") ui.rightOpen = s.rightOpen
                if (typeof s.leftPane === "string") ui.leftPane = s.leftPane
                if (typeof s.rightPane === "string") ui.rightPane = s.rightPane
                if (typeof s.expanded === "string") ui.expanded = s.expanded
                if (typeof s.paneProgram === "string") ui.paneProgram = s.paneProgram
                if (typeof s.graph === "string") ui.graph = s.graph
                if (typeof s.bookmarks === "string") ui.bookmarks = s.bookmarks
                if (typeof s.roots === "string") ui.roots = s.roots
                if (typeof s.theme === "string") ui.theme = s.theme
                if (typeof s.skillsLayout === "string") ui.skillsLayout = s.skillsLayout
                if (typeof s.agentSessions === "string") ui.agentSessions = s.agentSessions
                if (s.editMode === "live" || s.editMode === "source") ui.editMode = s.editMode
                if (typeof s.textSize === "number" && s.textSize > 0) { theme.setTextSize(s.textSize); ui.textSize = theme.baseSize }
            } catch (e) {}
            ui.loaded = true
        }
        function save() { if (ui.loaded) saveTimer.restart() }
        function write() {
            terminals.saveState(JSON.stringify({ leftWidth: ui.leftWidth, rightWidth: ui.rightWidth, leftOpen: ui.leftOpen, rightOpen: ui.rightOpen,
                                                 leftPane: ui.leftPane, rightPane: ui.rightPane, expanded: ui.expanded, paneProgram: ui.paneProgram, graph: ui.graph, bookmarks: ui.bookmarks, roots: ui.roots, theme: ui.theme, textSize: ui.textSize, skillsLayout: ui.skillsLayout, agentSessions: ui.agentSessions, editMode: ui.editMode }))
        }
    }
    Timer { id: saveTimer; interval: 400; onTriggered: ui.write() }

    // kind, title, slug, session, program, cwd, pinned are saved; unread and termTitle
    // live only while running.
    ListModel { id: tabs }

    // The machine, for the top bar.
    Desk { id: desk }
    // Folders on the machine, for the explorer's roots and the file tabs (TICKET-016).
    Folders { id: diskFolders }
    FolderDialog {
        id: rootDialog
        title: "Add a folder"
        currentFolder: "file://" + diskFolders.home
        onAccepted: win.addRoot(String(selectedFolder))
    }
    // Importing an Obsidian vault (TICKET-026): a picker, a plan to read, then the import.
    FolderDialog {
        id: importPicker
        title: "Import an Obsidian vault"
        currentFolder: "file://" + diskFolders.home
        onAccepted: win.planImport(String(selectedFolder).replace(/^file:\/\//, ""))
    }
    function planImport(path) { importDialog.path = path; importDialog.plan = null; importDialog.report = null; importDialog.open(); ask("brain_import_plan", { path: path }, "importPlan") }
    function runImport() { if (importDialog.path.length > 0 && importDialog.report === null) ask("brain_import", { path: importDialog.path }, "import") }
    // The plan's bookmarks in the app's shape, added when not already there.
    function mergeBookmarks(items) {
        const list = bookmarkList.slice()
        let added = 0
        for (const b of items || []) {
            const entry = b.kind === "search" ? { kind: "search", query: b.query, title: b.title }
                : b.kind === "heading" ? { kind: "heading", path: b.path, heading: b.heading, title: b.title }
                : { kind: b.kind, path: b.path, title: b.title }
            if (bookmarkIndex(entry) < 0) { list.push(entry); added++ }
        }
        if (added > 0) ui.bookmarks = JSON.stringify(list)
        return added
    }
    function importDetails(p) {
        const lines = []
        const section = function (title, items, none) { lines.push(title); if (items.length === 0) lines.push("  " + none); for (const i of items) lines.push("  " + i); lines.push("") }
        section("Pages", p.pages, "none")
        section("Attachments", p.attachments, "none")
        section("Collisions (already in the brain, left as they are)", p.collisions, "none")
        section("Unresolved links", p.unresolved_links, "none")
        section("Tags", p.tags.map(function (t) { return "#" + t }), "none")
        section("Bookmarks", p.bookmarks.map(function (b) { return b.kind + ": " + b.title + (b.query ? " (" + b.query + ")" : b.path ? " (" + b.path + (b.heading ? "#" + b.heading : "") + ")" : "") }), "none")
        section("Bookmarks not carried", p.bookmarks_skipped, "none")
        return lines.join("\n")
    }
    Dialog {
        id: importDialog
        title: "Import an Obsidian vault"
        modal: true
        anchors.centerIn: parent
        property string path: ""
        property var plan: null
        property var report: null
        readonly property var shown: report ? report.plan : plan
        function count(n, word) { return n + " " + word + (n === 1 ? "" : "s") }
        footer: DialogButtonBox {
            Button { text: "Import"; DialogButtonBox.buttonRole: DialogButtonBox.ActionRole; enabled: importDialog.plan !== null && importDialog.report === null && (importDialog.plan.pages.length + importDialog.plan.attachments.length) > 0; onClicked: win.runImport() }
            Button { text: importDialog.report ? "Close" : "Cancel"; DialogButtonBox.buttonRole: DialogButtonBox.RejectRole; onClicked: importDialog.close() }
        }
        ColumnLayout {
            spacing: 8
            width: 560
            Label { text: importDialog.path; elide: Text.ElideMiddle; Layout.fillWidth: true; font.family: theme.termFont }
            Label { visible: importDialog.shown === null; text: win.notice.length > 0 ? win.notice : "Reading the vault…"; wrapMode: Text.Wrap; Layout.fillWidth: true }
            Label {
                visible: importDialog.shown !== null
                wrapMode: Text.Wrap
                Layout.fillWidth: true
                text: importDialog.report
                    ? "Imported " + importDialog.count(importDialog.report.imported_pages, "page") + " and " + importDialog.count(importDialog.report.imported_attachments, "attachment") + "; " + importDialog.count(importDialog.report.links_rewritten, "link") + " rewritten to vault paths; " + importDialog.count(importDialog.report.plan.bookmarks.length, "bookmark") + " added. The report is " + importDialog.report.report_slug + "."
                    : importDialog.plan
                        ? "Will bring in " + importDialog.count(importDialog.plan.pages.length, "page") + " in " + importDialog.count(importDialog.plan.folders.length, "folder") + ", " + importDialog.count(importDialog.plan.attachments.length, "attachment") + ", " + importDialog.count(importDialog.plan.tags.length, "tag") + " and " + importDialog.count(importDialog.plan.bookmarks.length, "bookmark") + "; " + importDialog.count(importDialog.plan.collisions.length, "collision") + " skipped; " + importDialog.count(importDialog.plan.unresolved_links.length, "link") + " unresolved. The vault is read and never written."
                        : ""
            }
            ScrollView {
                Layout.fillWidth: true
                Layout.preferredHeight: 240
                TextArea { readOnly: true; selectByMouse: true; text: importDialog.shown ? win.importDetails(importDialog.shown) : ""; font.family: theme.termFont; font.pixelSize: Math.round(11 * theme.scale); wrapMode: TextEdit.Wrap }
            }
        }
    }
    // Capturing a source by URL (TICKET-027): the back end fetches, reads and keeps it
    // as a page under sources/; the page opens when it is in.
    Dialog {
        id: captureDialog
        title: "Capture a source"
        modal: true
        anchors.centerIn: parent
        standardButtons: Dialog.Ok | Dialog.Cancel
        property bool busy: false
        function openFresh() { urlField.text = ""; busy = false; open(); urlField.forceActiveFocus() }
        onAccepted: { const url = urlField.text.trim(); if (url.length > 0) { busy = true; win.ask("source_capture", { url: url }, "sourceCaptured") } }
        ColumnLayout {
            spacing: 8
            width: 480
            Label { text: "A web page, a PDF, a markdown or a text file. The readable text is kept as a page under sources/ and indexed like any page."; wrapMode: Text.Wrap; Layout.fillWidth: true }
            TextField { id: urlField; Layout.fillWidth: true; placeholderText: "https://…"; onAccepted: captureDialog.accept() }
        }
    }
    Timer { interval: 2000; repeat: true; running: true; onTriggered: desk.refresh() }

    property var tree: null
    property var pageList: []
    property var tags: []
    property var titles: ({})
    property var agents: []
    property var currentNote: null
    property string notice: ""
    property var pending: ({})
    readonly property bool terminalFocused: activeFocusItem !== null && activeFocusItem !== undefined && activeFocusItem.objectName === "term"
    readonly property var agentNames: ({ claude: "Claude Code", codex: "Codex", gemini: "Gemini", aider: "Aider", opencode: "OpenCode", shell: "Shell" })
    readonly property var agentGlyphs: ({ claude: "✳", codex: "◇", gemini: "✦", aider: "⌁", opencode: "◈", shell: "$" })
    readonly property var viewTitles: ({ tasks: "Tasks", memory: "Memory", skills: "Skills", secrets: "Secrets", settings: "Settings", graph: "Graph view", decisions: "Decisions" })
    // The page a local graph follows: the last page tab that was current.
    property string lastPageSlug: ""
    readonly property var graphSettings: { try { return JSON.parse(ui.graph || "{}") } catch (e) { return ({}) } }
    function saveGraphSettings(s) { ui.graph = JSON.stringify(s) }
    // Folder roots: `[{path, name}]` under `roots` in the state, per machine.
    readonly property var rootList: { try { return JSON.parse(ui.roots || "[]") } catch (e) { return [] } }
    // The agent pane's session per page, from the state.
    function sessionMap() { try { const m = JSON.parse(ui.agentSessions || "{}"); return m !== null && typeof m === "object" ? m : {} } catch (e) { return {} } }
    function addRoot(chosen) {
        const p = String(chosen).replace(/^file:\/\//, "").replace(/\/+$/, "")
        if (!p.startsWith("/")) return
        const list = rootList.filter(function (r) { return r.path !== p })
        list.push({ path: p, name: diskFolders.baseName(p) })
        ui.roots = JSON.stringify(list)
    }
    function removeRoot(path) { ui.roots = JSON.stringify(rootList.filter(function (r) { return r.path !== path })) }
    // A file from a root: a tab for markdown, text and images; the desktop for the rest.
    function openFile(path) {
        const existing = findTab(function (t) { return t.kind === "file" && t.slug === path })
        if (existing >= 0) { stack.currentIndex = existing; return }
        if (diskFolders.kindOf(path) === "other") { diskFolders.openExternally(path); return }
        tabs.append({ kind: "file", title: diskFolders.baseName(path), slug: path, session: "", program: "", cwd: "", pinned: false, unread: false, termTitle: "" })
        stack.currentIndex = tabs.count - 1
        saveTabs()
    }
    // The skin: a preset, the Omarchy theme, or a file; kept in the state and handed
    // to the Rust theme, which repaints every token.
    function selectTheme(source, name) { ui.theme = JSON.stringify({ source: source, name: name, scanlines: theme.scanlines }); theme.select(ui.theme) }
    function setScanlines(on) { ui.theme = JSON.stringify({ source: theme.source, name: theme.themeName, scanlines: on }); theme.select(ui.theme) }
    property int unresolvedCount: 0
    // Bookmarks: files, folders, searches and headings, kept in the workspace state as
    // one JSON array. Adding one that exists removes it, so every entry point toggles.
    readonly property var bookmarkList: { try { return JSON.parse(ui.bookmarks) } catch (e) { return [] } }
    function bookmarkKey(b) { return b.kind + ":" + (b.kind === "search" ? b.query : b.kind === "heading" ? b.path + "#" + b.heading : b.path) }
    function bookmarkIndex(b) { const k = bookmarkKey(b); return bookmarkList.findIndex(function (x) { return bookmarkKey(x) === k }) }
    function isBookmarkedPath(slug) { return bookmarkList.some(function (x) { return x.kind === "file" && x.path === slug }) }
    function addBookmark(b) {
        const list = bookmarkList.slice()
        const i = bookmarkIndex(b)
        if (i >= 0) { list.splice(i, 1); win.notice = "bookmark removed" } else { list.push(b); win.notice = "bookmarked " + b.title }
        ui.bookmarks = JSON.stringify(list)
    }
    function removeBookmark(i) { const list = bookmarkList.slice(); list.splice(i, 1); ui.bookmarks = JSON.stringify(list) }
    function retitleBookmark(i, title) { const list = bookmarkList.slice(); list[i].title = title; ui.bookmarks = JSON.stringify(list) }
    function bookmarkCurrentPage() { if (currentNote) addBookmark({ kind: "file", path: currentNote.slug, title: currentNote.title }) }
    function openBookmark(b) {
        if (b.kind === "file") openPage(b.path, false)
        else if (b.kind === "folder") { showLeft("files"); explorer.revealFolder(b.path) }
        else if (b.kind === "search") searchFor(b.query)
        else if (b.kind === "heading") {
            openPage(b.path, false)
            Qt.callLater(function () { if (currentNote && currentNote.slug === b.path) currentNote.scrollToHeadingText(b.heading) })
        }
    }
    readonly property var tokens: JSON.parse(theme.tokens || "{}")
    function agentLabel(p) { return agentNames[p] || p }
    function agentGlyph(p) { return agentGlyphs[p] || "▸" }
    function tabLabelFor(p) {
        if (p.startsWith("run:")) return "Run " + p.slice(p.lastIndexOf("/") + 1).replace(/\.sh$/, "")
        return p === "claude" ? "Claude" : (p.charAt(0).toUpperCase() + p.slice(1))
    }

    function ask(tool, args, kind) {
        const id = backend.call(tool, JSON.stringify(args))
        const p = pending; p[id] = kind; pending = p
    }
    function refreshData() {
        ask("brain_tree", {}, "tree")
        ask("brain_list_pages", { limit: 100000 }, "pages")
        ask("brain_tags", {}, "tags")
        ask("brain_unresolved", {}, "unresolved")
    }

    // ── Tabs ──────────────────────────────────────────────────────────────

    function currentTab() { return stack.currentIndex >= 0 && stack.currentIndex < tabs.count ? tabs.get(stack.currentIndex) : null }
    function findTab(pred) { for (let i = 0; i < tabs.count; i++) if (pred(tabs.get(i))) return i; return -1 }
    function baseName(slug) { return slug.slice(slug.lastIndexOf("/") + 1) }

    // Open a page: in the current page tab unless it is pinned, else in a new tab.
    function openPage(slug, newTab) {
        const existing = findTab(function (t) { return t.kind === "page" && t.slug === slug })
        if (existing >= 0 && !newTab) { stack.currentIndex = existing; return }
        const cur = currentTab()
        if (!newTab && cur && cur.kind === "page" && !cur.pinned && currentNote) { currentNote.open(slug); return }
        tabs.append({ kind: "page", title: baseName(slug), slug: slug, session: "", program: "", cwd: "", pinned: false, unread: false, termTitle: "" })
        stack.currentIndex = tabs.count - 1
        saveTabs()
    }
    function openView(kind) {
        const existing = findTab(function (t) { return t.kind === kind })
        if (existing >= 0) { stack.currentIndex = existing; return }
        tabs.append({ kind: kind, title: viewTitles[kind] || kind, slug: "", session: "", program: "", cwd: "", pinned: false, unread: false, termTitle: "" })
        stack.currentIndex = tabs.count - 1
        saveTabs()
    }
    // The global graph is one tab; a local graph is a tab of its own that follows the
    // page that was open when it was made and every page opened after.
    function openGraph(local) {
        if (!local) {
            const existing = findTab(function (t) { return t.kind === "graph" && t.slug === "" })
            if (existing >= 0) { stack.currentIndex = existing; return }
            tabs.append({ kind: "graph", title: "Graph view", slug: "", session: "", program: "", cwd: "", pinned: false, unread: false, termTitle: "" })
        } else {
            const slug = currentNote ? currentNote.slug : lastPageSlug
            if (slug.length === 0) { win.notice = "open a page first"; return }
            const existing = findTab(function (t) { return t.kind === "graph" && t.slug !== "" })
            if (existing >= 0) { stack.currentIndex = existing; return }
            tabs.append({ kind: "graph", title: "Local graph", slug: slug, session: "", program: "", cwd: "", pinned: false, unread: false, termTitle: "" })
        }
        stack.currentIndex = tabs.count - 1
        saveTabs()
    }
    function takenSessions() {
        const names = []
        for (let i = 0; i < tabs.count; i++) if (tabs.get(i).session.length > 0) names.push(tabs.get(i).session)
        for (const s of terminals.sessions()) names.push(s)
        return names
    }
    // Open an agent in a new tab, named after it ("Claude", "Claude 2", ...).
    function openTerminal(program, name, session, cwd) {
        const base = name && name.trim().length > 0 ? name.trim() : tabLabelFor(program)
        const names = []
        for (let i = 0; i < tabs.count; i++) names.push(tabs.get(i).title)
        let label = base
        for (let n = 2; names.indexOf(label) >= 0; n++) label = base + " " + n
        const sess = session && session.length > 0 ? session : terminals.sessionName(label, takenSessions())
        tabs.append({ kind: "terminal", title: label, slug: "", session: sess, program: program, cwd: (cwd || "").trim(), pinned: false, unread: false, termTitle: "" })
        stack.currentIndex = tabs.count - 1
        saveTabs()
    }
    function closeTab(i, endSession) {
        if (i < 0 || i >= tabs.count) return
        const t = tabs.get(i)
        if (t.pinned) { win.notice = "unpin the tab first"; return }
        const sess = t.session
        tabs.remove(i)
        saveTabs()
        if (endSession && sess.length > 0) terminals.endSession(sess)
        if (stack.currentIndex >= tabs.count) stack.currentIndex = tabs.count - 1
        updateCurrent()
    }
    function closeOthers(i) {
        for (let k = tabs.count - 1; k >= 0; k--) if (k !== i && !tabs.get(k).pinned) tabs.remove(k)
        stack.currentIndex = findTab(function (t) { return true })
        saveTabs()
        updateCurrent()
    }
    function pinToggle(i) { if (i >= 0 && i < tabs.count) { tabs.setProperty(i, "pinned", !tabs.get(i).pinned); saveTabs() } }
    function renameTab(i, name) {
        if (i < 0 || i >= tabs.count || name.trim().length === 0) return
        tabs.setProperty(i, "title", name.trim())
        saveTabs()
    }
    function moveTab(from, to) {
        if (from < 0 || from >= tabs.count || to < 0 || to >= tabs.count || from === to) return
        tabs.move(from, to, 1)
        saveTabs()
        stack.currentIndex = to
    }
    function nextTab() { if (tabs.count > 0) stack.currentIndex = (stack.currentIndex + 1) % tabs.count }
    function prevTab() { if (tabs.count > 0) stack.currentIndex = (stack.currentIndex + tabs.count - 1) % tabs.count }
    function saveTabs() {
        const out = []
        for (let i = 0; i < tabs.count; i++) {
            const t = tabs.get(i)
            out.push({ kind: t.kind, title: t.title, slug: t.slug, session: t.session, program: t.program, cwd: t.cwd, pinned: t.pinned })
        }
        terminals.save(JSON.stringify(out))
    }
    function tabNavigated(i, slug, title) {
        if (i < 0 || i >= tabs.count) return
        if (tabs.get(i).slug !== slug) { tabs.setProperty(i, "slug", slug); saveTabs() }
        tabs.setProperty(i, "title", baseName(slug))
        if (i === stack.currentIndex) { explorer.currentSlug = slug; lastPageSlug = slug }
    }
    function markUnread(i) {
        if (i === stack.currentIndex || i < 0 || i >= tabs.count) return
        if (!tabs.get(i).unread && theme.debug) console.log("rusty: unread tab", i, tabs.get(i).title)
        tabs.setProperty(i, "unread", true)
    }
    function attention(i, message) {
        if (i < 0 || i >= tabs.count) return
        terminals.notify(tabs.get(i).title, message)
        if (theme.debug) console.log("rusty: attention in tab", i)
    }
    function setTermTitle(i, t) { if (i >= 0 && i < tabs.count) tabs.setProperty(i, "termTitle", t) }
    function updateCurrent() {
        const h = hosts.itemAt(stack.currentIndex)
        currentNote = (h && h.kind === "page") ? h.item : null
        explorer.currentSlug = currentNote ? currentNote.slug : ""
        if (currentNote && currentNote.slug.length > 0) lastPageSlug = currentNote.slug
        const t = currentTab()
        if (t && t.kind === "terminal") tabs.setProperty(stack.currentIndex, "unread", false)
    }

    // ── Vault actions ─────────────────────────────────────────────────────

    function newNote() {
        const folder = currentNote ? currentNote.folder : ""
        ask("brain_new_page", { folder: folder }, "created")
    }
    function createPage(name) { ask("brain_new_page", { folder: "", name: name }, "created") }
    function todayNote() { ask("brain_daily_note", {}, "daily") }
    function capture(text, target) { if (text.trim().length > 0) ask("brain_capture", { text: text.trim(), target: target }, "captured") }
    function appendTimeline(text) {
        if (!currentNote || text.trim().length === 0) return
        ask("brain_add_timeline", { slug: currentNote.slug, summary: text.trim() }, "appended")
    }
    function searchFor(q) { ui.leftOpen = true; ui.leftPane = "search"; searchPane.searchFor(q) }
    function showLeft(pane) { ui.leftOpen = true; ui.leftPane = pane; if (pane === "search") searchPane.focusEntry(); else if (pane === "files") explorerList.forceActiveFocus() }
    // The base text size: the theme clamps it, the state keeps it.
    function setTextSize(n) { theme.setTextSize(n); ui.textSize = theme.baseSize }
    function showRight(pane) { ui.rightOpen = true; ui.rightPane = pane; rightPane.current = pane; if (pane === "agent") rightPane.focusAgent() }

    Connections {
        target: backend
        function onResult(id, tool, json, ok) {
            const kind = win.pending[id]
            if (kind === undefined) return
            const p = win.pending; delete p[id]; win.pending = p
            if (!ok) { win.notice = tool + ": " + json; return }
            win.notice = ""
            switch (kind) {
            case "tree": win.tree = JSON.parse(json); break
            case "tags": win.tags = JSON.parse(json); break
            case "unresolved": { const u = JSON.parse(json); win.unresolvedCount = Array.isArray(u) ? u.length : 0; break }
            case "pages": {
                const list = JSON.parse(json)
                win.pageList = list
                const map = {}
                for (const p of list) map[p.slug] = p.title
                win.titles = map
                break
            }
            case "created": { const slug = JSON.parse(json); win.openPage(slug, false); Qt.callLater(function () { if (win.currentNote && win.currentNote.slug === slug) win.currentNote.editTitle() }); break }
            case "daily": win.openPage(JSON.parse(json).slug, false); break
            case "captured": win.notice = "captured to " + JSON.parse(json).slug; break
            case "appended": win.notice = "appended to the timeline"; break
            case "importPlan": importDialog.plan = JSON.parse(json); break
            case "sourceCaptured": { const page = JSON.parse(json); win.openPage(page.slug, false); win.notice = (page.frontmatter && page.frontmatter.status === "failed" ? "capture failed: " : "captured ") + page.slug; break }
            case "import": { const r = JSON.parse(json); importDialog.report = r; const n = win.mergeBookmarks(r.plan.bookmarks); win.refreshData(); win.notice = "imported " + r.imported_pages + " pages" + (n > 0 ? ", " + n + " bookmarks added" : ""); break }
            }
        }
        function onDataChanged() { win.refreshData() }
    }

    Component.onCompleted: {
        win.width = win_settings.width
        win.height = win_settings.height
        ui.load()
        theme.watch()
        agentRegistry.watch()
        backend.start()
        agents = terminals.programs()
        rightPane.current = ui.rightPane
        try { explorer.expanded = JSON.parse(ui.expanded) } catch (e) { explorer.expanded = ({}) }
        const saved = JSON.parse(terminals.load())
        for (const t of saved) {
            const kind = t.kind || "terminal"
            tabs.append({ kind: kind, title: t.title || t.name || (kind === "page" ? baseName(t.slug || "") : viewTitles[kind] || kind), slug: t.slug || "",
                          session: t.session || "", program: t.program || "", cwd: t.cwd || "", pinned: !!t.pinned, unread: false, termTitle: "" })
        }
        const wanted = theme.startTab >= 0 ? theme.startTab : win_settings.lastTab
        stack.currentIndex = Math.max(0, Math.min(wanted, tabs.count - 1))
        updateCurrent()
    }

    // `RUSTY_SHOT=<png>`: grab the window once it has settled, then quit. Screenshots for
    // the docs and the record come from here, against a scratch vault. `RUSTY_SHOT_SCENE`
    // sets the scene first: `switcher`, `palette`, `edit`, `right:<pane>`, `left:<pane>`.
    Timer {
        id: shot
        running: theme.shotPath.length > 0
        interval: Math.max(200, theme.shotDelay - 900)
        // A scene path: absolute as given, anything else under `HOME`.
        function resolve(f) { return f.startsWith("/") ? f : theme.homeDir + "/" + f }
        onTriggered: {
            for (const part of theme.shotScene.split(",")) {
                const p = part.trim()
                if (p === "switcher") switcher.show()
                else if (p === "palette") palette.show()
                else if (p === "rename") renameDialog.openFor(stack.currentIndex)
                else if (p === "plus") plusMenu.popup(plusButton, 0, plusButton.height)
                else if (p.startsWith("skills:")) ui.skillsLayout = JSON.stringify({ width: 300, skills: p.slice(7) !== "collapse-skills", scripts: p.slice(7) !== "collapse-scripts" })
                else if (p === "file:edit") Qt.callLater(function () { const h = hosts.itemAt(stack.currentIndex); if (h && h.kind === "file" && h.item) h.item.toggleEditing() })
                else if (p.startsWith("file:")) win.openFile(shot.resolve(p.slice(5)))
                else if (p.startsWith("root:")) { const f = shot.resolve(p.slice(5)); win.addRoot(f); Qt.callLater(function () { explorer.expandPath(f) }) }
                else if (p.startsWith("expand:")) explorer.expandPath(shot.resolve(p.slice(7)))
                else if (p.startsWith("tagfield:")) { if (win.currentNote) win.currentNote.focusTagAdd(p.slice(9)) }
                else if (p.startsWith("agent:ask:")) { win.showRight("agent"); rightPane.askAgent(p.slice(10)) }
                else if (p.startsWith("import:")) win.planImport(shot.resolve(p.slice(7)))
                else if (p === "edit" && win.currentNote) { win.currentNote.editing = true }
                else if (p.startsWith("live:") && win.currentNote) { ui.editMode = "live"; win.currentNote.editing = true; const n = parseInt(p.slice(5)); Qt.callLater(function () { win.currentNote.editSection(isNaN(n) ? 0 : n, 0) }) }
                else if (p.startsWith("right:")) win.showRight(p.slice(6))
                else if (p.startsWith("left:")) win.showLeft(p.slice(5))
                else if (p.startsWith("search:")) win.searchFor(p.slice(7))
                else if (p.startsWith("open:")) win.openPage(p.slice(5), false)
                else if (p.startsWith("view:")) win.openView(p.slice(5))
                else if (p.startsWith("theme:")) { const parts = p.slice(6).split(":"); win.selectTheme(parts[0], parts[1] || "") }
                else if (p === "graph") win.openGraph(false)
                else if (p === "localgraph") win.openGraph(true)
                else if (p.startsWith("tab:")) stack.currentIndex = parseInt(p.slice(4))
            }
        }
    }
    Timer {
        running: theme.shotPath.length > 0
        interval: theme.shotDelay
        onTriggered: {
            const saved = tools.grabWindow(win, theme.shotPath)
            console.log("rusty: shot", theme.shotPath, saved ? "saved" : "not saved")
            Qt.quit()
        }
    }

    // ── Keys (Obsidian's defaults; suspended while a terminal has focus) ────

    Shortcut { sequences: ["Ctrl+O"]; enabled: !win.terminalFocused; onActivated: switcher.show() }
    Shortcut { sequences: ["Ctrl+P"]; enabled: !win.terminalFocused; onActivated: palette.show() }
    Shortcut { sequences: ["Ctrl+T"]; enabled: !win.terminalFocused; onActivated: switcher.show() }
    Shortcut { sequences: ["Ctrl+N"]; enabled: !win.terminalFocused; onActivated: win.newNote() }
    Shortcut { sequences: ["Ctrl+E"]; enabled: !win.terminalFocused && win.currentNote !== null; onActivated: win.currentNote.toggleEditing() }
    Shortcut { sequences: ["Ctrl+S"]; enabled: !win.terminalFocused && win.currentNote !== null; onActivated: win.currentNote.save() }
    Shortcut { sequences: ["Ctrl+W"]; enabled: !win.terminalFocused; onActivated: win.closeTab(stack.currentIndex, false) }
    Shortcut { sequences: ["Ctrl+Tab"]; enabled: !win.terminalFocused; onActivated: win.nextTab() }
    Shortcut { sequences: ["Ctrl+Shift+Tab"]; enabled: !win.terminalFocused; onActivated: win.prevTab() }
    Shortcut { sequences: ["Ctrl+PgDown"]; onActivated: win.nextTab() }
    Shortcut { sequences: ["Ctrl+PgUp"]; onActivated: win.prevTab() }
    Shortcut { sequences: ["Ctrl+Shift+F"]; enabled: !win.terminalFocused; onActivated: win.showLeft("search") }
    Shortcut { sequences: ["Ctrl+,"]; enabled: !win.terminalFocused; onActivated: win.openView("settings") }
    Shortcut { sequences: ["Ctrl+G"]; enabled: !win.terminalFocused; onActivated: win.openGraph(false) }
    Shortcut { sequences: ["Ctrl+D"]; enabled: !win.terminalFocused && win.currentNote !== null; onActivated: win.bookmarkCurrentPage() }
    Shortcut { sequences: ["Ctrl+=", "Ctrl++"]; enabled: !win.terminalFocused; onActivated: win.setTextSize(theme.baseSize + 1) }
    Shortcut { sequences: ["Ctrl+-"]; enabled: !win.terminalFocused; onActivated: win.setTextSize(theme.baseSize - 1) }
    Shortcut { sequences: ["Ctrl+0"]; enabled: !win.terminalFocused; onActivated: win.setTextSize(14) }
    Shortcut { sequences: ["Alt+Left"]; enabled: !win.terminalFocused && win.currentNote !== null; onActivated: win.currentNote.goBack() }
    Shortcut { sequences: ["Alt+Right"]; enabled: !win.terminalFocused && win.currentNote !== null; onActivated: win.currentNote.goForward() }
    Shortcut { sequences: ["F2"]; enabled: !win.terminalFocused; onActivated: { if (win.currentNote) win.currentNote.editTitle(); else if (win.currentTab() && win.currentTab().kind === "terminal") renameDialog.openFor(stack.currentIndex) } }
    Shortcut { sequences: ["Ctrl+Shift+T"]; onActivated: newTabDialog.openFresh() }
    Shortcut { sequences: ["Ctrl+Shift+W"]; onActivated: win.closeTab(stack.currentIndex, false) }
    Shortcut { sequences: ["Ctrl+Shift+PgUp"]; onActivated: win.moveTab(stack.currentIndex, stack.currentIndex - 1) }
    Shortcut { sequences: ["Ctrl+Shift+PgDown"]; onActivated: win.moveTab(stack.currentIndex, stack.currentIndex + 1) }

    // ── The palette's commands ────────────────────────────────────────────

    function commandList() {
        const list = [
            { name: "Quick switcher: Open quick switcher", keys: "Ctrl+O", run: function () { switcher.show() } },
            { name: "Create new note", keys: "Ctrl+N", run: function () { win.newNote() } },
            { name: "View: Larger text", keys: "Ctrl+=", run: function () { win.setTextSize(theme.baseSize + 1) } },
            { name: "View: Smaller text", keys: "Ctrl+-", run: function () { win.setTextSize(theme.baseSize - 1) } },
            { name: "View: Reset text size", keys: "Ctrl+0", run: function () { win.setTextSize(14) } },
            { name: "Daily notes: Open today's daily note", keys: "", run: function () { win.todayNote() } },
            { name: "Toggle reading view", keys: "Ctrl+E", enabled: win.currentNote !== null, run: function () { win.currentNote.toggleEditing() } },
            { name: "View: Reading view", keys: "", enabled: win.currentNote !== null, run: function () { if (win.currentNote.editing) win.currentNote.toggleEditing() } },
            { name: "View: Live preview", keys: "", enabled: win.currentNote !== null, run: function () { ui.editMode = "live"; if (!win.currentNote.editing) win.currentNote.toggleEditing() } },
            { name: "View: Source mode", keys: "", enabled: win.currentNote !== null, run: function () { ui.editMode = "source"; if (win.currentNote.editing) { win.currentNote.commitSection(); win.currentNote.liveWhole = false } else win.currentNote.toggleEditing() } },
            { name: "Save current file", keys: "Ctrl+S", enabled: win.currentNote !== null, run: function () { win.currentNote.save() } },
            { name: "Files: Reveal current file in explorer", keys: "", enabled: win.currentNote !== null, run: function () { win.showLeft("files"); explorer.reveal(win.currentNote.slug) } },
            { name: "Files: Rename current file", keys: "F2", enabled: win.currentNote !== null, run: function () { win.currentNote.editTitle() } },
            { name: "Files: Move current file to another folder", keys: "", enabled: win.currentNote !== null, run: function () { explorer.moveDialogFor(win.currentNote.slug) } },
            { name: "Files: Delete current file", keys: "", enabled: win.currentNote !== null, run: function () { explorer.deleteDialogFor(win.currentNote.slug) } },
            { name: "Files: Create new folder", keys: "", run: function () { win.showLeft("files"); explorer.newFolderAtRoot() } },
            { name: "Search: Search in all files", keys: "Ctrl+Shift+F", run: function () { win.showLeft("search") } },
            { name: "Bookmarks: Show bookmarks", keys: "", run: function () { win.showLeft("bookmarks") } },
            { name: "Favorites: Add or remove the current file", keys: "Ctrl+D", enabled: win.currentNote !== null, run: function () { win.bookmarkCurrentPage() } },
            { name: "Folders: Add a folder from the machine", keys: "", enabled: true, run: function () { rootDialog.open() } },
            { name: "Vault: Import an Obsidian vault…", keys: "", enabled: true, run: function () { importPicker.open() } },
            { name: "Sources: Capture a URL…", keys: "", enabled: true, run: function () { captureDialog.openFresh() } },
            { name: "Sources: Search sources", keys: "", enabled: true, run: function () { win.searchFor("type:source") } },
            { name: "Bookmarks: Bookmark the current search", keys: "", enabled: searchPane.query.trim().length > 0, run: function () { win.addBookmark({ kind: "search", query: searchPane.query.trim(), title: searchPane.query.trim() }) } },
            { name: "Capture: Append a line to today's daily page", keys: "", run: function () { promptDialog.openFor("Capture to today's daily page", "", function (text) { win.capture(text, "daily") }) } },
            { name: "Capture: Append a line to the inbox", keys: "", run: function () { promptDialog.openFor("Capture to the inbox", "", function (text) { win.capture(text, "inbox") }) } },
            { name: "Timeline: Append an entry to this page's timeline", keys: "", enabled: win.currentNote !== null, run: function () { promptDialog.openFor("Append to the timeline of " + win.currentNote.title, "", function (text) { win.appendTimeline(text) }) } },
            { name: "Toggle left sidebar", keys: "", run: function () { ui.leftOpen = !ui.leftOpen } },
            { name: "Toggle right sidebar", keys: "", run: function () { ui.rightOpen = !ui.rightOpen } },
            { name: "Backlinks: Show backlinks", keys: "", run: function () { win.showRight("backlinks") } },
            { name: "Outgoing links: Show outgoing links", keys: "", run: function () { win.showRight("outgoing") } },
            { name: "Outline: Show outline", keys: "", run: function () { win.showRight("outline") } },
            { name: "Tags: Show tags", keys: "", run: function () { win.showRight("tags"); rightPane.focusTags() } },
            { name: "Tags: Tag this page", keys: "", run: function () { if (win.currentNote) win.currentNote.focusTagAdd() } },
            { name: "Properties: Add a property to this page", keys: "", enabled: win.currentNote !== null, run: function () { win.currentNote.startAddProperty() } },
            { name: "Agent: Show the agent pane", keys: "", run: function () { win.showRight("agent") } },
            { name: "Graph view: Open graph view", keys: "Ctrl+G", run: function () { win.openGraph(false) } },
            { name: "Graph view: Open local graph", keys: "", enabled: win.currentNote !== null || win.lastPageSlug.length > 0, run: function () { win.openGraph(true) } },
            { name: "Tasks: Open tasks", keys: "", run: function () { win.openView("tasks") } },
            { name: "Memory: Open memories", keys: "", run: function () { win.openView("memory") } },
            { name: "Decisions: Open decisions", keys: "", run: function () { win.openView("decisions") } },
            { name: "Skills: Open skills", keys: "", run: function () { win.openView("skills") } },
            { name: "Secrets: Open secrets", keys: "", run: function () { win.openView("secrets") } },
            { name: "Settings: Open settings", keys: "Ctrl+,", run: function () { win.openView("settings") } },
            { name: "Terminal: New terminal (custom)", keys: "Ctrl+Shift+T", run: function () { newTabDialog.openFresh() } },
            { name: "Tabs: Close current tab", keys: "Ctrl+W", run: function () { win.closeTab(stack.currentIndex, false) } },
            { name: "Tabs: Close other tabs", keys: "", run: function () { win.closeOthers(stack.currentIndex) } },
            { name: "Tabs: Pin or unpin current tab", keys: "", run: function () { win.pinToggle(stack.currentIndex) } },
            { name: "Tabs: Next tab", keys: "Ctrl+Tab", run: function () { win.nextTab() } },
            { name: "Tabs: Previous tab", keys: "Ctrl+Shift+Tab", run: function () { win.prevTab() } },
            { name: "Navigate back", keys: "Alt+Left", enabled: win.currentNote !== null, run: function () { win.currentNote.goBack() } },
            { name: "Navigate forward", keys: "Alt+Right", enabled: win.currentNote !== null, run: function () { win.currentNote.goForward() } },
            { name: "Theme: Reload the Omarchy theme", keys: "", run: function () { theme.reload() } },
            { name: "Vault: Reload the file tree", keys: "", run: function () { win.refreshData() } }
        ]
        for (const a of win.agents) {
            list.push({ name: "Terminal: Open " + win.agentLabel(a) + " in a new tab", keys: "", run: (function (p) { return function () { win.openTerminal(p, "", "", "") } })(a) })
        }
        return list
    }

    QuickSwitcher { id: switcher; theme: win.theme; pages: win.pageList; favorites: win.bookmarkList.filter(function (b) { return b.kind === "file" }).map(function (b) { return b.path }); onOpenPage: (slug) => win.openPage(slug, false); onCreatePage: (name) => win.createPage(name) }
    CommandPalette { id: palette; theme: win.theme; onAboutToShow: commands = win.commandList() }

    Dialog {
        id: promptDialog
        modal: true
        anchors.centerIn: parent
        standardButtons: Dialog.Ok | Dialog.Cancel
        property var callback: null
        function openFor(titleText, initial, cb) { title = titleText; promptField.text = initial; callback = cb; open(); promptField.forceActiveFocus() }
        onAccepted: if (callback) callback(promptField.text)
        TextField { id: promptField; width: 420; onAccepted: promptDialog.accept() }
    }

    Menu {
        id: tabMenu
        property int tabIndex: -1
        readonly property var tab: tabIndex >= 0 && tabIndex < tabs.count ? tabs.get(tabIndex) : null
        MenuItem { text: tabMenu.tab && tabMenu.tab.pinned ? "Unpin" : "Pin"; onTriggered: win.pinToggle(tabMenu.tabIndex) }
        MenuItem { text: "Rename…"; visible: tabMenu.tab !== null && tabMenu.tab.kind === "terminal"; height: visible ? implicitHeight : 0; onTriggered: renameDialog.openFor(tabMenu.tabIndex) }
        MenuItem { text: "Move left"; enabled: tabMenu.tabIndex > 0; onTriggered: win.moveTab(tabMenu.tabIndex, tabMenu.tabIndex - 1) }
        MenuItem { text: "Move right"; enabled: tabMenu.tabIndex < tabs.count - 1; onTriggered: win.moveTab(tabMenu.tabIndex, tabMenu.tabIndex + 1) }
        MenuSeparator {}
        MenuItem { text: "Close"; onTriggered: win.closeTab(tabMenu.tabIndex, false) }
        MenuItem { text: "Close others"; onTriggered: win.closeOthers(tabMenu.tabIndex) }
        MenuItem { text: "Close and end the session"; visible: tabMenu.tab !== null && tabMenu.tab.kind === "terminal"; height: visible ? implicitHeight : 0; onTriggered: win.closeTab(tabMenu.tabIndex, true) }
    }

    FolderDialog {
        id: folderDialog
        title: "Working directory"
        onAccepted: cwdField.text = folderPath(selectedFolder)
    }
    function folderPath(url) {
        let s = String(url)
        if (s.startsWith("file://")) s = s.slice(7)
        return decodeURIComponent(s)
    }

    Dialog {
        id: newTabDialog
        title: "New terminal"
        modal: true
        anchors.centerIn: parent
        standardButtons: Dialog.Ok | Dialog.Cancel
        function openFresh() {
            programBox.model = terminals.programs()
            programBox.currentIndex = 0
            sessionBox.model = ["new session"].concat(terminals.sessions())
            sessionBox.currentIndex = 0
            nameField.text = ""
            cwdField.text = ""
            open()
            nameField.forceActiveFocus()
        }
        onAccepted: win.openTerminal(programBox.currentText, nameField.text, sessionBox.currentIndex > 0 ? sessionBox.currentText : "", cwdField.text)
        ColumnLayout {
            spacing: 10
            Label { text: "Name (optional)" }
            TextField { id: nameField; Layout.preferredWidth: 320; placeholderText: "Claude 2, Codex for droost, Scratch shell"; onAccepted: newTabDialog.accept() }
            Label { text: "Agent" }
            ComboBox { id: programBox; Layout.preferredWidth: 320 }
            Label { text: "Session" }
            ComboBox { id: sessionBox; Layout.preferredWidth: 320 }
            Label { text: "Working directory" }
            RowLayout {
                spacing: 6
                TextField { id: cwdField; Layout.preferredWidth: 240; placeholderText: theme.homeDir; onAccepted: newTabDialog.accept() }
                Button { text: "Browse…"; onClicked: { folderDialog.currentFolder = "file://" + (cwdField.text.length > 0 ? cwdField.text : theme.homeDir); folderDialog.open() } }
            }
        }
    }

    // What the strip's + offers: the page switcher it always opened, then the same agents
    // the top bar does, then the custom terminal. The agents come from an Instantiator,
    // which is how a Menu takes items from a model; a Repeater would not parent them into it.
    Menu {
        id: plusMenu
        MenuItem { text: "Note or page…"; onTriggered: switcher.show() }
        MenuSeparator {}
        Instantiator {
            model: win.agents
            delegate: MenuItem {
                required property string modelData
                text: win.agentLabel(modelData)
                onTriggered: win.openTerminal(modelData, "", "", "")
            }
            onObjectAdded: (index, object) => plusMenu.insertItem(index + 2, object)
            onObjectRemoved: (index, object) => plusMenu.removeItem(object)
        }
        MenuSeparator {}
        MenuItem { text: "Custom terminal…"; onTriggered: newTabDialog.openFresh() }
    }

    Dialog {
        id: renameDialog
        title: "Rename tab"
        modal: true
        anchors.centerIn: parent
        standardButtons: Dialog.Ok | Dialog.Cancel
        property int tabIndex: -1
        function openFor(i) { tabIndex = i; renameField.text = tabs.get(i).title; open(); renameField.forceActiveFocus(); renameField.selectAll() }
        onAccepted: win.renameTab(tabIndex, renameField.text)
        // A bare child with an explicit width does not size a Dialog's content; the field
        // spilled past the right edge. The layout is what newTabDialog uses.
        ColumnLayout {
            TextField { id: renameField; Layout.preferredWidth: 320; onAccepted: renameDialog.accept() }
        }
    }

    // ── One tab's content ─────────────────────────────────────────────────

    component TabHost: Item {
        id: host
        required property int index
        required property string kind
        required property string slug
        required property string session
        required property string program
        required property string cwd
        required property string title
        readonly property bool isCurrent: stack.currentIndex === index
        readonly property var item: loader.item
        Loader {
            id: loader
            anchors.fill: parent
            sourceComponent: host.kind === "page" ? pageComp
                           : host.kind === "file" ? fileComp
                           : host.kind === "terminal" ? termComp
                           : host.kind === "tasks" ? tasksComp
                           : host.kind === "memory" ? memoryComp
                           : host.kind === "decisions" ? decisionsComp
                           : host.kind === "skills" ? skillsComp
                           : host.kind === "secrets" ? secretsComp
                           : host.kind === "settings" ? settingsComp
                           : host.kind === "graph" ? graphComp : null
            onLoaded: if (host.isCurrent) win.updateCurrent()
        }
        Component {
            id: pageComp
            NoteTab {
                backend: win.backend
                theme: win.theme
                tags: win.tags
                sectionTools: tools
                editMode: ui.editMode
                isCurrent: host.isCurrent
                Component.onCompleted: open(host.slug)
                onNavigated: (s, t) => win.tabNavigated(host.index, s, t)
                onOpenTag: (tag) => win.searchFor("tag:" + tag)
                onRequestMove: (s) => explorer.moveDialogFor(s)
                onRequestDelete: (s) => explorer.deleteDialogFor(s)
                onRequestLocalGraph: (s) => win.openGraph(true)
                bookmarked: win.isBookmarkedPath(slug)
                onRequestBookmark: (s, t) => win.addBookmark({ kind: "file", path: s, title: t })
            }
        }
        Component {
            id: fileComp
            FileTab { backend: win.backend; theme: win.theme; folders: diskFolders; path: host.slug }
        }
        Component {
            id: termComp
            AgentTerminal {
                theme: win.theme
                terminals: win.terminals
                session: host.session
                program: host.program
                cwd: host.cwd
                isCurrent: host.isCurrent
                windowActive: win.active
                onUnread: win.markUnread(host.index)
                onAttention: (m) => win.attention(host.index, m)
                onTitleChanged: win.setTermTitle(host.index, title)
            }
        }
        Component {
            id: graphComp
            GraphView {
                backend: win.backend
                theme: win.theme
                around: host.slug.length > 0 ? (win.lastPageSlug.length > 0 ? win.lastPageSlug : host.slug) : ""
                settings: win.graphSettings
                onOpenPage: (slug) => win.openPage(slug, false)
                onSearchTag: (tag) => win.searchFor("tag:" + tag)
                onSettingsEdited: (s) => win.saveGraphSettings(s)
            }
        }
        Component { id: tasksComp; TasksPage { backend: win.backend; theme: win.theme } }
        Component { id: memoryComp; MemoryPage { backend: win.backend; theme: win.theme } }
        Component { id: decisionsComp; DecisionsPage { backend: win.backend; theme: win.theme; onOpenPage: (s) => win.openPage(s, false) } }
        Component { id: skillsComp; SkillsPage { backend: win.backend; theme: win.theme; savedLayout: ui.skillsLayout; onLayoutChanged: (json) => ui.skillsLayout = json; onRunScript: (path, name) => win.openTerminal("run:" + path, "Run " + name, "", "") } }
        Component { id: secretsComp; SecretsPage { backend: win.backend; theme: win.theme } }
        Component { id: settingsComp; SettingsPage { backend: win.backend; theme: win.theme; terminals: win.terminals; commands: win.commandList(); onSelectSkin: (s, n) => win.selectTheme(s, n); onSetScanlines: (on) => win.setScanlines(on); onSetTextSize: (n) => win.setTextSize(n) } }
    }

    component RibbonButton: Rectangle {
        id: rb
        property string icon: ""
        property string glyph: ""
        property string label: ""
        property string tip: ""
        property bool active: false
        signal clicked()
        readonly property bool lit: rb.active || (rbHover.hovered && enabled)
        width: Math.round(32 * theme.scale)
        height: Math.round((rb.label.length > 0 ? 38 : 30) * theme.scale)
        radius: theme.radius
        color: rb.lit ? theme.panel3 : "transparent"
        border.width: 1
        border.color: rb.lit ? theme.lineBright : "transparent"
        opacity: enabled ? 1 : 0.35
        Rectangle { visible: rb.active; x: -7; y: 8; width: 2; height: 14; color: theme.accent }
        Column {
            anchors.centerIn: parent
            spacing: 1
            Icon { visible: rb.icon.length > 0; anchors.horizontalCenter: parent.horizontalCenter; name: rb.icon; color: rb.lit ? theme.accent : theme.muted; size: 15 }
            Text { visible: rb.glyph.length > 0; anchors.horizontalCenter: parent.horizontalCenter; text: rb.glyph; color: rb.lit ? theme.accent : theme.muted; font.pixelSize: Math.round(15 * theme.scale) }
            Text { visible: rb.label.length > 0; anchors.horizontalCenter: parent.horizontalCenter; text: rb.label; color: rb.lit ? theme.accent : theme.muted; font.pixelSize: Math.round(7 * theme.scale) }
        }
        HoverHandler { id: rbHover; cursorShape: Qt.PointingHandCursor; enabled: theme.shotPath.length === 0 }
        TapHandler { onTapped: if (rb.enabled) rb.clicked() }
        ToolTip.visible: rbHover.hovered && rb.tip.length > 0
        ToolTip.text: rb.tip
        ToolTip.delay: 500
    }

    component SideTab: Rectangle {
        id: st
        property string icon
        property string tip: ""
        property bool active: false
        signal clicked()
        width: Math.round(28 * theme.scale)
        height: Math.round(26 * theme.scale)
        radius: 5
        color: st.active ? theme.active : (stHover.hovered ? theme.hover : "transparent")
        Icon { anchors.centerIn: parent; name: st.icon; color: st.active ? theme.foreground : theme.muted; size: 16 }
        HoverHandler { id: stHover; cursorShape: Qt.PointingHandCursor }
        TapHandler { onTapped: st.clicked() }
        ToolTip.visible: stHover.hovered && st.tip.length > 0
        ToolTip.text: st.tip
        ToolTip.delay: 600
    }

    // ── Layout ────────────────────────────────────────────────────────────

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        TopBar {
            Layout.fillWidth: true
            theme: win.theme; desk: desk; backend: win.backend
            pages: win.tree ? win.tree.pages : 0
            agents: win.agents; agentGlyphs: win.agentGlyphs; agentNames: win.agentNames
            onQuit: Qt.quit()
            onCommandRequested: palette.show()
            onAgentRequested: (p) => win.openTerminal(p, "", "", "")
            onAgentPaneRequested: (p) => win.showRight("agent")
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 0

            // The ribbon.
            Rectangle {
                Layout.fillHeight: true
                width: Math.round(45 * theme.scale)
                color: theme.panel
                ColumnLayout {
                    anchors.fill: parent
                    anchors.topMargin: 10
                    anchors.bottomMargin: 10
                    spacing: 6
                    Text { Layout.alignment: Qt.AlignHCenter; text: "⌘"; color: theme.accent; font.pixelSize: Math.round(16 * theme.scale) }
                    Rectangle { Layout.alignment: Qt.AlignHCenter; width: 30; height: 1; color: theme.line; Layout.bottomMargin: 2 }
                    RibbonButton { Layout.alignment: Qt.AlignHCenter; icon: "new-note"; label: "new"; tip: "New note (Ctrl+N)"; onClicked: win.newNote() }
                    RibbonButton { Layout.alignment: Qt.AlignHCenter; icon: "daily"; label: "daily"; tip: "Open today's daily note"; onClicked: win.todayNote() }
                    RibbonButton { Layout.alignment: Qt.AlignHCenter; icon: "graph"; label: "graph"; tip: "Graph view (Ctrl+G)"; active: win.currentTab() !== null && win.currentTab().kind === "graph"; onClicked: win.openGraph(false) }
                    Rectangle { Layout.alignment: Qt.AlignHCenter; width: 22; height: 1; color: theme.line; Layout.topMargin: 4; Layout.bottomMargin: 4 }
                    RibbonButton { Layout.alignment: Qt.AlignHCenter; icon: "tasks"; label: "tasks"; tip: "Tasks"; active: win.currentTab() !== null && win.currentTab().kind === "tasks"; onClicked: win.openView("tasks") }
                    RibbonButton { Layout.alignment: Qt.AlignHCenter; icon: "memory"; label: "memory"; tip: "Memory"; active: win.currentTab() !== null && win.currentTab().kind === "memory"; onClicked: win.openView("memory") }
                    RibbonButton { Layout.alignment: Qt.AlignHCenter; icon: "check-square"; label: "decide"; tip: "Decisions"; active: win.currentTab() !== null && win.currentTab().kind === "decisions"; onClicked: win.openView("decisions") }
                    RibbonButton { Layout.alignment: Qt.AlignHCenter; icon: "skills"; label: "skills"; tip: "Skills"; active: win.currentTab() !== null && win.currentTab().kind === "skills"; onClicked: win.openView("skills") }
                    RibbonButton { Layout.alignment: Qt.AlignHCenter; icon: "secrets"; label: "secrets"; tip: "Secrets"; active: win.currentTab() !== null && win.currentTab().kind === "secrets"; onClicked: win.openView("secrets") }
                    Item { Layout.fillHeight: true }
                    RibbonButton { Layout.alignment: Qt.AlignHCenter; icon: "settings"; label: "setup"; tip: "Settings (Ctrl+,)"; active: win.currentTab() !== null && win.currentTab().kind === "settings"; onClicked: win.openView("settings") }
                    Rectangle {
                        Layout.alignment: Qt.AlignHCenter
                        width: 30; height: 30
                        color: "transparent"
                        border.width: 1
                        border.color: theme.line
                        Text { anchors.centerIn: parent; text: desk.user.slice(0, 2).toUpperCase(); color: theme.alive; font.pixelSize: Math.round(10 * theme.scale) }
                    }
                }
            }
            Rectangle { width: 1; Layout.fillHeight: true; color: theme.line }

            // The left sidebar: files and search.
            Rectangle {
                visible: ui.leftOpen
                Layout.fillHeight: true
                width: ui.leftWidth
                color: theme.surface
                ColumnLayout {
                    anchors.fill: parent
                    spacing: 0
                    RowLayout {
                        Layout.fillWidth: true
                        Layout.leftMargin: 6
                        Layout.rightMargin: 6
                        Layout.topMargin: 6
                        Layout.bottomMargin: 4
                        spacing: 2
                        Text { text: ui.leftPane === "files" ? "Vault files" : ui.leftPane === "search" ? "Search" : "Bookmarks"; color: theme.bright; font.pixelSize: Math.round(9 * theme.scale); font.letterSpacing: 1.3; font.capitalization: Font.AllUppercase; Layout.leftMargin: 6 }
                        Item { Layout.fillWidth: true }
                        SideTab { icon: "files"; tip: "Files"; active: ui.leftPane === "files"; onClicked: win.showLeft("files") }
                        SideTab { icon: "search"; tip: "Search (Ctrl+Shift+F)"; active: ui.leftPane === "search"; onClicked: win.showLeft("search") }
                        SideTab { icon: "bookmark"; tip: "Bookmarks"; active: ui.leftPane === "bookmarks"; onClicked: win.showLeft("bookmarks") }
                        SideTab { icon: "panel-left"; tip: "Collapse"; onClicked: ui.leftOpen = false }
                    }
                    Rectangle { Layout.fillWidth: true; height: 1; color: theme.line }
                    StackLayout {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        currentIndex: ui.leftPane === "search" ? 1 : ui.leftPane === "bookmarks" ? 2 : 0
                        Explorer {
                            id: explorer
                            backend: win.backend
                            theme: win.theme
                            tree: win.tree
                            favorites: win.bookmarkList.filter(function (b) { return b.kind === "file" || b.kind === "folder" })
                            folders: diskFolders
                            roots: win.rootList
                            agents: win.agents
                            agentNames: win.agentNames
                            onOpenFile: (p) => win.openFile(p)
                            onOpenAgentAt: (program, dir) => win.openTerminal(program, "", "", dir)
                            onAddRootRequested: rootDialog.open()
                            onRemoveRoot: (p) => win.removeRoot(p)
                            onOpenFavorite: (b) => win.openBookmark(b)
                            onRemoveFavorite: (b) => win.addBookmark(b)
                            onOpenPage: (slug) => win.openPage(slug, false)
                            onCreated: (slug) => { win.openPage(slug, false); Qt.callLater(function () { if (win.currentNote && win.currentNote.slug === slug) win.currentNote.editTitle() }) }
                            onExpandedEdited: ui.expanded = JSON.stringify(expanded)
                            onRequestBookmark: (row) => win.addBookmark({ kind: row.kind === "folder" ? "folder" : "file", path: row.path, title: win.baseName(row.path) })
                            function newFolderAtRoot() { folderDialogRoot.openFor("") }
                            Dialog {
                                id: folderDialogRoot
                                title: "New folder"
                                modal: true
                                anchors.centerIn: Overlay.overlay
                                standardButtons: Dialog.Ok | Dialog.Cancel
                                property string parentFolder: ""
                                function openFor(folder) { parentFolder = folder; rootFolderName.text = ""; open(); rootFolderName.forceActiveFocus() }
                                onAccepted: explorer.newFolder(parentFolder, rootFolderName.text)
                                TextField { id: rootFolderName; width: 320; placeholderText: "Folder name"; onAccepted: folderDialogRoot.accept() }
                            }
                        }
                        SearchPane {
                            id: searchPane
                            backend: win.backend
                            theme: win.theme
                            onOpenPage: (slug) => win.openPage(slug, false)
                            onBookmarkSearch: (q) => win.addBookmark({ kind: "search", query: q, title: q })
                        }
                        BookmarksPane {
                            id: bookmarksPane
                            theme: win.theme
                            bookmarks: win.bookmarkList
                            onOpenBookmark: (b) => win.openBookmark(b)
                            onRemoveBookmark: (i) => win.removeBookmark(i)
                            onRetitleBookmark: (i, t) => win.retitleBookmark(i, t)
                        }
                    }
                    Rectangle { Layout.fillWidth: true; height: 1; color: theme.line; opacity: 0.6 }
                    RowLayout {
                        Layout.fillWidth: true
                        Layout.leftMargin: 10
                        Layout.rightMargin: 6
                        height: Math.round(30 * theme.scale)
                        spacing: 6
                        Text { text: (win.tree ? win.tree.pages + " notes" : "Indexing") + (win.unresolvedCount > 0 ? " · " + win.unresolvedCount + " unresolved" : ""); color: theme.faint; font.pixelSize: Math.round(9 * theme.scale); font.letterSpacing: 1; font.capitalization: Font.AllUppercase; elide: Text.ElideRight; Layout.fillWidth: true }
                        SideTab { icon: "help"; tip: "Command palette (Ctrl+P) lists every action with its key"; onClicked: palette.show() }
                        SideTab { icon: "settings"; tip: "Settings"; onClicked: win.openView("settings") }
                    }
                }
            }
            Splitter { visible: ui.leftOpen; Layout.fillHeight: true; theme: win.theme; value: ui.leftWidth; min: 180; max: 600; onMoved: (v) => ui.leftWidth = Math.round(v) }

            // The main area: the tab strip and the tabs.
            ColumnLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                spacing: 0
                Rectangle {
                    Layout.fillWidth: true
                    height: Math.round(42 * theme.scale)
                    color: theme.panel2
                    RowLayout {
                        anchors.fill: parent
                        spacing: 0
                        SideTab { visible: !ui.leftOpen; Layout.leftMargin: 6; icon: "panel-left"; tip: "Expand"; onClicked: ui.leftOpen = true }
                        Flickable {
                            id: tabStrip
                            Layout.fillWidth: true
                            Layout.fillHeight: true
                            contentWidth: tabRow.implicitWidth
                            clip: true
                            boundsBehavior: Flickable.StopAtBounds
                            Row {
                                id: tabRow
                                height: parent.height
                                spacing: 0
                                leftPadding: 0
                                property int dragFrom: -1
                                // The tab whose span holds x, or the last one to its left; only
                                // the delegates carry an `index`, which skips the spacer and the +.
                                function dropIndexAt(x) {
                                    let last = 0
                                    for (let i = 0; i < children.length; i++) {
                                        const c = children[i]
                                        if (c.index === undefined) continue
                                        if (x >= c.x && x < c.x + c.width) return c.index
                                        if (x >= c.x + c.width) last = c.index
                                    }
                                    return last
                                }
                                Repeater {
                                    model: tabs
                                    delegate: Item {
                                        id: tabItem
                                        required property int index
                                        required property string title
                                        required property string kind
                                        required property string program
                                        required property bool pinned
                                        required property bool unread
                                        required property string termTitle
                                        readonly property bool active: stack.currentIndex === index
                                        width: Math.min(230, Math.max(150, tabLabel.implicitWidth + 70))
                                        height: Math.round(42 * theme.scale)
                                        Rectangle {
                                            anchors.fill: parent
                                            color: tabItem.active ? theme.background : (tabHover.hovered ? theme.panel3 : theme.panel2)
                                            Rectangle { anchors.right: parent.right; width: 1; height: parent.height; color: theme.line }
                                            Rectangle { visible: tabItem.active; anchors.left: parent.left; anchors.right: parent.right; anchors.bottom: parent.bottom; height: 2; color: theme.accent }
                                        }
                                        RowLayout {
                                            anchors.fill: parent
                                            anchors.leftMargin: 13
                                            anchors.rightMargin: 6
                                            spacing: 7
                                            Icon { visible: tabItem.pinned; name: "pin"; color: theme.muted; size: 12 }
                                            Text { visible: tabItem.kind !== "terminal" && tabItem.kind !== "graph"; text: tabItem.active ? "◆" : "◇"; color: tabItem.active ? theme.accent : theme.muted; font.pixelSize: Math.round(10 * theme.scale) }
                                            Text { visible: tabItem.kind === "terminal"; text: win.agentGlyph(tabItem.program); color: tabItem.active ? theme.foreground : theme.muted; font.pixelSize: Math.round(12 * theme.scale) }
                                            Icon { visible: tabItem.kind === "graph"; name: "graph"; color: tabItem.active ? theme.foreground : theme.muted; size: 13 }
                                            Text {
                                                id: tabLabel
                                                Layout.fillWidth: true
                                                text: tabItem.title
                                                color: tabItem.active ? theme.bright : theme.muted
                                                font.pixelSize: Math.round(10 * theme.scale)
                                                elide: Text.ElideRight
                                            }
                                            Rectangle { visible: tabItem.unread; width: 7; height: 7; radius: 4; color: theme.accent }
                                            Rectangle {
                                                width: 18; height: 18; radius: 4
                                                color: closeHover.hovered ? theme.hover : "transparent"
                                                opacity: tabItem.active || tabHover.hovered ? 1 : 0
                                                Icon { anchors.centerIn: parent; name: "close"; color: theme.muted; size: 12 }
                                                HoverHandler { id: closeHover }
                                                TapHandler { onTapped: win.closeTab(tabItem.index, false) }
                                            }
                                        }
                                        HoverHandler { id: tabHover }
                                        TapHandler { acceptedButtons: Qt.LeftButton; onTapped: stack.currentIndex = tabItem.index }
                                        TapHandler { acceptedButtons: Qt.MiddleButton; onTapped: win.closeTab(tabItem.index, false) }
                                        TapHandler { acceptedButtons: Qt.RightButton; onTapped: { tabMenu.tabIndex = tabItem.index; tabMenu.popup() } }
                                        // Drag to reorder: the handler owns only the movement, so a click still
                                        // selects and a press that becomes a drag never fires the tap.
                                        DragHandler {
                                            id: tabDrag
                                            target: null
                                            yAxis.enabled: false
                                            onActiveChanged: {
                                                if (active) { tabRow.dragFrom = tabItem.index; return }
                                                if (tabRow.dragFrom < 0) return
                                                const p = tabItem.mapToItem(tabRow, tabDrag.centroid.position.x, 0)
                                                const to = tabRow.dropIndexAt(p.x)
                                                if (to !== tabRow.dragFrom) win.moveTab(tabRow.dragFrom, to)
                                                tabRow.dragFrom = -1
                                            }
                                        }
                                        ToolTip.visible: tabHover.hovered && tabItem.termTitle.length > 0
                                        ToolTip.text: tabItem.termTitle
                                        ToolTip.delay: 600
                                    }
                                }
                                Item { width: 4; height: 1 }
                                SideTab { id: plusButton; icon: "plus"; tip: "New tab · Ctrl+T for a page, Ctrl+Shift+T for a terminal"; anchors.verticalCenter: parent.verticalCenter; onClicked: plusMenu.popup(plusButton, 0, plusButton.height) }
                            }
                        }
                        Rectangle { width: 7; height: 7; radius: 4; color: backend.connected ? theme.accent : (win.tokens.red || theme.muted); Layout.rightMargin: 6; ToolTip.visible: dotHover.hovered; ToolTip.text: backend.connected ? "rusty-mcp connected" : backend.status; HoverHandler { id: dotHover } }
                        SideTab { visible: !ui.rightOpen; Layout.rightMargin: 6; icon: "panel-right"; tip: "Expand"; onClicked: ui.rightOpen = true }
                    }
                }
                Rectangle { Layout.fillWidth: true; height: 1; color: theme.line; opacity: 0.6 }

                Item {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    StackLayout {
                        id: stack
                        anchors.fill: parent
                        onCurrentIndexChanged: { win_settings.lastTab = currentIndex; win.updateCurrent() }
                        Repeater {
                            id: hosts
                            model: tabs
                            delegate: TabHost {}
                        }
                    }

                // Nothing open.
                ColumnLayout {
                    visible: tabs.count === 0
                    anchors.centerIn: parent
                    spacing: 10
                    Text { Layout.alignment: Qt.AlignHCenter; text: "No file is open"; color: theme.muted; font.pixelSize: Math.round(20 * theme.scale) }
                    Text { Layout.alignment: Qt.AlignHCenter; text: "Create new note (Ctrl+N)"; color: theme.link; font.pixelSize: Math.round(14 * theme.scale); TapHandler { onTapped: win.newNote() } }
                    Text { Layout.alignment: Qt.AlignHCenter; text: "Go to file (Ctrl+O)"; color: theme.link; font.pixelSize: Math.round(14 * theme.scale); TapHandler { onTapped: switcher.show() } }
                    Text { Layout.alignment: Qt.AlignHCenter; text: "Open today's daily note"; color: theme.link; font.pixelSize: Math.round(14 * theme.scale); TapHandler { onTapped: win.todayNote() } }
                    Text { Layout.alignment: Qt.AlignHCenter; visible: win.agents.length > 0; text: "Open " + win.agentLabel(win.agents[0]) + " in a terminal"; color: theme.link; font.pixelSize: Math.round(14 * theme.scale); TapHandler { onTapped: win.openTerminal(win.agents[0], "", "", "") } }
                }
                }
            }

            Splitter { visible: ui.rightOpen; Layout.fillHeight: true; theme: win.theme; value: ui.rightWidth; min: 200; max: 700; invert: true; onMoved: (v) => ui.rightWidth = Math.round(v) }

            // The right sidebar.
            Rectangle {
                visible: ui.rightOpen
                Layout.fillHeight: true
                width: ui.rightWidth
                color: theme.surface
                RightPane {
                    id: rightPane
                    anchors.fill: parent
                    backend: win.backend
                    theme: win.theme
                    terminals: win.terminals
                    agents: win.registry
                    sessions: ui.agentSessions
                    note: win.currentNote
                    titles: win.titles
                    tags: win.tags
                    windowActive: win.active
                    onOpenPage: (slug) => win.openPage(slug, false)
                    onCreatePage: (name) => win.createPage(name)
                    onSearchTag: (tag) => win.searchFor("tag:" + tag)
                    onTagPage: (tag) => { if (win.currentNote) win.currentNote.tagThePage(tag) }
                    onBookmarkHeading: (text) => { if (win.currentNote) win.addBookmark({ kind: "heading", path: win.currentNote.slug, heading: text, title: win.currentNote.title + " › " + text }) }
                    onPaneChanged: (name) => ui.rightPane = name
                    onSessionStarted: (slug, id) => { const m = win.sessionMap(); m[slug] = id; ui.agentSessions = JSON.stringify(m) }
                    onForgetSession: (slug) => { const m = win.sessionMap(); delete m[slug]; ui.agentSessions = JSON.stringify(m) }
                }
                SideTab { anchors.right: parent.right; anchors.top: parent.top; anchors.rightMargin: 6; anchors.topMargin: 4; icon: "panel-right"; tip: "Collapse"; onClicked: ui.rightOpen = false }
            }
        }

        // The status bar.
        Rectangle {
            Layout.fillWidth: true
            height: Math.round(24 * theme.scale)
            color: theme.panel
            Rectangle { anchors.top: parent.top; width: parent.width; height: 1; color: theme.line }
            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 12
                anchors.rightMargin: 12
                spacing: 14
                Text { text: backend.connected ? "" : backend.status; color: theme.faint; font.pixelSize: Math.round(9 * theme.scale); font.letterSpacing: 1; font.capitalization: Font.AllUppercase; elide: Text.ElideRight; Layout.fillWidth: true }
                Text { visible: win.currentNote !== null; text: (win.currentNote ? win.currentNote.backlinkCount : 0) + " backlinks"; color: theme.faint; font.pixelSize: Math.round(9 * theme.scale); font.letterSpacing: 1; font.capitalization: Font.AllUppercase }
                Text { visible: win.currentNote !== null; text: (win.currentNote ? win.currentNote.properties.length : 0) + " properties"; color: theme.faint; font.pixelSize: Math.round(9 * theme.scale); font.letterSpacing: 1; font.capitalization: Font.AllUppercase }
                Text { visible: win.currentNote !== null; text: (win.currentNote ? win.currentNote.words : 0) + " words"; color: theme.faint; font.pixelSize: Math.round(9 * theme.scale); font.letterSpacing: 1; font.capitalization: Font.AllUppercase }
                Text { visible: win.currentNote !== null; text: (win.currentNote ? win.currentNote.characters : 0) + " characters"; color: theme.faint; font.pixelSize: Math.round(9 * theme.scale); font.letterSpacing: 1; font.capitalization: Font.AllUppercase }
            }
        }
    }

    // The mock's toast, for the window's notices.
    Rectangle {
        visible: win.notice.length > 0
        z: 50
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.rightMargin: 22
        anchors.bottomMargin: 18
        width: toastText.implicitWidth + 24
        height: Math.round(30 * theme.scale)
        color: theme.panel
        border.width: 1
        border.color: theme.alive
        Text { id: toastText; anchors.centerIn: parent; text: win.notice; color: theme.alive; font.pixelSize: Math.round(10 * theme.scale) }
        Timer { running: win.notice.length > 0; interval: 2600; onTriggered: win.notice = "" }
    }

    // The CRT overlay, when the skin asks for it.
    Scanlines { anchors.fill: parent; z: 40; visible: theme.scanlines }
}
