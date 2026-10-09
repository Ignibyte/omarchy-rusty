@AGENTS.md

## Claude Code

`AGENTS.md`, imported above, is the guide; this part covers what only Claude Code does.

The hooks in `.claude/settings.json`:

- **Phase gate** (Edit, Write, MultiEdit): with the record checked out, a gated path changes
  only while the active spec is at Phase 3 or later, or under a waiver. Without the record
  it allows every edit.
- **Secrets** (Edit, Write, MultiEdit): refuses a write whose content looks like a
  credential.
- **Commit gate** (Bash): refuses `git commit` (in any form: `-a`, a pathspec,
  `git add … && git commit`, `git -c … commit`) while gated files differ from `HEAD` and
  no gate receipt matches the worktree; a staged gated file that differs from the
  worktree, or an untracked one left out; a commit that delivers a completed pipeline
  without a matching OpenWiki receipt; `--no-verify` and `-n`. It needs `jq`, and refuses
  without it.
- **OpenWiki receipt** (after `openwiki_finish`): writes `.git/rusty-openwiki-receipt` when
  the run returned `complete` and `openwiki/.last-update.json` shows that run at `HEAD`.

The project skills in `.claude/skills/` (`rusty-workflow`, `openwiki`) load on their own.

<!-- OPENWIKI:START -->

## OpenWiki

See [AGENTS.md](AGENTS.md) for OpenWiki agent instructions.

<!-- OPENWIKI:END -->
