---
type: "Reference"
title: "Development and validation"
openwiki_generated: true
sources:
  - id: openwiki-source-164e2da859b5277df81c7d94
    resource: repo://.github/workflows/ci.yml
  - id: openwiki-source-4d1d392666be6dfdd7a91a2e
    resource: repo://.github/workflows/release.yml
  - id: openwiki-source-0bb8016edf4f4744d3a09cf4
    resource: repo://bin/gate.sh
  - id: openwiki-source-bf866c9fdec1e356b0b26036
    resource: repo://bin/package-release.sh
  - id: openwiki-source-1de5221fd140fd89f39f87cd
    resource: repo://crates/rusty-cli/src/main.rs
  - id: openwiki-source-052c1f89ee9ee9321b802fb2
    resource: repo://crates/rusty-cmd/src/session.rs
  - id: openwiki-source-a8160e123db68363371d6a65
    resource: repo://crates/rusty-cmd/tests/command.rs
  - id: openwiki-source-637dadc84a3e86cb046587f2
    resource: repo://crates/rusty-core/src/skills/mod.rs
  - id: openwiki-source-8dd50c7304f494c3114c8599
    resource: repo://crates/rusty-core/tests/docs.rs
  - id: openwiki-source-4d8ab597958b0e5c2507d7fd
    resource: repo://omarchy/install.sh
  - id: openwiki-source-40bfddd6b1c627968cf41f77
    resource: repo://omarchy/wayland-wm-oom.conf
  - id: openwiki-source-74bdf832aa1ee5e3f40cd980
    resource: repo://packaging/PKGBUILD
  - id: openwiki-source-0e798ceb3f0435c95b002613
    resource: repo://packaging/rusty-bin/PKGBUILD
  - id: openwiki-source-484d3bfc1c9bd344c2b3a5f9
    resource: repo://packaging/rusty-bin/rusty-bin.install
generated: {by: "claude-code", at: "2026-10-09T18:25:07.825Z"}
verified:
  - by: openwiki/0.3.3
    at: 2026-10-09T18:25:07.825Z
---

# Development and validation

## Purpose

The commands that build, test and prove a change, the narrowest one for each kind of
change, and the ways the machine's state is kept out of the record.

## Building

- A Cargo workspace of four crates (`rusty-core`, `rusty-mcp`, `rusty-cli`, `rusty-cmd`).
  `cargo build` builds all; a stable toolchain and a C compiler are enough.
- One cargo command at a time, never killed: a killed or concurrent cargo corrupts the
  incremental cache.
- `omarchy/install.sh` needs `systemctl`, `curl` and `git`. From a release tarball it
  copies the three binaries in `bin/` beside its folder into `~/.local/bin`; from a
  checkout it builds them with `cargo install --force --locked` (`--force` replaces a
  binary an older install left under a name). Then the back end's user unit (installed,
  enabled, restarted and probed over HTTP), the MCP config snippets, and pointers to the
  compositor drop-in and the earlyoom line it does not apply. It also deletes a leftover
  `~/.local/bin/rusty-session` if one exists. Idempotent.
- `packaging/PKGBUILD` (`rusty-git`) builds the same for the AUR, `!lto`, depends on the C
  and Rust runtimes and `git`, and installs `rusty-mcp`, `rusty-cli`, `rusty` and the back
  end's unit (its path rewritten to `/usr/bin`), with the drop-in and the MCP config under
  `/usr/share/rusty/`. Its `check()` runs the `rusty-cmd` tests too.
  `packaging/rusty-bin/PKGBUILD` installs a release tarball's binaries the same way and
  conflicts with `rusty-git`; after a release, set its `pkgver` and run `updpkgsums`.
  Both ship `THIRD_PARTY_NOTICES.md` beside the licence, and both packages' install
  scripts only print the `systemctl --user` commands: pacman runs as root and cannot reach
  a user's service manager.

## Releasing

- The version is the workspace's, in `Cargo.toml`; every crate inherits it.
- `bin/package-release.sh` builds the three binaries with `cargo build --release --locked`
  and writes `dist/rusty-<version>-<arch>-linux.tar.gz` (the binaries under `bin/`, the
  installer, the unit, the MCP config, the compositor drop-in, README, LICENSE, the
  third-party notices, CHANGELOG)
  and a `.sha256` beside it. A maintainer runs it to test exactly what a release ships.
