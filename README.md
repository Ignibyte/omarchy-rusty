# Rusty

A local-first AI assistant for [Omarchy](https://omarchy.org): a knowledge workspace laid
out the way Obsidian lays out a vault, with a pure MCP back end, a markdown brain any
markdown tool can open, to-do lists, memories and skills, and terminals that run Claude Code
and Codex natively as tabs and as a pane beside the note.

![The workspace: explorer, a page in reading view, backlinks](docs/screenshots/workspace-reading.png)

**Status: v3, not yet released.** See `ROADMAP.md` for the plan and `docs/architecture.md`
for the shape.

## Status

Milestones M0 to M5 and M7 of `ROADMAP.md` landed on 2026-09-02: the back end, the desktop
app with agent terminals, and semantic search behind a provider setting. M8 (Obsidian inside
Rusty) landed on 2026-09-03: the workspace shell, tags and properties, graph views, search
operators and bookmarks, and the retirement of the Obsidian bridge. What is left in M6 is the
first release.

## Run it

```bash
rusty session start      # the back end, then the app, each under its user service; also stop, status
rusty                    # the app on its own, tied to this terminal
rusty-mcp                # the back end over stdio (agents); the user service serves HTTP
rusty-cli brain search "orbit"
```

## The workspace

The window is Obsidian's layout. A top bar carries the brand, the command palette button
and one small glyph per agent CLI found on the machine (Claude Code, Codex, Gemini, Aider,
OpenCode, a shell): a click opens the agent in a new tab, a right-click opens it in the
pane beside the note. A ribbon on the left: new note, today's daily note, the graph, then
Tasks, Memory, Skills and Secrets, and Settings at the bottom. The left sidebar holds the file explorer (the vault's real folders, with new note,
new folder, rename, move and delete on a right click; a rename rewrites every link to the
page) and search. The main area holds tabs: pages, agent terminals and the built-in views,
each closable, pinnable and remembered between runs. The right sidebar holds backlinks with
their context lines, outgoing links (an unresolved one creates the page), the outline, and an
agent pane that holds a conversation with Claude Code beside the note — one session per
page, running in a host of its own, so it survives the window being closed. The status bar counts backlinks,
properties, words and characters.

A page opens in reading view: the inline title (Enter renames the file), the properties from
its frontmatter, then the body rendered in Obsidian's flavour by the back end (wikilinks with
aliases and headings, page and image embeds, callouts, task boxes that toggle on click,
tables, footnotes, `==highlights==`, `#tags`, hidden `%% comments %%`, fenced code). Ctrl+E
switches to the source, the whole file with markdown highlighting; edits autosave after a
pause and on Ctrl+S.

![The source editor](docs/screenshots/workspace-source.png)

Tags and properties work as they do in Obsidian. Every tag counts, whether it sits in
the frontmatter list or inline as `#tag` in the body (nested `a/b` counts under `a`);
the Tags pane in the right sidebar lists them as a tree with counts, and a click, or a
`#tag` in a page, searches with `tag:<name>`, which `brain_search` understands alone or
with words. The properties block edits the frontmatter in place by type: text, a date,
a number, a checkbox, or a list of chips; a row can go, and "Add property" adds one. A
property edit changes only the frontmatter; the body stays byte for byte.

![Tags and properties](docs/screenshots/tags-and-properties.png)

