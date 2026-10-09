# Contributing

Thanks for looking. Rusty is small and opinionated; a short issue before a large change
saves both of us time.

## Proposing a change

1. Open an issue that says what you want to change and why, or pick an open one.
2. Fork, branch, and make the change with its tests.
3. Run the gate and make it green:

   ```bash
   bin/gate.sh --diff    # fmt, clippy (warnings as errors), tests, docs, shell, secrets, whitespace
   ```

   Run cargo commands one at a time; two at once, or a killed one, corrupt the incremental
   cache and force a full rebuild.
4. Open a pull request. CI runs the same gate.

## What a change should look like

- Idiomatic Rust: `Result` for errors, no panics on a user's path, no `unsafe` without a
  safety comment, doc comments on public items. No `#[allow]` without the reason on the
  line above.
- The managers in `rusty-core` own the logic; a tool in `rusty-mcp` is a thin wrapper and
  every write goes through `mutate()`. A new or renamed tool is a change to the tool
  contract; say so in the pull request and in `CHANGELOG.md`.
- The vault stays a folder of markdown any tool can open; SQLite holds what can be rebuilt
  from it, plus the data that has no file. A schema change is additive and migrated in
  `engine/db.rs`.
- Tests never touch a real store: use a scratch directory or `HOME`.
- Prose (docs, comments, messages, strings) is plain and specific. Say what a thing does;
  skip the announcements and the filler.

## The maintainers' workflow

`CONSTITUTION.md`, `AGENTS.md` and `.claude/` describe how the maintainers and their agents
work: a phase-gated pipeline with a private work record at `docs/planning/`. A
contributor's clone has no record; the hooks and the checks step aside, and the gate and CI
are what apply. `openwiki/` is the generated engineering wiki; you do not need to update it
in a pull request.

## Releasing (maintainers)

1. Set the version in `Cargo.toml`, and in `CHANGELOG.md` turn `unreleased` into the date.
2. Run `bin/gate.sh --diff`, then `bin/package-release.sh` and install the tarball
   somewhere disposable.
3. Tag `v<version>` and push the tag. The release workflow builds the tarball and attaches
   it, with its checksum, to a GitHub release whose notes are the changelog's section.
4. Update `packaging/rusty-bin/PKGBUILD` (`pkgver`, then `updpkgsums`) for the AUR.
