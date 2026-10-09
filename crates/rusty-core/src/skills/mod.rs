//! Agent Skills: the `SKILL.md` procedures Rusty keeps for agents. Rusty owns authoring,
//! staging, the safety scan and history; the agent loads and runs them.
//! [Agent Skills](https://code.claude.com/docs/en/skills) are files Claude Code discovers
//! under `.claude/skills/` in a directory it is given, so the store keeps its active skills
//! at exactly that path. Any agent can also read them through `skill_list` and
//! `skill_view`.
//!
//! ## On-disk layout
//!
//! ```text
//! ~/.rusty/skills/                     # the `skills_path` setting, or `RUSTY_SKILLS`
//! ├── .claude/skills/<name>/SKILL.md   # active skills: the path Claude Code discovers
//! ├── staging/<name>/SKILL.md          # pending skills, waiting for approval
//! └── .git/                            # history; every write commits
//! ```
//!
//! `claude --add-dir ~/.rusty/skills` hands Claude Code the store root and no more: the
//! database `~/.rusty/rusty.db` is a sibling, outside the grant, and staged skills are not
//! discovered. `--add-dir` grants read and write to that folder.
//!
//! The read API ([`SkillsManager::list`], [`SkillsManager::get`], [`Skill`],
//! [`parse_skill_md`], …) backs the CLI and the MCP tools; [`bootstrap`] makes the store
//! and deploys the seeds at startup.

use crate::engine::settings_manager::SettingsManager;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Scripts as commands: a `*.sh` beside a skill is `rusty <name>` (TICKET-010).
pub mod scripts;

/// Settings key: master switch for skills (`"true"` / `"false"`). Default enabled.
pub const SETTING_ENABLED: &str = "skills_enabled";
/// Settings key: filesystem root of the skills store. Default `~/.rusty/skills`.
pub const SETTING_PATH: &str = "skills_path";
/// Settings key (internal): set once seed skills have been deployed, so a seed the
/// user later deletes is not resurrected on the next launch.
const SETTING_SEEDED: &str = "skills_seeded";

/// Maximum size of a `SKILL.md` we will read — defense-in-depth against a runaway or
/// hostile file (the active dir is agent-writable). Real skills are a few KB.
const MAX_SKILL_MD_BYTES: u64 = 256 * 1024;

/// Bundled seed skills, deployed on first run if the active dir has no skills.
///
/// Each entry is `(directory_name, SKILL.md contents)`. Seeds must be safe to run
/// unattended: no pre-approved `allowed-tools`, no `` !`command` `` dynamic injection.
/// The brain loop for agents: Ask, Decide, Follow up (TICKET-018).
const SEED_ASK_DECIDE_FOLLOW_UP: &str = r#"---
name: ask-decide-follow-up
description: >-
  The brain loop: consult the brain before a decision (brain_ask), record the decision as
  a page linked to what it rested on (brain_decide), come back to say how it went
  (brain_follow_up). Use before any change of direction, design choice or tool pick.
---

Rusty's brain holds facts; this loop adds the reasoning. Two optional Claude Code hooks
(`rusty-cli hooks install`) hold a session to the first two steps when its directory has a
`.mcp.json` naming a `rusty` server: the first file write is blocked until the session has
called `brain_ask`, and a session that wrote files is refused its first attempt to stop
until it records a `brain_decide` or a `brain_no_decision`.

## Ask

Before you decide, call `brain_ask` with the question in plain words. It returns the
pages that touch it (text and vectors when a provider is set), the decisions already
taken on the topic with their status, the follow-ups due, and a consultation id. Read
what came back; a decision already taken and kept is the answer unless you have a reason
it no longer holds.

## Decide

Call `brain_decide` with the consultation id, a title, the choice, the rationale, the
alternatives you set aside, and a `follow_up_by` date when the outcome is worth checking
(a week for a tool pick, a month for a design). The page lands under `decisions/`, links
to every consulted page, and each of those pages gets a timeline entry. To replace an
earlier decision, pass its slug as `supersedes`.

When the consultation led to no decision, say so: `brain_no_decision` with the reason.
The Stop hook accepts that too.

## Follow up

