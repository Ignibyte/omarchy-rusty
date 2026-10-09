# Command reference

Rusty installs two commands: `rusty-cli`, the terminal client for the store, and `rusty`,
which starts the back end and runs store scripts. Both read the store under `~/.rusty`
(`$HOME/.rusty`; see [configuration.md](configuration.md) for the settings that move its
parts). `rusty-cli` opens the store in-process, so it works whether or not the service
runs. A command that fails prints the reason on standard error (most as `error: <what>`)
and exits 1; a mistake in how it was called (an unknown flag, a missing value, a count
that is not a number) exits 2. Flags take `--flag value` or `--flag=value`. The MCP tools
are in [tools.md](tools.md).

## `rusty`

```text
rusty session start        start the back end's user service, then print its status
rusty session status       the service's state, and whether 127.0.0.1:4174/mcp answers
rusty <script> [args...]   run an approved store script (a *.sh beside a skill)
rusty help                 the usage; `rusty` alone prints it too
```

`rusty session status` probes `RUSTY_MCP_ADDR` when it is set. A store script is looked up
in `RUSTY_SKILLS`, else `~/.rusty/skills`; a store moved with the `skills_path` setting
runs its scripts through `rusty-cli scripts run`.

## `rusty-cli`

`rusty-cli --help` prints the same list, grouped the same way. A slug is a page's vault
path without `.md` (`projects/orbit`).

### Brain pages

```text
rusty-cli brain search <query...> [--limit N] [--type TYPE]
rusty-cli brain read <slug>
rusty-cli brain new <type> <title...> [--content <text>]
rusty-cli brain set <slug> <content...>
rusty-cli brain append <slug> <summary...> [--detail <text>]
rusty-cli brain capture <text...> [--to daily|inbox] [--date YYYY-MM-DD]
rusty-cli brain daily [--date YYYY-MM-DD]
rusty-cli brain context <prompt...>
rusty-cli brain types
rusty-cli brain stats
rusty-cli bookmarks [add|rm <path or search:query>]
```

- `search` takes the same operators as the `brain_search` tool: `tag:`, `path:`, `file:`,
  `type:`, a value in quotes, a leading `-` to exclude.
- `new` makes a page of a type (`person`, `company`, `project`, `concept`, `idea`,
  `meeting`, …; `brain types` lists them) in that type's folder; `set` replaces a page's
  body and keeps its frontmatter; `append` adds a dated entry to its `## Timeline`.
- `capture` appends the text to today's daily page (`daily/YYYY-MM-DD`), or with
  `--to inbox` to the one inbox page (`inbox/inbox`), creating the page when needed.
- `context` prints the pages that best match a prompt as one block, the way a hook would
  hand them to an agent.

### Brain upkeep

```text
rusty-cli brain reindex
rusty-cli brain embed [--all]
rusty-cli brain semantic
rusty-cli brain migrate [--dry-run]
rusty-cli brain import <vault> [--dry-run]
```

- `reindex` rebuilds the page index from the vault; the vault is not touched.
- `embed` computes vectors for pages whose text changed (`--all`: every page) with the
  configured provider; `semantic` shows the provider and how much of the vault is indexed.
- `migrate` rewrites older pages to the current layout: a `## Timeline` section and
  wikilinks as vault paths. `--dry-run` lists what it would change.
- `import` brings an Obsidian vault in. The source is read and never written; a page that
  would overwrite one already in the brain is skipped and named in the report under
  `inbox/`.

### The brain loop

```text
rusty-cli brain ask <question>
rusty-cli brain decide <id> --title T --choice C --rationale R [--alt A]... [--follow-up-by DATE] [--supersedes SLUG]
rusty-cli brain follow-up <slug> --status kept|revised|superseded --outcome O [--successor SLUG] [--follow-up-by DATE]
rusty-cli brain no-decision <id> <reason>
rusty-cli brain due [--days N]
rusty-cli hooks install|uninstall|status
```