The graph view (Ctrl+G, the ribbon, or "Open local graph" on a page) draws the vault as
Obsidian does: pages as dots sized by their links, links as lines, laid out by forces on
a canvas, with a panel of Filters (search, tags, existing files only, orphans), Groups
(a `tag:`, `path:`, `type:` or text query with a colour from the theme's palette),
Display (arrows, text fade, node size, link thickness) and Forces (centre, repel, link,
link distance). Drag the background to pan, wheel to zoom, hover a node to see its title
and its neighbours, click it to open the page. A local graph shows one page's
neighbourhood to a chosen depth and follows the page you open.

![The graph view](docs/screenshots/graph-view.png)

Search narrows the way Obsidian's does. `path:`, `file:`, `tag:` and `type:` terms sit
in the query with the words (a value in quotes may hold spaces, a leading `-` excludes,
and operator terms alone list the matching pages); the two chips beside the field switch
match case and regular expressions; the third bookmarks the search. `brain_search` takes
the same query and the two modes as `case_sensitive` and `regex`, so an agent narrows a
search the way the pane does. The Bookmarks tab of the left sidebar keeps files,
folders, searches and headings, added from a page's menu, the explorer, the search pane,
the outline or the palette; a click opens the target. The files and folders among them
are your favorites: a star beside the reading toggle adds or removes the open page
(Ctrl+D does the same), a Favorites section sits above the file explorer's tree, and the
quick switcher lists favorites first, starred, until you type. Settings lists every
command with its key in a Hotkeys table with a filter.

![Bookmarks](docs/screenshots/bookmarks.png)

Keys follow Obsidian: Ctrl+O quick switcher (type a name that does not exist and Enter
creates it), Ctrl+P command palette (every command with its key), Ctrl+N new note, Ctrl+E
reading or source, Ctrl+W close tab, Ctrl+Tab and Ctrl+Shift+Tab (or Ctrl+PgUp/PgDn) switch
tabs, Ctrl+Shift+F search, Ctrl+, settings, Alt+Left/Right back and forward, F2 rename.
While a terminal has focus the workspace keys stand down, because the shell and Claude Code
use the same ones; Ctrl+Shift+T (custom terminal), Ctrl+Shift+W and Ctrl+PgUp/PgDn work
everywhere. Each terminal is a tmux session that outlives the window; a tab that gets output
while another is showing gets a mark, and a title that asks for attention raises a desktop
notification.

The look is a skin: one set of colour roles (a ground and three panel levels, two line
weights, three text weights, an accent and its softer twin, gold for titles, an "alive"
colour for links and state, red) that every surface and the renderer paint from, in the
monospace face the terminal uses. The default is Amber phosphor, the palette of the
design mock; Green phosphor, Ice and Paper ship beside it; "Follow Omarchy" maps the
desktop theme onto the same roles and follows `omarchy theme set`; and a file in
`~/.config/rusty/themes/<name>.toml` is a skin of your own (a `[colors]` table with `bg`,
`text` and `accent` at least, the rest derived). Settings picks the skin, the CRT
scanline overlay and the text size (12 to 18 pixels, 14 by default; Ctrl with plus, minus
or zero steps it, and everything but the terminals follows). The chrome is the mock's too: a top bar with the brand, the command
button and the agents, the vault's state, memory, CPU and the clock; rail buttons with labels; pane
heads as micro-labels; a note's meta line, heading marks, code header strips and "linked
from" footer; the assistant's header and context card on the agent pane.

![Follow Omarchy (Tokyo Night)](docs/screenshots/skin-omarchy.png)

![A skin from a theme file](docs/screenshots/skin-file.png)

![The Paper preset](docs/screenshots/workspace-light.png)

Under Omarchy the app's messages go to the journal when it is not started from a terminal:
`journalctl --user -t rusty`, or `QT_FORCE_STDERR_LOGGING=1`; `RUSTY_DEBUG=1` adds a line
per event. `scripts/screenshot.sh <dir>` renders the scenes above offscreen against a
scratch vault, which is how the screenshots in `docs/screenshots/` are made. Every view talks
to the back end over `http://127.0.0.1:4174/mcp`, so a Claude session changing a page or a
task shows up in the app as it happens.

## How work happens

Spec-driven and phase-gated: `CONSTITUTION.md` is the law, `AGENTS.md` (and `CLAUDE.md`)
route the work, `.claude/skills/rusty-workflow/` is the driving manual, and
`docs/planning/` holds tickets, the active spec/notes pair, the knowledge register and the
after-action reviews. `bin/gate.sh --diff` is the gate; green writes a receipt bound to the
worktree, and gated files cannot be committed without one. CodeGraph supplies structural
evidence at design and inspect, and OpenWiki keeps the generated engineering wiki under
`openwiki/`: every pipeline reconciles it at complete through the `openwiki` skill, and a
completed pipeline is delivered only with the completion receipt that run leaves.
`scripts/setup-pipeline-tools.sh` installs both, pinned and project-local.

## Layout

```
crates/rusty-core   the manager layer: tasks, notes, memories, brain vault + index, skills, secrets, settings
crates/rusty-mcp    the back end: an MCP server on rmcp, 85 tools, stdio and local Streamable HTTP
crates/rusty-app    the desktop app: the workspace in QML on cxx-qt, native agent terminals (binary `rusty`)
crates/rusty-cli    terminal access to the same store: brain, tasks, notes, refresh, conversation ingest
docs/               architecture and vault rules
omarchy/            the user service unit and MCP config snippets; installer, desktop entry and hooks arrive with M6
```

## Build

```bash
cargo build
cargo test
```

Data lives in `~/.rusty/`: `rusty.db` (SQLite), `brain/` (the vault), `notes/`, `skills/`,
`.secret` (the vault of secrets, mode 600).

## Use the MCP server

From Claude Code or Codex over stdio:

```json
{ "mcpServers": { "rusty": { "type": "stdio", "command": "rusty-mcp" } } }
```

For the app, or any HTTP client, one shared process on localhost:

```bash
rusty-mcp --http                 # Streamable HTTP at http://127.0.0.1:4174/mcp
```

On a machine that will run Rusty for real, `omarchy/install.sh` builds the three binaries
into `~/.local/bin`, installs the two user services, starts the back end and the app, and
points at the two protections it leaves to you. Run it again after pulling;
every step is idempotent. The back end restarts after any exit but a stop; the app starts
with the graphical session, comes back when it is killed, and stays quit when you quit it;
`rusty session start` from any terminal is the by-hand recovery, and `rusty help` lists the
commands. `omarchy/README.md` has the details.

## Semantic search

`brain_search` merges full-text hits with nearest neighbours from `sqlite-vec` when an embedding
provider is configured; without one it stays full-text and nothing else changes. The settings
(`settings_set`, or the Settings tab): `embedding_provider` is `auto` (the default: Ollama when
it answers on this machine), `ollama`, `openai`, or `off`; `embedding_model` overrides the
provider's default (`nomic-embed-text`, `text-embedding-3-small`); `ollama_url` defaults to
`http://127.0.0.1:11434`. OpenAI needs `openai_api_key` (or `OPENAI_API_KEY`) in the secrets vault and sends page
text to OpenAI, so it is never picked by itself. The server embeds new and changed pages a few
seconds after they change; `rusty-cli brain embed --all` or the `brain_reembed` tool rebuilds,
and `rusty-cli brain semantic` shows the state. Changing the provider or model rebuilds the
index, because vectors from different models do not compare.

## Notes

Notes are markdown files in the vault under `notes/`, so the explorer, search, links, the
graph and the semantic index cover them like any page (a file there is a page of type
`note`). The notes tools (`list_notes`, `read_note`, `write_note`, `create_note`,
`rename_note`, `delete_note`) and the `/note` skill work on that folder, or on the folder
the `notes_path` setting names. An older install kept notes in `~/.rusty/notes`; run
`rusty-cli notes adopt` once (`--dry-run` first, if you like) to move them in. It refuses
when a name already exists in the vault, deletes nothing, leaves a README behind that
says where the notes went, and points `notes_path` at the new folder.

## Agent sessions

A conversation with Claude Code is a session of its own, not a child of the window. Starting
one writes an entry under `~/.local/state/rusty/agents/` and runs a transient user unit,
`rusty-agent-<id>.service`, whose only job is to own the `claude -p` process over
stream-json, keep an append-only log of everything it says and serve that log over a socket
in `$XDG_RUNTIME_DIR/rusty/agents/`. The app is one of its clients: it attaches, replays the
conversation, and writes messages, permission answers and interrupts back. Closing Rusty,
restarting it, or letting the app unit crash leaves the session where it was; reopening the
page replays it.

```
rusty agent start --cwd . --title notes --reads --strict-mcp --idle 600
rusty agent list                 # id, state, whether a host answers, title, cwd
rusty agent attach <id>          # the event stream here; a typed line is a message
rusty agent stop <id>            # interrupt the turn, end the process, end the host
rusty agent rm <id>              # stop it and delete its entry and log
journalctl --user -u rusty-agent-<id>
```

The process itself is stopped when a session sits idle (ten minutes for the pane's), and the
next message starts it again with `--resume`, so a page with a conversation costs nothing
while nobody is talking to it. A permission asked or a turn finished while no client is
attached raises a desktop notification.

## Scripts as commands

A `*.sh` file beside a skill in the store is a command: `rusty usb-reset` runs
`dev-box-usb/usb-reset.sh` from any terminal, with its arguments, in place of the window
(the app binary checks its own nouns, then the store, before Qt starts and hands the
process to `rusty-cli scripts run`, which resolves the name and execs the script). A script is named by its
basename without the suffix; `skill/name` picks one when two skills share a name. A
script inside a pending skill does not run until the skill is approved, and the safety
scan that reads a skill reads a script's text too. The Skills tab lists scripts under the
skills, shows and edits them, and Run opens a terminal tab that runs the script and keeps
a shell. Every write commits the store. The binary's own nouns come first (TICKET-029):
`rusty session start|stop|status|run` and `rusty agent ...` are built in, a script named
`session` or `agent` is shadowed,
and a bare word that is neither a noun nor a script prints the usage and exits 2; an
argument that starts with a dash goes to Qt as it always has.

```bash
rusty-cli scripts list [--all]
rusty-cli scripts new usb-reset --skill dev-box-usb      # a script without --skill gets a skill of its name
rusty-cli scripts view|path|edit|rm usb-reset
rusty-cli scripts run usb-reset [args...]
rusty usb-reset [args...]
```

The tools `script_list`, `script_view`, `script_update` and `script_run` (approved
scripts only; status, stdout and stderr, cut after sixty seconds) serve agents. Both the
dispatch and the CLI read `RUSTY_SKILLS` when it is set, so the script the window finds is
the script that runs; after that the CLI honours the `skills_path` setting, while the bare
`rusty <name>` dispatch, which has no database open yet, looks in `~/.rusty/skills`. A
store moved with `skills_path` is reached through `rusty-cli scripts run`.

## The brain loop

Ask, Decide, Follow up. Before a decision an agent calls `brain_ask` with the question:
the answer is the pages that touch it (text, and vectors when a provider is set), the
decisions already taken on the topic with their status, the follow-ups due, and a
consultation id. `brain_decide` records the decision as a page under `decisions/` with the
question, the choice, the rationale, the alternatives, a link to every consulted page (each
of which gets a timeline entry) and a `follow_up_by` date; `supersedes` names the decision
it replaces. When the date comes (`brain_due`, the Decisions view, `/brief`),
`brain_follow_up` appends the outcome and sets the status to kept, revised or superseded.
`brain_no_decision` records that a consultation led nowhere, with the reason. The graph
draws a decision's typed edges (consulted, supersedes, follows up) dashed, behind a filter.

Two Claude Code hooks make the first two steps happen in a repository wired to Rusty (a
`.mcp.json` naming a `rusty` server): the first file write waits for a `brain_ask` that
did not fail, and a session that wrote files is refused its stop once until a
`brain_decide` or a `brain_no_decision` is in its transcript. They read the transcript,
fail open when they cannot, and ship inside `rusty-cli`:

```bash
rusty-cli hooks install      # ~/.rusty/hooks/*.sh, wired into ~/.claude/settings.json
rusty-cli hooks status
rusty-cli hooks uninstall
rusty-cli brain ask "should the index move off SQLite"
rusty-cli brain decide <id> --title "Keep SQLite" --choice "..." --rationale "..." --follow-up-by 2026-10-01
rusty-cli brain follow-up decisions/keep-sqlite --status kept --outcome "..."
rusty-cli brain due --days 7
```

The seed skill `ask-decide-follow-up` carries the loop for agents.

## Folders

The left pane can hold folders from the machine below the vault tree. "Add a folder" (the
plus in the pane's header, or the palette) opens a picker; the roots are remembered per
machine in the workspace state and removed from their own menu. Folders fold like the
vault's. A click on a file opens it read-only: markdown rendered as a page is, with a
Source toggle; text in a monospace viewer with line numbers; an image fitted to the tab;
anything else through the desktop's handler. A right-click on a folder offers one entry
per agent on the machine that opens a terminal tab with that folder as its working
directory, a shell there, copy path, reveal in the file manager, and Refresh (roots have
no watcher yet). Links, backlinks, graph and search stay vault-only. File operations and
git decorations are the next two parts.

## Secrets

Keys for providers and services live in `~/.rusty/.secret`, mode 600. The Secrets tab
lists names; a value is written once. Behind a PIN the back end keeps (an argon2id hash
in `~/.rusty/.pin`, mode 600, set from the tab), the tab reveals one value at a time,
edits it in place and copies it; the unlock lasts `pin_timeout_minutes` (five by default)
and ends on Lock, when the window loses focus, and when the back end restarts; five wrong
PINs in a row lock it for a minute. The PIN protects the screen, not the file: the back
end reads the file headless for the embeddings key, and an agent with a shell reads it
regardless. Never type the PIN to an agent. The tools behind it are `secret_pin_status`,
`secret_pin_set`, `secret_unlock`, `secret_lock`, `secret_reveal` and `secret_update`;
`secret_list` stays name-only, and no tool returns a value without a live unlock token.

## Vault tools

The workspace's own tools, which agents share: `brain_tree` (the folders and files),
`brain_render` (a page as rich text, with its outline, links, unresolved targets, counts,
properties and raw file), `brain_write_page` (the whole file, as an editor saves),
`brain_new_page`, `brain_new_folder`, `brain_delete_folder` (soft, into `archive/`),
`brain_rename` (page or folder, every link rewritten, index rows moved),
`brain_unresolved`, `brain_tags` (every tag with its count), `brain_set_property` and
`brain_remove_property` (one frontmatter key, typed), and `brain_graph` (pages and links
as nodes and edges, tags and unresolved targets on request, or one page's neighbourhood). A vault file without frontmatter is a page too: its title is the file
name and its type comes from its top folder (`people/` is `person`), or `note`. The server
also indexes files changed by another program (Obsidian, an editor, git) a few seconds after
they change.

## Obsidian

The brain folder is a plain Obsidian vault, and Obsidian still opens it; Rusty's own
workspace is where the vault is read and written. The bridge that drove Obsidian's
command-line interface (six `obsidian_*` tools, `rusty-cli obsidian`, the installer's
registration, the app's theme-snippet call) was retired on 2026-09-03 (TICKET-006) once the
workspace covered what it did: `brain_get_links`, `brain_unresolved` and `brain_rename` answer
for links and renames, and the app opens pages. Obsidian's per-machine state in `.obsidian/`
stays out of the vault's git history.

Two vault rules keep the two writers agreeing. A page's timeline is its `## Timeline` section,
and wikilinks are vault paths (`[[projects/orbit]]`). `rusty-cli brain migrate --dry-run` shows what
an older vault would change; without the flag it rewrites the pages, reindexes, and commits.

## License

MIT. The bundled `no-ai-slop` skill text is vendored from
[petergyang/no-ai-slop](https://github.com/petergyang/no-ai-slop) (MIT).