- `.github/workflows/release.yml` runs on a `v*` tag: it refuses a tag that differs from
  `Cargo.toml`, runs the tests, runs the packaging script, and attaches the tarball and its
  checksum to a GitHub release whose notes are that version's section of `CHANGELOG.md`,
  with the `gh` CLI and the job's own token.
- The tarball's binaries are built on GitHub's Ubuntu runner and link only the C runtime
  (rustls, bundled SQLite), so they run on Arch and other current distributions.
- Proof that a release installs is a disposable Omarchy VM: unpack, `omarchy/install.sh`,
  then the service, the tools over stdio and HTTP, the CLI, and an export and import.
  The installer restarts the user unit, so it is never tried on a machine whose back end
  is in use.

## Running as a service

- `omarchy/rusty-mcp.service` is wanted by `default.target`, so the back end serves with
  or without a desktop. `Restart=always` brings it back two seconds after any exit but
  `systemctl --user stop`; a session teardown and earlyoom both send SIGTERM, which
  `on-failure` would treat as clean. `OOMScoreAdjust=100` is the lowest a user unit can
  set (the user manager's own score; its services default to 200).
- `rusty session` is the way in from a terminal: a noun of the `rusty` command, decided in
  `crates/rusty-cmd/src/session.rs`. `start` starts the back end's unit and prints the
  status; `status` reads the unit and posts an `initialize` to the port; any other verb
  prints the usage. A test reads every file under `omarchy/` and `packaging/` and refuses
  invocations of a `rusty-session` script and, outside a README, the names of a desktop
  app, so the shipped files describe only what ships.
- `omarchy/wayland-wm-oom.conf` is a drop-in for the compositor unit (`OOMScoreAdjust=100`)
  that the installer points at and never applies, being another program's unit; the
  earlyoom avoid line, which needs root, is documented in `omarchy/README.md`.

## Testing

- `cargo test -p rusty-core`: the managers, the vault, the renderer, the scanner, the
  semantic index; scratch directories under the system temp dir, an in-memory database.
- `cargo test -p rusty-mcp`: the router tests (every tool listed once, described and in
  a family; `docs/tools.md` current) and `tests/smoke.rs`, which spawns the built binary
  over stdio in a scratch `HOME` and walks the tool families.
- `cargo test -p rusty-cli`: the argument parser, the hooks against a corpus of
  transcripts, and the check that `docs/cli.md` shows every line of the usage text.
- `crates/rusty-core/tests/docs.rs`: `docs/configuration.md` has a row for every setting
  the code reads.
- `cargo test -p rusty-cmd`: the command line (`session::tests`), the shipped-files check
  above, and `tests/command.rs`, which runs the built `rusty` with a logging stand-in
  `systemctl` and the probe on a closed port.
- The seed skills a new store gets name only current commands and tools; a test in
  `rusty-core`'s skills module refuses raw SQLite, the tables and paths of an earlier
  version, and "GUI" in them.
- Probes never touch real data or the live user manager; stand-ins on `PATH` and scratch
  directories take their place.

## The gate and CI

- `bin/gate.sh --fast` while working; `bin/gate.sh --diff` before delivery, which
  writes the receipt the commit hooks check; `--verify` to confirm it.
- `.github/workflows/ci.yml` runs `bin/gate.sh --diff` on every push to `main` and every
  pull request, so CI reruns exactly the local checks; it never replaces the local
  receipt.

## Logs

- The back end logs to stderr (`journalctl --user -u rusty-mcp` for the service).

## Data on this machine

`~/.rusty/`: `rusty.db`, `brain/` (a git repository the managers commit to, with notes
in `brain/notes/`), `skills/`, `.secret`, `.pin`, `hooks/`; `docs/configuration.md` has
the full layout. Tests never use these paths: they run in a scratch `HOME`.

## Primary sources

- `bin/gate.sh`, `.github/workflows/ci.yml`, `omarchy/install.sh`, `packaging/PKGBUILD`
- `bin/package-release.sh`, `.github/workflows/release.yml`, `packaging/rusty-bin/PKGBUILD`,
  `CHANGELOG.md`
- `crates/rusty-cmd/src/session.rs`, `crates/rusty-cmd/src/main.rs`,
  `crates/rusty-cmd/tests/command.rs`, `omarchy/rusty-mcp.service`,
  `omarchy/wayland-wm-oom.conf`
- `crates/rusty-mcp/tests/smoke.rs`
