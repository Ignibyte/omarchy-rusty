---
type: "Reference"
title: "Export and import: a whole store in one zip"
openwiki_generated: true
sources:
  - id: openwiki-source-1de5221fd140fd89f39f87cd
    resource: repo://crates/rusty-cli/src/main.rs
  - id: openwiki-source-79e92c26a49d3b5ce7f4c00a
    resource: repo://crates/rusty-core/src/brain/vault.rs
  - id: openwiki-source-705d180fc941297b1e844397
    resource: repo://crates/rusty-core/src/core.rs
  - id: openwiki-source-637dadc84a3e86cb046587f2
    resource: repo://crates/rusty-core/src/skills/mod.rs
  - id: openwiki-source-5725b482ae3caf2b45126fc1
    resource: repo://crates/rusty-core/src/transfer.rs
generated: {by: "claude-code", at: "2026-10-09T17:39:34.627Z"}
verified:
  - by: openwiki/0.3.3
    at: 2026-10-09T17:42:25.313Z
---

# Export and import: a whole store in one zip

## Purpose

Move a Rusty store to another machine, or keep it as one file. `rusty-cli export` writes
everything the store holds into a zip; `rusty-cli import` unpacks it into another home and
points the store's path settings at where the parts landed.

## Ownership

- `crates/rusty-core/src/transfer.rs`: `StoreLocation` (where a store's parts live, the one
  resolver `Core` also uses), `Manifest`, `export`, `read_manifest`, `import`.
- `crates/rusty-cli/src/main.rs`: `run_export`, `run_import`, and `servers_holding`, the
  check for a running server.

## What the zip holds

| Entry | What | When |
|---|---|---|
| `manifest.json` | format `rusty-export`, version 1, the Rusty version, the time, where notes sat, counts, what was skipped | always |
| `db/rusty.db` | the database, as a `VACUUM INTO` snapshot | always |
| `brain/` | the vault, `.git` included | always |
| `skills/` | the skills store, `.git` included | always |
| `notes/` | the notes folder | only when it lies outside the vault |
| `secrets/.secret`, `secrets/.pin` | the secrets file and the PIN's hash | only with `--include-secrets` |

The manifest's page count is what the brain would index: the vault's markdown outside its
dot-folders (`.git`, `.templates`, `.obsidian`) and the root's `archive/`, decided by the
vault's own `hidden` rule, so the count matches `brain stats`. Templates and deleted pages
still travel; they are files, not pages.

The home's other contents do not travel: the Claude Code hook scripts are rewritten by
`rusty-cli hooks install` (and carried across by a replacing import), the change sentinel
is recreated, and anything else under the home is not the store.

## Runtime flow

1. Export: refuse a destination that exists or lies inside the store; snapshot the
   database beside the destination; write the zip as `<dest>.partial` (mode 0600), walking
   each folder in name order with empty folders and file modes kept, symbolic links, git
   lock files and non-UTF-8 names skipped and listed; write the manifest last; rename the
   zip into place; delete the snapshot.
2. Import: read and check the manifest; check every entry (no symbolic link, no escaping
   path, only the known folders and files) before anything is written; with `--dry-run`,
   report and stop; refuse a home that holds a store unless `--replace`; unpack into
   `<home>.importing-<time>`; rewrite the path settings in the staged database; move an
   existing home to `<home>.before-import-<time>` and copy its `hooks/` across; rename the
   staged store into place.
3. The CLI refuses an import while a `rusty-mcp` holds the target database open.

## Invariants

- Nothing is deleted: an import that replaces moves the old home aside whole.
- Secrets travel only on request; the zip, the imported secrets and the imported database
  are owner-only.
- Path settings never point at the old machine after an import.

## Failure modes

- A failure while unpacking removes the staging folder and leaves the home as it was.
- Merging two stores is not supported; an import replaces or refuses.
- The zip is not encrypted; `--include-secrets` puts the secrets file in it in the clear.

## Tests

- `cargo test -p rusty-core transfer::`: six tests (round trip, secrets, replace and
  refuse, unsafe zips, refused destinations, notes outside the vault).

## Primary sources

- `crates/rusty-core/src/transfer.rs`, `crates/rusty-cli/src/main.rs`, `README.md`
  (Moving to another machine)
