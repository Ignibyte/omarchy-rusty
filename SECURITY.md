# Security

## Reporting a vulnerability

Report it privately through GitHub: the repository's **Security** tab, **Report a
vulnerability**. Please do not open a public issue for it. Expect an answer within a week;
a fix ships in the next release, and the advisory is published with it.

Only the latest release gets fixes.

## What Rusty trusts, and what it does not

Rusty is a single-user, local service. These are the boundaries it is built on, so a report
that one of them fails is welcome:

- **The HTTP endpoint has no authentication.** `rusty-mcp.service` listens on
  `127.0.0.1:4174` only, and any process on the machine can call it, another local user's
  included. Rusty is not meant for machines shared with people you do not trust; on one,
  run the server over stdio only and do not install the service.
- **Secrets are a plain text file** (`~/.rusty/.secret`, mode 600). The PIN protects
  reading and changing them through the tools: a value comes back only with a live unlock
  token, and settings whose key names a key, token, secret or password read back masked.
  The PIN does not encrypt the file; anything running as your user can read it.
- **Captured sources are untrusted.** A page fetched with `source_capture` is marked as
  untrusted data in every answer that carries it, so an agent does not take its text as
  instructions.
- **Skills are staged.** A skill an agent writes waits for approval, and a safety scan
  reads it, its scripts included, before it can run.
- **Nothing leaves the machine** unless you set an embedding provider that sends text
  (OpenAI) or ask Rusty to fetch a URL. The vault and the skills store commit to local git
  repositories and push nowhere.
- **Exports** carry the secrets only with `--include-secrets`, and the zip is readable by
  its owner only; it is not encrypted.