When the date comes (`brain_due` lists what is due, and the `morning-brief` skill reads
it), call
`brain_follow_up` with the slug, the outcome and a status: `kept` (clears the date),
`revised` (a new `follow_up_by`), or `superseded` (with the successor's slug).

## From a terminal

`rusty-cli brain ask <question>`, `brain decide <id> --title T --choice C --rationale R
[--alt A]... [--follow-up-by DATE] [--supersedes SLUG]`, `brain follow-up <slug> --status
kept|revised|superseded --outcome O [--successor SLUG]`, `brain no-decision <id> <reason>`,
`brain due [--days N]`. `rusty-cli hooks install` wires the two hooks into
`~/.claude/settings.json`; `hooks status` shows them; `hooks uninstall` removes them.
"#;

const SEED_SKILLS: &[(&str, &str)] = &[
    ("ask-decide-follow-up", SEED_ASK_DECIDE_FOLLOW_UP),
    ("file-research-finding", SEED_FILE_RESEARCH_FINDING),
    ("morning-brief", SEED_MORNING_BRIEF),
    ("no-ai-slop", SEED_NO_AI_SLOP),
];

/// Seed: file a cited research finding into the brain vault.
///
/// `name` matches the directory (Claude Code's invocation identity is the slug dir
/// name); the seed models the slug convention auto-authored skills should follow.
const SEED_FILE_RESEARCH_FINDING: &str = r#"---
name: file-research-finding
description: >-
  File a cited research finding into the brain vault. Use when the user shares a sourced
  fact to remember, or asks to capture / file / remember some research. Keywords:
  research, source, cite, brain, capture.
---

Use this when a durable, sourced fact should land in the brain vault rather than only in chat.

## Procedure
1. Find or create the target page:
   - Search first: `rusty-cli brain search "<topic>"`
   - If nothing fits: `rusty-cli brain new concept "<Topic>"`
2. Append the finding together with its source:
   `rusty-cli brain append <slug> "<one-line summary>" --detail "<source URL + short quote>"`
3. Tell running clients the brain changed: `rusty-cli refresh` (a write through the MCP
   tools needs no refresh)

## Pitfalls
- NEVER put a standalone `---` rule in a brain page body — `brain set` treats it as the
  compiled-truth / timeline separator and will split or duplicate the page. Divide sections
  with headings instead.

## Verification
- `rusty-cli brain search "<topic>"` returns the finding with its source.
"#;

/// Seed: give a concise morning brief from the tasks, the follow-ups due, today's daily
/// note and the brain's numbers.
const SEED_MORNING_BRIEF: &str = r#"---
name: morning-brief
description: >-
  Give a concise morning brief: open tasks by list, decisions due for a follow-up, today's
  daily note, and brain stats. Use when the user asks for a brief, "what's on my plate",
  or a daily standup. Keywords: brief, standup, today, plate, agenda.
---

Give a concise brief of what's on the user's plate. Gather these through the `rusty` MCP
server's tools (`rusty-cli` where shown), then summarize.

1. Open tasks by list: `list_task_groups`, then `list_tasks` with each group's id.
2. Decisions due: `brain_due` (or `rusty-cli brain due`).
3. Today's daily note: `brain_daily_note` (or `rusty-cli brain daily`); skip it when empty.
4. Brain pulse: `brain_stats` (or `rusty-cli brain stats`).

Summarize tightly: open tasks grouped by list, the follow-ups due, whether the daily note
has anything, and one or two suggested focuses. Don't pad it.
"#;

/// Seed: strip AI-slop patterns from prose without flattening the writer's voice.
///
/// Vendored from <https://github.com/petergyang/no-ai-slop> (MIT, Peter Yang; the notice
/// is in `THIRD_PARTY_NOTICES.md`), condensed into one file because a seed ships as a single
/// `SKILL.md`: the upstream `eval.md` checklist is inlined here.
const SEED_NO_AI_SLOP: &str = r#"---
name: no-ai-slop
description: >-
  Edit prose into sharper, more human writing while preserving the writer's voice, or
  detect AI-slop patterns without rewriting. Use when a draft should read clearer, more
  direct, or less AI-sounding, and whenever you write prose yourself — notes, brain pages,
  docs, commit messages, summaries, or a long answer. Keywords: writing, edit, draft,
  rewrite, voice, tone, AI slop.
---

Be a sharp human editor. Keep the writer's point and voice; remove AI patterns without
turning distinctive writing into generic polished prose.

## Two jobs

**Edit (default).** Someone hands you a draft. Make the minimum effective edit, then
return the full edited draft plus a short **What changed** section.

**Detect.** Someone asks whether a piece reads as AI, or asks for an audit. Name each
pattern below that appears, quote the line, give the fix in a few words. Don't rewrite,
don't score it, don't guess whether AI wrote it. Offer to edit afterward.

**Your own prose.** Apply the rules silently as you write. No *What changed* section, no
announcement. Code, identifiers, log strings, and quoted source material are exempt —
copy quoted material exactly, banned words and all.

## Principles

- Preserve the writer's real voice: vocabulary, cadence, bluntness, humor, uncertainty,
  digressions, level of polish. Don't make every paragraph equally tidy.
- Make the minimum effective edit. Leave strong human sentences alone.
- Lead with the point when the setup adds nothing. Keep a personal aside when it creates
  context, tension, or character.
- Don't invent claims, examples, stats, or opinions. If something is unclear, ask.
- Open it up, don't dumb it down. Strip jargon, long sentences, abstract nouns, and
  tangled structure; keep the substance and precision.
- Use active voice. Never let inanimate things do human verbs.
- Be concrete: names, numbers, dates, mechanisms. "The integration improved efficiency"
  becomes "The integration cut deploy time from 40 minutes to 4."
- Make verbs do the work: "made a decision" becomes "decided."
- Keep useful edge — strong opinions, blunt language, humor, honest admissions.
- Keep the writer's structure unless it hurts the piece. If you reorganize, say why.

## Words to cut

Banned: delve, foster, leverage, utilize, facilitate, empower, streamline, robust,
cutting-edge, paradigm shift, game changer, this is huge, this changes everything,
tapestry, realm, beacon, multifaceted, meticulous, intricate, paramount, transformative,
elevate, embark, supercharge, harness, ever-evolving.

Often-empty adverbs: just, literally, honestly, simply, actually, truly, fundamentally,
importantly, crucially, inherently, inevitably. Cut when they add nothing; keep when they
carry emphasis, uncertainty, contrast, or the writer's spoken rhythm.

Often-empty phrases: it's worth noting, it's important to note, at the end of the day,
when it comes to, at its core, in today's world, in the age of, the reality is, the truth
is, in terms of, in order to, going forward, in this article, let's dive in.

## Patterns to cut

- **Binary contrasts.** "It's not X, it's Y." State Y directly.
- **Throat-clearing openers.** "Here's the thing," "Let me be clear," "I'll be honest."
- **Faux-insight setups.** "What most people get wrong," "the part everyone misses."
  Cut the setup; let the claim stand alone.
- **Colon reveals.** "The best part: it learns." Rewrite as a plain sentence. Colons are
  for lists, labels, and quotes. Sentence case after a colon unless grammar requires
  otherwise.
- **Superficial analysis.** Trailing `-ing` clauses that fake meaning: "highlighting,"
  "underscoring," "reflecting," "showcasing." Say the actual consequence.
- **Importance puffery.** "Marks a pivotal moment," "stands as a testament," "plays a
  vital role." State the fact; let the reader judge.
- **Weasel attribution.** "Experts agree," "studies show." Name the source or cut it.
- **Fake-strong verbs.** Prefer "is" and "has" when clearer than "serves as a hub for."
- **Synonym cycling.** Repeat the right word instead of rotating terms for style.
- **Negative listing.** "Not a X. Not a Y. A Z." Just say Z.
- **Dramatic fragmentation.** "That's it. That's the whole thing." Use full sentences.
- **Robotic rhythm.** Vary sentence and paragraph shape only when it helps the point.
- **Rhetorical setups.** "What if I told you," "Think about it:", "Plot twist:".
- **Fake-profound kickers.** Delete the closing metaphor or mic-drop line; don't rewrite
  it into a better one. End on the clearest concrete sentence already there.
- **Summary-recap endings.** "In conclusion," "Ultimately," or a paragraph restating the
  piece. End on the last concrete point, takeaway, or next action.
- **Formatting slop.** Emoji headings, bold sprinkled mid-sentence, bullets where two
  sentences of prose read better, headers over two-sentence sections.
- **Em dashes.** None in short copy; 1-2 in a longer draft when they clearly beat commas,
  periods, or parentheses.

## The mechanical gate

Fix the em-dash budget before drafting: zero in short copy (social posts, commit messages,
titles, descriptions, UI strings, under ~150 words); at most two in long-form, bold labels
included; punctuate labels with colons. Count after drafting instead of eyeballing
(`grep -o '—' | wc -l` on a file), and grep for the banned words. Over budget means
restructure the sentence; a dash swapped for a colon keeps the same crutch. Published
material is not precedent for breaking any rule here.

## Workflow

1. Read the whole draft first.
2. Note the core point and 3-5 voice signals to preserve. Keep that note internal. If you
   can't find the core point, ask.
3. Detect request → return the findings report and stop.
4. Edit request → make the minimum effective changes, then run the checks below yourself.
5. Any check fails → fix and re-check.
6. Output the full edited draft and a short **What changed** section.

## Checks (answer pass/fail before returning)

1. Point preserved, nothing invented?
2. Distinctive vocabulary, cadence, and level of polish intact?
3. Strong human sentences left alone; cutting proportional to actual slop?
4. Active voice with human subjects where possible; concrete facts protected?
5. Banned words, empty adverbs, and filler phrases gone (unless quoted as examples)?
6. Every pattern above fixed — including kickers deleted rather than rewritten, and
   recap endings cut?
7. Formatting slop removed; em-dash budget verified by count (0 short copy, ≤2
   long-form); sentence case after colons?
8. No robotic symmetry or stacked punchy fragments?
9. Would the writer recognize this as their own voice, read aloud to a sharp colleague?
10. Output has the full draft plus a short **What changed** section (edit requests), or
    named patterns with quoted lines and no rewrite (detect requests)?
"#;

/// Provenance of a skill: hand-written by a human vs. authored by the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillOrigin {
    /// Authored or installed by a human.
    User,
    /// Authored automatically by the self-improvement loop.
    Auto,
}