`ask` returns a consultation id that `decide` and `no-decision` take. The loop and its
hooks are described in [architecture/brain-loop.md](architecture/brain-loop.md). `hooks
install` writes the two hook scripts to `~/.rusty/hooks/` and adds them to
`~/.claude/settings.json`, keeping every other entry; they act only in a directory whose
`.mcp.json` names a `rusty` server, and do nothing without `jq`.

### Sources

```text
rusty-cli source capture <url>
rusty-cli source search <query...> [--limit N]
```

`capture` fetches an http or https URL (a web page, a PDF, markdown or plain text) and
keeps the readable text as a `source` page under `sources/`; a PDF needs `pdftotext`.
Capturing the same URL again updates its page. Captured text is marked as untrusted
wherever a tool returns it and in `brain context`.

### Notes

```text
rusty-cli notes path
rusty-cli notes adopt [--dry-run]
```

Notes live in the vault's `notes/` folder by default. `adopt` moves a notes folder kept
elsewhere (the `notes_path` setting) into the vault once, refusing if any destination
exists, and points the setting at the new folder.

### Skills and scripts

```text
rusty-cli skills list [--all]
rusty-cli skills view <name>
rusty-cli skills new <name> [--desc <text>] [--body <text>] [--force]
rusty-cli skills rm <name>
rusty-cli skills path
rusty-cli skills review
rusty-cli skills approve <name> [--force]
rusty-cli skills reject <name>
rusty-cli scripts list [--all]
rusty-cli scripts view|path|edit|rm <name>
rusty-cli scripts new <name> [--skill S] [--body TEXT] [--force]
rusty-cli scripts run <name> [args...]
```

- `list --all` includes skills staged for approval. `review` shows the staged ones with
  what the safety scan found; `approve` refuses a skill the scan flags unless `--force`.
- A script is a `*.sh` file in a skill's folder and runs as `rusty <name>`; a script in a
  staged skill does not run. `scripts edit` opens `$VISUAL`, else `$EDITOR`.
- Every change commits the skills store.

### Conversations

```text
rusty-cli ingest-conversation <path|session-id>
rusty-cli ingest-conversation --all [--dir <path>] [--limit N]
rusty-cli conversations search <query...> [--limit N]
```

`ingest-conversation` reads a Claude Code transcript (`~/.claude/projects/…/<id>.jsonl`)
into a full-text archive and writes a `conversation` page to the brain. Nothing is read
until you run it; `--all` takes every transcript of the Claude Code project for the
current directory, or `--dir`.

### The store

```text
rusty-cli changes [--since <cursor>] [--limit N]
rusty-cli settings list
rusty-cli settings get <key>
rusty-cli settings set <key> <value>
rusty-cli refresh
rusty-cli export <file.zip> [--include-secrets]
rusty-cli import <file.zip> [--replace] [--dry-run]
```

- `changes` lists what changed after a cursor, whichever process made the change; without
  `--since` it prints only the current cursor, to start from.
- `settings` reads and writes the settings in
  [configuration.md](configuration.md#settings) through the same rules as the tools: a
  credential-looking value reads back as `•••`, writing that mask back is refused, and a
  path setting takes an absolute path (or one starting with `~/`) outside the store's
  home folder. A path setting applies when a server starts.
- `refresh` tells running servers to reload, after something wrote the store around them.
- `export` writes the whole store (database snapshot, vault, skills, notes kept outside
  the vault) to a new zip readable by you alone. Secrets go in only with
  `--include-secrets`, and the zip is not encrypted.
- `import` unpacks an export into `~/.rusty`. It refuses while a `rusty-mcp` holds that
  store open, and refuses a home that already holds a store unless `--replace`, which
  moves the old one aside as `~/.rusty.before-import-<time>`. `--dry-run` reports and
  writes nothing. [Moving to another machine](../README.md#moving-to-another-machine)
  shows the whole sequence.
