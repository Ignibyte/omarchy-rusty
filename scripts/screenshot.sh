#!/usr/bin/env bash
# Screenshots of the workspace against a scratch vault: no real data, no workspace
# switch. A scratch rusty-mcp serves invented pages on its own port; the app renders
# offscreen and grabs itself (RUSTY_SHOT), once per scene.
#
#   scripts/screenshot.sh <out-dir> [scene ...]
#   SHOT_THEME=<theme dir> SHOT_SIZE=WxH SHOT_ENV="VAR=value ..." override the defaults.
#
# Scenes are RUSTY_SHOT_SCENE values (see qml/Main.qml); the default set covers reading,
# editing, the switcher, the palette, the agent pane and the search pane. The binaries
# come from the workspace target dir (build first: cargo build -p rusty-app -p rusty-mcp).
set -Eeuo pipefail
out=${1:?out dir}
shift
theme=${SHOT_THEME:-$HOME/.config/omarchy/current/theme}
size=${SHOT_SIZE:-1500x950}
scenes=("$@")
[[ ${#scenes[@]} -gt 0 ]] || scenes=("reading" "edit" "switcher" "palette" "right:agent" "left:search,right:outline" "right:tags" "graph" "localgraph" "left:bookmarks" "search:tag:theme path:concepts" "view:settings" "theme:omarchy" "theme:file:ember")
root=$(git rev-parse --show-toplevel)
target=${CARGO_TARGET_DIR:-$root/target}
[[ -x "$target/debug/rusty" && -x "$target/debug/rusty-mcp" ]] || { echo "build rusty and rusty-mcp first" >&2; exit 1; }
mkdir -p "$out"
scratch=$(mktemp -d "${TMPDIR:-/tmp}/rusty-shot.XXXXXX")
cleanup() {
  [[ -n "${server:-}" ]] && kill "$server" 2>/dev/null || true
  for s in $(tmux list-sessions -F '#{session_name}' 2>/dev/null | grep -E '^rusty-(shot|pane)-' || true); do tmux kill-session -t "$s" 2>/dev/null || true; done
  # A scene that talks to the agent pane starts a session host, which the stand-in
  # `systemd-run` detaches on purpose: it must outlive the app the way a real unit does,
  # so nothing else ends it. Stop each one the way anybody would.
  for entry in "$scratch"/agents/*.json; do
    [[ -e "$entry" ]] || continue
    id=$(basename "$entry" .json)
    RUSTY_AGENT_STATE_DIR="$scratch/agents" RUSTY_AGENT_RUN_DIR="$scratch/run/agents" \
      "$target/debug/rusty" agent stop "$id" >/dev/null 2>&1 || true
  done
  if [[ -n ${SHOT_KEEP:-} ]]; then echo "scratch kept at $scratch"; else rm -rf "$scratch"; fi
}
trap cleanup EXIT
port=$(( 4300 + RANDOM % 500 ))

vault="$scratch/.rusty/brain"
mkdir -p "$vault/projects" "$vault/people" "$vault/concepts" "$vault/daily" "$vault/meetings" "$vault/ideas/archive" "$scratch/run" "$scratch/.config/Ignibyte"
cat > "$vault/projects/orbit.md" <<'MD'
---
title: Orbit
type: project
tags:
  - launcher
  - rust
status: active
created: 2026-08-14
updated: 2026-09-02
---

Orbit is a keyboard-first launcher for the desk: one key, a line of text, and the
thing you meant opens. It leans on [[concepts/compiled-truth|compiled truth]] for what it
knows about you and asks [[people/sarah-chen]] before it guesses.

> [!tip] Design directive
> Keep the graph, the files and the thinking surface visible at once. The agent is a
> pane in the workspace, not a chatbot bolted onto the side.

## North stars

- [x] Local-first feel: no glossy cloud dashboard chrome.
- [ ] Every surface answers: what can I do with the keyboard?
- [ ] Make links and context ==legible== before adding decoration.
- [ ] Let the agent summarise the current note without stealing focus. #design/next

## Layout contract

| Pane | Owner | Notes |
|---|---|---|
| vault | explorer | folders first |
| note | editor | reading or source |
| agent | terminal | tmux-backed |

```toml
workspace = "01_projects"
layout    = "dwindle"
panes     = ["vault", "note", "agent"]
```

A footnote on naming[^1] and a link to a page that is not written yet: [[ideas/orbit-mobile]].

[^1]: Orbit was almost called Halo.

## Timeline

- **2026-08-14** (kickoff) — Named the project and wrote the first north stars.
- **2026-09-02** (review) — Moved the layout contract into the page.
MD
cat > "$vault/people/sarah-chen.md" <<'MD'
---
title: Sarah Chen
type: person
role: CTO
company: Halo Labs
tags:
  - person/engineering
---

Sarah runs engineering at Halo Labs and reviews [[projects/orbit]] every other week. #follow-up

## Key context

- Prefers written proposals over meetings.
- Asked for a `--dry-run` on every migration.
MD
cat > "$vault/concepts/compiled-truth.md" <<'MD'
---
title: Compiled truth
type: concept
---

What a page states as settled, above its timeline. [[projects/orbit]] keeps one; so
does every person page.

> Evidence goes in the timeline; the compiled truth is what you would tell a newcomer.
MD
mkdir -p "$vault/decisions"
cat > "$vault/decisions/keep-sqlite.md" <<'MD'
---
title: Keep SQLite as the index
type: decision
question: Should the index move off SQLite?
status: decided
decided: 2026-09-01
follow_up_by: 2026-09-02
consulted:
  - projects/orbit
  - concepts/compiled-truth
---

## Question

Should the index move off SQLite?

## Choice

Keep SQLite; the vault stays the truth and the index is rebuilt from it.

## Rationale

Nothing outgrew it, and a rebuild is one command.

## Alternatives

- Postgres

## Consulted

- [[projects/orbit]]
- [[concepts/compiled-truth]]
MD
cat > "$vault/daily/2026-09-02.md" <<'MD'
---
title: 2026-09-02
type: daily
---

## Notes
- Read [[concepts/compiled-truth]] again.

## Tasks
- [ ] Reply to [[people/sarah-chen]]
MD
cat > "$vault/ideas/archive/old-idea.md" <<'MD'
---
title: Old idea
type: idea
---

Kept for the record, next to [[projects/orbit]].
MD
for n in 1 2 3 4; do
cat > "$vault/concepts/theme-$n.md" <<MD
---
title: Theme $n
type: concept
tags:
  - theme
---

One of the themes behind [[projects/orbit]]; see [[concepts/compiled-truth]] and [[concepts/theme-$(( n % 4 + 1 ))]].
MD
done
cat > "$vault/meetings/kickoff.md" <<'MD'
---
title: Kickoff
type: meeting
---

With [[people/sarah-chen]] about [[projects/orbit]]; decided on [[concepts/theme-1]].
MD
printf 'Loose note without frontmatter, as Obsidian writes them.\n\n- #inbox item\n' > "$vault/Loose note.md"
printf '{}' > "$vault/projects/data.json"
mkdir -p "$vault/sources"
cat > "$vault/sources/example-com-launchers.md" <<'MD'
---
title: A list for launchers
type: source
url: https://example.com/reading-list
site: example.com
captured: 2026-09-05T09:30:00+00:00
kind: html
---

## Why launchers

A launcher is one key, a line of text, and the thing you meant. The best ones stay out of
the way.

- Keep the index warm.
- Answer in under a frame.
MD

IFS=x read -r w h <<< "$size"
printf '[window]\nwidth=%s\nheight=%s\nlastTab=0\n' "$w" "$h" > "$scratch/.config/Ignibyte/rusty.conf"
store="$scratch/.rusty/skills/.claude/skills/dev-box-usb"
mkdir -p "$store"
cat > "$store/SKILL.md" <<'MD'
---
name: dev-box-usb
description: Reset the USB controller when the KVM's hub stops answering.
---

Run `rusty usb-reset`; the script is `usb-reset.sh` beside this file.
MD
cat > "$store/usb-reset.sh" <<'SH'
#!/usr/bin/env bash
# usb-reset: run with `rusty usb-reset`.
set -euo pipefail
echo "rebinding the USB controller (a stand-in for the screenshot)"
SH
chmod 755 "$store/usb-reset.sh"
cat > "$scratch/workspace.json" <<'JSON'
{"leftWidth":280,"rightWidth":320,"leftOpen":true,"rightOpen":true,"leftPane":"files","rightPane":"backlinks","expanded":"{\"projects\":true,\"people\":true}","paneProgram":"shell","bookmarks":"[{\"kind\":\"file\",\"path\":\"projects/orbit\",\"title\":\"Orbit\"},{\"kind\":\"folder\",\"path\":\"concepts\",\"title\":\"concepts\"},{\"kind\":\"search\",\"query\":\"tag:theme path:concepts\",\"title\":\"Themes\"},{\"kind\":\"heading\",\"path\":\"projects/orbit\",\"heading\":\"Timeline\",\"title\":\"Orbit › Timeline\"}]"}
JSON
mkdir -p "$scratch/.config/rusty/themes"
cat > "$scratch/.config/rusty/themes/ember.toml" <<'TOML'
[colors]
bg = "#12080a"
text = "#e8c9b8"
accent = "#ff5c39"
gold = "#ffb36b"
alive = "#7ee0c8"
[type]
radius = 3
TOML
cat > "$scratch/tabs.json" <<'JSON'
[{"kind":"page","title":"orbit","slug":"projects/orbit","session":"","program":"","cwd":"","pinned":false},
 {"kind":"page","title":"sarah-chen","slug":"people/sarah-chen","session":"","program":"","cwd":"","pinned":true},
 {"kind":"terminal","title":"Shell","slug":"","session":"rusty-shot-shell","program":"shell","cwd":"","pinned":false},
 {"kind":"tasks","title":"Tasks","slug":"","session":"","program":"","cwd":"","pinned":false}]
JSON

# A small repository for the folder scenes (`root:repo,expand:repo/src`): a committed
# tree, a modified file, a staged new file, an untracked file; nothing of the box's git
# configuration reaches it.
if command -v git >/dev/null 2>&1; then
  repo="$scratch/repo"
  mkdir -p "$repo/src"
  g() { HOME="$scratch" GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 git -C "$repo" -c user.name=shot -c user.email=shot@example.invalid -c commit.gpgsign=false "$@" >/dev/null 2>&1; }
  printf '# Orbit\n' > "$repo/README.md"
  printf 'pub fn launch() {}\n' > "$repo/src/lib.rs"
  if g init -q -b main && g add . && g commit -q -m "first"; then
    printf 'pub fn launch() { println!("go") }\n' > "$repo/src/lib.rs"
    printf 'pub fn dock() {}\n' > "$repo/src/dock.rs"
    g add src/dock.rs || true
    printf 'loose\n' > "$repo/notes.txt"
  else
    echo "scratch repository not seeded; the folder scenes show a plain folder" >&2
  fi
fi

# A small Obsidian vault for the import scene (`import:obsidian-vault`): two notes with
# bare-name links, an attachment, a bookmarks file, and the folders the walk skips.
ov="$scratch/obsidian-vault"
mkdir -p "$ov/sub" "$ov/assets" "$ov/.obsidian" "$ov/.trash"
printf '# Reading list\n\nStart with [[Orbit notes]] and [[A page that is not here]].\n\n#reading\n' > "$ov/Reading list.md"
printf -- '---\ntags:\n  - launcher\n---\n\nNotes on [[Reading list]].\n\n## Ideas\n\nOne key.\n' > "$ov/sub/Orbit notes.md"
printf 'not really a png\n' > "$ov/assets/sketch.png"
printf '{"items":[{"type":"file","path":"Reading list.md","title":"Reading"},{"type":"group","title":"g","items":[{"type":"folder","path":"sub"},{"type":"search","query":"tag:launcher","title":"launcher"},{"type":"file","path":"sub/Orbit notes.md","subpath":"#Ideas"}]}]}' > "$ov/.obsidian/bookmarks.json"
printf '{}' > "$ov/.obsidian/app.json"
printf 'gone\n' > "$ov/.trash/old.md"

# A stand-in for `claude -p` over stream-json for the agent pane scenes
# (`right:agent,agent:ask:<text>`): one canned turn per message — a tool call, its
# result, streamed text, a permission prompt, a result — and nothing leaves the machine.
mkdir -p "$scratch/bin"
cat > "$scratch/bin/claude" <<'FAKE'
#!/usr/bin/env bash
sid="scene-session-0001"
echo "{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"$sid\",\"tools\":[]}"
while IFS= read -r line; do
  case "$line" in *control_response*|*control_request*) continue ;; esac
  echo "{\"type\":\"stream_event\",\"event\":{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"mcp__rusty__brain_read_page\",\"input\":{}}}}"
  echo "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"mcp__rusty__brain_read_page\",\"input\":{\"slug\":\"projects/orbit\"}}]}}"
  echo "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_1\",\"content\":\"# Orbit\\n\\nOrbit is a keyboard-first launcher for the desk.\",\"is_error\":false}]}}"
  echo "{\"type\":\"stream_event\",\"event\":{\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}}"
  echo "{\"type\":\"stream_event\",\"event\":{\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"Orbit is a keyboard-first launcher: one key, a line of text, and the thing you meant opens. \"}}}"
  echo "{\"type\":\"stream_event\",\"event\":{\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"Its north stars are a local-first feel and a keyboard answer on every surface.\"}}}"
  # The whole block once it is complete, as Claude Code sends it: the deltas are live
  # only, and this is what a replayed conversation is rendered from.
  echo "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Orbit is a keyboard-first launcher: one key, a line of text, and the thing you meant opens. Its north stars are a local-first feel and a keyboard answer on every surface.\"}]}}"
  echo "{\"type\":\"control_request\",\"request_id\":\"req-1\",\"request\":{\"subtype\":\"can_use_tool\",\"tool_name\":\"mcp__rusty__brain_add_timeline\",\"input\":{\"slug\":\"projects/orbit\",\"summary\":\"Summarised for Chad\"},\"description\":\"Add a timeline entry to projects/orbit\"}}"
  echo "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"session_id\":\"$sid\",\"num_turns\":2,\"total_cost_usd\":0.0123,\"result\":\"Orbit is a keyboard-first launcher.\"}"
done
FAKE
chmod 755 "$scratch/bin/claude"

# A stand-in for `systemd-run --user` (TICKET-031): the scenes must never touch Chad's
# user manager, so the session host runs as a plain background child here. It logs its
# arguments beside itself, then runs whatever follows `--`.
cat > "$scratch/bin/systemd-run" <<'STANDIN'
#!/usr/bin/env bash
printf '%s\n' "$@" >> "$(dirname "$0")/systemd-run.log"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --) shift; break ;;
    *) shift ;;
  esac
done
[[ $# -eq 0 ]] && exit 1
setsid "$@" >>"$(dirname "$0")/../agent-host.log" 2>&1 &
exit 0
STANDIN
chmod 755 "$scratch/bin/systemd-run"
printf '#!/usr/bin/env bash\nexit 0\n' > "$scratch/bin/true"
chmod 755 "$scratch/bin/true"

HOME="$scratch" XDG_CONFIG_HOME="$scratch/.config" XDG_RUNTIME_DIR="$scratch/run" \
  "$target/debug/rusty-mcp" --http "127.0.0.1:$port" >"$scratch/server.log" 2>&1 &
server=$!
for _ in $(seq 1 50); do
  curl -s -o /dev/null "http://127.0.0.1:$port/mcp" && break
  sleep 0.1
done
sleep 1.5

for scene in "${scenes[@]}"; do
  cp "$scratch/workspace.json" "$scratch/workspace.json.orig" 2>/dev/null || true
  name=$(echo "$scene" | tr ':,/' '---')
  file="$out/$name.png"
  env ${SHOT_ENV:-} HOME="$scratch" XDG_CONFIG_HOME="$scratch/.config" XDG_RUNTIME_DIR="$scratch/run" \
    QT_QPA_PLATFORM="${SHOT_PLATFORM:-offscreen}" QT_FORCE_STDERR_LOGGING=1 \
    RUSTY_MCP_URL="http://127.0.0.1:$port/mcp" RUSTY_TABS="$scratch/tabs.json" RUSTY_STATE="$scratch/workspace.json" \
    RUSTY_OMARCHY_THEME_DIR="$theme" RUSTY_SHOT="$file" RUSTY_SHOT_DELAY="${RUSTY_SHOT_DELAY:-3500}" RUSTY_CLAUDE_BIN="$scratch/bin/claude" \
    RUSTY_SYSTEMD_RUN="$scratch/bin/systemd-run" RUSTY_AGENT_STATE_DIR="$scratch/agents" RUSTY_AGENT_RUN_DIR="$scratch/run/agents" RUSTY_NOTIFY_SEND="$scratch/bin/true" \
    RUSTY_SHOT_SCENE="$([[ "$scene" == reading ]] || printf '%s' "$scene")" RUSTY_DEBUG=1 \
    timeout 40 "$target/debug/rusty" >"$out/$name.log" 2>&1 || echo "scene $scene exited $?" >&2
  [[ -s "$file" ]] && echo "wrote $file" || echo "no image for $scene (see $out/$name.log, or journalctl -t rusty)" >&2
done