/// Lifecycle status of a skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillStatus {
    /// Discoverable and invocable by Claude (lives under `.claude/skills/`).
    Active,
    /// Awaiting approval (lives under `staging/`); not discoverable.
    Pending,
}

impl SkillOrigin {
    /// String form (`"user"` / `"auto"`).
    pub fn as_str(&self) -> &'static str {
        match self {
            SkillOrigin::User => "user",
            SkillOrigin::Auto => "auto",
        }
    }
}

impl SkillStatus {
    /// String form (`"active"` / `"pending"`).
    pub fn as_str(&self) -> &'static str {
        match self {
            SkillStatus::Active => "active",
            SkillStatus::Pending => "pending",
        }
    }
}

/// The `SKILL.md` YAML frontmatter fields Rusty cares about.
///
/// Unknown keys (e.g. `allowed-tools`, `when_to_use`, `rusty_origin`) are preserved in
/// [`SkillFrontmatter::extra`] for safety scanning and provenance.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct SkillFrontmatter {
    /// Human-readable display name (distinct from the directory/invocation name).
    #[serde(default)]
    pub name: Option<String>,
    /// What the skill does — the lever Claude uses to decide when to invoke it.
    #[serde(default)]
    pub description: Option<String>,
    /// All other frontmatter keys, preserved verbatim.
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// A parsed skill on disk.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Skill {
    /// Directory name — this is the skill's invocation identity in Claude Code.
    pub name: String,
    /// Display name from frontmatter, falling back to [`Skill::name`].
    pub display_name: String,
    /// Description from frontmatter (may be empty).
    pub description: String,
    /// Provenance (user vs. auto-authored).
    pub origin: SkillOrigin,
    /// Lifecycle status (active vs. pending).
    pub status: SkillStatus,
    /// Absolute path to the skill's `SKILL.md`.
    pub path: String,
    /// Markdown body (everything after the frontmatter).
    pub body: String,
}

/// Default skills root: `~/.rusty/skills` (falls back to `./.rusty/skills` if the home
/// directory cannot be resolved — matching the convention in `engine::db`).
pub fn default_root() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".rusty")
        .join("skills")
}

/// Resolve the skills root, expanding a leading `~`: `RUSTY_SKILLS` wins, then the
/// `skills_path` setting, then the default under `~/.rusty`. The app's `rusty <name>`
/// dispatch (TICKET-010) reads the variable before any database is open, so every reader
/// has to agree with it or the script it found is not the script that runs.
pub fn resolve_root(settings: &SettingsManager) -> PathBuf {
    configured_root(settings).unwrap_or_else(default_root)
}

/// The skills root `RUSTY_SKILLS` or the `skills_path` setting names (a leading `~`
/// expanded), or `None` when neither does and the default applies.
pub fn configured_root(settings: &SettingsManager) -> Option<PathBuf> {
    if let Some(from_env) = std::env::var_os("RUSTY_SKILLS") {
        let from_env = from_env.to_string_lossy().trim().to_string();
        if !from_env.is_empty() {
            return Some(PathBuf::from(expand_tilde(&from_env)));
        }
    }
    match settings.get(SETTING_PATH) {
        Ok(Some(p)) if !p.trim().is_empty() => Some(PathBuf::from(expand_tilde(p.trim()))),
        _ => None,
    }
}

/// Whether skills are enabled. Unset or a read error defaults to enabled; the common
/// falsy spellings (`false`, `0`, `no`, `off`, case/space-insensitive) count as off.
pub fn is_enabled(settings: &SettingsManager) -> bool {
    match settings.get(SETTING_ENABLED) {
        Ok(Some(v)) => !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "false" | "0" | "no" | "off"
        ),
        // Unset → default on. Read error → default on: a transient SQLite lock should
        // not silently disable the feature (the real safety boundary is the planned
        // content scan, not this toggle).
        _ => true,
    }
}

/// Initialize the skills store at startup: ensure directories and deploy seed skills.
///
/// Seeds deploy at most once (tracked by a persisted flag) and only while skills are
/// enabled, so a user who deletes a seed or disables skills is respected. Errors are
/// logged, not fatal — skills are an enhancement, not a hard startup dependency.
pub fn bootstrap(settings: &SettingsManager) {
    let mgr = SkillsManager::new(resolve_root(settings));
    if let Err(e) = mgr.ensure_dirs() {
        eprintln!("[skills] ensure_dirs failed: {e}");
    }

    let already_seeded = matches!(settings.get(SETTING_SEEDED), Ok(Some(ref v)) if v == "true");
    if is_enabled(settings) && !already_seeded {
        match mgr.deploy_seeds() {
            Ok(n) => {
                if n > 0 {
                    eprintln!("[skills] deployed {n} seed skill(s)");
                }
                let _ = settings.set(SETTING_SEEDED, "true");
            }
            Err(e) => eprintln!("[skills] deploy_seeds failed: {e}"),
        }
    }
}

/// `~` or `~/…` expanded against the home directory; any other path as given.
fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).to_string_lossy().to_string();
        }
    } else if path == "~" {
        if let Some(home) = dirs::home_dir() {
            return home.to_string_lossy().to_string();
        }
    }
    path.to_string()
}

/// Whether a directory name is a valid skill (invocation) name: lowercase ASCII
/// alphanumerics and interior hyphens, matching Claude Code's skill-name rules. This
/// also prevents odd names (spaces, quotes, `..`) from flowing into later command
/// construction.
fn is_valid_skill_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.ends_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Reads and manages the on-disk skills store.
pub struct SkillsManager {
    root: PathBuf,
    /// Where a change is recorded for other processes, when attached (TICKET-035).
    changes: Option<std::sync::Arc<crate::engine::db::Database>>,
}

