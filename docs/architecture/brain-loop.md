# The brain loop: Ask, Decide, Follow up

*Decided on 2026-09-03. The aim: an agent always consults the brain before a decision and
keeps it current after, a workflow like plan, code, test, document, but for decisions and
on the MCP layer: Ask, Decide, Follow up.*

## The shape

- **Ask.** `brain_ask(question)` runs the hybrid search (text alone without a provider),
  lists the decisions whose text matches with their status and dates, lists the follow-ups
  due, and records a consultation (`brain_consultations`: id, question, hits, created,
  outcome). The id is the receipt the next step refers to.
- **Decide.** `brain_decide(consultation, title, choice, rationale, alternatives,
  follow_up_by, supersedes)` writes `decisions/<slug>`: frontmatter `question`, `status:
  decided`, `decided`, `follow_up_by`, `consulted` (the hits), `consultation`,
  `supersedes`; a body of Question, Choice, Rationale, Alternatives and Consulted (a
  wikilink per hit, so backlinks and the plain graph see them too). Every consulted page
  gets a timeline entry; a superseded decision gets `status: superseded`,
  `superseded_by` and an entry; the consultation's outcome is the slug.
- **Follow up.** `brain_follow_up(slug, outcome, status, successor, follow_up_by)` appends a
  dated Follow-up section, sets the status (`kept` clears the date, `revised` takes a new
  one, `superseded` needs the successor), and adds a timeline entry. `brain_due(days)`
  lists what is due; the Decisions view and `/brief` show it. A follow-up also records
  its day as `followed_up`, and every summary carries it and `superseded_by`.
- **No decision.** `brain_no_decision(consultation, reason)` marks the outcome. It is the
  honest exit the Stop hook accepts.
- **Dates.** Every date the loop writes (`decided`, the Follow-up heading, timeline
  entries) and every comparison it makes is the machine's local day from
  `frontmatter::today_iso`. A decision is overdue when `follow_up_by` is
  before today, so one due today is not late.

## The decisions

1. **The hooks ship in this repo**, under `crates/rusty-cli/hooks/`, embedded in the
   binary; `rusty-cli hooks install` writes them to `~/.rusty/hooks/` and wires
   `~/.claude/settings.json` idempotently, keeping every other entry. Anyone who installs
   `rusty-cli` gets the loop with the binary.
2. **Receipts by transcript scan.** The server never sees a session id, so the hooks read
   the transcript: a `mcp__rusty__brain_ask` tool use whose result was not an error lets
   every later write through; a `mcp__rusty__brain_decide` or `mcp__rusty__brain_no_decision`
   after any write lets the stop through. Scoped to a working directory whose `.mcp.json`
   names a rusty server. No jq, no transcript, an unreadable one: fail open.
3. **The Stop rule refuses once.** `stop_hook_active` marks the second attempt, which
   passes. The honest way out is `brain_no_decision` with the reason.
4. **One decision page per question.** A topic's history is the decisions linked to its
   page, each of which left a timeline entry there.
5. **Only `brain_ask` counts as consultation**, since it is the call that records the
   receipt; `brain_search` and the reads do not.
6. **Typed edges come from the page.** `consulted`, `supersedes` and `superseded_by` in the
   frontmatter become `brain_graph` edges of kind `consulted`, `supersedes` and
   `follows_up` at graph time; the vault stays the truth and the index rebuildable.

## Limits, on purpose

- A write through a shell command is not a Write tool use; the hooks do not see it, and
  neither hook pretends to. Mining decisions out of archived transcripts is a later ticket.
- Sessions not wired to Rusty are untouched.
- Nothing leaves the machine: the loop is the vault, the database and the transcript on
  disk.
