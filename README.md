# Rusty

A local-first memory and knowledge store for AI agents, built for
[Omarchy](https://omarchy.org). Rusty runs on your machine as an MCP server and keeps your
to-do lists, memories, notes, skills, secrets and a markdown knowledge vault in one place
that Claude Code, Codex and any other MCP client can read and write. Nothing leaves the
machine unless you set it up to.

- **A brain you can open in any editor.** The vault is a folder of markdown with
  frontmatter and `[[wikilinks]]`, versioned with git, indexed in SQLite for full-text
  search, and optionally embedded for semantic search with a local Ollama model. Obsidian
  opens it as a vault.
- **90 MCP tools** over stdio for agents and Streamable HTTP for long-running clients:
  pages, links, tags, a graph, decisions with follow-ups, to-do lists, memories, notes,
  skills, scripts, sources, settings, secrets and a conversation archive.
- **A terminal CLI** (`rusty-cli`) for the same store, and a small `rusty` command that
  starts the back end and runs your store scripts.

**Status: 0.1.0, an early release.** It runs on Linux with systemd user services and is
built and used daily on Omarchy (Arch, Hyprland). Expect rough edges, and please
[open an issue](https://github.com/Ignibyte/omarchy-rusty/issues) for what you hit.

## Install

You need x86_64 Linux with a systemd user session, `git` and `curl`. Optional:
[Ollama](https://ollama.com) for semantic search, `pdftotext` (poppler) for capturing
PDFs, and `jq` for the brain loop's Claude Code hooks.

From a release, with no Rust toolchain: download `rusty-<version>-x86_64-linux.tar.gz`
and its `.sha256` from the [releases page](https://github.com/Ignibyte/omarchy-rusty/releases)
(if it lists no release yet, build from source as below), then:

```bash
sha256sum -c rusty-0.1.0-x86_64-linux.tar.gz.sha256
tar xzf rusty-0.1.0-x86_64-linux.tar.gz
rusty-0.1.0-x86_64-linux/omarchy/install.sh
```

From source, with a stable Rust toolchain and a C compiler (SQLite, `ring` and
`sqlite-vec` build C code):

```bash
git clone https://github.com/Ignibyte/omarchy-rusty.git
cd omarchy-rusty
omarchy/install.sh
```

The installer puts `rusty-mcp`, `rusty-cli` and `rusty` in `~/.local/bin` (built with
cargo from a checkout, copied from a release), installs the user service
`rusty-mcp.service` (Streamable HTTP on `127.0.0.1:4174/mcp`), starts it and checks it
answers. On its first start Rusty creates `~/.rusty/` with an empty vault, a skills store
holding four seed skills, and the database. `omarchy/README.md` covers the service and two
optional protections for low-memory machines; `packaging/` holds two Arch packages.

**Upgrading** is the same command with a newer release or a pulled checkout. It restarts
the service, so HTTP clients reconnect; an agent's own `rusty-mcp` keeps the old build
until the agent restarts. Database migrations run when a program opens the store, and
they only add. Run `rusty-cli export` first if you want a copy to go back to.

## Connect an agent

Claude Code, for every project or per project in `.mcp.json`:

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

Each agent starts its own `rusty-mcp` over stdio; every copy works on the same store. If
the agent does not have `~/.local/bin` on its `PATH`, give the full path as the command.
Any other MCP client can use the service over Streamable HTTP at
`http://127.0.0.1:4174/mcp`; `omarchy/mcp-config.json` has both forms.
[Marley](https://github.com/Ignibyte/marley_ide), a Zed fork, draws a knowledge workspace
(pages, graph, to-do lists, decisions, memories, skills, secrets) over that endpoint.

The store's skills live in `~/.rusty/skills/.claude/skills/`. Any agent reads them with
`skill_list` and `skill_view`. Claude Code loads them as its own skills for a session
started with `claude --add-dir ~/.rusty/skills`, or in every session for a skill you link
into `~/.claude/skills/` (`ln -s ~/.rusty/skills/.claude/skills/<name> ~/.claude/skills/`).

## First steps from a terminal

```bash
rusty session status                 # is the back end running and answering?
rusty-cli brain new concept "Release plan"
rusty-cli brain search "release plan"
rusty-cli brain capture "call the bank about the card" --to inbox
rusty-cli brain due                  # decisions due for a follow-up
rusty-cli --help                     # everything else
```

## Documentation

| Read | For |
|---|---|
| [docs/tools.md](docs/tools.md) | every MCP tool with its parameters, and the `rusty://` resources |
| [docs/cli.md](docs/cli.md) | `rusty-cli` and `rusty`, command by command |
| [docs/configuration.md](docs/configuration.md) | the store's layout, settings, secrets, environment variables, the service, semantic search setup |
| [docs/architecture.md](docs/architecture.md) | how the crates and the store fit together |
| [docs/architecture/brain-loop.md](docs/architecture/brain-loop.md) | ask, decide, follow up |
| [SECURITY.md](SECURITY.md) | what Rusty trusts, and how to report a vulnerability |
| [CONTRIBUTING.md](CONTRIBUTING.md) | proposing a change |
| [CHANGELOG.md](CHANGELOG.md), [ROADMAP.md](ROADMAP.md) | what changed, what comes next |

## Where your data lives

Everything is under `~/.rusty`: `rusty.db` (SQLite: to-do lists, memories, settings, the
change log, the conversation archive, and the vault's search index), `brain/` (the vault,
a git repository), `skills/` (the skills store, a git repository), `.secret` (plain text,
mode 600) and `.pin` (the PIN's hash, once you set one).
[docs/configuration.md](docs/configuration.md) has the full layout and the settings that
move the vault, the notes or the skills store elsewhere.

## What leaves the machine

Nothing, by default. These can reach the network, each because you set it up:

- **Embeddings.** `embedding_provider` is `auto`: Ollama at `ollama_url` (this machine by
  default) when it answers, otherwise none. Page text goes wherever `ollama_url` points,
  so keep it on a machine you trust. Set the provider to `openai` with a key in the secrets
  file and page text goes to OpenAI; Rusty never picks that by itself.
- **Sources.** `source_capture` (and `rusty-cli source capture`) fetches the URL it is
  given.
- **Your agents.** What an agent reads from Rusty goes to that agent's model; that is the
  agent's traffic, not Rusty's.

The vault and the skills store commit to their own local git repositories and push
nowhere.

## Moving to another machine

One zip carries the whole store: the database (a consistent snapshot, taken while the
server runs), the vault and the skills store with their git history, and a notes folder
kept outside the vault. On the old machine:

```bash
rusty-cli export ~/rusty-backup.zip                     # secrets left out
rusty-cli export ~/rusty-backup.zip --include-secrets   # the secrets file and the PIN too
```

The zip is readable by you alone and is not encrypted: with `--include-secrets` it holds
your secrets in the clear. Symbolic links and git's lock files are left out, and the
export says so.

On the new machine, install Rusty, then stop everything that holds the store open (the
service, and any agent or editor running its own `rusty-mcp`) and import over the fresh
store the installer created:

```bash
systemctl --user stop rusty-mcp
rusty-cli import ~/rusty-backup.zip --dry-run    # what is inside and what would happen
rusty-cli import ~/rusty-backup.zip --replace
systemctl --user start rusty-mcp
```

The import checks every entry before it writes anything, builds the store beside
`~/.rusty` and renames it into place. `--replace` moves the existing store aside as
`~/.rusty.before-import-<time>`, whole, and carries its Claude Code hook scripts across;
nothing is deleted. Path settings that named the old machine's folders are rewritten.
Merging two stores is not supported.

## The brain

Pages are markdown files with YAML frontmatter, in folders by type (`people/`,
`projects/`, `concepts/`, `decisions/`, `sources/`, …) or any folder you make. A file
without frontmatter is a page too: its title is the file name and its type comes from its
folder, or `note`. Wikilinks are vault paths (`[[projects/orbit]]`), and a page's history
is its `## Timeline` section. Deletes are soft: the page moves to `archive/`. Rusty commits
every change it makes, and the running server indexes and commits files another program
changes (Obsidian, an editor, git) a few seconds after they change.

Search is full text with operators (`tag:`, `path:`, `file:`, `type:`, quoted values, `-`
to exclude), plus vectors when an embedding provider is set
([docs/configuration.md](docs/configuration.md#semantic-search) shows the Ollama setup).
Notes are the pages in the vault's `notes/` folder, and the notes tools work on that
folder. Obsidian opens the vault as it is; its per-machine `.obsidian/` state stays out of
the vault's history. An existing Obsidian vault comes in with
`rusty-cli brain import <vault>` (`--dry-run` first): pages keep their paths, nothing in
the brain is overwritten, and a report page lists what happened.

## The brain loop

Ask, decide, follow up. Before a decision an agent calls `brain_ask` with the question and
gets the pages that touch it, the decisions already taken on the topic, the follow-ups
due, and a consultation id. `brain_decide` records the decision as a page under
`decisions/` linked to every page it rested on, with a `follow_up_by` date. When the date
comes (`brain_due`, or the `morning-brief` seed skill), `brain_follow_up` records how it
went. `brain_no_decision` records a consultation that led nowhere.

Two optional Claude Code hooks hold a session to the first two steps:
`rusty-cli hooks install` writes them to `~/.rusty/hooks/` and adds them to
`~/.claude/settings.json`. They act only when the session's directory has a `.mcp.json`
naming a server called `rusty` (a server added with `claude mcp add --scope user` alone
does not count), and they need `jq`. The first file write is blocked until the session
has called `brain_ask`; a session that wrote files is refused its first stop until it
records a decision or `brain_no_decision`. They read the transcript and step aside when
they cannot. [docs/architecture/brain-loop.md](docs/architecture/brain-loop.md) has the
design.

## Skills and scripts

A skill is a `SKILL.md` in the store. One created with `skill_create` or
`rusty-cli skills new` is active at once; with `pending: true` it waits in `staging/`
until someone approves it, and approval runs the safety scan first (`force` skips it).
`skill_scan` scans any skill on demand. A `*.sh` file beside a skill is a script: run it
as `rusty <name>` from a terminal, `rusty-cli scripts run`, or `script_run` (a script in a
staged skill does not run). Every skill and script change commits the store.

Any client that can reach the server can write a skill or a script and run an approved
script as you; see [SECURITY.md](SECURITY.md).

## Secrets

Keys for providers and services live in `~/.rusty/.secret`, mode 600; the tools list
names, never values. Set a PIN with `secret_pin_set` to read and change values through
the tools: `secret_unlock` returns a token that lasts `pin_timeout_minutes` (five by
default) on that server process, and `secret_reveal` and `secret_update` need it, as do
`secret_set` and `secret_delete` once a PIN exists. Five wrong PINs in a row lock unlocking
for a minute. The PIN guards the tools, not the file: anything running as your user can
read it. Never type the PIN to an agent. A setting whose key names a key, token, secret or
password reads back as `•••`; credentials belong in the secrets file.

## Conversations

`rusty-cli ingest-conversation <transcript or session id>` keeps a Claude Code session:
the dialogue goes into a full-text archive and a `conversation` page goes into the brain,
linked to related pages. Nothing is read until you run it. `search_conversations` and
`rusty-cli conversations search` search the archive.

## Troubleshooting

- **Is it running?** `rusty session status`, then `journalctl --user -u rusty-mcp -e`.
- **An agent cannot start `rusty-mcp`.** Its `PATH` lacks `~/.local/bin`; use the full
  path in its MCP config.
- **Port 4174 is taken.** Change `--http` in the unit's `ExecStart`, then
  `systemctl --user daemon-reload` and restart it.
- **A client does not see a change another process made.** Each server tells its own
  clients about its writes and about file changes it notices; a change another process
  made only in the database (a to-do, a memory) shows up through `changes_since`. After
  writing the database by hand, run `rusty-cli refresh`.
- **Semantic search is off.** `rusty-cli brain semantic` shows the provider and the index;
  with Ollama, run `ollama pull nomic-embed-text` and `rusty-cli brain embed --all`.
- **The brain loop's hooks do nothing.** `rusty-cli hooks status`, then check for `jq` and
  a `.mcp.json` naming `rusty` in the project.
- **An import is refused.** Stop the service and every agent's `rusty-mcp`; add
  `--replace` when the target already holds a store.

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
bin/gate.sh --diff      # fmt, clippy (warnings as errors), tests, docs, shell, secrets, whitespace
```

Run cargo commands one at a time. [CONTRIBUTING.md](CONTRIBUTING.md) says how to propose a
change, and `openwiki/` is the generated engineering wiki. The maintainers' agent workflow
(`CONSTITUTION.md`, `AGENTS.md`, `.claude/`) is in the repository too; a contributor
follows the "Without `docs/planning`" section of `AGENTS.md`.

## License

MIT; see `LICENSE`. The seed skill `no-ai-slop` is condensed from
[petergyang/no-ai-slop](https://github.com/petergyang/no-ai-slop) (MIT); its notice is in
`THIRD_PARTY_NOTICES.md`.