impl SkillsManager {
    /// Create a manager rooted at `root` (the directory passed to `--add-dir`).
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            changes: None,
        }
    }

    /// Record every skill and script write in `db`'s change log.
    pub fn with_changes(mut self, db: std::sync::Arc<crate::engine::db::Database>) -> Self {
        self.changes = Some(db);
        self
    }

    /// Record a change to a skill or a script by name.
    pub(crate) fn record(&self, kind: &str, name: &str, op: &str) {
        if let Some(db) = &self.changes {
            db.record_change(kind, name, op, "");
        }
    }

    /// The skills root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The active (discoverable) skills directory: `<root>/.claude/skills`.
    pub fn active_dir(&self) -> PathBuf {
        self.root.join(".claude").join("skills")
    }

    /// The staging (pending-approval) directory: `<root>/staging`.
    pub fn staging_dir(&self) -> PathBuf {
        self.root.join("staging")
    }

    /// Ensure the active and staging directories exist.
    pub fn ensure_dirs(&self) -> Result<(), String> {
        for dir in [self.active_dir(), self.staging_dir()] {
            std::fs::create_dir_all(&dir)
                .map_err(|e| format!("Failed to create skills dir {}: {e}", dir.display()))?;
        }
        Ok(())
    }

    /// Deploy bundled seed skills if the active directory currently has none.
    ///
    /// Returns the number of seeds written (0 if any active skill already exists).
    pub fn deploy_seeds(&self) -> Result<usize, String> {
        let active = self.active_dir();
        std::fs::create_dir_all(&active)
            .map_err(|e| format!("Failed to create skills dir {}: {e}", active.display()))?;

        if !self.list(false).is_empty() {
            return Ok(0);
        }

        let mut written = 0;
        for (name, content) in SEED_SKILLS {
            let dir = active.join(name);
            std::fs::create_dir_all(&dir)
                .map_err(|e| format!("Failed to create seed dir {}: {e}", dir.display()))?;
            let file = dir.join("SKILL.md");
            std::fs::write(&file, content)
                .map_err(|e| format!("Failed to write seed {}: {e}", file.display()))?;
            written += 1;
        }
        Ok(written)
    }

    /// List skills. When `include_pending` is true, staging skills are included.
    /// Results are sorted by name; malformed or invalid skills are skipped (and logged).
    pub fn list(&self, include_pending: bool) -> Vec<Skill> {
        let mut out = self.scan_dir(&self.active_dir(), SkillStatus::Active);
        if include_pending {
            out.extend(self.scan_dir(&self.staging_dir(), SkillStatus::Pending));
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// Find a skill by directory name, searching active then staging.
    pub fn get(&self, name: &str) -> Option<Skill> {
        self.list(true).into_iter().find(|s| s.name == name)
    }

    /// Create an active (User-origin) skill. See `create_skill_at`.
    /// Does not commit — call [`SkillsManager::git_commit_blocking`].
    pub fn create_skill(
        &self,
        name: &str,
        description: &str,
        body: &str,
        force: bool,
    ) -> Result<Skill, String> {
        self.create_skill_at(
            &self.active_dir(),
            name,
            description,
            body,
            SkillOrigin::User,
            force,
        )
    }

    /// Create a pending (staging) skill marked agent-authored. Used by the self-authoring
    /// loop; overwrites any same-named staging proposal. Does not commit.
    pub fn create_pending_skill(
        &self,
        name: &str,
        description: &str,
        body: &str,
    ) -> Result<Skill, String> {
        self.create_skill_at(
            &self.staging_dir(),
            name,
            description,
            body,
            SkillOrigin::Auto,
            true,
        )
    }

    /// Write `<base>/<name>/SKILL.md` with the given fields and return the parsed [`Skill`].
    /// Validates the name; errors if it already exists (unless `force`). Does not commit.
    fn create_skill_at(
        &self,
        base: &Path,
        name: &str,
        description: &str,
        body: &str,
        origin: SkillOrigin,
        force: bool,
    ) -> Result<Skill, String> {
        if !is_valid_skill_name(name) {
            return Err(format!(
                "invalid skill name {name:?}: use lowercase letters, digits, and hyphens"
            ));
        }
        let dir = base.join(name);
        let file = dir.join("SKILL.md");
        if file.exists() && !force {
            return Err(format!(
                "skill {name:?} already exists (use --force to overwrite)"
            ));
        }
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create skill dir {}: {e}", dir.display()))?;
        let content = render_skill_md(name, description, body, origin)?;
        std::fs::write(&file, content)
            .map_err(|e| format!("Failed to write {}: {e}", file.display()))?;
        // Read back from the directory we wrote (not active-first `get()`), so the result
        // reflects this file even if a same-named skill exists in the other directory.
        let status = if base == self.active_dir().as_path() {
            SkillStatus::Active
        } else {
            SkillStatus::Pending
        };
        let written = self
            .scan_dir(base, status)
            .into_iter()
            .find(|s| s.name == name)
            .ok_or_else(|| "skill written but could not be read back".to_string())?;
        self.record("skill", name, "created");
        Ok(written)
    }

    /// Rewrite a skill's description and/or body in place, keeping every other
    /// frontmatter key. Works on active and pending skills alike. Returns the skill as
    /// re-read from disk and the safety scan of the new file, so a caller can show the
    /// findings. Does not commit — call [`SkillsManager::git_commit_blocking`].
    pub fn update_skill(
        &self,
        name: &str,
        description: Option<&str>,
        body: Option<&str>,
    ) -> Result<(Skill, Vec<String>), String> {
        let skill = self
            .get(name)
            .ok_or_else(|| format!("skill {name:?} not found"))?;
        let path = PathBuf::from(&skill.path);
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
        let (mut fm, old_body) = parse_skill_md(&raw)?;
        if let Some(d) = description {
            fm.description = Some(d.to_string());
        }
        if fm.name.is_none() {
            fm.name = Some(name.to_string());
        }
        let content = render_skill_frontmatter(&fm, body.unwrap_or(&old_body))?;
        std::fs::write(&path, &content)
            .map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
        let findings = scan_skill_md(&content);
        let updated = self
            .get(name)
            .ok_or_else(|| "skill written but could not be read back".to_string())?;
        self.record("skill", name, "updated");
        Ok((updated, findings))
    }

    /// Delete a skill (active or staging) by removing its directory.
    /// Does not commit — call [`SkillsManager::git_commit_blocking`].
    pub fn delete_skill(&self, name: &str) -> Result<(), String> {
        let skill = self
            .get(name)
            .ok_or_else(|| format!("skill {name:?} not found"))?;
        let dir = Path::new(&skill.path)
            .parent()
            .ok_or_else(|| format!("bad skill path: {}", skill.path))?;
        std::fs::remove_dir_all(dir).map_err(|e| format!("Failed to remove skill: {e}"))?;
        self.record("skill", name, "deleted");
        Ok(())
    }

    /// Run the safety scan over a skill's `SKILL.md`. Returns findings (empty = clean);
    /// errors only if the skill can't be found or read.
    pub fn scan(&self, name: &str) -> Result<Vec<String>, String> {
        let skill = self
            .get(name)
            .ok_or_else(|| format!("skill {name:?} not found"))?;
        let folder = Path::new(&skill.path)
            .parent()
            .ok_or_else(|| format!("{} has no folder", skill.path))?;
        Ok(scan_folder(folder))
    }

    /// Approve a pending (staged) skill: safety-scan it, then move it into the active dir.
    /// Refuses if the scan finds issues unless `force`. Does not commit.
    pub fn approve(&self, name: &str, force: bool) -> Result<(), String> {
        if !is_valid_skill_name(name) {
            return Err(format!("invalid skill name {name:?}"));
        }
        let src = self.staging_dir().join(name);
        let src_md = src.join("SKILL.md");
        if !src_md.exists() {
            return Err(format!("no pending skill {name:?} to approve"));
        }
        let dst = self.active_dir().join(name);
        if dst.exists() {
            return Err(format!("an active skill {name:?} already exists"));
        }
        // Everything that would become runnable is read: the SKILL.md and the scripts and
        // other files beside it.
        let findings = scan_folder(&src);
        if !findings.is_empty() && !force {
            return Err(format!(
                "blocked by safety scan ({} issue(s)): {} — re-run with --force to override",
                findings.len(),
                findings.join("; ")
            ));
        }
        std::fs::create_dir_all(self.active_dir())
            .map_err(|e| format!("Failed to create active dir: {e}"))?;
        std::fs::rename(&src, &dst).map_err(|e| format!("Failed to promote skill: {e}"))?;
        self.record("skill", name, "approved");
        Ok(())
    }

    /// Reject (delete) a pending (staged) skill. Does not commit.
    pub fn reject(&self, name: &str) -> Result<(), String> {
        if !is_valid_skill_name(name) {
            return Err(format!("invalid skill name {name:?}"));
        }
        let src = self.staging_dir().join(name);
        if !src.is_dir() {
            return Err(format!("no pending skill {name:?} to reject"));
        }
        std::fs::remove_dir_all(&src).map_err(|e| format!("Failed to reject skill: {e}"))?;
        self.record("skill", name, "rejected");
        Ok(())
    }

    /// Auto-commit the skills store synchronously (blocks until git returns). Lazily
    /// `git init`s. Mirrors the brain vault's git behavior so skill edits are versioned.
    /// The CLI and the MCP server both commit through this after every write; git's
    /// output is ignored, so a failed commit leaves the change for the next one.
    pub fn git_commit_blocking(&self, message: &str) {
        run_git_commit(&self.root, message);
    }

    /// Scan one directory for `<name>/SKILL.md` entries, tagging each with `status`.
    fn scan_dir(&self, dir: &Path, status: SkillStatus) -> Vec<Skill> {
        let mut skills = Vec::new();
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return skills, // missing dir → no skills
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            // Hidden/control dirs (e.g. `.git`, a future `.pending`) skip silently.
            if name.starts_with('.') {
                continue;
            }
            if !is_valid_skill_name(&name) {
                eprintln!("[skills] skipping dir with invalid skill name: {name:?}");
                continue;
            }
            let skill_md = path.join("SKILL.md");
            // Bound the read: skip (and log) an oversized SKILL.md.
            if let Ok(meta) = std::fs::metadata(&skill_md) {
                if meta.len() > MAX_SKILL_MD_BYTES {
                    eprintln!(
                        "[skills] skipping oversized SKILL.md ({} bytes): {}",
                        meta.len(),
                        skill_md.display()
                    );
                    continue;
                }
            }
            let raw = match std::fs::read_to_string(&skill_md) {
                Ok(r) => r,
                // A dir without a SKILL.md simply is not a skill — skip silently.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                // Any other I/O error (permissions, mid-write) is worth surfacing.
                Err(e) => {
                    eprintln!("[skills] cannot read {}: {e}", skill_md.display());
                    continue;
                }
            };
            match parse_skill_md(&raw) {
                Ok((fm, body)) => {
                    let display_name = fm.name.clone().unwrap_or_else(|| name.clone());
                    let description = fm.description.clone().unwrap_or_default();
                    let origin = match fm.extra.get("rusty_origin").and_then(|v| v.as_str()) {
                        Some("auto") => SkillOrigin::Auto,
                        _ => SkillOrigin::User,
                    };
                    skills.push(Skill {
                        name,
                        display_name,
                        description,
                        origin,
                        status,
                        path: skill_md.to_string_lossy().to_string(),
                        body,
                    });
                }
                Err(e) => eprintln!("[skills] skipping {}: {e}", skill_md.display()),
            }
        }
        skills
    }
}

