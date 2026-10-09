# Security

## Reporting a vulnerability

Report it privately through GitHub: the repository's **Security** tab, **Report a
vulnerability**. Please do not open a public issue for it. Expect an answer within a week;
a fix ships in the next release, and the advisory is published with it.

Only the latest release gets fixes.

## What Rusty trusts, and what it does not

Rusty is a single-user, local service. It trusts every process that runs as you and every
client that can reach its endpoint. These are the boundaries it is built on, so a report
that one of them fails is welcome:

- **The HTTP endpoint has no authentication.** `rusty-mcp.service` listens on
  `127.0.0.1:4174` only, and any process on the machine can call it, another local user's
  included. The server accepts only a localhost `Host` header, which stops a web page from
  reaching it through DNS rebinding. Rusty is not meant for machines shared with people
  you do not trust; on one, do not install the service and let agents start `rusty-mcp`
  over stdio.
- **A client can run code as you.** Any client of the server can write a skill, write a
  script into an approved skill (`script_update`), and run an approved script
  (`script_run`). A skill created without `pending: true` is active at once, and its
  scripts can run. The safety scan reads a staged skill's whole folder, its scripts
  included, when it is approved (`force` skips it), and any skill on demand
  (`skill_scan`); it flags known risky patterns and is not a sandbox. `script_run` stops a
  script and everything it started at its time limit.
- **Secrets are a plain text file** (`~/.rusty/.secret`, mode 600). The PIN guards the
  tools that reveal or change a value: `secret_reveal` and `secret_update` need a live
  unlock token, and once a PIN is set `secret_set` and `secret_delete` do too. Settings
  whose key names a key, token, secret or password read back masked. Five wrong PINs lock
  unlocking for a minute in every server process on the store. The PIN does not encrypt
  the file; anything running as you can read it. The note and page tools cannot reach it: a
  note path cannot name a dot-file, and the path settings refuse the store's home.
- **Captured sources are untrusted.** A page fetched with `source_capture` is marked as
  untrusted data in every MCP answer that carries its text (search, reads, renders,
  `brain_ask`, link context, the page resource) and in `rusty-cli brain context`, so an
  agent does not take it as instructions. Its HTML is shown as text and its links keep only
  safe schemes. A capture reads at most 8 MiB after decompression.
- **What leaves the machine.** Page text goes to the embedding provider: Ollama at
  `ollama_url` (this machine unless you change it) or OpenAI when you choose `openai`.
  `source_capture` fetches the URL it is given. The vault and the skills store commit to
  local git repositories and push nowhere.
- **Exports** carry the secrets only with `--include-secrets`. The zip is readable by its
  owner only and is not encrypted.
