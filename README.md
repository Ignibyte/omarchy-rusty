# Rusty

A local-first memory and knowledge store for AI agents, built for
[Omarchy](https://omarchy.org). Rusty runs on your machine as an MCP server and keeps your
to-do lists, memories, notes, skills, secrets and a markdown knowledge vault in one place
that Claude Code, Codex and any other MCP client can read and write. Nothing leaves the
machine unless you turn on a setting that sends it.

- **A brain you can open in any editor.** The vault is a folder of markdown with
  frontmatter and `[[wikilinks]]`, versioned with git, indexed in SQLite for full-text
  search, and optionally embedded for semantic search with a local Ollama model. Obsidian
  opens it as a vault.
- **90 MCP tools** over stdio for agents and Streamable HTTP for long-running clients:
  pages, links, tags, a graph, decisions with follow-ups, tasks, memories, notes, skills,
  scripts, sources, settings and secrets.
- **A terminal CLI** (`rusty-cli`) for the same store, and a small `rusty` command that
  starts the back end and runs your store scripts.

**Status: 0.1.0, an early release.** It runs on Linux with systemd user services and is
built and used daily on Omarchy (Arch, Hyprland). Expect rough edges and say what you hit.

## Install

You need `git`, `curl` and a systemd user session on x86_64 Linux. Optional:
[Ollama](https://ollama.com) for semantic search, `pdftotext` (poppler) for capturing PDFs.

From a release (no Rust toolchain needed): download `rusty-<version>-x86_64-linux.tar.gz`
from the [releases page](https://github.com/Ignibyte/omarchy-rusty/releases), check it
against its `.sha256`, unpack it and run the installer inside:

```bash
sha256sum -c rusty-0.1.0-x86_64-linux.tar.gz.sha256
tar xzf rusty-0.1.0-x86_64-linux.tar.gz
rusty-0.1.0-x86_64-linux/omarchy/install.sh
```

From source, with a Rust toolchain (stable):

```bash
git clone https://github.com/Ignibyte/omarchy-rusty.git
cd omarchy-rusty
omarchy/install.sh
```

The installer puts `rusty-mcp`, `rusty-cli` and `rusty` in `~/.local/bin` (built with
cargo from a checkout, copied from a release), installs the user service
`rusty-mcp.service` (Streamable HTTP on `127.0.0.1:4174/mcp`), starts it and checks it
answers. Run it again to upgrade; every step is idempotent. `omarchy/README.md` covers the
service and two optional protections for low-memory machines. `packaging/` holds Arch
packages: `rusty-git` from the main branch and `rusty-bin` from a release.

On first start Rusty creates `~/.rusty/` with an empty vault, a skills store holding a
few seed skills, and the database.

## Connect an agent

Claude Code, for every project with `claude mcp add`, or per project in `.mcp.json`:

```bash
claude mcp add --scope user rusty -- rusty-mcp
```

```json
{ "mcpServers": { "rusty": { "type": "stdio", "command": "rusty-mcp" } } }
```

Codex, in `~/.codex/config.toml`:

```toml
[mcp_servers.rusty]
command = "rusty-mcp"
```

Any other MCP client can use the service over Streamable HTTP at
`http://127.0.0.1:4174/mcp`; `omarchy/mcp-config.json` has both forms. Marley, a Zed fork,
draws a knowledge workspace (pages, graph, tasks, decisions, memory, skills, secrets) over
that endpoint.

## Use it from a terminal

```bash
rusty session start                  # start the back end's service; `status` checks it
rusty-cli brain search "release plan"
rusty-cli brain new concept "Release plan"
rusty-cli brain capture "call the bank about the card" --to inbox
rusty-cli brain due                  # decisions due for a follow-up
rusty-cli --help                     # everything else
```

## Where your data lives

| Path | What |
|---|---|
| `~/.rusty/rusty.db` | SQLite: tasks, memories, settings, the change log, and the vault's search index (rebuildable from the files) |
| `~/.rusty/brain/` | the vault: markdown pages, a git repository |
| `~/.rusty/skills/` | the skills store (`.claude/skills/<name>/SKILL.md`), a git repository |
| `~/.rusty/.secret` | the secrets file, plain text, mode 600 |
| `~/.rusty/.pin` | the PIN's argon2id hash, when you set one |

The vault and skills paths can move with the `brain_vault_path` and `skills_path`
settings; notes live in the vault's `notes/` folder unless `notes_path` says otherwise.

## What leaves the machine

Nothing, by default. Three things can reach the network, each because you asked:

- **Embeddings.** `embedding_provider` is `auto`: Ollama on this machine when it answers,
  otherwise none. Set it to `openai` with a key in the secrets file and page text goes to
  OpenAI; Rusty never picks that by itself.
- **Sources.** `source_capture` (and `rusty-cli source capture`) fetches the URL you give it.
- **Your agents.** What an agent reads from Rusty goes to that agent's model; that is the
  agent's traffic, not Rusty's.

The vault and skills store commit to their own local git repositories and push nowhere.

## Moving to another machine

One zip carries the whole store: the database (a consistent snapshot, taken while the
server runs), the brain vault and the skills store with their git history, and a notes
folder kept outside the vault.

```bash
rusty-cli export ~/rusty-backup.zip                     # secrets left out
rusty-cli export ~/rusty-backup.zip --include-secrets   # the secrets file and the PIN too
```

The zip is readable by its owner only. With `--include-secrets` it holds the secrets file
in the clear, so keep it as carefully as the machine itself. Symbolic links and git's lock
files are left out, and the export says so.

On the other machine, stop the back end first (an import refuses while a `rusty-mcp` holds
the store open), then:

```bash
rusty-cli import ~/rusty-backup.zip --dry-run    # what is inside and what would happen
rusty-cli import ~/rusty-backup.zip              # into an empty ~/.rusty
rusty-cli import ~/rusty-backup.zip --replace    # an existing ~/.rusty moves aside, whole
systemctl --user start rusty-mcp
```

The import checks every entry before it writes anything, builds the store beside
`~/.rusty` and renames it into place; with `--replace` the old home becomes
`~/.rusty.before-import-<time>`, never deleted, and its Claude Code hook scripts come
across. Path settings that named the old machine's folders are rewritten for the new
layout. Merging two stores is not supported.

## Semantic search

`brain_search` merges full-text hits with nearest neighbours from `sqlite-vec` when an
embedding provider is configured; without one it stays full-text and nothing else changes.
The settings (`setting_set`): `embedding_provider` is `auto` (the default: Ollama when it
answers on this machine), `ollama`, `openai`, or `off`; `embedding_model` overrides the
provider's default (`nomic-embed-text`, `text-embedding-3-small`); `ollama_url` defaults
to `http://127.0.0.1:11434`. OpenAI needs `openai_api_key` (or `OPENAI_API_KEY`) in the
secrets vault and sends page text to OpenAI, so it is never picked by itself. The server
embeds new and changed pages a few seconds after they change; `rusty-cli brain embed
--all` or the `brain_reembed` tool rebuilds, and `rusty-cli brain semantic` shows the
state. Changing the provider or model rebuilds the index, because vectors from different
models do not compare.

## Notes

Notes are markdown files in the vault under `notes/`, so search, links, the graph and the
semantic index cover them like any page (a file there is a page of type
`note`). The notes tools (`list_notes`, `read_note`, `write_note`, `create_note`,
`rename_note`, `delete_note`) work on that folder, or on the folder the `notes_path`
setting names.

## Scripts as commands

A `*.sh` file beside a skill in the store is a command: `rusty snapshot` runs
`backup/snapshot.sh` from any terminal, with its arguments (the `rusty` command checks its
own nouns, then the store, and hands the process to `rusty-cli scripts run`, which
resolves the name and execs the script). A script is named by its basename without the
suffix; `skill/name` picks one when two skills share a name. A script inside a pending
skill does not run until the skill is approved, and the safety scan that reads a skill
reads a script's text too. Every write commits the store. The binary's own nouns come
first: `rusty session start|status` is built in, a script named `session` is shadowed, and
a bare word or a flag that is neither a noun nor a script prints the usage and exits 2.

```bash
rusty-cli scripts list [--all]
rusty-cli scripts new snapshot --skill backup      # a script without --skill gets a skill of its name
rusty-cli scripts view|path|edit|rm snapshot
rusty-cli scripts run snapshot [args...]
rusty snapshot [args...]
```

The tools `script_list`, `script_view`, `script_update` and `script_run` (approved
scripts only; status, stdout and stderr, cut after sixty seconds) serve agents. Both the
dispatch and the CLI read `RUSTY_SKILLS` when it is set, so the script `rusty <name>` finds
is the script that runs; after that the CLI honours the `skills_path` setting, while the bare
`rusty <name>` dispatch, which has no database open yet, looks in `~/.rusty/skills`. A
store moved with `skills_path` is reached through `rusty-cli scripts run`.

The store is a git repository. Every skill and script write commits it before it
returns, from the CLI or over MCP (`skill_create`, `skill_update`, `skill_delete`,
`skill_approve`, `skill_reject`, `script_update`), with a subject naming the change
(`skills: add`, `skills: stage`, `skills: approve`, and so on); a commit that fails
leaves the change for the next one and never fails the write.

## The brain loop

Ask, Decide, Follow up. Before a decision an agent calls `brain_ask` with the question:
the answer is the pages that touch it (text, and vectors when a provider is set), the
decisions already taken on the topic with their status, the follow-ups due, and a
consultation id. `brain_decide` records the decision as a page under `decisions/` with the
question, the choice, the rationale, the alternatives, a link to every consulted page (each
of which gets a timeline entry) and a `follow_up_by` date; `supersedes` names the decision
it replaces. When the date comes (`brain_due`, or the `morning-brief` seed skill),
`brain_follow_up` appends the outcome and sets the status to kept, revised or superseded.
`brain_no_decision` records that a consultation led nowhere, with the reason.
`brain_graph` returns a decision's typed edges (consulted, supersedes, follows up) on
request.

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

## Secrets

Keys for providers and services live in `~/.rusty/.secret`, mode 600. A client lists
names; a value is written once. Behind a PIN the back end keeps (an argon2id hash in
`~/.rusty/.pin`, mode 600), a client reveals one value at a time and edits it in place; the
unlock lasts `pin_timeout_minutes` (five by default) and ends on `secret_lock` and when the
back end restarts; five wrong PINs in a row lock it for a minute. The PIN protects the
screen, not the file: the back
end reads the file headless for the embeddings key, and an agent with a shell reads it
regardless. Never type the PIN to an agent. The tools behind it are `secret_pin_status`,
`secret_pin_set`, `secret_unlock`, `secret_lock`, `secret_reveal` and `secret_update`;
`secret_list` stays name-only, and no tool returns a value without a live unlock token.
Once a PIN is set, `secret_set` and `secret_delete` need that token too; with none set they
need nothing. A token is good only on the server process that issued it.
`settings_list` and `setting_get` show a setting whose key names a key, token, secret or
password as `•••`, and `setting_set` refuses that mask written back; credentials a client
must read belong in the vault.

## Vault tools

The tools an editor of the vault uses, Marley's workspace and agents alike: `brain_tree`
(the folders and files), `brain_render` (a page as rich text, with its outline, links,
unresolved targets, counts, properties and raw file), `brain_write_page` (the whole file,
as an editor saves), `brain_new_page`, `brain_new_folder`, `brain_delete_folder` (soft,
into `archive/`), `brain_rename` (page or folder, every link rewritten, index rows moved),
`brain_unresolved`, `brain_tags` (every tag with its count), `brain_set_property` and
`brain_remove_property` (one frontmatter key, typed), and `brain_graph` (pages and links
as nodes and edges, tags and unresolved targets on request, or one page's neighbourhood).
A vault file without frontmatter is a page too: its title is the file name and its type
comes from its top folder (`people/` is `person`), or `note`. The server also indexes
files changed by another program (Obsidian, an editor, git) a few seconds after they
change.

## Obsidian

The brain folder is a plain Obsidian vault, and Obsidian opens it; Rusty's tools are where
agents read and write it, and `brain_get_links`, `brain_unresolved` and `brain_rename`
answer for links and renames. Obsidian's per-machine state in `.obsidian/` stays out of
the vault's git history. An existing Obsidian vault comes in with `rusty-cli brain import
<vault>` (`--dry-run` first): pages keep their paths, nothing is overwritten, and a report
page lists what happened.

Two vault rules keep the two writers agreeing. A page's timeline is its `## Timeline` section,
and wikilinks are vault paths (`[[projects/orbit]]`). `rusty-cli brain migrate --dry-run` shows what
an older vault would change; without the flag it rewrites the pages, reindexes, and commits.

## Uninstall

```bash
rusty-cli hooks uninstall          # only if you installed the brain loop's hooks
systemctl --user disable --now rusty-mcp
rm ~/.config/systemd/user/rusty-mcp.service ~/.local/bin/rusty ~/.local/bin/rusty-cli ~/.local/bin/rusty-mcp
systemctl --user daemon-reload
```

Your data stays in `~/.rusty/`; export it first if you want it in one file, and delete the
folder when you are sure.

## Development

```bash
cargo build
cargo test
bin/gate.sh --diff      # fmt, clippy (warnings as errors), tests, docs, shell and secrets checks
```

Run cargo commands one at a time. `CONTRIBUTING.md` says how to propose a change;
`docs/architecture.md` describes the shape, and `openwiki/` is the generated engineering
wiki. The repository also carries the agent workflow its maintainers use
(`CONSTITUTION.md`, `AGENTS.md`, `.claude/`); a contributor's clone runs the gate and CI
without it.

## License

MIT. The bundled `no-ai-slop` skill text is vendored from
[petergyang/no-ai-slop](https://github.com/petergyang/no-ai-slop) (MIT).