/// Parse a `SKILL.md` into its frontmatter and markdown body.
///
/// Expects the file to start with a `---` fenced YAML block. Returns an error if the
/// frontmatter is missing, unterminated, or not valid YAML.
pub fn parse_skill_md(raw: &str) -> Result<(SkillFrontmatter, String), String> {
    let trimmed = raw.trim_start();
    if !trimmed.starts_with("---") {
        return Err("SKILL.md missing frontmatter (must start with ---)".to_string());
    }
    let after_open = trimmed[3..].trim_start_matches(['\r', '\n']);
    let close = after_open
        .find("\n---")
        .ok_or_else(|| "SKILL.md missing closing --- delimiter".to_string())?;
    let yaml_str = &after_open[..close];
    let body = after_open[close + 4..]
        .trim_start_matches(['\r', '\n'])
        .to_string();
    let frontmatter: SkillFrontmatter = serde_yaml::from_str(yaml_str)
        .map_err(|e| format!("Failed to parse SKILL.md frontmatter: {e}"))?;
    Ok((frontmatter, body))
}

/// Render a `SKILL.md` from a name, description, and body, with valid YAML frontmatter.
/// Agent-authored skills (`origin == Auto`) are tagged with `rusty_origin: auto`.
fn render_skill_md(
    name: &str,
    description: &str,
    body: &str,
    origin: SkillOrigin,
) -> Result<String, String> {
    let mut extra = HashMap::new();
    if origin == SkillOrigin::Auto {
        extra.insert(
            "rusty_origin".to_string(),
            serde_json::Value::String("auto".to_string()),
        );
    }
    let fm = SkillFrontmatter {
        name: Some(name.to_string()),
        description: Some(description.to_string()),
        extra,
    };
    render_skill_frontmatter(&fm, body)
}

/// Render a `SKILL.md` from parsed frontmatter and a body, keeping every key.
fn render_skill_frontmatter(fm: &SkillFrontmatter, body: &str) -> Result<String, String> {
    let yaml = serde_yaml::to_string(fm).map_err(|e| format!("serialize frontmatter: {e}"))?;
    // serde_yaml may emit a leading `---` document marker; strip it so we control the fences.
    let yaml_clean = yaml.trim_end().trim_start_matches("---").trim();
    Ok(format!("---\n{yaml_clean}\n---\n\n{}\n", body.trim_end()))
}

/// Maximum lines in a `SKILL.md` before the scan flags it as oversized.
const MAX_SKILL_LINES: usize = 200;

/// Heuristic markers that look like leaked secrets (a human reviews any findings).
const SECRET_MARKERS: &[&str] = &[
    "ghp_",
    "github_pat_",
    "gho_",
    "glpat-",
    "AKIA",
    "xoxb-",
    "xoxp-",
    "-----BEGIN",
    "AIzaSy",
    "sk-ant-",
    "sk-proj-",
];

