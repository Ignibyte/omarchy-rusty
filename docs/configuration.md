# Configuration

Where Rusty keeps its data, the settings that change its behaviour, the secrets file, and
the environment variables the programs read. Everything lives under `~/.rusty` unless a
setting moves a part of it.

## The store

```text
~/.rusty/
├── rusty.db              SQLite (WAL): the page index, to-do lists, memories, settings,
│                         the change log, decisions' consultations, the conversation archive
├── brain/                the vault: markdown pages, a git repository
│   ├── people/ companies/ projects/ concepts/ meetings/ ideas/
│   ├── daily/ inbox/ decisions/ conversations/ sources/
│   ├── notes/            notes, also pages of type `note` (deleted ones in notes/.deleted/)
│   ├── archive/          deleted pages and folders (not pages; ignored by git)
│   ├── .templates/       page templates, one per type (ignored by git)
│   └── .rusty/bookmarks.json
│                         (a new vault's .gitignore also leaves out Obsidian's .obsidian/)
├── skills/               the skills store, a git repository
│   ├── .claude/skills/<name>/SKILL.md   active skills, with any *.sh scripts beside them
│   └── staging/<name>/SKILL.md          skills waiting for approval
├── .secret               secrets, KEY='value' lines, mode 0600
├── .pin                  the PIN's argon2id hash, when a PIN is set
├── .pin-attempts         wrong PIN tries and any lockout, shared by every server process
├── hooks/                the brain loop's Claude Code hooks, after `rusty-cli hooks install`
└── .changed              touched by `rusty-cli refresh` so running servers reload
```

Any other folder in the vault is a folder of notes; a page outside the type folders has the
type `note`. The vault opens in Obsidian as it is. Rusty commits every change it makes to
`brain/` and `skills/`; a running server also commits edits other programs make there.

To move the whole store to another machine, see `rusty-cli export` and `import` in
[cli.md](cli.md#the-store).

## Settings

Settings are rows in `rusty.db`. Any MCP client reads and writes them with
`settings_list`, `setting_get` and `setting_set`, and a terminal with `rusty-cli settings`;
both record the change in the change log. A value whose key names a key, token, secret or
password reads back as `•••`, and writing `•••` back is refused.

```bash
rusty-cli settings set embedding_provider ollama
rusty-cli settings list
```

| Key | Default | What it does |
|---|---|---|
| `embedding_provider` | `auto` | Semantic search: `auto` (Ollama when it answers), `ollama`, `openai`, or `off`. |
| `embedding_model` | the provider's | `nomic-embed-text` for Ollama, `text-embedding-3-small` for OpenAI. |
| `ollama_url` | `http://127.0.0.1:11434` | Where Ollama listens. Page text is sent there to be embedded, so point it only at a machine you trust. |
| `pin_timeout_minutes` | `5` | How long a PIN unlock lasts. |
| `brain_vault_path` | `~/.rusty/brain` | The vault's folder. |
| `notes_path` | `<vault>/notes` | The notes folder. Outside the vault, notes are not pages. |
| `skills_path` | `~/.rusty/skills` | The skills store. `RUSTY_SKILLS` overrides it. |
| `skills_enabled` | on | `false`, `0`, `no` or `off` stops the seed skills from being installed into a new store. |

The three path settings take an absolute path, or one starting with `~/`, and never the
store's home (`~/.rusty`) or a folder that contains it, since the note and page tools would
then reach the secrets. They are read when a program starts: restart the service
(`systemctl --user restart rusty-mcp`) and the agents' servers after changing one. The
provider is resolved at most once a minute, so a provider change takes effect within a
minute. `skills_seeded` is set by Rusty once the seeds are installed; deleting a seed skill
does not bring it back.

## Semantic search

Full-text search always works. Vectors are added when a provider is set:

1. Install [Ollama](https://ollama.com) and pull the model: `ollama pull nomic-embed-text`.
2. Leave `embedding_provider` at `auto`, or set it to `ollama`. `auto` checks only that
   Ollama answers, not that the model is there.
3. Embed what is already in the vault: `rusty-cli brain embed --all`. After that the
   server embeds pages as they change.
4. Check: `rusty-cli brain semantic` shows the provider and how many pages have vectors.

With `openai`, page text is sent to OpenAI to be embedded, and the key comes from the
secrets file (`openai_api_key`, or `OPENAI_API_KEY`). Nothing is sent to OpenAI under
`auto`.

## Secrets

`~/.rusty/.secret` holds `KEY='value'` lines (a shell can `source` it); a value is one
line, so a multi-line key goes in base64. Rusty writes the file whole and at mode 0600
every time. The
`secret_list`, `secret_set`, `secret_delete`, `secret_reveal` and `secret_update` tools
read and write it; `secret_list` returns names only. `secret_reveal` and `secret_update`
always need the token `secret_unlock` returns, so they work only once a PIN is set
(`secret_pin_set`); from then on `secret_set` and `secret_delete` need it too. A token is
good for `pin_timeout_minutes` on the server process that issued it, and five wrong PINs
in a row lock unlocking for a minute, in every server process on the store. The PIN guards the tools, not the file: anything running as your user can
read `.secret`. You can also edit the file by hand; the next read sees the change.

## Environment variables

| Variable | Read by | What it does |
|---|---|---|
| `HOME` | all | The store is `$HOME/.rusty`. |
| `RUSTY_SKILLS` | all | The skills store, over the `skills_path` setting. |
| `RUSTY_MCP_ADDR` | `rusty session status` | The address it probes and reports, an address or a name (default `127.0.0.1:4174`). |
| `VISUAL`, `EDITOR` | `rusty-cli scripts edit` | The editor it opens. |

## The service

`install.sh` installs `~/.config/systemd/user/rusty-mcp.service`, which runs
`rusty-mcp --http 127.0.0.1:4174` for clients that speak Streamable HTTP. It starts with
your user session (or at boot with lingering enabled), restarts two seconds after any exit
but a stop, and logs to the journal:

```bash
rusty session status
journalctl --user -u rusty-mcp -e
systemctl --user restart rusty-mcp
```

To use another port, override `ExecStart` in a drop-in, which a reinstall leaves alone,
and keep the address on loopback (the server answers only `localhost`, `127.0.0.1` and
`::1`):

```bash
systemctl --user edit rusty-mcp
#   [Service]
#   ExecStart=
#   ExecStart=%h/.local/bin/rusty-mcp --http 127.0.0.1:4180
systemctl --user restart rusty-mcp
```

Set `RUSTY_MCP_ADDR=127.0.0.1:4180` for `rusty session status`; the installer's own check
still expects 4174. Agents do not need the service: each one starts its own `rusty-mcp`
over stdio, and every copy works on the same store.
