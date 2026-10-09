@AGENTS.md

## Claude Code

`AGENTS.md`, imported above, is the guide; this part covers what only Claude Code does.

The hooks in `.claude/settings.json`:

- **Phase gate** (Edit, Write, MultiEdit): with the record checked out, a gated path changes
  only while the active spec is at Phase 3 or later, or under a waiver. Without the record
  it allows every edit.
- **Secrets** (Edit, Write, MultiEdit): refuses a write whose content looks like a
  credential.
- **Commit gate** (Bash): refuses `git commit` of gated files without a gate receipt that
  matches the worktree, a commit that delivers a completed pipeline without a matching
  OpenWiki receipt, and `--no-verify`.
- **OpenWiki receipt** (after `openwiki_finish`): writes `.git/rusty-openwiki-receipt` when
  the run returned `complete`.

The project skills in `.claude/skills/` (`rusty-workflow`, `openwiki`) load on their own.

<!-- OPENWIKI:START -->

## OpenWiki

See [AGENTS.md](AGENTS.md) for OpenWiki agent instructions.

<!-- OPENWIKI:END -->