/// Safety-scan a `SKILL.md`'s raw text, returning human-readable findings (empty = clean).
///
/// Flags a declared `allowed-tools` key (pre-approves tools), dynamic command injection
/// (bang-backtick), likely secret markers, and oversized files. This gates promotion of
/// agent-authored proposals to the active (invocable) set.
pub fn scan_skill_md(raw: &str) -> Vec<String> {
    let mut findings = Vec::new();
    match parse_skill_md(raw) {
        Ok((fm, _body)) => {
            if fm.extra.contains_key("allowed-tools") || fm.extra.contains_key("allowed_tools") {
                findings.push(
                    "declares allowed-tools (pre-approves tools without prompting)".to_string(),
                );
            }
        }
        // Frontmatter the scan cannot read is frontmatter it cannot vouch for.
        Err(e) => findings.push(format!("frontmatter does not parse: {e}")),
    }
    findings.extend(scan_text(raw));
    findings
}

/// The safety scan over a skill's folder: its `SKILL.md`, and every other file in it (the
/// scripts above all), each finding outside `SKILL.md` named by its file.
pub(crate) fn scan_folder(dir: &Path) -> Vec<String> {
    fn files(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_name() == ".git" {
                continue;
            }
            if path.is_dir() {
                files(&path, out);
            } else {
                out.push(path);
            }
        }
    }
    let mut all = Vec::new();
    files(dir, &mut all);
    all.sort();
    let mut findings = Vec::new();
    for path in all {
        let rel = path
            .strip_prefix(dir)
            .unwrap_or(&path)
            .display()
            .to_string();
        let Ok(text) = std::fs::read_to_string(&path) else {
            findings.push(format!("{rel}: not text, so it cannot be scanned"));
            continue;
        };
        if rel == "SKILL.md" {
            findings.extend(scan_skill_md(&text));
        } else {
            findings.extend(scan_text(&text).into_iter().map(|f| format!("{rel}: {f}")));
        }
    }
    findings
}

/// The checks that apply to any file in a skill: its size, shell-injection markers, and
/// things that look like secrets.
pub(crate) fn scan_text(raw: &str) -> Vec<String> {
    let mut findings = Vec::new();

    let lines = raw.lines().count();
    if lines > MAX_SKILL_LINES {
        findings.push(format!("oversized: {lines} lines (> {MAX_SKILL_LINES})"));
    }

    // Claude Code runs `!`...`` / fenced `!` as shell before the model sees the content.
    if raw.contains("!`") || raw.contains("```!") {
        findings.push("contains a dynamic command-injection marker (bang-backtick)".to_string());
    }

    for marker in SECRET_MARKERS {
        if raw.contains(marker) {
            findings.push(format!("possible secret marker: {marker}"));
        }
    }

    findings
}

/// Run `git init` (if needed) + `git add -A` + `git commit` in `root`, blocking. Best-effort:
/// git errors (not a repo, git missing, unset identity) are ignored, matching the brain vault.
fn run_git_commit(root: &Path, message: &str) {
    use std::process::Command;
    if !root.join(".git").exists() {
        let _ = Command::new("git")
            .args(["init"])
            .current_dir(root)
            .output();
    }
    crate::git::ensure_identity(root);
    let _ = Command::new("git")
        .args(["add", "-A"])
        .current_dir(root)
        .output();
    let _ = Command::new("git")
        .args([
            "commit",
            "-m",
            message,
            "--allow-empty-message",
            "--no-gpg-sign",
        ])
        .current_dir(root)
        .output();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::db::Database;
    use rusqlite::Connection;
    use std::sync::Arc;

    /// A temp directory that removes itself on drop — panic-safe test cleanup.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let p =
                std::env::temp_dir().join(format!("rusty-skills-test-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&p).unwrap();
            TempDir(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// An in-memory settings store for decision-surface tests.
    fn test_settings() -> SettingsManager {
        let conn = Connection::open_in_memory().unwrap();
        let db = Database::from_conn(conn);
        db.migrate().unwrap();
        SettingsManager::new(Arc::new(db))
    }

    fn write_skill(root: &Path, sub: &str, name: &str, contents: &str) {
        let dir = root.join(sub).join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), contents).unwrap();
    }

    #[test]
    fn parse_basic_frontmatter() {
        let raw =
            "---\nname: My Skill\ndescription: Does a thing.\n---\n\n## Procedure\nStep one.\n";
        let (fm, body) = parse_skill_md(raw).unwrap();
        assert_eq!(fm.name.as_deref(), Some("My Skill"));
        assert_eq!(fm.description.as_deref(), Some("Does a thing."));
        assert!(body.contains("## Procedure"));
        assert!(body.contains("Step one."));
    }

    #[test]
    fn parse_preserves_extra_fields() {
        let raw = "---\nname: X\ndescription: Y\nallowed-tools: Bash(npm *)\nrusty_origin: auto\n---\nbody\n";
        let (fm, _) = parse_skill_md(raw).unwrap();
        assert_eq!(
            fm.extra.get("allowed-tools").and_then(|v| v.as_str()),
            Some("Bash(npm *)")
        );
        assert_eq!(
            fm.extra.get("rusty_origin").and_then(|v| v.as_str()),
            Some("auto")
        );
    }

    #[test]
    fn parse_missing_frontmatter_errors() {
        assert!(parse_skill_md("no frontmatter here").is_err());
    }

    #[test]
    fn parse_unterminated_frontmatter_errors() {
        assert!(parse_skill_md("---\nname: X\nno closing fence").is_err());
    }

    #[test]
    fn valid_skill_names() {
        assert!(is_valid_skill_name("file-research-finding"));
        assert!(is_valid_skill_name("a1"));
        assert!(!is_valid_skill_name("Bad Name"));
        assert!(!is_valid_skill_name("UPPER"));
        assert!(!is_valid_skill_name("-leading"));
        assert!(!is_valid_skill_name("trailing-"));
        assert!(!is_valid_skill_name("under_score"));
        assert!(!is_valid_skill_name(""));
    }

    #[test]
    fn ensure_dirs_creates_layout() {
        let tmp = TempDir::new();
        let mgr = SkillsManager::new(tmp.path().to_path_buf());
        mgr.ensure_dirs().unwrap();
        assert!(mgr.active_dir().is_dir());
        assert!(mgr.staging_dir().is_dir());
    }

    #[test]
    fn list_active_and_pending() {
        let tmp = TempDir::new();
        let mgr = SkillsManager::new(tmp.path().to_path_buf());
        mgr.ensure_dirs().unwrap();
        write_skill(
            tmp.path(),
            ".claude/skills",
            "alpha",
            "---\nname: Alpha\ndescription: A\n---\nbody",
        );
        write_skill(
            tmp.path(),
            "staging",
            "beta",
            "---\nname: Beta\ndescription: B\nrusty_origin: auto\n---\nbody",
        );

        let active_only = mgr.list(false);
        assert_eq!(active_only.len(), 1);
        assert_eq!(active_only[0].name, "alpha");
        assert_eq!(active_only[0].status, SkillStatus::Active);
        assert_eq!(active_only[0].origin, SkillOrigin::User);

        let all = mgr.list(true);
        assert_eq!(all.len(), 2);
        let beta = all.iter().find(|s| s.name == "beta").unwrap();
        assert_eq!(beta.status, SkillStatus::Pending);
        assert_eq!(beta.origin, SkillOrigin::Auto);
    }

    #[test]
    fn list_skips_dirs_without_skill_md_and_malformed() {
        let tmp = TempDir::new();
        let mgr = SkillsManager::new(tmp.path().to_path_buf());
        mgr.ensure_dirs().unwrap();
        std::fs::create_dir_all(tmp.path().join(".claude/skills/empty")).unwrap();
        write_skill(tmp.path(), ".claude/skills", "bad", "not valid frontmatter");
        write_skill(
            tmp.path(),
            ".claude/skills",
            "good",
            "---\ndescription: ok\n---\nbody",
        );

        let skills = mgr.list(false);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "good");
        // No frontmatter `name` → display_name falls back to dir name.
        assert_eq!(skills[0].display_name, "good");
    }

    #[test]
    fn list_skips_invalid_named_dirs() {
        let tmp = TempDir::new();
        let mgr = SkillsManager::new(tmp.path().to_path_buf());
        mgr.ensure_dirs().unwrap();
        write_skill(
            tmp.path(),
            ".claude/skills",
            "valid-name",
            "---\ndescription: ok\n---\nb",
        );
        // Invalid name (uppercase + space): must be skipped, not surfaced.
        let bad = tmp.path().join(".claude/skills").join("Bad Name");
        std::fs::create_dir_all(&bad).unwrap();
        std::fs::write(bad.join("SKILL.md"), "---\ndescription: x\n---\nb").unwrap();

        let skills = mgr.list(false);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "valid-name");
    }

    #[test]
    fn list_skips_oversized_skill_md() {
        let tmp = TempDir::new();
        let mgr = SkillsManager::new(tmp.path().to_path_buf());
        mgr.ensure_dirs().unwrap();
        let dir = tmp.path().join(".claude/skills").join("huge");
        std::fs::create_dir_all(&dir).unwrap();
        let mut content = String::from("---\ndescription: big\n---\n");
        content.push_str(&"x".repeat((MAX_SKILL_MD_BYTES + 1) as usize));
        std::fs::write(dir.join("SKILL.md"), content).unwrap();

        assert!(mgr.list(false).is_empty());
    }

    #[test]
    fn deploy_seeds_once_then_idempotent() {
        let tmp = TempDir::new();
        let mgr = SkillsManager::new(tmp.path().to_path_buf());
        mgr.ensure_dirs().unwrap();

        let n = mgr.deploy_seeds().unwrap();
        assert_eq!(n, SEED_SKILLS.len());
        // Seeds present → a second deploy writes nothing.
        assert_eq!(mgr.deploy_seeds().unwrap(), 0);

        let skills = mgr.list(false);
        assert_eq!(skills.len(), SEED_SKILLS.len());
        assert!(skills.iter().any(|s| s.name == "file-research-finding"));
        // The seed itself is valid (slug name, no allowed-tools).
        let seed = skills
            .iter()
            .find(|s| s.name == "file-research-finding")
            .unwrap();
        assert_eq!(seed.origin, SkillOrigin::User);
    }

    #[test]
    fn get_finds_by_name() {
        let tmp = TempDir::new();
        let mgr = SkillsManager::new(tmp.path().to_path_buf());
        mgr.ensure_dirs().unwrap();
        write_skill(
            tmp.path(),
            ".claude/skills",
            "findme",
            "---\ndescription: here\n---\nbody",
        );
        assert!(mgr.get("findme").is_some());
        assert!(mgr.get("missing").is_none());
    }

    #[test]
    fn is_enabled_default_and_overrides() {
        let s = test_settings();
        assert!(is_enabled(&s)); // unset → default on
        s.set(SETTING_ENABLED, "false").unwrap();
        assert!(!is_enabled(&s));
        s.set(SETTING_ENABLED, "OFF").unwrap();
        assert!(!is_enabled(&s));
        s.set(SETTING_ENABLED, " no ").unwrap();
        assert!(!is_enabled(&s));
        s.set(SETTING_ENABLED, "true").unwrap();
        assert!(is_enabled(&s));
    }

    #[test]
    fn resolve_root_default_and_custom() {
        let s = test_settings();
        assert_eq!(resolve_root(&s), default_root());
        s.set(SETTING_PATH, "/tmp/custom-skills").unwrap();
        assert_eq!(resolve_root(&s), PathBuf::from("/tmp/custom-skills"));
    }

    /// The seeds a new store gets name only what exists now: no raw SQLite against the
    /// store, no v2 paths or tables, no window (TICKET-056).
    #[test]
    fn seeds_use_only_current_commands() {
        let names: Vec<&str> = SEED_SKILLS.iter().map(|(name, _)| *name).collect();
        assert!(!names.contains(&"recent-agents"), "{names:?}");
        for (name, body) in SEED_SKILLS {
            for stale in [
                "sqlite3",
                "Projects/rusty",
                "FROM agents",
                "FROM tasks",
                "GUI",
            ] {
                assert!(!body.contains(stale), "seed {name} still says {stale}");
            }
        }
    }

    #[test]
    fn bootstrap_seeds_once_and_sets_flag() {
        let tmp = TempDir::new();
        let s = test_settings();
        s.set(SETTING_PATH, tmp.path().to_str().unwrap()).unwrap();

        bootstrap(&s);
        assert_eq!(s.get(SETTING_SEEDED).unwrap().as_deref(), Some("true"));
        let mgr = SkillsManager::new(tmp.path().to_path_buf());
        assert!(mgr.get("file-research-finding").is_some());

        // Delete the seed and re-bootstrap: the flag prevents resurrection.
        std::fs::remove_dir_all(tmp.path().join(".claude/skills/file-research-finding")).unwrap();
        bootstrap(&s);
        assert!(mgr.get("file-research-finding").is_none());
    }

    #[test]
    fn bootstrap_skips_seeding_when_disabled() {
        let tmp = TempDir::new();
        let s = test_settings();
        s.set(SETTING_PATH, tmp.path().to_str().unwrap()).unwrap();
        s.set(SETTING_ENABLED, "false").unwrap();

        bootstrap(&s);
        let mgr = SkillsManager::new(tmp.path().to_path_buf());
        assert!(mgr.list(false).is_empty());
        // Dirs still created, but nothing seeded and no flag set.
        assert!(mgr.active_dir().is_dir());
        assert_ne!(s.get(SETTING_SEEDED).unwrap().as_deref(), Some("true"));
    }

    #[test]
    fn create_and_delete_skill() {
        let tmp = TempDir::new();
        let mgr = SkillsManager::new(tmp.path().to_path_buf());
        mgr.ensure_dirs().unwrap();

        let s = mgr
            .create_skill("my-skill", "Does X.", "## Procedure\nstep one", false)
            .unwrap();
        assert_eq!(s.name, "my-skill");
        assert_eq!(s.description, "Does X.");
        assert!(s.body.contains("## Procedure"));
        assert!(mgr.get("my-skill").is_some());

        // Duplicate without force errors; with force overwrites.
        assert!(mgr.create_skill("my-skill", "x", "y", false).is_err());
        mgr.create_skill("my-skill", "new desc", "z", true).unwrap();
        assert_eq!(mgr.get("my-skill").unwrap().description, "new desc");

        // Invalid names rejected.
        assert!(mgr.create_skill("Bad Name", "x", "y", false).is_err());

        // Delete removes it; deleting again errors.
        mgr.delete_skill("my-skill").unwrap();
        assert!(mgr.get("my-skill").is_none());
        assert!(mgr.delete_skill("my-skill").is_err());
    }

    #[test]
    fn create_pending_skill_is_auto_and_staged() {
        let tmp = TempDir::new();
        let mgr = SkillsManager::new(tmp.path().to_path_buf());
        mgr.ensure_dirs().unwrap();
        let s = mgr
            .create_pending_skill(
                "auto-skill",
                "Proposed by the agent.",
                "## Procedure\ndo it",
            )
            .unwrap();
        assert_eq!(s.name, "auto-skill");
        assert_eq!(s.origin, SkillOrigin::Auto);
        assert_eq!(s.status, SkillStatus::Pending);
        // Staged, not active: only visible when including pending.
        assert!(mgr.list(false).is_empty());
        assert_eq!(mgr.list(true).len(), 1);
    }

    /// A staged skill with a clean SKILL.md and a risky script beside it is not approved
    /// without `force`, and frontmatter the scan cannot read is a finding.
    #[test]
    fn approval_scans_the_scripts_and_unreadable_frontmatter() {
        let tmp = TempDir::new();
        let mgr = SkillsManager::new(tmp.path().to_path_buf());
        mgr.ensure_dirs().unwrap();
        mgr.create_pending_skill("evil", "Looks fine.", "## Procedure\nrun it")
            .unwrap();
        let staged = tmp.path().join("staging/evil");
        std::fs::write(staged.join("evil.sh"), "echo !`id`\n").unwrap();
        let findings = mgr.scan("evil").unwrap();
        assert!(
            findings.iter().any(|f| f.starts_with("evil.sh:")),
            "{findings:?}"
        );
        assert!(mgr.approve("evil", false).is_err());
        assert!(!scan_skill_md("---\ndescription: [a, b\n---\nbody").is_empty());
    }

    #[test]
    fn scan_flags_risky_content() {
        assert!(scan_skill_md("---\nname: x\ndescription: d\n---\nclean body").is_empty());
        assert!(
            !scan_skill_md("---\nname: x\ndescription: d\nallowed-tools: Bash(*)\n---\nbody")
                .is_empty()
        );
        assert!(!scan_skill_md("---\nname: x\ndescription: d\n---\nrun !`whoami`").is_empty());
        assert!(!scan_skill_md("---\nname: x\ndescription: d\n---\nkey ghp_abc123").is_empty());
    }

    #[test]
    fn approve_clean_promotes_to_active() {
        let tmp = TempDir::new();
        let mgr = SkillsManager::new(tmp.path().to_path_buf());
        mgr.ensure_dirs().unwrap();
        mgr.create_pending_skill("ok-skill", "fine", "## Procedure\nstep")
            .unwrap();
        assert_eq!(mgr.list(false).len(), 0);
        mgr.approve("ok-skill", false).unwrap();
        let active = mgr.list(false);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].name, "ok-skill");
        assert_eq!(active[0].status, SkillStatus::Active);
        // No longer in staging (only the one active copy remains).
        assert_eq!(mgr.list(true).len(), 1);
    }

    #[test]
    fn approve_flagged_blocked_unless_forced() {
        let tmp = TempDir::new();
        let mgr = SkillsManager::new(tmp.path().to_path_buf());
        mgr.ensure_dirs().unwrap();
        mgr.create_pending_skill("risky", "x", "## Procedure\nrun !`rm -rf /`")
            .unwrap();
        assert!(mgr.approve("risky", false).is_err());
        assert_eq!(mgr.list(false).len(), 0); // still not active
        mgr.approve("risky", true).unwrap(); // force overrides
        assert_eq!(mgr.list(false).len(), 1);
    }

    #[test]
    fn reject_removes_pending() {
        let tmp = TempDir::new();
        let mgr = SkillsManager::new(tmp.path().to_path_buf());
        mgr.ensure_dirs().unwrap();
        mgr.create_pending_skill("nope", "x", "body").unwrap();
        assert_eq!(mgr.list(true).len(), 1);
        mgr.reject("nope").unwrap();
        assert_eq!(mgr.list(true).len(), 0);
        assert!(mgr.reject("nope").is_err());
    }

    #[test]
    fn approve_reject_refuse_path_traversal() {
        let tmp = TempDir::new();
        let mgr = SkillsManager::new(tmp.path().to_path_buf());
        mgr.ensure_dirs().unwrap();
        // Traversal / absolute names are rejected before any filesystem op.
        assert!(mgr.approve("../escape", false).is_err());
        assert!(mgr.approve("../escape", true).is_err());
        assert!(mgr.reject("../escape").is_err());
        assert!(mgr.reject("/etc").is_err());
        assert!(mgr.reject("a/b").is_err());
    }

    #[test]
    fn update_skill_keeps_unknown_frontmatter_keys() {
        let tmp = TempDir::new();
        let mgr = SkillsManager::new(tmp.path().to_path_buf());
        mgr.ensure_dirs().unwrap();
        mgr.create_skill("tidy", "Tidy things", "# Tidy\n\nStep one.", false)
            .unwrap();
        let file = mgr.active_dir().join("tidy").join("SKILL.md");
        let with_extra = std::fs::read_to_string(&file).unwrap().replacen(
            "---\n",
            "---\nallowed-tools: Bash\n",
            1,
        );
        std::fs::write(&file, with_extra).unwrap();

        let (skill, findings) = mgr.update_skill("tidy", Some("Tidy faster"), None).unwrap();
        assert_eq!(skill.description, "Tidy faster");
        assert!(skill.body.contains("Step one."));
        // The scan sees the pre-approved tools the test planted; that is the point of
        // handing findings back.
        assert_eq!(
            findings,
            vec!["declares allowed-tools (pre-approves tools without prompting)"]
        );
        let raw = std::fs::read_to_string(&file).unwrap();
        assert!(raw.contains("allowed-tools: Bash"), "{raw}");

        let (skill, _) = mgr
            .update_skill("tidy", None, Some("# Tidy\n\nStep two."))
            .unwrap();
        assert_eq!(skill.description, "Tidy faster");
        assert!(skill.body.contains("Step two."));
        assert!(mgr.update_skill("missing", Some("x"), None).is_err());
    }
}
