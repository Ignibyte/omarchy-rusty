//! Brain engine: entity-centric knowledge base with FTS5 search.
//!
//! Combines a markdown vault on disk (`~/.rusty/brain/`) with a SQLite index
//! for fast full-text search, graph queries, and metadata lookups. Markdown files
//! are the source of truth; the database is a derived index that can be rebuilt
//! from the vault at any time.

pub mod blocks;
pub mod bookmarks;
pub mod decisions;
pub mod enrichment;
pub mod frontmatter;
pub mod import;
pub mod links;
pub mod render;
pub mod semantic;
pub mod sources;
pub mod vault;

use crate::engine::changes;
use crate::engine::db::Database;
use frontmatter::{
    parse_lenient, parse_page, properties_of, render_body, render_page, split_raw, today_iso,
    BrainFrontmatter,
};
use links::scan as scan_links;
use render::{Rendered, Resolver, Style};
use semantic::{Embedder, IndexReport, SemanticIndex};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Arc;
use vault::{clean_rel, title_to_slug, type_to_dir, VaultManager, VaultNode};

/// A full brain page with parsed content.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BrainPage {
    /// Canonical slug (e.g., "people/sarah-chen").
    pub slug: String,
    /// Entity type (person, company, project, etc.).
    pub page_type: String,
    /// Page title.
    pub title: String,
    /// Synthesized knowledge (above the separator).
    pub compiled_truth: String,
    /// Append-only evidence log (below the separator).
    pub timeline: String,
    /// Parsed YAML frontmatter.
    pub frontmatter: BrainFrontmatter,
    /// SHA-256 hash of the raw file content.
    pub content_hash: String,
    /// Unix timestamp of creation.
    pub created_at: i64,
    /// Unix timestamp of last update.
    pub updated_at: i64,
    /// The page's file, absolute, for a client that opens it in an editor (TICKET-042).
    #[serde(default)]
    pub file: String,
}

/// Summary of a brain page for list views.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BrainPageSummary {
    /// Canonical slug.
    pub slug: String,
    /// Entity type.
    pub page_type: String,
    /// Page title.
    pub title: String,
    /// Unix timestamp of last update.
    pub updated_at: i64,
    /// The page's aliases, empty when it has none (TICKET-047).
    #[serde(default)]
    pub aliases: Vec<String>,
    /// The properties a caller asked for that the page has; absent unless asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties: Option<serde_json::Map<String, serde_json::Value>>,
}

/// A search result from FTS5 queries.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BrainSearchResult {
    /// Page slug.
    pub slug: String,
    /// Entity type.
    pub page_type: String,
    /// Page title.
    pub title: String,
    /// Matching text snippet.
    pub snippet: String,
    /// BM25 relevance rank (lower is more relevant).
    pub rank: f64,
}

/// How a search runs, beyond the query text.
#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    /// Maximum results (10 when unset).
    pub limit: Option<usize>,
    /// Restrict to one page type.
    pub page_type: Option<String>,
    /// Keep only pages whose text holds the words as typed, case included.
    pub case_sensitive: bool,
    /// Treat the words as a regular expression over the page text.
    pub regex: bool,
}

/// A query taken apart: the words, and the operator terms that admit or exclude pages.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedQuery {
    /// What is left once the operator terms are removed, joined by single spaces.
    pub words: String,
    /// `tag:` terms: a page carries every one, or a tag nested under it.
    pub tags: Vec<String>,
    /// `-tag:` terms: a page carrying any is out.
    pub not_tags: Vec<String>,
    /// `path:` terms: the slug contains every one.
    pub paths: Vec<String>,
    /// `-path:` terms: the slug contains none.
    pub not_paths: Vec<String>,
    /// `file:` terms: the file name (the slug's last segment) contains every one.
    pub files: Vec<String>,
    /// `-file:` terms: the file name contains none.
    pub not_files: Vec<String>,
    /// `type:` terms: the page type is one of them.
    pub types: Vec<String>,
    /// `-type:` terms: the page type is none of them.
    pub not_types: Vec<String>,
}

impl ParsedQuery {
    /// Whether any operator term is present.
    pub fn has_operators(&self) -> bool {
        !(self.tags.is_empty()
            && self.not_tags.is_empty()
            && self.paths.is_empty()
            && self.not_paths.is_empty()
            && self.files.is_empty()
            && self.not_files.is_empty()
            && self.types.is_empty()
            && self.not_types.is_empty())
    }
}

/// A timeline entry for a brain page.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TimelineEntry {
    /// Row ID.
    pub id: i64,
    /// Page slug.
    pub slug: String,
    /// ISO date of the event.
    pub date: String,
    /// Source of the entry (conversation, meeting, manual, etc.).
    pub source: String,
    /// Short summary.
    pub summary: String,
    /// Detailed content.
    pub detail: String,
    /// Unix timestamp when this entry was created.
    pub created_at: i64,
}

/// A link between two brain pages.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LinkEntry {
    /// Source page slug.
    pub from_slug: String,
    /// Target page slug.
    pub to_slug: String,
    /// Link type (reference, works_at, invested_in, etc.).
    pub link_type: String,
    /// The line the link sits on, for backlink context.
    pub context: String,
    /// Whether `to_slug` names a page that exists.
    #[serde(default)]
    pub resolved: bool,
}

/// One frontmatter property, in file order.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Property {
    /// The key as written.
    pub key: String,
    /// The value as JSON.
    pub value: serde_json::Value,
}

/// A page rendered for the workspace: identity, properties, the rich text and what the
/// renderer learned, plus the raw file for the editor.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RenderedPage {
    /// The page slug.
    pub slug: String,
    /// The title (frontmatter, else the file name).
    pub title: String,
    /// The page type (frontmatter, else the folder's).
    pub page_type: String,
    /// Frontmatter properties in file order.
    pub properties: Vec<Property>,
    /// The raw file, for the source editor.
    pub raw: String,
    /// The page's file, absolute (TICKET-042); empty for text that is not a page.
    #[serde(default)]
    pub file: String,
    /// The body as typed blocks, when asked for (TICKET-036); their ranges are byte
    /// offsets into the body, which starts at `body_start` in `raw`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocks: Option<Vec<blocks::Block>>,
    /// Where the body starts in `raw`, given with `blocks`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_start: Option<usize>,
    /// The rendering.
    #[serde(flatten)]
    pub rendered: Rendered,
}

/// What a rename or move did.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RenameReport {
    /// The slug or folder as it was.
    pub from: String,
    /// The slug or folder as it is now.
    pub to: String,
    /// `page` or `folder`.
    pub kind: String,
    /// Pages whose links were rewritten.
    pub pages_rewritten: usize,
}

/// A tag with the number of pages carrying it (nested tags count for their parents).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TagCount {
    /// The tag as first written, without `#`.
    pub tag: String,
    /// Pages carrying it or a tag nested under it.
    pub count: usize,
}

/// What the graph should include.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct GraphOptions {
    /// Tags as nodes, with an edge from every page carrying them.
    #[serde(default)]
    pub tags: bool,
    /// Unresolved link targets as nodes.
    #[serde(default)]
    pub unresolved: bool,
    /// Keep only the neighbourhood of this page (a local graph).
    #[serde(default)]
    pub around: Option<String>,
    /// How many links away the neighbourhood reaches (default 1).
    #[serde(default)]
    pub depth: Option<usize>,
}

/// One node of the graph: a page, a tag or an unresolved target.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GraphNode {
    /// The slug for a page, `tag:<name>` for a tag, `new:<target>` for a missing page.
    pub id: String,
    /// `page`, `tag` or `unresolved`.
    pub kind: String,
    /// What to show: the page title, `#tag`, or the target as written.
    pub title: String,
    /// The page type, empty for the other kinds.
    pub page_type: String,
    /// The page's folder, empty at the root and for the other kinds.
    pub folder: String,
    /// The page's tags, for group queries.
    pub tags: Vec<String>,
}

fn default_edge_kind() -> String {
    "link".to_string()
}

/// One edge, from a page to a page, a tag or an unresolved target.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GraphEdge {
    /// The linking page's slug.
    pub from: String,
    /// The target node id.
    pub to: String,
    /// `link`, or a decision's `consulted`, `supersedes` or `follows_up`.
    #[serde(default = "default_edge_kind")]
    pub kind: String,
}

/// The vault as a graph.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Graph {
    /// Every node.
    pub nodes: Vec<GraphNode>,
    /// Every edge, deduplicated.
    pub edges: Vec<GraphEdge>,
}

/// A wikilink whose target is no page.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UnresolvedLink {
    /// The page holding the link.
    pub from_slug: String,
    /// The target as written.
    pub target: String,
    /// The line the link sits on.
    pub context: String,
}

/// Outbound links and backlinks for a page.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PageLinks {
    /// Links from this page to other pages.
    pub outbound: Vec<LinkEntry>,
    /// Links from other pages to this page.
    pub backlinks: Vec<LinkEntry>,
}

/// Brain statistics.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BrainStats {
    /// Total number of pages.
    pub page_count: i64,
    /// Total number of links.
    pub link_count: i64,
    /// Number of distinct tags.
    pub tag_count: i64,
    /// Total timeline entries.
    pub timeline_count: i64,
    /// Pages grouped by type.
    pub pages_by_type: std::collections::HashMap<String, i64>,
    /// The vault's folder, absolute (TICKET-042).
    #[serde(default)]
    pub vault_root: String,
}

/// Where a quick capture lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptureTarget {
    /// Today's daily page (`daily/YYYY-MM-DD`).
    Daily,
    /// The single inbox page (`inbox/inbox`).
    Inbox,
}

impl CaptureTarget {
    /// Parse `daily` or `inbox`.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_lowercase().as_str() {
            "daily" => Ok(Self::Daily),
            "inbox" => Ok(Self::Inbox),
            other => Err(format!(
                "capture target must be 'daily' or 'inbox', got '{other}'"
            )),
        }
    }
}

/// What a capture did: the page that took it and the timeline row it made.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CaptureReceipt {
    /// The page the entry went to.
    pub slug: String,
    /// The `brain_timeline` row id.
    pub entry_id: i64,
    /// Whether the page had to be created first.
    pub created_page: bool,
}

/// One page type, its vault folder, and how many pages it holds.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PageTypeInfo {
    /// The `type:` value pages carry.
    pub page_type: String,
    /// The folder under the vault root.
    pub dir: String,
    /// Pages of this type in the index.
    pub count: i64,
}

/// What a vault migration did, or would do with `dry_run`.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct MigrationReport {
    /// Nothing was written.
    pub dry_run: bool,
    /// Pages read.
    pub pages_scanned: usize,
    /// Pages whose text changed (or would change).
    pub pages_changed: usize,
    /// Pages whose bare `---` timeline rule became a `## Timeline` section.
    pub timelines_converted: usize,
    /// Wikilinks rewritten to vault paths.
    pub links_rewritten: usize,
    /// Links no page answers to, as `slug: [[target]]`; these are left as written.
    pub unresolved_links: Vec<String>,
    /// The slugs that changed.
    pub changed_slugs: Vec<String>,
}

/// Manages brain pages: CRUD, search, and sync between vault and SQLite index.
pub struct BrainManager {
    db: Arc<Database>,
    vault: VaultManager,
}

impl BrainManager {
    /// Create a new BrainManager.
    pub fn new(db: Arc<Database>, vault_path: PathBuf) -> Self {
        Self {
            db,
            vault: VaultManager::new(vault_path),
        }
    }

    /// Ensure all vault directories exist and default templates are created.
    pub fn ensure_vault(&self) -> Result<(), String> {
        self.vault.ensure_dirs()?;
        self.write_default_templates();
        Ok(())
    }

    /// Wait for pending vault git commits to finish.
    ///
    /// Brain writes auto-commit in a background thread; a short-lived process
    /// (rusty-cli) must call this before exit so the commit isn't dropped.
    pub fn flush_commits(&self) {
        self.vault.flush_commits();
    }

    /// The shared database handle, for components that need direct access to the
    /// same connection (e.g. the conversation archive).
    pub fn db(&self) -> Arc<crate::engine::db::Database> {
        Arc::clone(&self.db)
    }

    /// Create a new brain page.
    ///
    /// Generates a slug from the type and title, writes the markdown file,
    /// and indexes it in SQLite.
    pub fn create_page(
        &self,
        page_type: &str,
        title: &str,
        content: &str,
    ) -> Result<BrainPage, String> {
        let dir = type_to_dir(page_type)?;
        let base_slug = title_to_slug(title)?;
        let slug = self.unique_slug(dir, &base_slug)?;

        let fm = BrainFrontmatter::new(page_type, title);
        // Use template content if no content provided
        let page_content = if content.is_empty() {
            self.load_template(page_type).unwrap_or_default()
        } else {
            content.to_string()
        };
        let raw = render_page(&fm, &page_content, "")?;

        // Write to vault
        self.vault.write_page(&slug, &raw)?;

        // Index in SQLite
        let hash = compute_hash(&raw);
        let now = unix_now();
        self.index_page(&IndexEntry {
            slug: &slug,
            page_type,
            title,
            content: &page_content,
            hash: &hash,
            created_at: now,
            updated_at: now,
            properties: &properties_json(&raw),
            op: "created",
        })?;

        // Index aliases and tags (frontmatter and inline)
        self.sync_aliases(&slug, &fm.aliases)?;
        self.sync_tags(&slug, &fm.tags, &page_content)?;

        // Auto-commit
        self.vault
            .git_commit(&format!("create: {title} ({page_type})"));

        Ok(BrainPage {
            page_type: page_type.to_string(),
            title: title.to_string(),
            compiled_truth: page_content,
            timeline: String::new(),
            frontmatter: fm,
            content_hash: hash,
            created_at: now,
            updated_at: now,
            file: self.page_file(&slug),
            slug,
        })
    }

    /// Read a brain page by slug.
    ///
    /// Reads from the filesystem and parses frontmatter + content sections.
    pub fn read_page(&self, slug: &str) -> Result<Option<BrainPage>, String> {
        let raw = match self.vault.read_page(slug)? {
            Some(content) => content,
            None => return Ok(None),
        };

        let mut parsed = parse_lenient(&raw);
        parsed.frontmatter.fill_defaults(slug);
        let hash = compute_hash(&raw);

        // Get timestamps from index (fall back to 0 if not indexed)
        let (created_at, updated_at) = self.get_timestamps(slug).unwrap_or((0, 0));

        Ok(Some(BrainPage {
            slug: slug.to_string(),
            page_type: parsed.frontmatter.page_type.clone(),
            title: parsed.frontmatter.title.clone(),
            compiled_truth: parsed.compiled_truth,
            timeline: parsed.timeline,
            frontmatter: parsed.frontmatter,
            content_hash: hash,
            created_at,
            updated_at,
            file: self.page_file(slug),
        }))
    }

    /// Update a brain page's content.
    ///
    /// Snapshots the current version before overwriting.
    pub fn update_page(&self, slug: &str, content: &str) -> Result<BrainPage, String> {
        // Read current state
        let current_raw = self
            .vault
            .read_page(slug)?
            .ok_or_else(|| format!("Page not found: {slug}"))?;

        // Snapshot current version
        let mut current_parsed = parse_lenient(&current_raw);
        current_parsed.frontmatter.fill_defaults(slug);
        self.create_version(slug, &current_raw, &current_parsed.frontmatter)?;

        // The new body under the frontmatter as written, with only `updated` moved; a
        // page without frontmatter keeps having none (TICKET-046).
        let mut fm = current_parsed.frontmatter;
        let today = today_iso();
        let next = frontmatter::replace_body(
            &current_raw,
            &render_body(content, &current_parsed.timeline),
        );
        let raw = frontmatter::set_scalar_line(&next, "updated", &today)?.unwrap_or(next);
        fm.updated = today;
        self.vault.write_page(slug, &raw)?;

        // Re-index
        let hash = compute_hash(&raw);
        let now = unix_now();
        let created = self.get_timestamps(slug).map(|(c, _)| c).unwrap_or(now);
        self.remove_from_index(slug)?;
        let full_content = if current_parsed.timeline.is_empty() {
            content.to_string()
        } else {
            format!("{content}\n\n{}", current_parsed.timeline)
        };
        self.index_page(&IndexEntry {
            slug,
            page_type: &fm.page_type,
            title: &fm.title,
            content: &full_content,
            hash: &hash,
            created_at: created,
            updated_at: now,
            properties: &properties_json(&raw),
            op: "updated",
        })?;

        // Re-sync aliases and tags
        self.sync_aliases(slug, &fm.aliases)?;
        self.sync_tags(slug, &fm.tags, &full_content)?;

        // Auto-commit
        self.vault.git_commit(&format!("update: {}", fm.title));

        Ok(BrainPage {
            slug: slug.to_string(),
            page_type: fm.page_type.clone(),
            title: fm.title.clone(),
            compiled_truth: content.to_string(),
            timeline: current_parsed.timeline,
            frontmatter: fm,
            content_hash: hash,
            created_at: created,
            updated_at: now,
            file: self.page_file(slug),
        })
    }

    /// Soft-delete a brain page (move to archive/, remove from index).
    pub fn delete_page(&self, slug: &str) -> Result<(), String> {
        self.vault.delete_page(slug)?;
        self.forget_page(slug)?;
        self.vault.git_commit(&format!("delete: {slug}"));
        // After the delete's own commit, so it keeps its paths (TICKET-037).
        // The page is already archived; a bookmarks file that cannot be read must not
        // turn that into a failure the caller would retry against a missing page.
        if let Err(e) = self.drop_bookmarks(slug, false) {
            eprintln!("rusty: bookmarks of {slug} left as they are: {e}");
        }
        Ok(())
    }

    /// The vault's folder, absolute: canonical when it can be resolved, else made
    /// absolute against the working directory (TICKET-042). The `brain_vault_path`
    /// setting may hold a relative path; a client is never left to resolve it.
    pub fn vault_root(&self) -> PathBuf {
        let root = self.vault.root();
        root.canonicalize()
            .or_else(|_| std::path::absolute(root))
            .unwrap_or_else(|_| root.to_path_buf())
    }

    /// The absolute file of the page `slug`, whether or not it exists yet.
    pub fn page_file(&self, slug: &str) -> String {
        self.vault_root()
            .join(format!("{slug}.md"))
            .to_string_lossy()
            .into_owned()
    }

    /// List brain pages with optional type filter, newest first, each with its aliases.
    pub fn list_pages(
        &self,
        page_type: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<BrainPageSummary>, String> {
        self.list_pages_with(page_type, limit, &[])
    }

    /// [`Self::list_pages`], and for each page the values of the properties named in
    /// `properties` that it has, read from the index (TICKET-047).
    pub fn list_pages_with(
        &self,
        page_type: Option<&str>,
        limit: Option<usize>,
        properties: &[String],
    ) -> Result<Vec<BrainPageSummary>, String> {
        let lim = limit.unwrap_or(50) as i64;
        type Row = (String, String, String, i64, Option<String>);
        let rows: Vec<Row> = {
            let conn = self.db.conn()?;
            let mut stmt = conn
                .prepare(
                    "SELECT slug, page_type, title, updated_at, frontmatter FROM brain_pages \
                     WHERE ?1 IS NULL OR page_type = ?1 ORDER BY updated_at DESC LIMIT ?2",
                )
                .map_err(|e| format!("Query error: {e}"))?;
            let rows: Vec<Row> = stmt
                .query_map(rusqlite::params![page_type, lim], |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                })
                .map_err(|e| format!("Query error: {e}"))?
                .flatten()
                .collect();
            rows
        };
        let mut aliases: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        {
            let conn = self.db.conn()?;
            let mut stmt = conn
                .prepare("SELECT slug, alias FROM brain_aliases ORDER BY rowid")
                .map_err(|e| format!("Query error: {e}"))?;
            let pairs = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|e| format!("Query error: {e}"))?;
            for (slug, alias) in pairs.flatten() {
                aliases.entry(slug).or_default().push(alias);
            }
        }
        Ok(rows
            .into_iter()
            .map(|(slug, page_type, title, updated_at, stored)| {
                let properties = (!properties.is_empty()).then(|| {
                    let stored: serde_json::Map<String, serde_json::Value> = stored
                        .and_then(|s| serde_json::from_str(&s).ok())
                        .unwrap_or_default();
                    properties
                        .iter()
                        .filter_map(|k| stored.get(k).map(|v| (k.clone(), v.clone())))
                        .collect()
                });
                BrainPageSummary {
                    aliases: aliases.remove(&slug).unwrap_or_default(),
                    slug,
                    page_type,
                    title,
                    updated_at,
                    properties,
                }
            })
            .collect())
    }

    /// Full-text search across brain pages using FTS5, with the operators of
    /// [`parse_query`] (`tag:`, `path:`, `file:`, `type:`, a leading `-` excluding); a
    /// query of operator terms alone lists the pages they admit, newest first.
    pub fn search(
        &self,
        query: &str,
        limit: Option<usize>,
        page_type: Option<&str>,
    ) -> Result<Vec<BrainSearchResult>, String> {
        self.search_with(
            query,
            &SearchOptions {
                limit,
                page_type: page_type.map(str::to_string),
                ..Default::default()
            },
        )
    }

    /// Search with operators and modes: the operator terms of [`parse_query`] narrow
    /// the pages; the words match through FTS5, as typed when `case_sensitive`, or as a
    /// pattern over the page text when `regex`. Operator terms alone list the pages
    /// they admit, newest first.
    pub fn search_with(
        &self,
        query: &str,
        options: &SearchOptions,
    ) -> Result<Vec<BrainSearchResult>, String> {
        let q = parse_query(query);
        let limit = options.limit.unwrap_or(10);
        let page_type = options.page_type.as_deref();
        let allowed = self.allowed_pages(&q)?;
        if q.words.is_empty() {
            return match allowed {
                Some(set) => self.pages_in(&set, Some(limit), page_type),
                None => Ok(Vec::new()),
            };
        }
        if options.regex {
            return self.search_regex(&q.words, allowed.as_ref(), options);
        }
        let wide = if allowed.is_some() || options.case_sensitive {
            limit.saturating_mul(10).max(50)
        } else {
            limit
        };
        let mut hits = self.search_text(&q.words, Some(wide), page_type)?;
        if let Some(set) = &allowed {
            hits.retain(|r| set.contains(&r.slug));
        }
        if options.case_sensitive {
            hits = self.keep_case(hits, &q.words)?;
        }
        hits.truncate(limit);
        Ok(hits)
    }

    /// The slugs the operator terms admit, `None` when the query has none.
    fn allowed_pages(
        &self,
        q: &ParsedQuery,
    ) -> Result<Option<std::collections::HashSet<String>>, String> {
        if !q.has_operators() {
            return Ok(None);
        }
        let lower = |v: &[String]| -> Vec<String> { v.iter().map(|s| s.to_lowercase()).collect() };
        let (paths, not_paths) = (lower(&q.paths), lower(&q.not_paths));
        let (files, not_files) = (lower(&q.files), lower(&q.not_files));
        let (types, not_types) = (lower(&q.types), lower(&q.not_types));
        let mut set: std::collections::HashSet<String> = self
            .list_pages(None, Some(100_000))?
            .into_iter()
            .filter(|p| {
                let slug = p.slug.to_lowercase();
                let name = slug.rsplit('/').next().unwrap_or_default().to_string();
                let kind = p.page_type.to_lowercase();
                paths.iter().all(|x| slug.contains(x.as_str()))
                    && !not_paths.iter().any(|x| slug.contains(x.as_str()))
                    && files.iter().all(|x| name.contains(x.as_str()))
                    && !not_files.iter().any(|x| name.contains(x.as_str()))
                    && (types.is_empty() || types.contains(&kind))
                    && !not_types.contains(&kind)
            })
            .map(|p| p.slug)
            .collect();
        if !q.tags.is_empty() {
            let tagged = self.pages_with_tags(&q.tags)?;
            set.retain(|s| tagged.contains(s));
        }
        for tag in &q.not_tags {
            let tagged = self.pages_with_tags(std::slice::from_ref(tag))?;
            set.retain(|s| !tagged.contains(s));
        }
        Ok(Some(set))
    }

    /// The hits whose title or text holds every word as typed; the snippet is rebuilt
    /// around the first word's first occurrence.
    fn keep_case(
        &self,
        hits: Vec<BrainSearchResult>,
        words: &str,
    ) -> Result<Vec<BrainSearchResult>, String> {
        use rusqlite::OptionalExtension;
        let needles: Vec<String> = query_tokens(words)
            .into_iter()
            .map(|t| t.trim_matches('"').trim_end_matches('*').to_string())
            .filter(|t| !t.is_empty())
            .collect();
        let conn = self.db.conn()?;
        let mut out = Vec::new();
        for mut hit in hits {
            let row: Option<(String, String)> = conn
                .query_row(
                    "SELECT title, content FROM brain_fts WHERE slug = ?1",
                    rusqlite::params![hit.slug],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(|e| format!("Query error: {e}"))?;
            let Some((title, content)) = row else {
                continue;
            };
            let text = format!("{title}\n{content}");
            if !needles.iter().all(|n| text.contains(n.as_str())) {
                continue;
            }
            if let Some(range) = needles
                .first()
                .and_then(|n| text.find(n.as_str()).map(|i| i..i + n.len()))
            {
                hit.snippet = snippet_around(&text, range);
            }
            out.push(hit);
        }
        Ok(out)
    }

    /// Pages whose title or text matches the pattern, ranked by the number of matches,
    /// with a snippet around the first.
    fn search_regex(
        &self,
        pattern: &str,
        allowed: Option<&std::collections::HashSet<String>>,
        options: &SearchOptions,
    ) -> Result<Vec<BrainSearchResult>, String> {
        type TextRow = (String, String, String, String);
        let re = regex::RegexBuilder::new(pattern)
            .case_insensitive(!options.case_sensitive)
            .size_limit(1 << 20)
            .build()
            .map_err(|e| format!("Bad pattern: {e}"))?;
        let mut rows: Vec<TextRow> = Vec::new();
        {
            let conn = self.db.conn()?;
            let mut stmt = conn
                .prepare(
                    "SELECT f.slug, f.page_type, p.title, f.content FROM brain_fts f \
                     JOIN brain_pages p ON p.slug = f.slug ORDER BY p.updated_at DESC",
                )
                .map_err(|e| format!("Query error: {e}"))?;
            let found = stmt
                .query_map([], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                })
                .map_err(|e| format!("Query error: {e}"))?;
            for row in found.flatten() {
                rows.push(row);
            }
        }
        let mut out = Vec::new();
        for (slug, page_type, title, content) in rows {
            if options.page_type.as_deref().is_some_and(|t| t != page_type)
                || allowed.is_some_and(|set| !set.contains(&slug))
            {
                continue;
            }
            let text = format!("{title}\n{content}");
            let mut count = 0usize;
            let mut first = None;
            for m in re.find_iter(&text) {
                count += 1;
                if first.is_none() {
                    first = Some(m.range());
                }
            }
            let Some(range) = first else {
                continue;
            };
            out.push(BrainSearchResult {
                slug,
                page_type,
                title,
                snippet: snippet_around(&text, range),
                rank: -(count as f64),
            });
        }
        out.sort_by(|a, b| a.rank.total_cmp(&b.rank));
        out.truncate(options.limit.unwrap_or(10));
        Ok(out)
    }

    /// The slugs carrying every tag in `tags` (or a tag nested under each).
    fn pages_with_tags(
        &self,
        tags: &[String],
    ) -> Result<std::collections::HashSet<String>, String> {
        let conn = self.db.conn()?;
        let mut allowed: Option<std::collections::HashSet<String>> = None;
        for tag in tags {
            let lower = tag.to_lowercase();
            let mut stmt = conn
                .prepare(
                    "SELECT DISTINCT slug FROM brain_tags \
                     WHERE LOWER(tag) = ?1 OR LOWER(tag) LIKE ?1 || '/%'",
                )
                .map_err(|e| format!("Query error: {e}"))?;
            let set: std::collections::HashSet<String> = stmt
                .query_map(rusqlite::params![lower], |row| row.get(0))
                .map_err(|e| format!("Query error: {e}"))?
                .flatten()
                .collect();
            allowed = Some(match allowed {
                Some(prev) => prev.intersection(&set).cloned().collect(),
                None => set,
            });
        }
        Ok(allowed.unwrap_or_default())
    }

    /// Search results for a set of slugs, newest first, with no snippet.
    fn pages_in(
        &self,
        slugs: &std::collections::HashSet<String>,
        limit: Option<usize>,
        page_type: Option<&str>,
    ) -> Result<Vec<BrainSearchResult>, String> {
        let mut pages: Vec<BrainPageSummary> = self
            .list_pages(page_type, Some(100_000))?
            .into_iter()
            .filter(|p| slugs.contains(&p.slug))
            .collect();
        pages.truncate(limit.unwrap_or(10));
        Ok(pages
            .into_iter()
            .map(|p| BrainSearchResult {
                slug: p.slug,
                page_type: p.page_type,
                title: p.title,
                snippet: String::new(),
                rank: 0.0,
            })
            .collect())
    }

    /// The vault as a graph: pages and their resolved links, plus tags and unresolved
    /// targets when asked, or only the neighbourhood of one page to a depth.
    pub fn graph(&self, options: &GraphOptions) -> Result<Graph, String> {
        use std::collections::{HashMap, HashSet, VecDeque};
        type PageRow = (String, String, String);
        type LinkRow = (String, String, bool);
        type TagRow = (String, String);
        let (pages, links, tags): (Vec<PageRow>, Vec<LinkRow>, Vec<TagRow>) = {
            let conn = self.db.conn()?;
            let mut stmt = conn
                .prepare("SELECT slug, title, page_type FROM brain_pages ORDER BY slug")
                .map_err(|e| format!("Query error: {e}"))?;
            let pages = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                .map_err(|e| format!("Query error: {e}"))?
                .flatten()
                .collect();
            let mut stmt = conn
                .prepare(
                    "SELECT l.from_slug, l.to_slug, p.slug IS NOT NULL FROM brain_links l \
                     LEFT JOIN brain_pages p ON p.slug = l.to_slug ORDER BY l.from_slug, l.to_slug",
                )
                .map_err(|e| format!("Query error: {e}"))?;
            let links = stmt
                .query_map([], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get::<_, i64>(2)? != 0))
                })
                .map_err(|e| format!("Query error: {e}"))?
                .flatten()
                .collect();
            let mut stmt = conn
                .prepare("SELECT slug, tag FROM brain_tags ORDER BY slug, tag")
                .map_err(|e| format!("Query error: {e}"))?;
            let tags = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .map_err(|e| format!("Query error: {e}"))?
                .flatten()
                .collect();
            (pages, links, tags)
        };
        let mut tags_of: HashMap<String, Vec<String>> = HashMap::new();
        for (slug, tag) in &tags {
            tags_of.entry(slug.clone()).or_default().push(tag.clone());
        }
        let mut nodes: Vec<GraphNode> = Vec::new();
        let mut known: HashSet<String> = HashSet::new();
        for (slug, title, page_type) in &pages {
            known.insert(slug.clone());
            nodes.push(GraphNode {
                id: slug.clone(),
                kind: "page".to_string(),
                title: title.clone(),
                page_type: page_type.clone(),
                folder: slug
                    .rfind('/')
                    .map(|i| slug[..i].to_string())
                    .unwrap_or_default(),
                tags: tags_of.get(slug).cloned().unwrap_or_default(),
            });
        }
        let mut edges: Vec<GraphEdge> = Vec::new();
        let mut seen_edges: HashSet<(String, String)> = HashSet::new();
        let mut push_edge = |from: &str, to: &str, kind: &str, edges: &mut Vec<GraphEdge>| {
            if seen_edges.insert((from.to_string(), to.to_string())) {
                edges.push(GraphEdge {
                    from: from.to_string(),
                    to: to.to_string(),
                    kind: kind.to_string(),
                });
            }
        };
        // A decision's typed edges first, so they win over the plain links its body holds.
        for (from, to, kind) in self.decision_edges()? {
            if known.contains(&from) && known.contains(&to) {
                push_edge(&from, &to, &kind, &mut edges);
            }
        }
        for (from, to, resolved) in &links {
            if !known.contains(from) {
                continue;
            }
            if *resolved {
                push_edge(from, to, "link", &mut edges);
            } else if options.unresolved {
                let id = format!("new:{to}");
                if known.insert(id.clone()) {
                    nodes.push(GraphNode {
                        id: id.clone(),
                        kind: "unresolved".to_string(),
                        title: to.clone(),
                        page_type: String::new(),
                        folder: String::new(),
                        tags: Vec::new(),
                    });
                }
                push_edge(from, &id, "link", &mut edges);
            }
        }
        if options.tags {
            for (slug, tag) in &tags {
                if !known.contains(slug) {
                    continue;
                }
                let id = format!("tag:{}", tag.to_lowercase());
                if known.insert(id.clone()) {
                    nodes.push(GraphNode {
                        id: id.clone(),
                        kind: "tag".to_string(),
                        title: format!("#{tag}"),
                        page_type: String::new(),
                        folder: String::new(),
                        tags: Vec::new(),
                    });
                }
                push_edge(slug, &id, "link", &mut edges);
            }
        }
        let Some(around) = options
            .around
            .as_deref()
            .map(str::trim)
            .filter(|a| !a.is_empty())
        else {
            return Ok(Graph { nodes, edges });
        };
        // The neighbourhood: breadth first over the undirected edges.
        let depth = options.depth.unwrap_or(1).max(1);
        let mut adjacent: HashMap<&str, Vec<&str>> = HashMap::new();
        for e in &edges {
            adjacent
                .entry(e.from.as_str())
                .or_default()
                .push(e.to.as_str());
            adjacent
                .entry(e.to.as_str())
                .or_default()
                .push(e.from.as_str());
        }
        let mut keep: HashSet<String> = HashSet::new();
        if known.contains(around) {
            let mut queue: VecDeque<(&str, usize)> = VecDeque::new();
            queue.push_back((around, 0));
            keep.insert(around.to_string());
            while let Some((id, d)) = queue.pop_front() {
                if d >= depth {
                    continue;
                }
                for next in adjacent.get(id).into_iter().flatten() {
                    if keep.insert((*next).to_string()) {
                        queue.push_back((next, d + 1));
                    }
                }
            }
        }
        Ok(Graph {
            nodes: nodes.into_iter().filter(|n| keep.contains(&n.id)).collect(),
            edges: edges
                .into_iter()
                .filter(|e| keep.contains(&e.from) && keep.contains(&e.to))
                .collect(),
        })
    }

    /// Every tag with its page count, nested tags counted under each parent, sorted.
    pub fn tags(&self) -> Result<Vec<TagCount>, String> {
        let rows: Vec<(String, String)> = {
            let conn = self.db.conn()?;
            let mut stmt = conn
                .prepare("SELECT slug, tag FROM brain_tags")
                .map_err(|e| format!("Query error: {e}"))?;
            let rows = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .map_err(|e| format!("Query error: {e}"))?
                .flatten()
                .collect();
            rows
        };
        // lowercase tag → (as first written, the slugs)
        let mut by_tag: std::collections::BTreeMap<
            String,
            (String, std::collections::HashSet<String>),
        > = std::collections::BTreeMap::new();
        for (slug, tag) in rows {
            let parts: Vec<&str> = tag.split('/').filter(|p| !p.is_empty()).collect();
            for depth in 1..=parts.len() {
                let name = parts[..depth].join("/");
                let entry = by_tag
                    .entry(name.to_lowercase())
                    .or_insert_with(|| (name.clone(), std::collections::HashSet::new()));
                entry.1.insert(slug.clone());
            }
        }
        Ok(by_tag
            .into_values()
            .map(|(tag, slugs)| TagCount {
                tag,
                count: slugs.len(),
            })
            .collect())
    }

    /// Set one frontmatter property (a JSON value) on a page; the other keys keep their
    /// order and the body its bytes. The previous text is kept as a version.
    pub fn set_property(
        &self,
        slug: &str,
        key: &str,
        value: serde_json::Value,
    ) -> Result<BrainPage, String> {
        let raw = self
            .vault
            .read_page(slug)?
            .ok_or_else(|| format!("Page not found: {slug}"))?;
        let next = frontmatter::set_property(&raw, key, value)?;
        self.write_edited(slug, &raw, &next, &format!("property: {key} on {slug}"))
    }

    /// Remove one frontmatter property from a page.
    pub fn remove_property(&self, slug: &str, key: &str) -> Result<BrainPage, String> {
        let raw = self
            .vault
            .read_page(slug)?
            .ok_or_else(|| format!("Page not found: {slug}"))?;
        let next = frontmatter::remove_property(&raw, key)?;
        self.write_edited(
            slug,
            &raw,
            &next,
            &format!("property: remove {key} from {slug}"),
        )
    }

    /// Write a changed page: version, file, index, commit; unchanged text is a no-op.
    fn write_edited(
        &self,
        slug: &str,
        raw: &str,
        next: &str,
        message: &str,
    ) -> Result<BrainPage, String> {
        if next == raw {
            return self
                .read_page(slug)?
                .ok_or_else(|| format!("Page not found: {slug}"));
        }
        let mut parsed = parse_lenient(raw);
        parsed.frontmatter.fill_defaults(slug);
        self.create_version(slug, raw, &parsed.frontmatter)?;
        self.vault.write_page(slug, next)?;
        self.sync_page(slug)?;
        let page = self
            .read_page(slug)?
            .ok_or_else(|| format!("Page not found after write: {slug}"))?;
        self.vault.git_commit(message);
        Ok(page)
    }

    /// The FTS5 half of [`Self::search`], the query as plain words.
    fn search_text(
        &self,
        query: &str,
        limit: Option<usize>,
        page_type: Option<&str>,
    ) -> Result<Vec<BrainSearchResult>, String> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.db.conn()?;
        let lim = limit.unwrap_or(10) as i64;

        let mut results = Vec::new();

        if let Some(pt) = page_type {
            let mut stmt = conn
                .prepare(
                    "SELECT f.slug, f.page_type, p.title, \
                     snippet(brain_fts, 2, '<b>', '</b>', '...', 32) as snip, \
                     rank \
                     FROM brain_fts f \
                     JOIN brain_pages p ON p.slug = f.slug \
                     WHERE brain_fts MATCH ?1 AND f.page_type = ?2 \
                     ORDER BY rank LIMIT ?3",
                )
                .map_err(|e| format!("Search query error: {e}"))?;
            let rows = stmt
                .query_map(rusqlite::params![query, pt, lim], row_to_search_result)
                .map_err(|e| format!("Search error: {e}"))?;
            for r in rows.flatten() {
                results.push(r);
            }
        } else {
            let mut stmt = conn
                .prepare(
                    "SELECT f.slug, f.page_type, p.title, \
                     snippet(brain_fts, 2, '<b>', '</b>', '...', 32) as snip, \
                     rank \
                     FROM brain_fts f \
                     JOIN brain_pages p ON p.slug = f.slug \
                     WHERE brain_fts MATCH ?1 \
                     ORDER BY rank LIMIT ?2",
                )
                .map_err(|e| format!("Search query error: {e}"))?;
            let rows = stmt
                .query_map(rusqlite::params![query, lim], row_to_search_result)
                .map_err(|e| format!("Search error: {e}"))?;
            for r in rows.flatten() {
                results.push(r);
            }
        }

        Ok(results)
    }

    /// Sync a single page from the filesystem into the SQLite index.
    #[allow(dead_code)] // Public API for future phases (file watcher, manual sync)
    pub fn sync_page(&self, slug: &str) -> Result<(), String> {
        let raw = self
            .vault
            .read_page(slug)?
            .ok_or_else(|| format!("Page file not found: {slug}"))?;

        let hash = compute_hash(&raw);
        let mut parsed = parse_lenient(&raw);
        parsed.frontmatter.fill_defaults(slug);

        // Already indexed with the same hash: only the link rows are refreshed, so rows
        // written by an older scanner (raw targets, no context) catch up.
        let known = self.get_content_hash(slug);
        let op = if known.is_some() {
            "updated"
        } else {
            "created"
        };
        if let Some(existing_hash) = known {
            if existing_hash == hash {
                let full_content = if parsed.timeline.is_empty() {
                    parsed.compiled_truth.clone()
                } else {
                    format!("{}\n\n{}", parsed.compiled_truth, parsed.timeline)
                };
                let conn = self.db.conn()?;
                // A row indexed before TICKET-047 holds `{}`; fill it in.
                conn.execute(
                    "UPDATE brain_pages SET frontmatter = ?1 WHERE slug = ?2 AND frontmatter IS NOT ?1",
                    rusqlite::params![properties_json(&raw), slug],
                )
                .map_err(|e| format!("Failed to store properties: {e}"))?;
                return index_links(&conn, slug, &full_content);
            }
        }
        let now = unix_now();
        let created = self.get_timestamps(slug).map(|(c, _)| c).unwrap_or(now);

        let full_content = if parsed.timeline.is_empty() {
            parsed.compiled_truth.clone()
        } else {
            format!("{}\n\n{}", parsed.compiled_truth, parsed.timeline)
        };

        self.remove_from_index(slug)?;
        self.index_page(&IndexEntry {
            slug,
            page_type: &parsed.frontmatter.page_type,
            title: &parsed.frontmatter.title,
            content: &full_content,
            hash: &hash,
            created_at: created,
            updated_at: now,
            properties: &properties_json(&raw),
            op,
        })?;

        self.sync_aliases(slug, &parsed.frontmatter.aliases)?;
        self.sync_tags(slug, &parsed.frontmatter.tags, &full_content)?;

        Ok(())
    }

    /// Sync all files in the vault to the SQLite index.
    ///
    /// Indexes new/changed files and removes orphan rows for deleted files.
    #[allow(dead_code)] // Public API for future phases (full vault rebuild)
    pub fn sync_all(&self) -> Result<usize, String> {
        let files = self.vault.list_all_files()?;
        let mut synced = 0;

        // Collect valid slugs
        let mut valid_slugs: Vec<String> = Vec::new();

        // One unreadable file (not UTF-8, gone mid-walk) is reported and skipped; it must
        // not stop the rest of the vault from being indexed.
        for (slug, _path) in &files {
            valid_slugs.push(slug.clone());
            match self.sync_page(slug) {
                Ok(_) => synced += 1,
                Err(e) => eprintln!("rusty: vault sync: skipped {slug}: {e}"),
            }
        }

        // Remove orphan index rows (pages in DB but not on disk). The connection guard
        // is scoped so it is released before `remove_from_index` takes its own; the DB
        // is one `Mutex<Connection>`, and holding it across that call deadlocked the
        // first reindex that ever met an orphan (2026-09-02).
        let indexed_slugs: Vec<String> = {
            let conn = self.db.conn()?;
            let mut stmt = conn
                .prepare("SELECT slug FROM brain_pages")
                .map_err(|e| format!("Query error: {e}"))?;
            let slugs = stmt
                .query_map([], |row| row.get(0))
                .map_err(|e| format!("Query error: {e}"))?
                .flatten()
                .collect();
            slugs
        };
        for slug in &indexed_slugs {
            // A page written since the walk listed the files (by this process or another)
            // is indexed but not in the list: look again before forgetting it.
            let still_there = !vault::in_archive(slug)
                && !slug.split('/').any(|part| part.starts_with('.'))
                && self.vault.page_exists(slug);
            if !valid_slugs.contains(slug) && !still_there {
                self.forget_page(slug)?;
            }
        }
        // Link rows whose source page left the index another way (TICKET-040).
        {
            let conn = self.db.conn()?;
            conn.execute(
                "DELETE FROM brain_links WHERE from_slug NOT IN (SELECT slug FROM brain_pages)",
                [],
            )
            .map_err(|e| format!("Failed to drop links from missing pages: {e}"))?;
        }
        self.resolve_pending_links()?;

        Ok(synced)
    }

    /// Give unresolved link rows another chance: a page indexed after the page that
    /// links to it by bare name resolves now.
    fn resolve_pending_links(&self) -> Result<(), String> {
        let conn = self.db.conn()?;
        let pending: Vec<(i64, String)> = {
            let mut stmt = conn
                .prepare(
                    "SELECT l.id, l.to_slug FROM brain_links l \
                     LEFT JOIN brain_pages p ON p.slug = l.to_slug WHERE p.slug IS NULL",
                )
                .map_err(|e| format!("Query error: {e}"))?;
            let rows = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .map_err(|e| format!("Query error: {e}"))?
                .flatten()
                .collect();
            rows
        };
        for (id, target) in pending {
            if let Some(slug) = resolve_on(&conn, &target) {
                conn.execute(
                    "UPDATE OR IGNORE brain_links SET to_slug = ?1 WHERE id = ?2",
                    rusqlite::params![slug, id],
                )
                .map_err(|e| format!("Failed to resolve link: {e}"))?;
            }
        }
        Ok(())
    }

    // ── Timeline ───────────────────────────────────────────────────────

    /// Add a timeline entry to a brain page.
    ///
    /// Writes through to **all live layers**, not just SQLite. The entry is
    /// (1) inserted into the `brain_timeline` table for structured queries,
    /// (2) appended to the page's markdown timeline section on disk, and
    /// (3) re-indexed into FTS + the wiki-link graph and git-committed. Step (2)
    /// also trips the vault file-watcher, refreshing the live GUI.
    ///
    /// This closes the historical "wrote to the DB but never pushed it live" gap:
    /// captures via `/capture`, `/today`, `brain_add_timeline`, and conversation
    /// enrichment used to land only in `brain_timeline`, invisible to `brain read`,
    /// `brain search`, the GUI, and git. They are now durable and searchable.
    pub fn add_timeline(
        &self,
        slug: &str,
        date: &str,
        source: &str,
        summary: &str,
        detail: Option<&str>,
    ) -> Result<i64, String> {
        // Read the current page — this both verifies existence and gives us the
        // markdown sections to append to.
        let current_raw = self
            .vault
            .read_page(slug)?
            .ok_or_else(|| format!("Page not found: {slug}"))?;
        let mut parsed = parse_lenient(&current_raw);
        parsed.frontmatter.fill_defaults(slug);

        // 1. Structured row in brain_timeline. Scope the connection guard so it is
        //    dropped before the index helpers below acquire their own connection
        //    (the DB is a single Mutex<Connection>; holding two would deadlock).
        let now = unix_now();
        let id = {
            let conn = self.db.conn()?;
            conn.execute(
                "INSERT INTO brain_timeline (slug, entry_date, source, summary, detail, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![slug, date, source, summary, detail.unwrap_or(""), now],
            )
            .map_err(|e| format!("Failed to add timeline entry: {e}"))?;
            conn.last_insert_rowid()
        };

        // 2. Append the entry to the markdown timeline section so it shows up in
        //    `brain read`/the GUI and is searchable via FTS.
        let entry_md = Self::format_timeline_entry(date, source, summary, detail);
        let new_timeline = if parsed.timeline.trim().is_empty() {
            entry_md
        } else {
            format!("{}\n\n{entry_md}", parsed.timeline.trim_end())
        };
        // The body alone is rewritten; the frontmatter stays as written (TICKET-046).
        let raw = frontmatter::replace_body(
            &current_raw,
            &render_body(&parsed.compiled_truth, &new_timeline),
        );
        self.vault.write_page(slug, &raw)?;

        // 3. Refresh the FTS row + page hash so the captured text is searchable,
        //    and additively record any [[wiki-links]] in the timeline. Unlike
        //    index_page (used by create/update) this does NOT rebuild the page's
        //    whole link set — appending an entry must not delete links the page
        //    already has (e.g. explicit add_link edges or compiled-truth links).
        let hash = compute_hash(&raw);
        let full_content = if parsed.compiled_truth.trim().is_empty() {
            new_timeline.clone()
        } else {
            format!("{}\n\n{new_timeline}", parsed.compiled_truth)
        };
        {
            let conn = self.db.conn()?;
            conn.execute(
                "DELETE FROM brain_fts WHERE slug = ?1",
                rusqlite::params![slug],
            )
            .map_err(|e| format!("Failed to refresh FTS: {e}"))?;
            conn.execute(
                "INSERT INTO brain_fts (slug, title, content, page_type) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    slug,
                    parsed.frontmatter.title,
                    full_content,
                    parsed.frontmatter.page_type
                ],
            )
            .map_err(|e| format!("Failed to index FTS: {e}"))?;
            conn.execute(
                "UPDATE brain_pages SET content_hash = ?1, updated_at = ?2 WHERE slug = ?3",
                rusqlite::params![hash, now, slug],
            )
            .map_err(|e| format!("Failed to update page index: {e}"))?;
            changes::record(&conn, "page", slug, "updated", "", &hash);

            // Additive (INSERT OR IGNORE) so existing links are preserved.
            for link in scan_links(&new_timeline) {
                if link.target.is_empty() {
                    continue;
                }
                let to_slug = resolve_on(&conn, &link.target)
                    .unwrap_or_else(|| links::normalise_target(&link.target));
                if to_slug != slug {
                    conn.execute(
                        "INSERT OR IGNORE INTO brain_links (from_slug, to_slug, link_type, context, created_at) \
                         VALUES (?1, ?2, 'reference', ?3, ?4)",
                        rusqlite::params![slug, to_slug, link.line, now],
                    )
                    .map_err(|e| format!("Failed to insert wiki link: {e}"))?;
                }
            }
        }

        // 4. Auto-commit (fire-and-forget) so the capture is durable in git.
        self.vault.git_commit(&format!("timeline: {slug} ({date})"));

        Ok(id)
    }

    /// Format a timeline entry as a markdown bullet for a page's timeline section.
    ///
    /// `- **<date>** (<source>) — <summary>` with optional indented detail lines.
    fn format_timeline_entry(
        date: &str,
        source: &str,
        summary: &str,
        detail: Option<&str>,
    ) -> String {
        let mut entry = if source.trim().is_empty() {
            format!("- **{date}** — {summary}")
        } else {
            format!("- **{date}** ({source}) — {summary}")
        };
        if let Some(d) = detail {
            let d = d.trim();
            if !d.is_empty() {
                // Indent continuation lines so they render under the bullet.
                let indented = d.replace('\n', "\n  ");
                entry.push_str("\n  ");
                entry.push_str(&indented);
            }
        }
        entry
    }

    /// Get timeline entries for a brain page, ordered by date descending.
    pub fn get_timeline(
        &self,
        slug: &str,
        limit: Option<usize>,
    ) -> Result<Vec<TimelineEntry>, String> {
        let conn = self.db.conn()?;
        let lim = limit.unwrap_or(50) as i64;

        let mut stmt = conn
            .prepare(
                "SELECT id, slug, entry_date, source, summary, detail, created_at \
                 FROM brain_timeline WHERE slug = ?1 \
                 ORDER BY entry_date DESC, id DESC LIMIT ?2",
            )
            .map_err(|e| format!("Query error: {e}"))?;

        let rows = stmt
            .query_map(rusqlite::params![slug, lim], |row| {
                Ok(TimelineEntry {
                    id: row.get(0)?,
                    slug: row.get(1)?,
                    date: row.get(2)?,
                    source: row.get(3)?,
                    summary: row.get(4)?,
                    detail: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })
            .map_err(|e| format!("Query error: {e}"))?;

        let mut entries = Vec::new();
        for entry in rows.flatten() {
            entries.push(entry);
        }
        Ok(entries)
    }

    // ── Daily pages, capture, types ────────────────────────────────────

    /// Today's daily page, or the one for `date` (`YYYY-MM-DD`), created when missing.
    pub fn daily_page(&self, date: Option<&str>) -> Result<BrainPage, String> {
        let date = normalize_date(date)?;
        let slug = format!("daily/{date}");
        if let Some(page) = self.read_page(&slug)? {
            return Ok(page);
        }
        self.create_page("daily", &date, "")
    }

    /// Append a quick note to the daily page or the inbox page, creating the page when
    /// it does not exist yet. `source` names who captured (`terminal`, `mcp`, ...).
    pub fn capture(
        &self,
        text: &str,
        target: CaptureTarget,
        date: Option<&str>,
        source: &str,
    ) -> Result<CaptureReceipt, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("Nothing to capture".to_string());
        }
        let date = normalize_date(date)?;
        let (slug, created_page) = match target {
            CaptureTarget::Daily => {
                let slug = format!("daily/{date}");
                let existed = self.read_page(&slug)?.is_some();
                (self.daily_page(Some(&date))?.slug, !existed)
            }
            CaptureTarget::Inbox => {
                let slug = "inbox/inbox".to_string();
                if self.read_page(&slug)?.is_some() {
                    (slug, false)
                } else {
                    (self.create_page("inbox", "Inbox", "")?.slug, true)
                }
            }
        };
        let entry_id = self.add_timeline(&slug, &date, source, text, None)?;
        Ok(CaptureReceipt {
            slug,
            entry_id,
            created_page,
        })
    }

    /// Every page type with its folder and page count.
    pub fn page_types(&self) -> Result<Vec<PageTypeInfo>, String> {
        let counts = self.stats()?.pages_by_type;
        Ok(vault::page_types()
            .iter()
            .map(|(t, d)| PageTypeInfo {
                page_type: t.to_string(),
                dir: d.to_string(),
                count: counts.get(*t).copied().unwrap_or(0),
            })
            .collect())
    }

    // ── Semantic index ─────────────────────────────────────────────────

    /// The vector index over this brain's pages.
    pub fn semantic(&self) -> SemanticIndex {
        SemanticIndex::new(Arc::clone(&self.db))
    }

    /// The text a page is embedded as: title, compiled truth, timeline.
    fn embed_text(page: &BrainPage) -> String {
        let mut text = page.title.clone();
        if !page.compiled_truth.trim().is_empty() {
            text.push_str("\n\n");
            text.push_str(page.compiled_truth.trim());
        }
        if !page.timeline.trim().is_empty() {
            text.push_str("\n\n");
            text.push_str(page.timeline.trim());
        }
        text
    }

    fn content_hash_of(&self, slug: &str) -> Result<String, String> {
        let conn = self.db.conn()?;
        conn.query_row(
            "SELECT COALESCE(content_hash, '') FROM brain_pages WHERE slug = ?1",
            rusqlite::params![slug],
            |row| row.get(0),
        )
        .map_err(|e| format!("Query error: {e}"))
    }

    /// Embed every page whose vectors are missing, out of date, or from another model
    /// (every page when `force`), and drop chunks of pages that no longer exist. One page
    /// failing does not stop the pass; failures are listed in the report.
    pub fn index_stale(&self, embedder: &dyn Embedder, force: bool) -> Result<IndexReport, String> {
        let index = self.semantic();
        let model = embedder.id();
        let (stale, orphaned) = index.stale_slugs(&model)?;
        let slugs: Vec<String> = if force {
            self.list_pages(None, Some(100_000))?
                .into_iter()
                .map(|p| p.slug)
                .collect()
        } else {
            stale
        };
        let mut report = IndexReport {
            model,
            ..Default::default()
        };
        for slug in orphaned {
            index.remove(&slug)?;
            report.pages_removed += 1;
        }
        for slug in slugs {
            let page = match self.read_page(&slug) {
                Ok(Some(page)) => page,
                Ok(None) => {
                    index.remove(&slug)?;
                    report.pages_removed += 1;
                    continue;
                }
                Err(e) => {
                    report.pages_failed.push(format!("{slug}: {e}"));
                    continue;
                }
            };
            let hash = self.content_hash_of(&slug)?;
            match index.index_page(embedder, &slug, &Self::embed_text(&page), &hash) {
                Ok(n) => {
                    report.pages_indexed += 1;
                    report.chunks_written += n;
                }
                Err(e) => report.pages_failed.push(format!("{slug}: {e}")),
            }
        }
        Ok(report)
    }

    /// Full-text and vector search merged by reciprocal rank fusion. Pages only the
    /// vectors found get a snippet from their closest chunk.
    pub fn search_hybrid(
        &self,
        query: &str,
        limit: Option<usize>,
        page_type: Option<&str>,
        embedder: &dyn Embedder,
    ) -> Result<Vec<BrainSearchResult>, String> {
        self.search_hybrid_with(
            query,
            &SearchOptions {
                limit,
                page_type: page_type.map(str::to_string),
                ..Default::default()
            },
            embedder,
        )
    }

    /// Hybrid search with the operators of [`parse_query`]; match case and regex are
    /// text searches, so those modes go to [`Self::search_with`].
    pub fn search_hybrid_with(
        &self,
        query: &str,
        options: &SearchOptions,
        embedder: &dyn Embedder,
    ) -> Result<Vec<BrainSearchResult>, String> {
        if options.regex || options.case_sensitive {
            return self.search_with(query, options);
        }
        let limit = options.limit.unwrap_or(10).max(1);
        let page_type = options.page_type.as_deref();
        let q = parse_query(query);
        let allowed = self.allowed_pages(&q)?;
        if q.words.is_empty() {
            return match allowed {
                Some(set) => self.pages_in(&set, Some(limit), page_type),
                None => Ok(Vec::new()),
            };
        }
        let query = q.words.as_str();
        let fts = self.search_text(query, Some(limit * 2), page_type)?;
        // The vector half needs the provider; when it is down or slow, the text half still
        // answers rather than the whole search failing.
        let mut hits = match self.semantic().search(embedder, query, limit * 3) {
            Ok(hits) => hits,
            Err(e) => {
                eprintln!("rusty: search: vectors skipped: {e}");
                Vec::new()
            }
        };
        if let Some(set) = &allowed {
            hits.retain(|h| set.contains(&h.slug));
        }
        let mut best_chunk: std::collections::HashMap<String, &semantic::VecHit> =
            std::collections::HashMap::new();
        let mut vec_order: Vec<String> = Vec::new();
        for hit in &hits {
            if !best_chunk.contains_key(&hit.slug) {
                best_chunk.insert(hit.slug.clone(), hit);
                vec_order.push(hit.slug.clone());
            }
        }
        let fts_order: Vec<String> = fts
            .iter()
            .filter(|r| allowed.as_ref().is_none_or(|set| set.contains(&r.slug)))
            .map(|r| r.slug.clone())
            .collect();
        let fused = semantic::fuse(&fts_order, &vec_order);
        let by_slug: std::collections::HashMap<&str, &BrainSearchResult> =
            fts.iter().map(|r| (r.slug.as_str(), r)).collect();
        let mut out = Vec::new();
        for (slug, score) in fused {
            if out.len() >= limit {
                break;
            }
            if let Some(r) = by_slug.get(slug.as_str()) {
                let mut r = (*r).clone();
                r.rank = -score;
                out.push(r);
                continue;
            }
            let Some(hit) = best_chunk.get(&slug) else {
                continue;
            };
            let (title, kind) = {
                let conn = self.db.conn()?;
                conn.query_row(
                    "SELECT title, page_type FROM brain_pages WHERE slug = ?1",
                    rusqlite::params![slug],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .map_err(|e| format!("Query error: {e}"))?
            };
            if page_type.is_some_and(|t| t != kind) {
                continue;
            }
            let snippet: String = hit.text.chars().take(200).collect();
            out.push(BrainSearchResult {
                slug: slug.clone(),
                page_type: kind,
                title,
                snippet,
                rank: -score,
            });
        }
        Ok(out)
    }

    // ── Vault migration ────────────────────────────────────────────────

    /// Bring every page onto the current vault rules: the timeline as a `## Timeline`
    /// section instead of a bare `---` rule, and wikilinks as vault paths
    /// (`[[projects/orbit]]`) instead of bare names or titles. The frontmatter text is
    /// left byte for byte as it was; only the body is rewritten, and only when something
    /// changes. With `dry_run` nothing is written. Otherwise the index is rebuilt and one
    /// git commit records the pass.
    pub fn migrate_vault(&self, dry_run: bool) -> Result<MigrationReport, String> {
        let files = self.vault.list_all_files()?;
        let index = LinkIndex::build(&files);
        let mut report = MigrationReport {
            dry_run,
            ..Default::default()
        };
        for (slug, path) in &files {
            report.pages_scanned += 1;
            let raw = std::fs::read_to_string(path)
                .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
            let (prefix, body) = match split_raw(&raw) {
                Ok(parts) => parts,
                Err(e) => {
                    report
                        .unresolved_links
                        .push(format!("{slug}: not migrated ({e})"));
                    continue;
                }
            };
            let legacy = frontmatter::uses_legacy_rule(body);
            let (truth, timeline) = frontmatter::split_body(body);
            let (truth, n_truth, mut unresolved) = index.rewrite_links(&truth);
            let (timeline, n_timeline, more) = index.rewrite_links(&timeline);
            unresolved.extend(more);
            for target in unresolved {
                let line = format!("{slug}: [[{target}]]");
                if !report.unresolved_links.contains(&line) {
                    report.unresolved_links.push(line);
                }
            }
            let rewritten = n_truth + n_timeline;
            if !legacy && rewritten == 0 {
                continue;
            }
            let new_raw = format!("{prefix}{}", render_body(&truth, &timeline));
            if new_raw == raw {
                continue;
            }
            report.pages_changed += 1;
            report.timelines_converted += usize::from(legacy);
            report.links_rewritten += rewritten;
            report.changed_slugs.push(slug.clone());
            if !dry_run {
                self.vault.write_page(slug, &new_raw)?;
            }
        }
        if !dry_run && report.pages_changed > 0 {
            self.sync_all()?;
            self.vault.git_commit(&format!(
                "migrate: timeline sections and vault-path links ({} pages)",
                report.pages_changed
            ));
            self.vault.flush_commits();
        }
        Ok(report)
    }

    // ── Sources (TICKET-027) ──────────────────────────────────────────

    /// The source page whose `url` property is `url`, if one exists.
    pub fn source_for_url(&self, url: &str) -> Result<Option<String>, String> {
        for (slug, path) in self.vault.list_all_files()? {
            if !sources::is_source_slug(&slug) {
                continue;
            }
            let raw = std::fs::read_to_string(&path)
                .map_err(|e| format!("read {}: {e}", path.display()))?;
            let parsed = parse_lenient(&raw);
            if parsed.frontmatter.extra.get("url").and_then(|v| v.as_str()) == Some(url) {
                return Ok(Some(slug));
            }
        }
        Ok(None)
    }

    /// Capture a URL as a source page: fetched, read, written under `sources/` and
    /// indexed; a URL captured before updates its page; a failure is recorded on the
    /// page rather than leaving an empty one.
    pub fn capture_url(&self, url: &str) -> Result<BrainPage, String> {
        self.capture_fetched(url, sources::fetch(url))
    }

    /// The capture from what the fetch answered (or failed with), so a test needs no
    /// network. `created` is kept for a page captured before; a failure on a page that
    /// has text keeps the text and records `status: failed` and `error`.
    pub fn capture_fetched(
        &self,
        url: &str,
        fetched: Result<sources::Fetched, String>,
    ) -> Result<BrainPage, String> {
        let url = url.trim();
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err("only http and https URLs are captured".to_string());
        }
        let existing = self.source_for_url(url)?;
        let captured = chrono::Local::now()
            .format("%Y-%m-%dT%H:%M:%S%:z")
            .to_string();
        let site = sources::site_of(url);
        let outcome = fetched.and_then(|f| sources::extract(&f));
        let (slug, raw) = match (outcome, existing) {
            (Ok(extracted), existing) => {
                let created = existing
                    .as_deref()
                    .and_then(|slug| self.read_page(slug).ok().flatten())
                    .map(|page| page.frontmatter.created)
                    .filter(|c| !c.is_empty());
                let slug = match existing {
                    Some(slug) => slug,
                    None => {
                        self.unique_slug(sources::DIR, &sources::slug_base(url, &extracted.title))?
                    }
                };
                let mut fm = BrainFrontmatter::new(sources::PAGE_TYPE, &extracted.title);
                if let Some(created) = created {
                    fm.created = created;
                }
                fm.extra.insert(
                    "url".to_string(),
                    serde_json::Value::String(url.to_string()),
                );
                fm.extra
                    .insert("site".to_string(), serde_json::Value::String(site));
                fm.extra
                    .insert("captured".to_string(), serde_json::Value::String(captured));
                fm.extra.insert(
                    "kind".to_string(),
                    serde_json::Value::String(extracted.kind.to_string()),
                );
                let body = format!("{}\n", extracted.text.trim_end());
                (slug, render_page(&fm, &body, "")?)
            }
            (Err(error), Some(slug)) => {
                let raw = self
                    .vault
                    .read_page(&slug)?
                    .ok_or_else(|| format!("Page not found: {slug}"))?;
                let raw = frontmatter::set_property(&raw, "status", serde_json::json!("failed"))?;
                let raw = frontmatter::set_property(&raw, "error", serde_json::json!(error))?;
                (slug, raw)
            }
            (Err(error), None) => {
                let title = site.clone();
                let slug = self.unique_slug(sources::DIR, &sources::slug_base(url, ""))?;
                let mut fm = BrainFrontmatter::new(sources::PAGE_TYPE, &title);
                fm.extra.insert(
                    "url".to_string(),
                    serde_json::Value::String(url.to_string()),
                );
                fm.extra
                    .insert("site".to_string(), serde_json::Value::String(site));
                fm.extra
                    .insert("captured".to_string(), serde_json::Value::String(captured));
                fm.extra.insert(
                    "status".to_string(),
                    serde_json::Value::String("failed".into()),
                );
                fm.extra.insert(
                    "error".to_string(),
                    serde_json::Value::String(error.clone()),
                );
                let body = format!("Capture failed: {error}\n");
                (slug, render_page(&fm, &body, "")?)
            }
        };
        self.vault.write_page(&slug, &raw)?;
        self.sync_page(&slug)?;
        self.vault.git_commit(&format!("capture: {slug}"));
        self.read_page(&slug)?
            .ok_or_else(|| format!("Page not found after capture: {slug}"))
    }

    // ── Importing an Obsidian vault (TICKET-026) ──────────────────────

    /// What importing the vault at `source` would do: the pages and attachments that
    /// come in at their own paths, the collisions with the brain (skipped, never
    /// overwritten), the tags, the links that would not resolve, and the bookmarks from
    /// `.obsidian/bookmarks.json`. The source is read and never written.
    #[cfg(test)]
    fn page_exists_for_test(&self, slug: &str) -> bool {
        self.vault.page_exists(slug)
    }

    pub fn import_plan(&self, source: &std::path::Path) -> Result<import::ImportPlan, String> {
        let source = source
            .canonicalize()
            .map_err(|e| format!("{}: {e}", source.display()))?;
        if source == self.vault.root() || source.starts_with(self.vault.root()) {
            return Err("that folder is the brain itself".to_string());
        }
        let scan = import::scan_vault(&source)?;
        let mut plan = import::ImportPlan {
            source: source.display().to_string(),
            name: source
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "vault".to_string()),
            ..Default::default()
        };
        let mut incoming: Vec<(String, PathBuf)> = Vec::new();
        // A path that exists, or one in the archive (which takes no new pages), is a
        // collision: skipped and named.
        for (slug, path) in &scan.pages {
            if self.vault.page_exists(slug) || vault::in_archive(slug) {
                plan.collisions.push(slug.clone());
            } else {
                plan.pages.push(slug.clone());
                incoming.push((slug.clone(), path.clone()));
            }
        }
        for (rel, _) in &scan.attachments {
            if self.vault.exists(rel) || vault::in_archive(rel) {
                plan.collisions.push(rel.clone());
            } else {
                plan.attachments.push(rel.clone());
            }
        }
        let mut all = self.vault.list_all_files()?;
        all.extend(incoming.iter().cloned());
        let index = LinkIndex::build(&all);
        let mut seen_tags = std::collections::HashSet::new();
        for (slug, path) in &incoming {
            let raw = std::fs::read_to_string(path)
                .map_err(|e| format!("read {}: {e}", path.display()))?;
            let body = match split_raw(&raw) {
                Ok((_, body)) => body.to_string(),
                Err(_) => raw.clone(),
            };
            let (_, _, unresolved) = index.rewrite_links(&body);
            for target in unresolved {
                let line = format!("{slug}: [[{target}]]");
                if !plan.unresolved_links.contains(&line) {
                    plan.unresolved_links.push(line);
                }
            }
            let parsed = parse_lenient(&raw);
            for tag in parsed
                .frontmatter
                .tags
                .iter()
                .map(|t| t.trim().trim_start_matches('#').to_string())
                .chain(links::tags(&body))
            {
                if !tag.is_empty() && seen_tags.insert(tag.to_lowercase()) {
                    plan.tags.push(tag);
                }
            }
        }
        let mut with_folders: Vec<String> = plan.pages.clone();
        with_folders.extend(plan.attachments.iter().cloned());
        plan.folders = import::folders_of(&with_folders);
        let bookmarks_file = source.join(".obsidian").join("bookmarks.json");
        if let Ok(json) = std::fs::read_to_string(&bookmarks_file) {
            for b in import::parse_bookmarks(&json) {
                let kept = match b.kind.as_str() {
                    "search" => true,
                    "folder" => plan.folders.contains(&b.path) || self.vault.is_folder(&b.path),
                    _ => plan.pages.contains(&b.path) || self.vault.page_exists(&b.path),
                };
                if kept {
                    plan.bookmarks.push(b);
                } else {
                    plan.bookmarks_skipped
                        .push(format!("{} `{}`: no such page or folder", b.kind, b.path));
                }
            }
        }
        Ok(plan)
    }

    /// Import the vault at `source` as [`import_plan`](Self::import_plan) says: pages
    /// first (the frontmatter as it was, bare-name links rewritten to vault paths), then
    /// attachments, then a report page under `inbox/`; the index rebuilt and one commit.
    /// Every path this run creates is recorded, and a failure removes them all, rebuilds
    /// the index and answers with the error, so the brain is either the import whole or
    /// as it was. The source is never written.
    pub fn import_vault(&self, source: &std::path::Path) -> Result<import::ImportReport, String> {
        let plan = self.import_plan(source)?;
        let source = PathBuf::from(&plan.source);
        let mut all = self.vault.list_all_files()?;
        let incoming: Vec<(String, PathBuf)> = plan
            .pages
            .iter()
            .map(|slug| (slug.clone(), source.join(format!("{slug}.md"))))
            .collect();
        all.extend(incoming.iter().cloned());
        let index = LinkIndex::build(&all);
        let mut report = import::ImportReport {
            plan,
            ..Default::default()
        };
        let mut created: Vec<PathBuf> = Vec::new();
        let outcome = (|| -> Result<(), String> {
            for (slug, path) in &incoming {
                let raw = std::fs::read_to_string(path)
                    .map_err(|e| format!("read {}: {e}", path.display()))?;
                let (prefix, body) = match split_raw(&raw) {
                    Ok((prefix, body)) => (prefix.to_string(), body.to_string()),
                    Err(_) => (String::new(), raw.clone()),
                };
                let (body, n, _) = index.rewrite_links(&body);
                report.links_rewritten += n;
                self.vault.write_page(slug, &format!("{prefix}{body}"))?;
                created.push(self.vault.root().join(format!("{slug}.md")));
                report.imported_pages += 1;
            }
            for rel in &report.plan.attachments {
                let from = source.join(rel);
                let to = self.vault.root().join(rel);
                if let Some(parent) = to.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("create {}: {e}", parent.display()))?;
                }
                std::fs::copy(&from, &to).map_err(|e| format!("copy {rel}: {e}"))?;
                self.vault.touch(rel);
                created.push(to);
                report.imported_attachments += 1;
            }
            let stamp = chrono::Local::now();
            let name: String = report
                .plan
                .name
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() {
                        c.to_ascii_lowercase()
                    } else {
                        '-'
                    }
                })
                .collect();
            let base = format!(
                "inbox/import-{}-{}",
                stamp.format("%Y-%m-%d-%H%M"),
                name.trim_matches('-')
            );
            // A second import in the same minute keeps the first report.
            let mut slug = base.clone();
            let mut n = 2;
            while self.vault.page_exists(&slug) {
                slug = format!("{base}-{n}");
                n += 1;
            }
            let page = import::report_page(&report, &stamp.format("%Y-%m-%d").to_string());
            self.vault.write_page(&slug, &page)?;
            created.push(self.vault.root().join(format!("{slug}.md")));
            report.report_slug = slug;
            Ok(())
        })();
        if let Err(e) = outcome {
            for path in &created {
                let _ = std::fs::remove_file(path);
            }
            let _ = self.sync_all();
            return Err(format!(
                "import stopped: {e}; nothing of it remains in the brain"
            ));
        }
        self.sync_all()?;
        self.vault.git_commit(&format!(
            "import: {} pages and {} attachments from {}",
            report.imported_pages, report.imported_attachments, report.plan.name
        ));
        // The source vault's bookmarks join the store's. The import is done and committed
        // by now, so a bookmarks file that cannot be read is reported, not an error.
        let incoming = report.plan.bookmarks.iter().filter(|b| b.check().is_ok());
        let added = self.bookmarks().and_then(|mut list| {
            let before = list.len();
            list.extend(incoming.cloned());
            Ok(self.set_bookmarks(list)?.len() - before)
        });
        match added {
            Ok(n) => report.bookmarks_added = n,
            Err(e) => eprintln!("rusty: import: bookmarks not added: {e}"),
        }
        self.vault.flush_commits();
        Ok(report)
    }

    // ── Links ─────────────────────────────────────────────────────────

    /// Add a typed link between two pages.
    pub fn add_link(
        &self,
        from_slug: &str,
        to_slug: &str,
        link_type: Option<&str>,
        context: Option<&str>,
    ) -> Result<(), String> {
        let conn = self.db.conn()?;
        let lt = link_type.unwrap_or("reference");
        let now = unix_now();

        conn.execute(
            "INSERT OR IGNORE INTO brain_links (from_slug, to_slug, link_type, context, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![from_slug, to_slug, lt, context.unwrap_or(""), now],
        )
        .map_err(|e| format!("Failed to add link: {e}"))?;

        Ok(())
    }

    /// Remove a link between two pages.
    pub fn remove_link(&self, from_slug: &str, to_slug: &str) -> Result<(), String> {
        let conn = self.db.conn()?;
        conn.execute(
            "DELETE FROM brain_links WHERE from_slug = ?1 AND to_slug = ?2",
            rusqlite::params![from_slug, to_slug],
        )
        .map_err(|e| format!("Failed to remove link: {e}"))?;
        Ok(())
    }

    /// Get outbound links and backlinks for a page.
    pub fn get_links(&self, slug: &str) -> Result<PageLinks, String> {
        let conn = self.db.conn()?;

        // Outbound links
        let mut stmt = conn
            .prepare(
                "SELECT l.from_slug, l.to_slug, l.link_type, l.context, p.slug IS NOT NULL \
                 FROM brain_links l LEFT JOIN brain_pages p ON p.slug = l.to_slug \
                 WHERE l.from_slug = ?1 ORDER BY l.id",
            )
            .map_err(|e| format!("Query error: {e}"))?;
        let outbound: Vec<LinkEntry> = stmt
            .query_map(rusqlite::params![slug], row_to_link)
            .map_err(|e| format!("Query error: {e}"))?
            .flatten()
            .collect();

        // Backlinks
        let mut stmt = conn
            .prepare(
                "SELECT l.from_slug, l.to_slug, l.link_type, l.context, 1 \
                 FROM brain_links l WHERE l.to_slug = ?1 ORDER BY l.from_slug",
            )
            .map_err(|e| format!("Query error: {e}"))?;
        let backlinks: Vec<LinkEntry> = stmt
            .query_map(rusqlite::params![slug], row_to_link)
            .map_err(|e| format!("Query error: {e}"))?
            .flatten()
            .collect();

        Ok(PageLinks {
            outbound,
            backlinks,
        })
    }

    // ── Tags ──────────────────────────────────────────────────────────

    /// Add a tag to a page (also updates the frontmatter on disk).
    pub fn add_tag(&self, slug: &str, tag: &str) -> Result<(), String> {
        let conn = self.db.conn()?;
        conn.execute(
            "INSERT OR IGNORE INTO brain_tags (slug, tag) VALUES (?1, ?2)",
            rusqlite::params![slug, tag],
        )
        .map_err(|e| format!("Failed to add tag: {e}"))?;
        Ok(())
    }

    /// Remove a tag from a page.
    pub fn remove_tag(&self, slug: &str, tag: &str) -> Result<(), String> {
        let conn = self.db.conn()?;
        conn.execute(
            "DELETE FROM brain_tags WHERE slug = ?1 AND tag = ?2",
            rusqlite::params![slug, tag],
        )
        .map_err(|e| format!("Failed to remove tag: {e}"))?;
        Ok(())
    }

    /// Get all links in the brain (for graph visualization).
    pub fn get_all_links(&self) -> Result<Vec<LinkEntry>, String> {
        let conn = self.db.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT l.from_slug, l.to_slug, l.link_type, l.context, p.slug IS NOT NULL \
                 FROM brain_links l LEFT JOIN brain_pages p ON p.slug = l.to_slug",
            )
            .map_err(|e| format!("Query error: {e}"))?;
        let links: Vec<LinkEntry> = stmt
            .query_map([], row_to_link)
            .map_err(|e| format!("Query error: {e}"))?
            .flatten()
            .collect();
        Ok(links)
    }

    // ── Stats & Resolution ────────────────────────────────────────────

    /// Get brain statistics: total pages, pages by type, total links, tags, timeline entries.
    pub fn stats(&self) -> Result<BrainStats, String> {
        let conn = self.db.conn()?;

        let page_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM brain_pages", [], |row| row.get(0))
            .map_err(|e| format!("Query error: {e}"))?;

        let link_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM brain_links", [], |row| row.get(0))
            .map_err(|e| format!("Query error: {e}"))?;

        let tag_count: i64 = conn
            .query_row("SELECT COUNT(DISTINCT tag) FROM brain_tags", [], |row| {
                row.get(0)
            })
            .map_err(|e| format!("Query error: {e}"))?;

        let timeline_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM brain_timeline", [], |row| row.get(0))
            .map_err(|e| format!("Query error: {e}"))?;

        // Pages by type
        let mut stmt = conn
            .prepare("SELECT page_type, COUNT(*) FROM brain_pages GROUP BY page_type")
            .map_err(|e| format!("Query error: {e}"))?;
        let mut pages_by_type = std::collections::HashMap::new();
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|e| format!("Query error: {e}"))?;
        for row in rows.flatten() {
            pages_by_type.insert(row.0, row.1);
        }

        Ok(BrainStats {
            page_count,
            link_count,
            tag_count,
            timeline_count,
            pages_by_type,
            vault_root: self.vault_root().to_string_lossy().into_owned(),
        })
    }

    /// Resolve a partial name to matching slugs via aliases and title matching.
    pub fn resolve_slug(&self, partial: &str) -> Result<Vec<String>, String> {
        let conn = self.db.conn()?;
        let pattern = format!("%{partial}%");
        let mut results = Vec::new();

        // Search aliases
        let mut stmt = conn
            .prepare("SELECT DISTINCT slug FROM brain_aliases WHERE alias LIKE ?1 LIMIT 10")
            .map_err(|e| format!("Query error: {e}"))?;
        let alias_matches: Vec<String> = stmt
            .query_map(rusqlite::params![pattern], |row| row.get(0))
            .map_err(|e| format!("Query error: {e}"))?
            .flatten()
            .collect();
        results.extend(alias_matches);

        // Search titles
        let mut stmt = conn
            .prepare(
                "SELECT slug FROM brain_pages WHERE title LIKE ?1 AND slug NOT IN \
                 (SELECT DISTINCT slug FROM brain_aliases WHERE alias LIKE ?1) LIMIT 10",
            )
            .map_err(|e| format!("Query error: {e}"))?;
        let title_matches: Vec<String> = stmt
            .query_map(rusqlite::params![pattern], |row| row.get(0))
            .map_err(|e| format!("Query error: {e}"))?
            .flatten()
            .collect();
        results.extend(title_matches);

        Ok(results)
    }

    // ── Context Injection ──────────────────────────────────────────────

    /// Maximum characters for the brain context block in the system prompt.
    const MAX_BRAIN_CONTEXT_CHARS: usize = 4000;

    /// Build a `<brain_context>` block for system prompt injection.
    ///
    /// Extracts significant words from the user's prompt, searches the brain
    /// using FTS5, and returns a formatted block with compiled truth from
    /// matching pages. Returns empty string if no matches are found.
    pub fn build_context_for_prompt(&self, prompt: &str) -> String {
        // Extract significant words (3+ chars, not common stop words) for FTS5 query
        let query = Self::build_fts_query(prompt);
        if query.is_empty() {
            return String::new();
        }

        let results = match self.search(&query, Some(5), None) {
            Ok(r) => r,
            Err(_) => return String::new(),
        };

        if results.is_empty() {
            return String::new();
        }

        let mut parts = vec!["<brain_context>".to_string()];
        parts.push("## Relevant Brain Pages\n".to_string());
        let mut total_chars = 0;

        for result in &results {
            // Read the full page to get compiled truth
            let page = match self.read_page(&result.slug) {
                Ok(Some(p)) => p,
                _ => continue,
            };

            // A captured page is web text: it is marked, normalised and kept as data.
            let section = if page.page_type == sources::PAGE_TYPE {
                format!(
                    "### {} (source, untrusted)\n{}\n{}\n",
                    page.title,
                    sources::UNTRUSTED_NOTE,
                    sources::normalise(&page.compiled_truth)
                )
            } else {
                format!(
                    "### {} ({})\n{}\n",
                    page.title, page.page_type, page.compiled_truth
                )
            };

            total_chars += section.len();
            if total_chars > Self::MAX_BRAIN_CONTEXT_CHARS {
                break;
            }

            parts.push(section);
        }

        parts.push("</brain_context>".to_string());
        parts.join("\n")
    }

    // ── The workspace: tree, rendering, whole-file edits, files and folders ──

    /// The vault as a tree, folders first, dot-entries left out.
    pub fn tree(&self) -> Result<VaultNode, String> {
        self.vault.tree()
    }

    /// A page rendered for the workspace, or `None` when there is no such page.
    pub fn render_page(&self, slug: &str, style: &Style) -> Result<Option<RenderedPage>, String> {
        let Some(raw) = self.vault.read_page(slug)? else {
            return Ok(None);
        };
        let mut parsed = parse_lenient(&raw);
        parsed.frontmatter.fill_defaults(slug);
        let resolver = DbResolver { brain: self };
        let rendered = render::render(render::body_of(&raw), style, &resolver, Some(slug));
        let properties = properties_of(&raw)
            .into_iter()
            .map(|(key, value)| Property { key, value })
            .collect();
        Ok(Some(RenderedPage {
            slug: slug.to_string(),
            title: parsed.frontmatter.title,
            page_type: parsed.frontmatter.page_type,
            properties,
            raw,
            file: self.page_file(slug),
            blocks: None,
            body_start: None,
            rendered,
        }))
    }

    /// [`Self::render_page`], with the body as typed blocks too (TICKET-036).
    pub fn render_page_with_blocks(
        &self,
        slug: &str,
        style: &Style,
    ) -> Result<Option<RenderedPage>, String> {
        let Some(mut page) = self.render_page(slug, style)? else {
            return Ok(None);
        };
        let body = render::body_of(&page.raw);
        let resolver = DbResolver { brain: self };
        page.body_start = Some(page.raw.len() - body.len());
        page.blocks = Some(blocks::build(body, &resolver, Some(slug)));
        Ok(Some(page))
    }

    /// [`Self::render_text`], with the body as typed blocks too.
    pub fn render_text_with_blocks(&self, markdown: &str, style: &Style) -> RenderedPage {
        let mut page = self.render_text(markdown, style);
        let body = render::body_of(markdown);
        let resolver = DbResolver { brain: self };
        page.body_start = Some(markdown.len() - body.len());
        page.blocks = Some(blocks::build(body, &resolver, None));
        page
    }

    /// Render markdown that is not a page (a file from a folder root) with the page
    /// renderer; links resolve against the vault as they would in a page. The slug is
    /// empty and the title is whatever the text's frontmatter says.
    pub fn render_text(&self, markdown: &str, style: &Style) -> RenderedPage {
        let parsed = parse_lenient(markdown);
        let resolver = DbResolver { brain: self };
        let rendered = render::render(render::body_of(markdown), style, &resolver, None);
        let properties = properties_of(markdown)
            .into_iter()
            .map(|(key, value)| Property { key, value })
            .collect();
        RenderedPage {
            slug: String::new(),
            title: parsed.frontmatter.title,
            page_type: parsed.frontmatter.page_type,
            properties,
            raw: markdown.to_string(),
            file: String::new(),
            blocks: None,
            body_start: None,
            rendered,
        }
    }

    /// Replace a page's whole file, frontmatter and timeline included, the way an
    /// editor saves. The previous text is kept as a version; an unchanged file is not
    /// written or committed.
    pub fn write_raw(&self, slug: &str, content: &str) -> Result<BrainPage, String> {
        let existing = self.vault.read_page(slug)?;
        if existing.as_deref() == Some(content) {
            return self
                .read_page(slug)?
                .ok_or_else(|| format!("Page not found: {slug}"));
        }
        if let Some(raw) = &existing {
            let mut parsed = parse_lenient(raw);
            parsed.frontmatter.fill_defaults(slug);
            self.create_version(slug, raw, &parsed.frontmatter)?;
        }
        self.vault.write_page(slug, content)?;
        self.sync_page(slug)?;
        let page = self
            .read_page(slug)?
            .ok_or_else(|| format!("Page not found after write: {slug}"))?;
        self.vault.git_commit(&format!("edit: {}", page.title));
        Ok(page)
    }

    /// A new page in `folder` (the root when empty) named `name`, or `Untitled`,
    /// `Untitled 1`, ... when no name is given, typed after the folder and started from
    /// that type's template. Returns the slug.
    pub fn new_page(&self, folder: &str, name: Option<&str>) -> Result<String, String> {
        let folder = clean_rel(folder);
        if folder.contains("..") {
            return Err("Invalid folder".to_string());
        }
        if !folder.is_empty() && !self.vault.is_folder(&folder) {
            return Err(format!("No folder {folder}"));
        }
        let base = name
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(|n| n.trim_end_matches(".md").replace('/', "-"))
            .unwrap_or_else(|| "Untitled".to_string());
        let prefix = if folder.is_empty() {
            String::new()
        } else {
            format!("{folder}/")
        };
        let mut candidate = base.clone();
        let mut n = 1;
        while self.vault.exists(&format!("{prefix}{candidate}.md")) {
            candidate = format!("{base} {n}");
            n += 1;
        }
        let slug = format!("{prefix}{candidate}");
        let page_type = vault::type_for_slug(&slug);
        let fm = BrainFrontmatter::new(page_type, &candidate);
        let body = self.load_template(page_type).unwrap_or_default();
        let raw = render_page(&fm, &body, "")?;
        self.vault.write_page(&slug, &raw)?;
        self.sync_page(&slug)?;
        self.vault.git_commit(&format!("create: {candidate}"));
        Ok(slug)
    }

    /// The page at exactly `path` (a vault path such as a link target, `.md` optional):
    /// its folders are made when missing, an existing page is returned as it is, and a
    /// new one is titled after the last segment, typed after its folder and started from
    /// that type's template (TICKET-041). Returns the slug.
    pub fn new_page_at(&self, path: &str) -> Result<String, String> {
        let raw = path.trim();
        if raw.starts_with('/') || raw.contains("..") {
            return Err(format!("Invalid page path: {raw}"));
        }
        if raw.ends_with('/') {
            return Err("A page path needs a name".to_string());
        }
        let slug = clean_rel(raw.strip_suffix(".md").unwrap_or(raw));
        let Some(name) = slug.rsplit('/').next().filter(|n| !n.trim().is_empty()) else {
            return Err("A page path needs a name".to_string());
        };
        if self.vault.page_exists(&slug) {
            return Ok(slug);
        }
        let page_type = vault::type_for_slug(&slug);
        let fm = BrainFrontmatter::new(page_type, name);
        let body = self.load_template(page_type).unwrap_or_default();
        let page = render_page(&fm, &body, "")?;
        self.vault.write_page(&slug, &page)?;
        self.sync_page(&slug)?;
        self.vault.git_commit(&format!("create: {slug}"));
        Ok(slug)
    }

    /// Commit what changed in the vault outside the tools, in a commit of its own
    /// (TICKET-043); the server's indexer calls it after each sync. Returns the paths.
    pub fn commit_outside_edits(&self) -> Result<Vec<String>, String> {
        self.vault.commit_outside_edits()
    }

    /// A new folder; returns its cleaned path.
    pub fn new_folder(&self, path: &str) -> Result<String, String> {
        let created = self.vault.create_folder(path)?;
        self.vault.git_commit(&format!("folder: {created}"));
        Ok(created)
    }

    /// Soft-delete a folder into `archive/` and drop its pages from the index.
    pub fn delete_folder(&self, path: &str) -> Result<String, String> {
        let folder = clean_rel(path);
        let prefix = format!("{folder}/");
        let under: Vec<String> = self
            .vault
            .list_all_files()?
            .into_iter()
            .map(|(slug, _)| slug)
            .filter(|slug| slug.starts_with(&prefix))
            .collect();
        let archived = self.vault.delete_folder(&folder)?;
        for slug in &under {
            self.forget_page(slug)?;
        }
        self.vault.git_commit(&format!("delete folder: {folder}"));
        if let Err(e) = self.drop_bookmarks(&folder, true) {
            eprintln!("rusty: bookmarks under {folder} left as they are: {e}");
        }
        Ok(archived)
    }

    /// Rename or move a page or a folder. `to` is the new slug or folder path; a `to`
    /// that ends in `/` means "into that folder" under the same name. Every link to
    /// what moved is rewritten in every page (fenced code untouched), the index rows
    /// follow, and one commit records it.
    pub fn rename(&self, from: &str, to: &str) -> Result<RenameReport, String> {
        let from = clean_rel(from.strip_suffix(".md").unwrap_or(from));
        let into_folder = to.trim().ends_with('/');
        let mut to = clean_rel(to.strip_suffix(".md").unwrap_or(to));
        if from.contains("..") || to.contains("..") {
            return Err("Invalid path".to_string());
        }
        if into_folder {
            let name = from.rsplit('/').next().unwrap_or(&from);
            to = if to.is_empty() {
                name.to_string()
            } else {
                format!("{to}/{name}")
            };
        }
        if to.is_empty() {
            return Err("A name is needed".to_string());
        }
        if self.vault.page_exists(&from) {
            self.rename_page(&from, &to)
        } else if self.vault.is_folder(&from) {
            self.rename_folder(&from, &to)
        } else {
            Err(format!("Not found: {from}"))
        }
    }

    fn rename_page(&self, from: &str, to: &str) -> Result<RenameReport, String> {
        if from == to {
            return Ok(RenameReport {
                from: from.to_string(),
                to: to.to_string(),
                kind: "page".to_string(),
                pages_rewritten: 0,
            });
        }
        if self.vault.exists(&format!("{to}.md")) {
            return Err(format!("Already exists: {to}"));
        }
        let files = self.vault.list_all_files()?;
        let basename = from.rsplit('/').next().unwrap_or(from).to_lowercase();
        let unique = files
            .iter()
            .filter(|(slug, _)| slug.rsplit('/').next().unwrap_or(slug).to_lowercase() == basename)
            .count()
            == 1;
        self.vault
            .rename_path(&format!("{from}.md"), &format!("{to}.md"))?;
        // A title that was the old file name follows the new one.
        if let Some(raw) = self.vault.read_page(to)? {
            let parsed = parse_lenient(&raw);
            let old_name = from.rsplit('/').next().unwrap_or(from);
            let new_name = to.rsplit('/').next().unwrap_or(to);
            if parsed.frontmatter.title.eq_ignore_ascii_case(old_name) && old_name != new_name {
                // Only the title line changes (TICKET-046).
                if let Some(new_raw) = frontmatter::set_scalar_line(&raw, "title", new_name)? {
                    self.vault.write_page(to, &new_raw)?;
                }
            }
        }
        let map = links::move_map(from, to, unique);
        let rewritten = self.rewrite_everywhere(&map, from, to)?;
        self.move_index_rows(from, to, false)?;
        self.sync_page(to)?;
        for slug in &rewritten {
            self.sync_page(slug)?;
        }
        self.resolve_pending_links()?;
        self.vault.git_commit(&format!(
            "move: {from} to {to} ({} pages updated)",
            rewritten.len()
        ));
        if let Err(e) = self.follow_bookmarks(from, to, false) {
            eprintln!("rusty: bookmarks of {from} left as they are: {e}");
        }
        Ok(RenameReport {
            from: from.to_string(),
            to: to.to_string(),
            kind: "page".to_string(),
            pages_rewritten: rewritten.len(),
        })
    }

    fn rename_folder(&self, from: &str, to: &str) -> Result<RenameReport, String> {
        if from == to {
            return Ok(RenameReport {
                from: from.to_string(),
                to: to.to_string(),
                kind: "folder".to_string(),
                pages_rewritten: 0,
            });
        }
        if self.vault.exists(to) {
            return Err(format!("Already exists: {to}"));
        }
        self.vault.rename_path(from, to)?;
        let map = links::folder_move_map(from, to);
        let rewritten = self.rewrite_everywhere(&map, from, to)?;
        self.move_index_rows(from, to, true)?;
        for slug in &rewritten {
            self.sync_page(slug)?;
        }
        self.resolve_pending_links()?;
        self.vault.git_commit(&format!(
            "move folder: {from} to {to} ({} pages updated)",
            rewritten.len()
        ));
        if let Err(e) = self.follow_bookmarks(from, to, true) {
            eprintln!("rusty: bookmarks under {from} left as they are: {e}");
        }
        Ok(RenameReport {
            from: from.to_string(),
            to: to.to_string(),
            kind: "folder".to_string(),
            pages_rewritten: rewritten.len(),
        })
    }

    /// Rewrite links in every page after a move; returns the slugs that changed.
    fn rewrite_everywhere(
        &self,
        map: &dyn Fn(&str) -> Option<String>,
        _from: &str,
        _to: &str,
    ) -> Result<Vec<String>, String> {
        let mut changed = Vec::new();
        for (slug, path) in self.vault.list_all_files()? {
            // A file that cannot be read as text keeps its links as they are; failing here
            // would leave the move done and the rewrite half-applied.
            let raw = match std::fs::read_to_string(&path) {
                Ok(raw) => raw,
                Err(e) => {
                    eprintln!(
                        "rusty: rename: links in {} left as they are: {e}",
                        path.display()
                    );
                    continue;
                }
            };
            let (new_raw, n) = links::rewrite_targets(&raw, map);
            if n > 0 && new_raw != raw {
                std::fs::write(&path, new_raw)
                    .map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
                self.vault.touch(&format!("{slug}.md"));
                changed.push(slug);
            }
        }
        Ok(changed)
    }

    /// Point every index row at the new slug (or folder prefix).
    fn move_index_rows(&self, from: &str, to: &str, folder: bool) -> Result<(), String> {
        let conn = self.db.conn()?;
        let tables = [
            ("brain_pages", "slug"),
            ("brain_fts", "slug"),
            ("brain_links", "from_slug"),
            ("brain_links", "to_slug"),
            ("brain_tags", "slug"),
            ("brain_aliases", "slug"),
            ("brain_timeline", "slug"),
            ("brain_versions", "slug"),
            ("brain_chunks", "slug"),
        ];
        for (table, column) in tables {
            let sql = if folder {
                format!(
                    "UPDATE OR IGNORE {table} SET {column} = ?2 || substr({column}, length(?1) + 1) \
                     WHERE substr({column}, 1, length(?1)) = ?1"
                )
            } else {
                format!("UPDATE OR IGNORE {table} SET {column} = ?2 WHERE {column} = ?1")
            };
            let (a, b) = if folder {
                (format!("{from}/"), format!("{to}/"))
            } else {
                (from.to_string(), to.to_string())
            };
            conn.execute(&sql, rusqlite::params![a, b])
                .map_err(|e| format!("Failed to move index rows in {table}: {e}"))?;
        }
        // Each page that moved, under its new slug with its old one (TICKET-035).
        if folder {
            let moved: Vec<String> = {
                let mut stmt = conn
                    .prepare("SELECT slug FROM brain_pages WHERE substr(slug, 1, length(?1)) = ?1")
                    .map_err(|e| format!("Query error: {e}"))?;
                let rows: Vec<String> = stmt
                    .query_map(rusqlite::params![format!("{to}/")], |row| row.get(0))
                    .map_err(|e| format!("Query error: {e}"))?
                    .flatten()
                    .collect();
                rows
            };
            for slug in &moved {
                let old = format!("{from}{}", &slug[to.len()..]);
                changes::record(&conn, "page", slug, "moved", &old, "");
            }
        } else {
            changes::record(&conn, "page", to, "moved", from, "");
        }
        Ok(())
    }

    /// Every wikilink whose target is no page, with the line it sits on.
    pub fn unresolved(&self) -> Result<Vec<UnresolvedLink>, String> {
        let conn = self.db.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT l.from_slug, l.to_slug, l.context FROM brain_links l \
                 LEFT JOIN brain_pages p ON p.slug = l.to_slug \
                 WHERE p.slug IS NULL ORDER BY l.from_slug, l.id",
            )
            .map_err(|e| format!("Query error: {e}"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(UnresolvedLink {
                    from_slug: row.get(0)?,
                    target: row.get(1)?,
                    context: row.get(2)?,
                })
            })
            .map_err(|e| format!("Query error: {e}"))?
            .flatten()
            .collect();
        Ok(rows)
    }

    // ── Private helpers ────────────────────────────────────────────────

    /// Build an FTS5 OR query from a natural language prompt.
    ///
    /// Extracts significant words (3+ chars, alphanumeric only, not common
    /// stop words) and joins them with OR for a broad FTS5 match.
    fn build_fts_query(prompt: &str) -> String {
        let stop_words = [
            "the", "and", "for", "are", "but", "not", "you", "all", "can", "had", "her", "was",
            "one", "our", "out", "has", "have", "been", "some", "them", "than", "its", "over",
            "also", "that", "this", "from", "with", "what", "when", "where", "which", "who",
            "will", "would", "could", "should", "about", "into", "your", "just", "been", "more",
            "tell", "know", "like", "does", "how",
        ];

        let words: Vec<String> = prompt
            .split_whitespace()
            .map(|w| {
                w.chars()
                    .filter(|c| c.is_alphanumeric())
                    .collect::<String>()
                    .to_lowercase()
            })
            .filter(|w| w.len() >= 3 && !stop_words.contains(&w.as_str()))
            .collect();

        if words.is_empty() {
            return String::new();
        }

        // Join with OR for broad matching
        words.join(" OR ")
    }

    /// Load a template for a page type from `.templates/{type}.md`.
    fn load_template(&self, page_type: &str) -> Option<String> {
        let path = self.vault.root().join(format!(".templates/{page_type}.md"));
        std::fs::read_to_string(path).ok()
    }

    /// Write default templates if they don't exist.
    fn write_default_templates(&self) {
        let templates_dir = self.vault.root().join(".templates");
        let defaults = [
            ("person", "## Key Context\n- \n\n## Links\n- "),
            (
                "company",
                "## Overview\n\n## Products\n\n## Team\n\n## Links\n- ",
            ),
            (
                "project",
                "## Overview\n\n## Goals\n\n## Status\n\n## Links\n- ",
            ),
            (
                "concept",
                "## Definition\n\n## Key Points\n- \n\n## Links\n- ",
            ),
            (
                "meeting",
                "## Attendees\n- \n\n## Notes\n\n## Action Items\n- [ ] ",
            ),
            ("idea", "## Description\n\n## Why\n\n## Next Steps\n- "),
            ("daily", "## Notes\n- \n\n## Tasks\n- [ ] \n\n## Links\n- "),
        ];
        for (page_type, content) in &defaults {
            let path = templates_dir.join(format!("{page_type}.md"));
            if !path.exists() {
                let _ = std::fs::write(path, content);
            }
        }
    }

    /// Generate a unique slug, appending -2, -3, etc. on collision.
    fn unique_slug(&self, dir: &str, base: &str) -> Result<String, String> {
        let slug = format!("{dir}/{base}");
        if !self.vault.page_exists(&slug) {
            return Ok(slug);
        }

        for i in 2..100 {
            let candidate = format!("{dir}/{base}-{i}");
            if !self.vault.page_exists(&candidate) {
                return Ok(candidate);
            }
        }

        Err(format!("Too many slug collisions for {dir}/{base}"))
    }

    /// Insert a page into the brain_pages table and brain_fts index.
    fn index_page(&self, entry: &IndexEntry) -> Result<(), String> {
        let conn = self.db.conn()?;

        conn.execute(
            "INSERT OR REPLACE INTO brain_pages (slug, page_type, title, frontmatter, content_hash, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![entry.slug, entry.page_type, entry.title, entry.properties, entry.hash, entry.created_at, entry.updated_at],
        )
        .map_err(|e| format!("Failed to index page: {e}"))?;

        // Two processes indexing one outside edit can both get past the hash check; the
        // delete keeps the page to one full-text row whichever inserts last.
        conn.execute(
            "DELETE FROM brain_fts WHERE slug = ?1",
            rusqlite::params![entry.slug],
        )
        .map_err(|e| format!("Failed to index FTS: {e}"))?;
        conn.execute(
            "INSERT INTO brain_fts (slug, title, content, page_type) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![entry.slug, entry.title, entry.content, entry.page_type],
        )
        .map_err(|e| format!("Failed to index FTS: {e}"))?;

        // For other processes, on this connection (TICKET-035).
        changes::record(&conn, "page", entry.slug, entry.op, "", entry.hash);

        // The wiki links, on the same connection guard (a second take would deadlock).
        index_links(&conn, entry.slug, entry.content)
    }

    /// A page that left the vault: its index rows and every link it made, typed edges
    /// included, so it is no longer anyone's backlink. A page about to be re-indexed
    /// goes through [`Self::remove_from_index`] alone, which keeps its typed edges.
    fn forget_page(&self, slug: &str) -> Result<(), String> {
        self.remove_from_index(slug)?;
        let conn = self.db.conn()?;
        conn.execute(
            "DELETE FROM brain_links WHERE from_slug = ?1",
            rusqlite::params![slug],
        )
        .map_err(|e| format!("Failed to remove links: {e}"))?;
        changes::record(&conn, "page", slug, "deleted", "", "");
        Ok(())
    }

    /// Remove a page from the brain_pages table, brain_fts, aliases, and tags.
    fn remove_from_index(&self, slug: &str) -> Result<(), String> {
        let conn = self.db.conn()?;
        conn.execute(
            "DELETE FROM brain_fts WHERE slug = ?1",
            rusqlite::params![slug],
        )
        .map_err(|e| format!("Failed to remove FTS: {e}"))?;
        conn.execute(
            "DELETE FROM brain_aliases WHERE slug = ?1",
            rusqlite::params![slug],
        )
        .map_err(|e| format!("Failed to remove aliases: {e}"))?;
        conn.execute(
            "DELETE FROM brain_tags WHERE slug = ?1",
            rusqlite::params![slug],
        )
        .map_err(|e| format!("Failed to remove tags: {e}"))?;
        conn.execute(
            "DELETE FROM brain_pages WHERE slug = ?1",
            rusqlite::params![slug],
        )
        .map_err(|e| format!("Failed to remove page: {e}"))?;
        Ok(())
    }

    /// Get the stored content hash for a slug.
    #[allow(dead_code)] // Used by sync_page which is reserved for future phases
    fn get_content_hash(&self, slug: &str) -> Option<String> {
        let conn = self.db.conn().ok()?;
        conn.query_row(
            "SELECT content_hash FROM brain_pages WHERE slug = ?1",
            rusqlite::params![slug],
            |row| row.get(0),
        )
        .ok()
    }

    /// Get created_at and updated_at timestamps for a slug.
    fn get_timestamps(&self, slug: &str) -> Option<(i64, i64)> {
        let conn = self.db.conn().ok()?;
        conn.query_row(
            "SELECT created_at, updated_at FROM brain_pages WHERE slug = ?1",
            rusqlite::params![slug],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok()
    }

    /// Create a version snapshot of the current page content.
    fn create_version(
        &self,
        slug: &str,
        content: &str,
        fm: &BrainFrontmatter,
    ) -> Result<(), String> {
        let conn = self.db.conn()?;
        let fm_json = serde_json::to_string(fm).unwrap_or_default();
        let now = unix_now();
        conn.execute(
            "INSERT INTO brain_versions (slug, content, frontmatter, snapshot_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![slug, content, fm_json, now],
        )
        .map_err(|e| format!("Failed to create version: {e}"))?;
        Ok(())
    }

    /// Sync aliases from frontmatter into brain_aliases table.
    fn sync_aliases(&self, slug: &str, aliases: &[String]) -> Result<(), String> {
        let conn = self.db.conn()?;
        conn.execute(
            "DELETE FROM brain_aliases WHERE slug = ?1",
            rusqlite::params![slug],
        )
        .map_err(|e| format!("Failed to clear aliases: {e}"))?;

        for alias in aliases {
            conn.execute(
                "INSERT OR IGNORE INTO brain_aliases (slug, alias) VALUES (?1, ?2)",
                rusqlite::params![slug, alias],
            )
            .map_err(|e| format!("Failed to insert alias: {e}"))?;
        }
        Ok(())
    }

    /// Sync a page's tags into `brain_tags`: the frontmatter list and the inline `#tags`
    /// of `content`, deduplicated without case, stored as first written.
    fn sync_tags(&self, slug: &str, tags: &[String], content: &str) -> Result<(), String> {
        let mut all: Vec<String> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for tag in tags
            .iter()
            .map(|t| {
                t.trim()
                    .trim_start_matches('#')
                    .trim_end_matches('/')
                    .to_string()
            })
            .chain(links::tags(content))
        {
            if !tag.is_empty() && seen.insert(tag.to_lowercase()) {
                all.push(tag);
            }
        }
        let conn = self.db.conn()?;
        conn.execute(
            "DELETE FROM brain_tags WHERE slug = ?1",
            rusqlite::params![slug],
        )
        .map_err(|e| format!("Failed to clear tags: {e}"))?;
        for tag in all {
            conn.execute(
                "INSERT OR IGNORE INTO brain_tags (slug, tag) VALUES (?1, ?2)",
                rusqlite::params![slug, tag],
            )
            .map_err(|e| format!("Failed to insert tag: {e}"))?;
        }
        Ok(())
    }
}

/// Data needed to insert a page into the SQLite index.
struct IndexEntry<'a> {
    slug: &'a str,
    page_type: &'a str,
    title: &'a str,
    content: &'a str,
    hash: &'a str,
    created_at: i64,
    updated_at: i64,
    /// The page's properties as a JSON object, kept in `brain_pages.frontmatter`.
    properties: &'a str,
    /// `created` or `updated`, for the change log (TICKET-035).
    op: &'a str,
}

/// A page's frontmatter properties as a JSON object, for the index (TICKET-047).
fn properties_json(raw: &str) -> String {
    let map: serde_json::Map<String, serde_json::Value> = properties_of(raw).into_iter().collect();
    serde_json::Value::Object(map).to_string()
}

/// `date` as `YYYY-MM-DD`, defaulting to today.
fn normalize_date(date: Option<&str>) -> Result<String, String> {
    let date = match date.map(str::trim).filter(|d| !d.is_empty()) {
        Some(d) => d.to_string(),
        None => today_iso(),
    };
    let bytes = date.as_bytes();
    let shaped = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && date
            .chars()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
    if !shaped {
        return Err(format!("Date must be YYYY-MM-DD, got '{date}'"));
    }
    Ok(date)
}

/// What every wikilink target in the vault can resolve to.
pub(crate) struct LinkIndex {
    slugs: std::collections::HashSet<String>,
    /// lowercase basename → slugs
    by_basename: std::collections::HashMap<String, Vec<String>>,
    /// lowercase title or alias → slugs
    by_name: std::collections::HashMap<String, Vec<String>>,
}

impl LinkIndex {
    pub(crate) fn build(files: &[(String, PathBuf)]) -> Self {
        let mut index = LinkIndex {
            slugs: Default::default(),
            by_basename: Default::default(),
            by_name: Default::default(),
        };
        for (slug, path) in files {
            index.slugs.insert(slug.clone());
            let basename = slug.rsplit('/').next().unwrap_or(slug).to_lowercase();
            index
                .by_basename
                .entry(basename)
                .or_default()
                .push(slug.clone());
            let Ok(raw) = std::fs::read_to_string(path) else {
                continue;
            };
            let Ok(parsed) = parse_page(&raw) else {
                continue;
            };
            let mut names = vec![parsed.frontmatter.title.clone()];
            names.extend(parsed.frontmatter.aliases.iter().cloned());
            for name in names {
                let key = name.trim().to_lowercase();
                if key.is_empty() {
                    continue;
                }
                let entry = index.by_name.entry(key).or_default();
                if !entry.contains(slug) {
                    entry.push(slug.clone());
                }
            }
        }
        index
    }

    /// The slug a link target names: an exact slug, else a unique basename, else a
    /// unique title or alias. `None` when nothing (or more than one page) matches.
    fn resolve(&self, target: &str) -> Option<String> {
        let t = target.trim().trim_start_matches('/');
        let t = t.strip_suffix(".md").unwrap_or(t);
        if t.is_empty() {
            return None;
        }
        if self.slugs.contains(t) {
            return Some(t.to_string());
        }
        let key = t.to_lowercase();
        for table in [&self.by_basename, &self.by_name] {
            if let Some(v) = table.get(&key) {
                if v.len() == 1 {
                    return Some(v[0].clone());
                }
            }
        }
        None
    }

    /// Rewrite every `[[target]]` whose target resolves to a slug spelled differently.
    /// A title or alias used as the target becomes the display text, so the prose reads
    /// as before. Returns the text, how many links changed, and the unresolved targets.
    pub(crate) fn rewrite_links(&self, text: &str) -> (String, usize, Vec<String>) {
        let mut out = String::with_capacity(text.len());
        let mut rewritten = 0;
        let mut unresolved = Vec::new();
        let mut rest = text;
        while let Some(start) = rest.find("[[") {
            let Some(end) = rest[start + 2..].find("]]") else {
                break;
            };
            let inner = &rest[start + 2..start + 2 + end];
            out.push_str(&rest[..start]);
            let cut = inner.find(['#', '|']).unwrap_or(inner.len());
            let (target, tail) = inner.split_at(cut);
            match self.resolve(target) {
                Some(slug) if slug != target.trim() => {
                    let basename = slug.rsplit('/').next().unwrap_or(&slug);
                    let has_display = tail.contains('|');
                    let keep_words = !has_display && !target.trim().eq_ignore_ascii_case(basename);
                    if keep_words {
                        out.push_str(&format!("[[{slug}{tail}|{}]]", target.trim()));
                    } else {
                        out.push_str(&format!("[[{slug}{tail}]]"));
                    }
                    rewritten += 1;
                }
                Some(_) => out.push_str(&format!("[[{inner}]]")),
                None => {
                    if !target.trim().is_empty() && !unresolved.contains(&target.trim().to_string())
                    {
                        unresolved.push(target.trim().to_string());
                    }
                    out.push_str(&format!("[[{inner}]]"));
                }
            }
            rest = &rest[start + 2 + end + 2..];
        }
        out.push_str(rest);
        (out, rewritten, unresolved)
    }
}

/// Compute SHA-256 hash of content.
fn compute_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Get current Unix timestamp.
fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Replace a page's wiki-link rows from its content: every distinct target, resolved
/// through the index when it can be, with the line it sits on as context.
fn index_links(conn: &rusqlite::Connection, slug: &str, content: &str) -> Result<(), String> {
    conn.execute(
        "DELETE FROM brain_links WHERE from_slug = ?1 AND link_type = 'reference'",
        rusqlite::params![slug],
    )
    .map_err(|e| format!("Failed to clear wiki links: {e}"))?;
    let now = unix_now();
    let mut seen = std::collections::HashSet::new();
    for link in scan_links(content) {
        if link.target.is_empty() {
            continue;
        }
        let to_slug =
            resolve_on(conn, &link.target).unwrap_or_else(|| links::normalise_target(&link.target));
        if to_slug == slug || !seen.insert(to_slug.clone()) {
            continue;
        }
        conn.execute(
            "INSERT OR IGNORE INTO brain_links (from_slug, to_slug, link_type, context, created_at) \
             VALUES (?1, ?2, 'reference', ?3, ?4)",
            rusqlite::params![slug, to_slug, link.line, now],
        )
        .map_err(|e| format!("Failed to insert wiki link: {e}"))?;
    }
    Ok(())
}

/// Split a query into its plain words and its `tag:` terms (`tag:a/b`, `tag:#a`).
/// Take a query apart: `tag:`, `path:`, `file:` and `type:` terms (a value in quotes
/// may hold spaces; a leading `-` excludes) and the words that remain, which keep their
/// quotes for FTS5 phrases. A term with another prefix, such as a URL, stays a word.
pub fn parse_query(query: &str) -> ParsedQuery {
    let mut out = ParsedQuery::default();
    let mut words: Vec<String> = Vec::new();
    for token in query_tokens(query) {
        let (negated, body) = match token.strip_prefix('-') {
            Some(rest) if rest.contains(':') => (true, rest),
            _ => (false, token.as_str()),
        };
        let Some((op, raw)) = body.split_once(':') else {
            words.push(token.clone());
            continue;
        };
        let list = match (op, negated) {
            ("tag", false) => &mut out.tags,
            ("tag", true) => &mut out.not_tags,
            ("path", false) => &mut out.paths,
            ("path", true) => &mut out.not_paths,
            ("file", false) => &mut out.files,
            ("file", true) => &mut out.not_files,
            ("type", false) => &mut out.types,
            ("type", true) => &mut out.not_types,
            _ => {
                words.push(token.clone());
                continue;
            }
        };
        let value = raw.trim_matches('"').trim();
        let value = if op == "tag" {
            value.trim_start_matches('#').trim_end_matches('/')
        } else {
            value
        };
        if !value.is_empty() {
            list.push(value.to_string());
        }
    }
    out.words = words.join(" ");
    out
}

/// Split on whitespace outside double quotes; the quotes stay in the token.
fn query_tokens(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in query.chars() {
        if c == '"' {
            quoted = !quoted;
            current.push(c);
        } else if c.is_whitespace() && !quoted {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// A stretch of text around `range`, the match wrapped in `<b>`, line breaks as
/// spaces, an ellipsis where it is cut.
fn snippet_around(text: &str, range: std::ops::Range<usize>) -> String {
    let mut start = range.start.saturating_sub(60);
    while !text.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = (range.end + 100).min(text.len());
    while !text.is_char_boundary(end) {
        end += 1;
    }
    let flat = |s: &str| s.replace(['\n', '\r'], " ");
    let mut out = String::new();
    if start > 0 {
        out.push_str("...");
    }
    out.push_str(&flat(&text[start..range.start]));
    out.push_str("<b>");
    out.push_str(&flat(&text[range.start..range.end]));
    out.push_str("</b>");
    out.push_str(&flat(&text[range.end..end]));
    if end < text.len() {
        out.push_str("...");
    }
    out
}

/// Row mapper for link rows selected as `from, to, type, context, resolved`.
fn row_to_link(row: &rusqlite::Row) -> rusqlite::Result<LinkEntry> {
    Ok(LinkEntry {
        from_slug: row.get(0)?,
        to_slug: row.get(1)?,
        link_type: row.get(2)?,
        context: row.get(3)?,
        resolved: row.get::<_, i64>(4)? != 0,
    })
}

/// The slug a link target names, from the index: the exact slug (case-insensitive),
/// else a unique file name anywhere in the vault, else a unique title or alias.
fn resolve_on(conn: &rusqlite::Connection, target: &str) -> Option<String> {
    let t = links::normalise_target(target);
    if t.is_empty() {
        return None;
    }
    if let Ok(slug) = conn.query_row(
        "SELECT slug FROM brain_pages WHERE LOWER(slug) = LOWER(?1)",
        rusqlite::params![t],
        |row| row.get::<_, String>(0),
    ) {
        return Some(slug);
    }
    let unique = |sql: &str| -> Option<String> {
        let mut stmt = conn.prepare(sql).ok()?;
        let rows: Vec<String> = stmt
            .query_map(rusqlite::params![t], |row| row.get(0))
            .ok()?
            .flatten()
            .collect();
        if rows.len() == 1 {
            Some(rows[0].clone())
        } else {
            None
        }
    };
    if !t.contains('/') {
        if let Some(slug) = unique(
            "SELECT slug FROM brain_pages \
             WHERE LOWER(substr(slug, length(slug) - length(?1) + 1)) = LOWER(?1) \
             AND substr(slug, length(slug) - length(?1), 1) = '/' LIMIT 2",
        ) {
            return Some(slug);
        }
    }
    unique(
        "SELECT slug FROM brain_pages WHERE LOWER(title) = LOWER(?1) \
         UNION SELECT slug FROM brain_aliases WHERE LOWER(alias) = LOWER(?1) LIMIT 2",
    )
}

/// The renderer's view of the vault: the index for resolution, the disk for content.
struct DbResolver<'a> {
    brain: &'a BrainManager,
}

impl Resolver for DbResolver<'_> {
    fn resolve(&self, target: &str) -> Option<String> {
        let t = links::normalise_target(target);
        if self.brain.vault.page_exists(&t) {
            return Some(t);
        }
        let conn = self.brain.db.conn().ok()?;
        resolve_on(&conn, &t)
    }

    fn page(&self, slug: &str) -> Option<(String, String)> {
        let raw = self.brain.vault.read_page(slug).ok()??;
        let mut parsed = parse_lenient(&raw);
        parsed.frontmatter.fill_defaults(slug);
        Some((parsed.frontmatter.title, raw))
    }

    fn file_url(&self, target: &str) -> Option<String> {
        self.brain
            .vault
            .find_file(target)
            .map(|p| format!("file://{}", p.display()))
    }
}

/// Row mapper for FTS5 search results.
fn row_to_search_result(row: &rusqlite::Row) -> rusqlite::Result<BrainSearchResult> {
    Ok(BrainSearchResult {
        slug: row.get(0)?,
        page_type: row.get(1)?,
        title: row.get(2)?,
        snippet: row.get(3)?,
        rank: row.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::db::Database;
    use rusqlite::Connection;
    use std::fs;

    fn test_brain(name: &str) -> (PathBuf, BrainManager) {
        let dir =
            std::env::temp_dir().join(format!("rusty_brain_test_{}_{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let conn = Connection::open_in_memory().unwrap();
        let db = Database::from_conn(conn);
        db.migrate().unwrap();

        let bm = BrainManager::new(Arc::new(db), dir.clone());
        bm.ensure_vault().unwrap();
        (dir, bm)
    }

    fn cleanup(dir: &std::path::Path) {
        let _ = fs::remove_dir_all(dir);
    }

    /// The frontmatter prefix of a raw page, fences included.
    fn front(raw: &str) -> &str {
        split_raw(raw).unwrap().0
    }

    const HAND_WRITTEN: &str = "---\n# kept by hand\ntitle: launch\ntype: project\nzeta: \"quoted\"\nalpha: 1   # odd spacing\nupdated: 2026-01-01\ntags:\n  - x\n---\n\nBody with [[people/nobody]].\n";

    /// How many rows of `table` have a `column` starting with `prefix`.
    fn rows_under(bm: &BrainManager, table: &str, column: &str, prefix: &str) -> i64 {
        let conn = bm.db.conn().unwrap();
        conn.query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE {column} LIKE ?1"),
            rusqlite::params![format!("{prefix}%")],
            |row| row.get(0),
        )
        .unwrap()
    }

    #[test]
    fn list_pages_carries_aliases_and_asked_properties() {
        let (dir, bm) = test_brain("list_props");
        bm.write_raw(
            "projects/orbit",
            "---\ntitle: Orbit\ntype: project\naliases: [Launcher, Orb]\npath: /srv/orbit\ntask_group: 3\n---\nBody.\n",
        )
        .unwrap();
        bm.write_raw("projects/plain", "---\ntitle: Plain\ntype: project\n---\n")
            .unwrap();
        let plain = bm.list_pages(Some("project"), None).unwrap();
        let orbit = plain.iter().find(|p| p.slug == "projects/orbit").unwrap();
        assert_eq!(orbit.aliases, vec!["Launcher", "Orb"]);
        assert!(orbit.properties.is_none());
        let json = serde_json::to_value(orbit).unwrap();
        assert!(json.get("properties").is_none(), "{json}");

        let names = vec!["path".to_string(), "task_group".to_string()];
        let with = bm.list_pages_with(Some("project"), None, &names).unwrap();
        let orbit = with.iter().find(|p| p.slug == "projects/orbit").unwrap();
        let props = orbit.properties.as_ref().unwrap();
        assert_eq!(props["path"], serde_json::json!("/srv/orbit"));
        assert_eq!(props["task_group"], serde_json::json!(3));
        let plain = with.iter().find(|p| p.slug == "projects/plain").unwrap();
        assert!(plain.properties.as_ref().unwrap().is_empty());
        assert!(plain.aliases.is_empty());
        cleanup(&dir);
    }

    #[test]
    fn an_unchanged_page_gets_its_properties_backfilled() {
        let (dir, bm) = test_brain("props_backfill");
        bm.write_raw(
            "projects/orbit",
            "---\ntitle: Orbit\npath: /srv/orbit\n---\n",
        )
        .unwrap();
        // As the index was before TICKET-047.
        bm.db
            .conn()
            .unwrap()
            .execute("UPDATE brain_pages SET frontmatter = '{}'", [])
            .unwrap();
        let names = vec!["path".to_string()];
        let before = bm.list_pages_with(None, None, &names).unwrap();
        assert!(before[0].properties.as_ref().unwrap().is_empty());
        bm.sync_all().unwrap();
        let after = bm.list_pages_with(None, None, &names).unwrap();
        assert_eq!(
            after[0].properties.as_ref().unwrap()["path"],
            serde_json::json!("/srv/orbit")
        );
        cleanup(&dir);
    }

    #[test]
    fn pages_and_stats_carry_absolute_paths() {
        let (dir, _) = test_brain("absolute_paths");
        // The same vault opened through a relative path, as a relative
        // `brain_vault_path` setting would give it.
        let cwd = std::env::current_dir().unwrap();
        let relative = pathdiff_rel(&dir, &cwd);
        let conn = Connection::open_in_memory().unwrap();
        let db = Database::from_conn(conn);
        db.migrate().unwrap();
        let bm = BrainManager::new(Arc::new(db), relative.clone());
        assert!(relative.is_relative(), "{}", relative.display());
        let root = dir.canonicalize().unwrap();
        let page = bm.create_page("person", "Ada", "Hello.").unwrap();
        assert_eq!(page.file, root.join("people/ada.md").to_string_lossy());
        assert_eq!(bm.read_page("people/ada").unwrap().unwrap().file, page.file);
        assert_eq!(
            bm.update_page("people/ada", "Again.").unwrap().file,
            page.file
        );
        let style = render::Style::default();
        assert_eq!(
            bm.render_page("people/ada", &style).unwrap().unwrap().file,
            page.file
        );
        assert_eq!(bm.render_text("# x", &style).file, "");
        assert_eq!(bm.stats().unwrap().vault_root, root.to_string_lossy());
        cleanup(&dir);
    }

    /// `path` relative to `base`, through `..` where needed.
    fn pathdiff_rel(path: &std::path::Path, base: &std::path::Path) -> PathBuf {
        let path = path.canonicalize().unwrap();
        let base = base.canonicalize().unwrap();
        let common = path
            .components()
            .zip(base.components())
            .take_while(|(a, b)| a == b)
            .count();
        let mut out = PathBuf::new();
        for _ in base.components().skip(common) {
            out.push("..");
        }
        for part in path.components().skip(common) {
            out.push(part);
        }
        out
    }

    #[test]
    fn deleted_folders_stay_out_of_the_index() {
        let (dir, bm) = test_brain("archive_index");
        bm.write_raw(
            "zones/plan",
            "---\ntags: [zone]\n---\nSee [[people/sarah]].\n",
        )
        .unwrap();
        bm.write_raw("people/sarah", "Sarah.\n").unwrap();
        bm.add_link("zones/plan", "people/sarah", Some("owns"), None)
            .unwrap();
        bm.delete_folder("zones").unwrap();
        bm.sync_all().unwrap();
        for prefix in ["archive/", "zones/"] {
            assert_eq!(
                rows_under(&bm, "brain_pages", "slug", prefix),
                0,
                "{prefix}"
            );
            assert_eq!(
                rows_under(&bm, "brain_links", "from_slug", prefix),
                0,
                "{prefix}"
            );
            assert_eq!(rows_under(&bm, "brain_tags", "slug", prefix), 0, "{prefix}");
            assert_eq!(
                rows_under(&bm, "brain_chunks", "slug", prefix),
                0,
                "{prefix}"
            );
        }
        assert!(bm.get_links("people/sarah").unwrap().backlinks.is_empty());
        cleanup(&dir);
    }

    #[test]
    fn sync_drops_archived_pages_and_ghost_links() {
        let (dir, bm) = test_brain("archive_sync");
        bm.write_raw("people/sarah", "Sarah.\n").unwrap();
        // An index written before the fix: an archived page indexed, and a link row
        // from a page that is gone.
        fs::create_dir_all(dir.join("archive/zones_1")).unwrap();
        fs::write(
            dir.join("archive/zones_1/plan.md"),
            "See [[people/sarah]].\n",
        )
        .unwrap();
        bm.sync_page("archive/zones_1/plan").unwrap();
        bm.add_link("projects/gone", "people/sarah", None, None)
            .unwrap();
        assert_eq!(rows_under(&bm, "brain_pages", "slug", "archive/"), 1);
        assert_eq!(bm.get_links("people/sarah").unwrap().backlinks.len(), 2);

        bm.sync_all().unwrap();
        assert_eq!(rows_under(&bm, "brain_pages", "slug", "archive/"), 0);
        assert_eq!(rows_under(&bm, "brain_links", "from_slug", "archive/"), 0);
        assert!(bm.get_links("people/sarah").unwrap().backlinks.is_empty());
        cleanup(&dir);
    }

    #[test]
    fn deleting_a_page_drops_its_links_but_an_update_keeps_typed_ones() {
        let (dir, bm) = test_brain("forget_links");
        bm.write_raw("people/alice", "Alice at [[companies/acme]].\n")
            .unwrap();
        bm.write_raw("companies/acme", "Acme.\n").unwrap();
        bm.add_link("people/alice", "companies/acme", Some("works_at"), None)
            .unwrap();
        bm.update_page("people/alice", "Alice, still at [[companies/acme]].")
            .unwrap();
        let kinds: Vec<String> = bm
            .get_links("people/alice")
            .unwrap()
            .outbound
            .into_iter()
            .map(|l| l.link_type)
            .collect();
        assert!(kinds.contains(&"works_at".to_string()), "{kinds:?}");
        bm.delete_page("people/alice").unwrap();
        assert!(bm.get_links("companies/acme").unwrap().backlinks.is_empty());
        cleanup(&dir);
    }

    /// Run git in `dir`; its stdout.
    fn git(dir: &std::path::Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// A brain whose vault is a repository with a local identity and one commit.
    fn committed_brain(name: &str) -> (PathBuf, BrainManager) {
        let (dir, bm) = test_brain(name);
        git(&dir, &["config", "user.email", "test@example.invalid"]);
        git(&dir, &["config", "user.name", "Test"]);
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-qm", "base", "--no-gpg-sign"]);
        (dir, bm)
    }

    #[test]
    fn a_tool_commit_holds_only_its_paths() {
        let (dir, bm) = committed_brain("commit_paths");
        bm.write_raw("ideas/mine", "Mine.\n").unwrap();
        bm.flush_commits();
        fs::write(dir.join("ideas/outside.md"), "Typed in an editor.\n").unwrap();
        bm.update_page("ideas/mine", "Mine, changed.").unwrap();
        bm.flush_commits();
        let files = git(&dir, &["show", "--name-only", "--format=", "HEAD"]);
        assert_eq!(files.trim(), "ideas/mine.md");
        assert!(git(&dir, &["log", "-1", "--format=%s"]).starts_with("update: "));
        assert!(git(&dir, &["status", "--porcelain"]).contains("ideas/outside.md"));
        cleanup(&dir);
    }

    #[test]
    fn an_outside_edit_gets_its_own_commit() {
        let (dir, bm) = committed_brain("commit_outside");
        fs::write(dir.join("ideas/outside.md"), "Typed in an editor.\n").unwrap();
        let committed = bm.commit_outside_edits().unwrap();
        assert_eq!(committed, vec!["ideas/outside.md"]);
        assert_eq!(
            git(&dir, &["log", "-1", "--format=%s"]).trim(),
            "edit outside the tools: ideas/outside.md"
        );
        assert_eq!(git(&dir, &["status", "--porcelain"]), "");
        assert!(
            bm.commit_outside_edits().unwrap().is_empty(),
            "nothing left"
        );
        cleanup(&dir);
    }

    #[test]
    fn a_page_named_like_a_pattern_commits_only_itself() {
        let (dir, bm) = committed_brain("commit_literal");
        fs::write(dir.join("ideas/other.md"), "Not mine.\n").unwrap();
        bm.write_raw("ideas/o*", "Starred.\n").unwrap();
        bm.flush_commits();
        let files = git(&dir, &["show", "--name-only", "--format=", "HEAD"]);
        assert_eq!(files.trim(), "ideas/o*.md");
        assert!(git(&dir, &["status", "--porcelain"]).contains("ideas/other.md"));
        cleanup(&dir);
    }

    #[test]
    fn two_processes_commit_an_outside_edit_once() {
        let (dir, first) = committed_brain("commit_twice");
        let conn = Connection::open_in_memory().unwrap();
        let db = Database::from_conn(conn);
        db.migrate().unwrap();
        let second = BrainManager::new(Arc::new(db), dir.clone());
        let before = git(&dir, &["rev-list", "--count", "HEAD"]);
        fs::write(dir.join("ideas/outside.md"), "Once.\n").unwrap();
        assert_eq!(first.commit_outside_edits().unwrap().len(), 1);
        assert!(second.commit_outside_edits().unwrap().is_empty());
        let after = git(&dir, &["rev-list", "--count", "HEAD"]);
        assert_eq!(
            after.trim().parse::<i64>().unwrap(),
            before.trim().parse::<i64>().unwrap() + 1
        );
        cleanup(&dir);
    }

    #[test]
    fn a_failed_commit_is_reported_and_picked_up_later() {
        let (dir, bm) = committed_brain("commit_locked");
        let lock = dir.join(".git/index.lock");
        fs::write(&lock, "").unwrap();
        // A tool write whose commit loses the race, and an outside edit beside it.
        bm.write_raw("ideas/mine", "Mine.\n").unwrap();
        bm.flush_commits();
        fs::write(dir.join("ideas/outside.md"), "Typed.\n").unwrap();
        let err = bm.commit_outside_edits().unwrap_err();
        assert!(err.contains("index.lock"), "{err}");
        fs::remove_file(&lock).unwrap();
        let mut committed = bm.commit_outside_edits().unwrap();
        committed.sort();
        assert_eq!(committed, vec!["ideas/mine.md", "ideas/outside.md"]);
        assert_eq!(git(&dir, &["status", "--porcelain"]), "");
        cleanup(&dir);
    }

    fn bookmark(kind: &str, path: &str) -> bookmarks::Bookmark {
        bookmarks::Bookmark {
            kind: kind.to_string(),
            path: path.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn bookmarks_live_in_a_tracked_vault_file() {
        let (dir, bm) = committed_brain("bookmarks_file");
        let start = crate::engine::changes::since(&bm.db, None, None)
            .unwrap()
            .cursor;
        let mut search = bookmark("search", "");
        search.query = "tag:launcher".to_string();
        let list = bm
            .set_bookmarks(vec![
                bookmark("file", "ideas/a"),
                search.clone(),
                bookmark("file", "ideas/a"),
            ])
            .unwrap();
        assert_eq!(list.len(), 2, "a repeated bookmark is kept once");
        assert_eq!(list[0].title, "a", "a missing title comes from the path");
        bm.flush_commits();
        let text = fs::read_to_string(dir.join(".rusty/bookmarks.json")).unwrap();
        let stored: Vec<bookmarks::Bookmark> = serde_json::from_str(&text).unwrap();
        assert_eq!(stored, list);
        assert!(
            !text.contains("\"query\": \"\""),
            "empty fields stay out: {text}"
        );
        let ignored = std::process::Command::new("git")
            .args(["check-ignore", "-q", ".rusty/bookmarks.json"])
            .current_dir(&dir)
            .status()
            .unwrap();
        assert!(!ignored.success(), "git tracks the bookmarks");
        assert!(git(&dir, &["log", "-1", "--format=%s"]).starts_with("bookmarks:"));
        assert!(bm
            .tree()
            .unwrap()
            .children
            .iter()
            .all(|c| c.path != ".rusty"));
        let logged = crate::engine::changes::since(&bm.db, Some(start), None).unwrap();
        assert!(logged.changes.iter().any(|c| c.kind == "bookmarks"));
        // Add, remove, refuse.
        assert_eq!(
            bm.add_bookmark(bookmark("folder", "concepts"))
                .unwrap()
                .len(),
            3
        );
        assert_eq!(
            bm.add_bookmark(bookmark("folder", "concepts"))
                .unwrap()
                .len(),
            3
        );
        assert_eq!(bm.remove_bookmark(&search).unwrap().len(), 2);
        assert!(bm.remove_bookmark(&search).is_err());
        assert!(bm.add_bookmark(bookmark("nonsense", "x")).is_err());
        assert!(bm.add_bookmark(bookmark("file", "")).is_err());
        cleanup(&dir);
    }

    #[test]
    fn bookmarks_follow_renames_and_go_with_deletes() {
        let (dir, bm) = test_brain("bookmarks_follow");
        bm.write_raw("ideas/a", "# H\n").unwrap();
        bm.write_raw("zones/b", "B.\n").unwrap();
        let mut heading = bookmark("heading", "ideas/a");
        heading.heading = "H".to_string();
        let mut search = bookmark("search", "");
        search.query = "zones".to_string();
        bm.set_bookmarks(vec![
            bookmark("file", "ideas/a"),
            heading,
            bookmark("folder", "zones"),
            bookmark("file", "zones/b"),
            search,
        ])
        .unwrap();
        let paths = |bm: &BrainManager| -> Vec<String> {
            bm.bookmarks()
                .unwrap()
                .into_iter()
                .map(|b| if b.kind == "search" { b.query } else { b.path })
                .collect()
        };
        bm.rename("ideas/a", "ideas/c").unwrap();
        bm.rename("zones", "areas").unwrap();
        assert_eq!(
            paths(&bm),
            ["ideas/c", "ideas/c", "areas", "areas/b", "zones"]
        );
        bm.delete_page("ideas/c").unwrap();
        assert_eq!(paths(&bm), ["areas", "areas/b", "zones"]);
        bm.delete_folder("areas").unwrap();
        assert_eq!(paths(&bm), ["zones"], "the search stays");
        cleanup(&dir);
    }

    #[test]
    fn rename_keeps_the_frontmatter_but_the_title() {
        let (dir, bm) = test_brain("keep_rename");
        bm.write_raw("projects/launch", HAND_WRITTEN).unwrap();
        bm.rename("projects/launch", "projects/liftoff").unwrap();
        let raw = fs::read_to_string(dir.join("projects/liftoff.md")).unwrap();
        assert_eq!(
            front(&raw),
            front(HAND_WRITTEN).replace("title: launch\n", "title: liftoff\n")
        );
        cleanup(&dir);
    }

    #[test]
    fn update_page_changes_only_updated() {
        let (dir, bm) = test_brain("keep_update");
        bm.write_raw("projects/launch", HAND_WRITTEN).unwrap();
        bm.update_page("projects/launch", "A new body.").unwrap();
        let raw = fs::read_to_string(dir.join("projects/launch.md")).unwrap();
        let today = today_iso();
        assert_eq!(
            front(&raw),
            front(HAND_WRITTEN).replace("updated: 2026-01-01\n", &format!("updated: {today}\n"))
        );
        assert!(raw.ends_with("\nA new body.\n"), "{raw}");
        cleanup(&dir);
    }

    #[test]
    fn add_timeline_leaves_the_frontmatter() {
        let (dir, bm) = test_brain("keep_timeline");
        bm.write_raw("projects/launch", HAND_WRITTEN).unwrap();
        bm.add_timeline("projects/launch", "2026-10-04", "test", "Kicked off", None)
            .unwrap();
        let raw = fs::read_to_string(dir.join("projects/launch.md")).unwrap();
        assert_eq!(front(&raw), front(HAND_WRITTEN));
        assert!(raw.contains("## Timeline"), "{raw}");
        assert!(raw.contains("Kicked off"), "{raw}");
        cleanup(&dir);
    }

    #[test]
    fn writers_leave_a_page_without_frontmatter_without_it() {
        let (dir, bm) = test_brain("keep_bare");
        bm.write_raw("ideas/plain", "Just text.\n").unwrap();
        bm.add_timeline("ideas/plain", "2026-10-04", "test", "Noted", None)
            .unwrap();
        bm.update_page("ideas/plain", "New text.").unwrap();
        bm.rename("ideas/plain", "ideas/bare").unwrap();
        let raw = fs::read_to_string(dir.join("ideas/bare.md")).unwrap();
        assert!(!raw.starts_with("---"), "{raw}");
        assert!(raw.starts_with("New text.\n"), "{raw}");
        assert!(raw.contains("Noted"), "{raw}");
        let page = bm.read_page("ideas/bare").unwrap().unwrap();
        assert_eq!(page.title, "bare");
        cleanup(&dir);
    }

    #[test]
    fn tree_render_and_whole_file_edits() {
        let (dir, bm) = test_brain("workspace");
        bm.create_page(
            "project",
            "Orbit",
            "Links [[people/sarah-chen]] and [[nobody]].",
        )
        .unwrap();
        bm.create_page("person", "Sarah Chen", "Works on [[orbit]].")
            .unwrap();
        fs::write(
            dir.join("2026-09-02.md"),
            "# Loose\n\nNo frontmatter, #tagged.\n",
        )
        .unwrap();
        bm.sync_all().unwrap();

        let tree = bm.tree().unwrap();
        assert_eq!(tree.pages, 3);
        assert!(tree
            .children
            .iter()
            .any(|c| c.name == "2026-09-02" && c.kind == "page"));

        let page = bm.read_page("2026-09-02").unwrap().unwrap();
        assert_eq!(page.title, "2026-09-02");
        assert_eq!(page.page_type, "note");

        let rendered = bm
            .render_page("projects/orbit", &Style::default())
            .unwrap()
            .unwrap();
        assert!(rendered
            .rendered
            .html
            .contains("rusty:page/people/sarah-chen"));
        assert_eq!(rendered.rendered.unresolved, vec!["nobody"]);
        assert_eq!(rendered.properties[0].key, "title");
        assert!(rendered.raw.starts_with("---\n"));
        // The bare name resolved through the index.
        let sarah = bm
            .render_page("people/sarah-chen", &Style::default())
            .unwrap()
            .unwrap();
        assert!(sarah.rendered.html.contains("rusty:page/projects/orbit"));
        let links = bm.get_links("people/sarah-chen").unwrap();
        assert_eq!(links.outbound[0].to_slug, "projects/orbit");
        assert!(links.outbound[0].resolved);
        assert_eq!(links.outbound[0].context, "Works on [[orbit]].");
        let back = bm.get_links("projects/orbit").unwrap();
        assert_eq!(back.backlinks.len(), 1);
        assert_eq!(back.backlinks[0].from_slug, "people/sarah-chen");
        let unresolved = bm.unresolved().unwrap();
        assert_eq!(unresolved.len(), 1);
        assert_eq!(unresolved[0].target, "nobody");
        assert_eq!(unresolved[0].from_slug, "projects/orbit");

        // A whole-file edit keeps what the editor did not touch.
        let raw = fs::read_to_string(dir.join("projects/orbit.md")).unwrap();
        let edited = raw.replace("[[nobody]]", "[[somebody]]");
        let page = bm.write_raw("projects/orbit", &edited).unwrap();
        assert_eq!(
            fs::read_to_string(dir.join("projects/orbit.md")).unwrap(),
            edited
        );
        assert_eq!(page.title, "Orbit");
        assert!(bm
            .unresolved()
            .unwrap()
            .iter()
            .any(|u| u.target == "somebody"));
        let same = bm.write_raw("projects/orbit", &edited).unwrap();
        assert_eq!(same.content_hash, page.content_hash);
        assert!(bm
            .search("somebody", None, None)
            .unwrap()
            .iter()
            .any(|r| r.slug == "projects/orbit"));

        cleanup(&dir);
    }

    #[test]
    fn tags_index_search_and_properties() {
        let (dir, bm) = test_brain("tags");
        bm.create_page("project", "Orbit", "Ship it #launch/soon and #Rust.")
            .unwrap();
        bm.write_raw(
            "concepts/tagged",
            "---\ntitle: Tagged\ntype: concept\ntags:\n  - rust\n  - launch\n---\n\nInline #ops here.\n",
        )
        .unwrap();
        bm.create_page("person", "Plain", "No tags at all.")
            .unwrap();

        let tags = bm.tags().unwrap();
        let counts: Vec<(&str, usize)> = tags.iter().map(|t| (t.tag.as_str(), t.count)).collect();
        assert_eq!(
            counts,
            vec![("launch", 2), ("launch/soon", 1), ("ops", 1), ("Rust", 2)]
        );

        let hits = bm.search("tag:launch", None, None).unwrap();
        let slugs: Vec<&str> = hits.iter().map(|h| h.slug.as_str()).collect();
        assert_eq!(slugs.len(), 2);
        assert!(slugs.contains(&"projects/orbit") && slugs.contains(&"concepts/tagged"));
        let hits = bm.search("tag:#launch/soon", None, None).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].slug, "projects/orbit");
        let hits = bm.search("inline tag:rust", None, None).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].slug, "concepts/tagged");
        assert!(bm.search("tag:nothing", None, None).unwrap().is_empty());
        assert_eq!(bm.search("tag:rust tag:ops", None, None).unwrap().len(), 1);
        assert_eq!(
            parse_query("  a tag:x b tag:#y/ "),
            ParsedQuery {
                words: "a b".to_string(),
                tags: vec!["x".to_string(), "y".to_string()],
                ..Default::default()
            }
        );

        let page = bm
            .set_property("projects/orbit", "status", serde_json::json!("active"))
            .unwrap();
        assert_eq!(
            page.frontmatter.extra["status"],
            serde_json::json!("active")
        );
        let raw = fs::read_to_string(dir.join("projects/orbit.md")).unwrap();
        assert!(raw.contains("status: active"), "{raw}");
        assert!(raw.contains("Ship it #launch/soon and #Rust."));
        let props = bm
            .render_page("projects/orbit", &Style::default())
            .unwrap()
            .unwrap()
            .properties;
        let keys: Vec<&str> = props.iter().map(|p| p.key.as_str()).collect();
        assert_eq!(keys.last().copied(), Some("status"));
        let page = bm
            .set_property(
                "projects/orbit",
                "tags",
                serde_json::json!(["Rust", "next"]),
            )
            .unwrap();
        assert_eq!(page.frontmatter.tags, vec!["Rust", "next"]);
        assert!(bm.tags().unwrap().iter().any(|t| t.tag == "next"));
        // The deliberate path (TICKET-024): a tag added or removed through the property
        // shows in the counts and in `tag:` search at once.
        assert_eq!(bm.search("tag:next", None, None).unwrap().len(), 1);
        bm.set_property("projects/orbit", "tags", serde_json::json!(["Rust"]))
            .unwrap();
        assert!(!bm.tags().unwrap().iter().any(|t| t.tag == "next"));
        assert!(bm.search("tag:next", None, None).unwrap().is_empty());
        bm.remove_property("projects/orbit", "status").unwrap();
        assert!(!fs::read_to_string(dir.join("projects/orbit.md"))
            .unwrap()
            .contains("status:"));
        assert!(bm
            .set_property("nope/page", "k", serde_json::json!(1))
            .is_err());

        cleanup(&dir);
    }

    #[test]
    fn search_operators_and_modes() {
        let (_dir, bm) = test_brain("operators");
        bm.create_page("project", "Orbit", "Orbit ships in autumn. #launch")
            .unwrap();
        bm.create_page("person", "Sarah Chen", "Sarah runs the orbit launch.")
            .unwrap();
        bm.create_page("concept", "Theme One", "An orbit theme, the first of four.")
            .unwrap();

        let slugs = |hits: Vec<BrainSearchResult>| -> Vec<String> {
            hits.into_iter().map(|h| h.slug).collect()
        };
        let plain = |q: &str| slugs(bm.search(q, None, None).unwrap());
        assert_eq!(plain("path:projects"), vec!["projects/orbit"]);
        assert_eq!(plain("type:person"), vec!["people/sarah-chen"]);
        assert_eq!(plain("file:theme"), vec!["concepts/theme-one"]);
        assert_eq!(plain("orbit -path:projects").len(), 2);
        assert_eq!(plain("orbit tag:launch"), vec!["projects/orbit"]);
        assert_eq!(plain("orbit -tag:launch").len(), 2);
        assert_eq!(
            plain("orbit type:concept path:\"concepts\""),
            vec!["concepts/theme-one"]
        );
        assert!(plain("orbit type:nothing").is_empty());

        let parsed = parse_query("  a tag:x b -path:\"p q\" file:f type:t -tag:#y/ ");
        assert_eq!(parsed.words, "a b");
        assert_eq!(parsed.tags, vec!["x"]);
        assert_eq!(parsed.not_tags, vec!["y"]);
        assert_eq!(parsed.not_paths, vec!["p q"]);
        assert_eq!(parsed.files, vec!["f"]);
        assert_eq!(parsed.types, vec!["t"]);
        assert_eq!(
            parse_query("\"a phrase\" http://x").words,
            "\"a phrase\" http://x"
        );

        let cased = SearchOptions {
            case_sensitive: true,
            ..Default::default()
        };
        let hits = bm.search_with("Orbit", &cased).unwrap();
        assert_eq!(slugs(hits.clone()), vec!["projects/orbit"]);
        assert!(
            hits[0].snippet.contains("<b>Orbit</b>"),
            "{}",
            hits[0].snippet
        );
        assert_eq!(bm.search_with("orbit", &cased).unwrap().len(), 2);

        let re = SearchOptions {
            regex: true,
            ..Default::default()
        };
        let hits = bm.search_with(r"the\w+ one", &re).unwrap();
        assert_eq!(slugs(hits.clone()), vec!["concepts/theme-one"]);
        assert!(
            hits[0].snippet.contains("<b>Theme One</b>"),
            "{}",
            hits[0].snippet
        );
        let hits = bm.search_with(r"orbit\b type:person", &re).unwrap();
        assert_eq!(slugs(hits), vec!["people/sarah-chen"]);
        assert!(bm.search_with("(", &re).is_err());
    }

    #[test]
    fn graph_nodes_edges_and_neighbourhoods() {
        let (dir, bm) = test_brain("graph");
        bm.create_page(
            "project",
            "Orbit",
            "Links [[people/ann]] and [[nobody]] #core.",
        )
        .unwrap();
        bm.create_page(
            "person",
            "Ann",
            "Works on [[projects/orbit]] and [[concepts/far]].",
        )
        .unwrap();
        bm.create_page("concept", "Far", "Alone but linked.")
            .unwrap();
        bm.create_page("idea", "Island", "No links.").unwrap();
        bm.sync_all().unwrap();

        let g = bm.graph(&GraphOptions::default()).unwrap();
        assert_eq!(g.nodes.len(), 4);
        assert!(g.nodes.iter().all(|n| n.kind == "page"));
        let pairs: Vec<(&str, &str)> = g
            .edges
            .iter()
            .map(|e| (e.from.as_str(), e.to.as_str()))
            .collect();
        assert!(pairs.contains(&("projects/orbit", "people/ann")));
        assert!(pairs.contains(&("people/ann", "projects/orbit")));
        assert!(pairs.contains(&("people/ann", "concepts/far")));
        assert_eq!(pairs.len(), 3);
        let orbit = g.nodes.iter().find(|n| n.id == "projects/orbit").unwrap();
        assert_eq!(orbit.folder, "projects");
        assert_eq!(orbit.tags, vec!["core"]);

        let g = bm
            .graph(&GraphOptions {
                tags: true,
                unresolved: true,
                ..Default::default()
            })
            .unwrap();
        assert!(g
            .nodes
            .iter()
            .any(|n| n.id == "tag:core" && n.kind == "tag" && n.title == "#core"));
        assert!(g
            .nodes
            .iter()
            .any(|n| n.id == "new:nobody" && n.kind == "unresolved"));
        assert!(g
            .edges
            .iter()
            .any(|e| e.from == "projects/orbit" && e.to == "tag:core"));
        assert!(g
            .edges
            .iter()
            .any(|e| e.from == "projects/orbit" && e.to == "new:nobody"));

        let local = bm
            .graph(&GraphOptions {
                around: Some("projects/orbit".into()),
                depth: Some(1),
                ..Default::default()
            })
            .unwrap();
        let ids: Vec<&str> = local.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"projects/orbit") && ids.contains(&"people/ann"));
        let wider = bm
            .graph(&GraphOptions {
                around: Some("projects/orbit".into()),
                depth: Some(2),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(wider.nodes.len(), 3);
        assert!(wider.nodes.iter().any(|n| n.id == "concepts/far"));
        let none = bm
            .graph(&GraphOptions {
                around: Some("missing/page".into()),
                ..Default::default()
            })
            .unwrap();
        assert!(none.nodes.is_empty() && none.edges.is_empty());

        cleanup(&dir);
    }

    #[test]
    fn new_pages_folders_and_moves_rewrite_links() {
        let (dir, bm) = test_brain("moves");
        let a = bm.new_page("projects", None).unwrap();
        let b = bm.new_page("projects", None).unwrap();
        assert_eq!(
            (a.as_str(), b.as_str()),
            ("projects/Untitled", "projects/Untitled 1")
        );
        let page = bm.read_page(&a).unwrap().unwrap();
        assert_eq!(page.page_type, "project");
        assert_eq!(page.title, "Untitled");
        assert!(bm.new_page("nope", None).is_err());
        let root = bm.new_page("", Some("Loose one")).unwrap();
        assert_eq!(root, "Loose one");
        assert_eq!(bm.read_page(&root).unwrap().unwrap().page_type, "note");
        assert_eq!(bm.new_folder("areas/health").unwrap(), "areas/health");

        bm.write_raw("people/alice", "---\ntitle: Alice\ntype: person\n---\n\nSee [[projects/Untitled|the plan]] and [[Untitled 1]] and [[projects/Untitled#Goals]].\n\n```\n[[projects/Untitled]]\n```\n").unwrap();
        bm.write_raw(
            "projects/Untitled 1",
            "---\ntitle: Untitled 1\ntype: project\n---\n\nSibling [[Untitled]].\n",
        )
        .unwrap();
        bm.add_timeline(
            "people/alice",
            "2026-09-02",
            "test",
            "Talked about [[projects/Untitled]]",
            None,
        )
        .unwrap();

        // Rename a page: the title that was the file name follows; links move.
        let report = bm.rename("projects/Untitled", "projects/launch").unwrap();
        assert_eq!(report.kind, "page");
        assert_eq!(report.pages_rewritten, 2, "{report:?}");
        assert!(dir.join("projects/launch.md").exists());
        assert!(!dir.join("projects/Untitled.md").exists());
        let launch = bm.read_page("projects/launch").unwrap().unwrap();
        assert_eq!(launch.title, "launch");
        let alice = fs::read_to_string(dir.join("people/alice.md")).unwrap();
        assert!(alice.contains("[[projects/launch|the plan]]"), "{alice}");
        assert!(alice.contains("[[projects/launch#Goals]]"));
        assert!(alice.contains("[[Untitled 1]]"));
        assert!(alice.contains("```\n[[projects/Untitled]]\n```"));
        assert!(alice.contains("Talked about [[projects/launch]]"));
        let sibling = fs::read_to_string(dir.join("projects/Untitled 1.md")).unwrap();
        assert!(sibling.contains("Sibling [[projects/launch]]"), "{sibling}");
        let links = bm.get_links("projects/launch").unwrap();
        assert_eq!(links.backlinks.len(), 2);
        assert!(bm.get_timeline("projects/launch", None).unwrap().is_empty());
        assert_eq!(bm.get_timeline("people/alice", None).unwrap().len(), 1);
        assert!(bm.read_page("projects/Untitled").unwrap().is_none());
        assert!(bm
            .list_pages(None, None)
            .unwrap()
            .iter()
            .all(|p| p.slug != "projects/Untitled"));

        // Move into a folder with a trailing slash; move a folder.
        let report = bm.rename("projects/launch", "areas/health/").unwrap();
        assert_eq!(report.to, "areas/health/launch");
        assert!(fs::read_to_string(dir.join("people/alice.md"))
            .unwrap()
            .contains("[[areas/health/launch|the plan]]"));
        let report = bm.rename("areas", "zones").unwrap();
        assert_eq!(report.kind, "folder");
        assert!(dir.join("zones/health/launch.md").exists());
        let alice = fs::read_to_string(dir.join("people/alice.md")).unwrap();
        assert!(
            alice.contains("[[zones/health/launch|the plan]]"),
            "{alice}"
        );
        assert!(bm.read_page("zones/health/launch").unwrap().is_some());
        assert_eq!(
            bm.get_links("zones/health/launch").unwrap().backlinks.len(),
            2
        );
        assert!(bm.rename("zones", "people").is_err());
        assert!(bm.rename("missing/page", "x").is_err());

        let archived = bm.delete_folder("zones").unwrap();
        assert!(archived.starts_with("archive/zones_"));
        assert!(bm.read_page("zones/health/launch").unwrap().is_none());
        assert!(bm
            .list_pages(None, None)
            .unwrap()
            .iter()
            .all(|p| !p.slug.starts_with("zones/")));
        assert!(bm
            .unresolved()
            .unwrap()
            .iter()
            .any(|u| u.target == "zones/health/launch"));

        cleanup(&dir);
    }

    #[test]
    fn new_page_at_makes_the_link_target() {
        let (dir, bm) = test_brain("new_page_at");
        let slug = bm.new_page_at("decisions/foo").unwrap();
        assert_eq!(slug, "decisions/foo");
        assert!(dir.join("decisions/foo.md").is_file());
        assert_eq!(bm.read_page("decisions/foo").unwrap().unwrap().title, "foo");
        // A folder that did not exist is made, and `.md` is optional.
        assert_eq!(bm.new_page_at("a/b/Deep one.md").unwrap(), "a/b/Deep one");
        assert!(dir.join("a/b/Deep one.md").is_file());
        // An existing page comes back as it is.
        bm.write_raw("decisions/foo", "Mine.\n").unwrap();
        assert_eq!(bm.new_page_at("decisions/foo").unwrap(), "decisions/foo");
        assert_eq!(
            fs::read_to_string(dir.join("decisions/foo.md")).unwrap(),
            "Mine.\n"
        );
        assert!(!dir.join("decisions/foo 1.md").exists());
        // A link to it resolves.
        bm.write_raw("ideas/x", "See [[decisions/foo]].\n").unwrap();
        let links = bm.get_links("ideas/x").unwrap().outbound;
        assert!(links
            .iter()
            .any(|l| l.to_slug == "decisions/foo" && l.resolved));
        for bad in ["", "  ", "/etc/x", "a/../b", "archive/x", "folder/"] {
            assert!(bm.new_page_at(bad).is_err(), "{bad:?}");
        }
        cleanup(&dir);
    }

    #[test]
    fn create_page_writes_file_and_index() {
        let (dir, bm) = test_brain("create");
        let page = bm
            .create_page("person", "Sarah Chen", "She is a CTO.")
            .unwrap();

        assert_eq!(page.slug, "people/sarah-chen");
        assert_eq!(page.page_type, "person");
        assert_eq!(page.title, "Sarah Chen");
        assert_eq!(page.compiled_truth, "She is a CTO.");

        // File exists on disk
        assert!(dir.join("people/sarah-chen.md").exists());

        // Indexed in DB
        let pages = bm.list_pages(None, None).unwrap();
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].slug, "people/sarah-chen");

        cleanup(&dir);
    }

    #[test]
    fn read_page_returns_parsed_content() {
        let (dir, bm) = test_brain("read");
        bm.create_page(
            "concept",
            "Compiled Truth",
            "A knowledge synthesis pattern.",
        )
        .unwrap();

        let page = bm.read_page("concepts/compiled-truth").unwrap().unwrap();
        assert_eq!(page.title, "Compiled Truth");
        assert_eq!(page.page_type, "concept");
        assert_eq!(page.compiled_truth, "A knowledge synthesis pattern.");
        assert!(page.timeline.is_empty());

        cleanup(&dir);
    }

    #[test]
    fn update_page_creates_version() {
        let (dir, bm) = test_brain("update");
        bm.create_page("person", "Alice", "Version 1.").unwrap();

        let updated = bm.update_page("people/alice", "Version 2.").unwrap();
        assert_eq!(updated.compiled_truth, "Version 2.");

        // Version snapshot exists
        let conn = bm.db.conn().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM brain_versions WHERE slug = ?1",
                rusqlite::params!["people/alice"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        cleanup(&dir);
    }

    #[test]
    fn delete_page_soft_deletes() {
        let (dir, bm) = test_brain("delete");
        bm.create_page("idea", "Bad Idea", "This won't work.")
            .unwrap();

        assert!(bm.read_page("ideas/bad-idea").unwrap().is_some());
        bm.delete_page("ideas/bad-idea").unwrap();

        // File gone from vault, in archive
        assert!(!dir.join("ideas/bad-idea.md").exists());
        let archive_count = fs::read_dir(dir.join("archive"))
            .unwrap()
            .filter_map(|e| e.ok())
            .count();
        assert_eq!(archive_count, 1);

        // Gone from index
        let pages = bm.list_pages(None, None).unwrap();
        assert!(pages.is_empty());

        cleanup(&dir);
    }

    #[test]
    fn list_pages_filters_by_type() {
        let (dir, bm) = test_brain("list_filter");
        bm.create_page("person", "Alice", "Person A.").unwrap();
        bm.create_page("person", "Bob", "Person B.").unwrap();
        bm.create_page("concept", "Testing", "A concept.").unwrap();

        let people = bm.list_pages(Some("person"), None).unwrap();
        assert_eq!(people.len(), 2);

        let concepts = bm.list_pages(Some("concept"), None).unwrap();
        assert_eq!(concepts.len(), 1);

        let all = bm.list_pages(None, None).unwrap();
        assert_eq!(all.len(), 3);

        cleanup(&dir);
    }

    #[test]
    fn search_fts5_basic() {
        let (dir, bm) = test_brain("search");
        bm.create_page(
            "person",
            "Sarah Chen",
            "Expert in distributed systems and Rust programming.",
        )
        .unwrap();
        bm.create_page(
            "concept",
            "MCP Protocol",
            "Model Context Protocol for AI agents.",
        )
        .unwrap();

        let results = bm.search("distributed", None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].slug, "people/sarah-chen");

        let results = bm.search("protocol", None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].slug, "concepts/mcp-protocol");

        cleanup(&dir);
    }

    #[test]
    fn search_fts5_type_filter() {
        let (dir, bm) = test_brain("search_filter");
        bm.create_page("person", "Alice Rust", "Loves Rust programming.")
            .unwrap();
        bm.create_page(
            "concept",
            "Rust Language",
            "A systems programming language called Rust.",
        )
        .unwrap();

        // Search all types
        let all = bm.search("Rust", None, None).unwrap();
        assert_eq!(all.len(), 2);

        // Filter to concepts only
        let concepts = bm.search("Rust", None, Some("concept")).unwrap();
        assert_eq!(concepts.len(), 1);
        assert_eq!(concepts[0].slug, "concepts/rust-language");

        cleanup(&dir);
    }

    #[test]
    fn sync_page_updates_index() {
        let (dir, bm) = test_brain("sync");
        bm.create_page("person", "Sync Test", "Original content.")
            .unwrap();

        // Modify file externally
        let path = dir.join("people/sync-test.md");
        let raw = fs::read_to_string(&path).unwrap();
        let updated_raw = raw.replace("Original content.", "Updated externally.");
        fs::write(&path, updated_raw).unwrap();

        // Sync picks up the change
        bm.sync_page("people/sync-test").unwrap();

        // Search for the new content
        let results = bm.search("externally", None, None).unwrap();
        assert_eq!(results.len(), 1);

        // Old content not findable
        let old_results = bm.search("Original", None, None).unwrap();
        assert!(old_results.is_empty());

        cleanup(&dir);
    }

    #[test]
    fn content_hash_detects_changes() {
        let hash1 = compute_hash("hello world");
        let hash2 = compute_hash("hello world");
        let hash3 = compute_hash("different content");
        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn slug_collision_appends_suffix() {
        let (dir, bm) = test_brain("collision");
        let page1 = bm.create_page("person", "Alice", "First Alice.").unwrap();
        let page2 = bm.create_page("person", "Alice", "Second Alice.").unwrap();

        assert_eq!(page1.slug, "people/alice");
        assert_eq!(page2.slug, "people/alice-2");

        cleanup(&dir);
    }

    #[test]
    fn add_and_get_timeline() {
        let (dir, bm) = test_brain("timeline");
        bm.create_page("person", "Alice", "A person.").unwrap();

        bm.add_timeline(
            "people/alice",
            "2026-04-12",
            "conversation",
            "Discussed Rust",
            None,
        )
        .unwrap();
        bm.add_timeline(
            "people/alice",
            "2026-04-11",
            "meeting",
            "Weekly sync",
            Some("Covered project timeline."),
        )
        .unwrap();

        let entries = bm.get_timeline("people/alice", None).unwrap();
        assert_eq!(entries.len(), 2);
        // Ordered by date DESC
        assert_eq!(entries[0].date, "2026-04-12");
        assert_eq!(entries[0].source, "conversation");
        assert_eq!(entries[1].date, "2026-04-11");

        cleanup(&dir);
    }

    #[test]
    fn add_timeline_nonexistent_page_fails() {
        let (dir, bm) = test_brain("timeline_fail");
        let result = bm.add_timeline("people/nobody", "2026-04-12", "manual", "test", None);
        assert!(result.is_err());
        cleanup(&dir);
    }

    #[test]
    fn add_timeline_writes_through_to_markdown_and_fts() {
        // Regression guard for the "wrote to the DB but never pushed it live" bug:
        // a timeline entry must land in the markdown page AND the FTS index, not
        // only the brain_timeline table.
        let (dir, bm) = test_brain("timeline_writethrough");
        bm.create_page("daily", "2026-06-28", "").unwrap();
        let slug = "daily/2026-06-28";

        bm.add_timeline(
            slug,
            "2026-06-28",
            "terminal",
            "Draft the Quokkanaut launch article",
            Some("Angle: local-first agents."),
        )
        .unwrap();

        // 1. Visible in the markdown timeline section (what `brain read`/GUI render).
        let page = bm.read_page(slug).unwrap().unwrap();
        assert!(
            page.timeline.contains("Quokkanaut"),
            "timeline entry missing from markdown: {:?}",
            page.timeline
        );
        assert!(page.timeline.contains("**2026-06-28**"));
        assert!(page.timeline.contains("local-first agents"));

        // 2. Persisted to disk (so git auto-commit captures it).
        let on_disk = fs::read_to_string(dir.join("daily/2026-06-28.md")).unwrap();
        assert!(on_disk.contains("Quokkanaut"));

        // 3. Findable via FTS — the whole point of "pushing it live".
        let hits = bm.search("Quokkanaut", None, None).unwrap();
        assert!(
            hits.iter().any(|r| r.slug == slug),
            "captured text not searchable via FTS: {hits:?}"
        );

        // 4. Structured row still recorded for the timeline API.
        let entries = bm.get_timeline(slug, None).unwrap();
        assert_eq!(entries.len(), 1);

        cleanup(&dir);
    }

    #[test]
    fn add_timeline_records_wiki_links_without_wiping_existing() {
        // Appending an entry must ADD wiki-link edges from the summary without
        // destroying links the page already has (regression for the index_page
        // rebuild that wiped explicit 'reference' edges).
        let (dir, bm) = test_brain("timeline_links");
        bm.create_page("person", "Alice", "A person.").unwrap();
        bm.create_page("company", "Acme", "A company.").unwrap();
        bm.create_page("project", "Orbit", "A project.").unwrap();

        bm.add_link("people/alice", "companies/acme", Some("works_at"), None)
            .unwrap();
        bm.add_timeline(
            "people/alice",
            "2026-06-28",
            "terminal",
            "Kicked off [[projects/orbit]]",
            None,
        )
        .unwrap();

        let out: Vec<String> = bm
            .get_links("people/alice")
            .unwrap()
            .outbound
            .iter()
            .map(|l| l.to_slug.clone())
            .collect();
        assert!(
            out.contains(&"companies/acme".to_string()),
            "existing link wiped by add_timeline: {out:?}"
        );
        assert!(
            out.contains(&"projects/orbit".to_string()),
            "wiki-link in timeline not recorded: {out:?}"
        );

        cleanup(&dir);
    }

    #[test]
    fn add_and_get_links() {
        let (dir, bm) = test_brain("links");
        bm.create_page("person", "Alice", "A person.").unwrap();
        bm.create_page("company", "Acme", "A company.").unwrap();

        bm.add_link(
            "people/alice",
            "companies/acme",
            Some("works_at"),
            Some("CTO"),
        )
        .unwrap();

        let links = bm.get_links("people/alice").unwrap();
        assert_eq!(links.outbound.len(), 1);
        assert_eq!(links.outbound[0].to_slug, "companies/acme");
        assert_eq!(links.outbound[0].link_type, "works_at");

        let back = bm.get_links("companies/acme").unwrap();
        assert_eq!(back.backlinks.len(), 1);
        assert_eq!(back.backlinks[0].from_slug, "people/alice");

        cleanup(&dir);
    }

    #[test]
    fn remove_link() {
        let (dir, bm) = test_brain("remove_link");
        bm.create_page("person", "Alice", "A person.").unwrap();
        bm.create_page("company", "Acme", "A company.").unwrap();

        bm.add_link("people/alice", "companies/acme", None, None)
            .unwrap();
        assert_eq!(bm.get_links("people/alice").unwrap().outbound.len(), 1);

        bm.remove_link("people/alice", "companies/acme").unwrap();
        assert_eq!(bm.get_links("people/alice").unwrap().outbound.len(), 0);

        cleanup(&dir);
    }

    #[test]
    fn add_and_remove_tags() {
        let (dir, bm) = test_brain("tags");
        bm.create_page("person", "Alice", "A person.").unwrap();

        bm.add_tag("people/alice", "engineering").unwrap();
        bm.add_tag("people/alice", "rust").unwrap();

        // Verify via stats
        let stats = bm.stats().unwrap();
        assert_eq!(stats.tag_count, 2);

        bm.remove_tag("people/alice", "rust").unwrap();
        let stats = bm.stats().unwrap();
        assert_eq!(stats.tag_count, 1);

        cleanup(&dir);
    }

    #[test]
    fn stats_returns_counts() {
        let (dir, bm) = test_brain("stats");
        bm.create_page("person", "Alice", "A person.").unwrap();
        bm.create_page("person", "Bob", "A person.").unwrap();
        bm.create_page("company", "Acme", "A company.").unwrap();

        bm.add_link("people/alice", "companies/acme", None, None)
            .unwrap();
        bm.add_timeline("people/alice", "2026-04-12", "manual", "test", None)
            .unwrap();

        let stats = bm.stats().unwrap();
        assert_eq!(stats.page_count, 3);
        assert_eq!(stats.link_count, 1);
        assert_eq!(stats.timeline_count, 1);
        assert_eq!(*stats.pages_by_type.get("person").unwrap(), 2);
        assert_eq!(*stats.pages_by_type.get("company").unwrap(), 1);

        cleanup(&dir);
    }

    #[test]
    fn resolve_slug_by_title() {
        let (dir, bm) = test_brain("resolve");
        bm.create_page("person", "Sarah Chen", "A person.").unwrap();
        bm.create_page("person", "Sarah Williams", "Another person.")
            .unwrap();

        let results = bm.resolve_slug("Sarah").unwrap();
        assert_eq!(results.len(), 2);

        let results = bm.resolve_slug("Chen").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], "people/sarah-chen");

        cleanup(&dir);
    }

    #[test]
    fn build_context_finds_matching_pages() {
        let (dir, bm) = test_brain("context");
        bm.create_page(
            "person",
            "Sarah Chen",
            "CTO of Acme Corp. Expert in distributed systems.",
        )
        .unwrap();
        bm.create_page("company", "Acme Corp", "Enterprise SaaS platform.")
            .unwrap();

        let ctx = bm.build_context_for_prompt("Tell me about Sarah Chen");
        assert!(ctx.contains("<brain_context>"));
        assert!(ctx.contains("Sarah Chen"));
        assert!(ctx.contains("CTO of Acme"));
        assert!(ctx.contains("</brain_context>"));

        cleanup(&dir);
    }

    #[test]
    fn build_context_empty_when_no_matches() {
        let (dir, bm) = test_brain("context_empty");
        bm.create_page("person", "Alice", "A person.").unwrap();

        let ctx = bm.build_context_for_prompt("something completely unrelated xyz123");
        assert!(ctx.is_empty());

        cleanup(&dir);
    }

    #[test]
    fn parse_wiki_links_basic() {
        let links = links::targets("Links to [[people/alice]] and [[companies/acme]].");
        assert_eq!(links.len(), 2);
        assert!(links.contains(&"people/alice".to_string()));
        assert!(links.contains(&"companies/acme".to_string()));
    }

    #[test]
    fn parse_wiki_links_with_display_text() {
        let links = links::targets("See [[people/alice|Alice]] for details.");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0], "people/alice");
    }

    #[test]
    fn parse_wiki_links_deduplicates() {
        let links = links::targets("First [[people/alice]] then [[people/alice]] again.");
        assert_eq!(links.len(), 1);
    }

    #[test]
    fn parse_wiki_links_empty() {
        let links = links::targets("No links here.");
        assert!(links.is_empty());
    }

    #[test]
    fn wiki_links_stored_on_create() {
        let (dir, bm) = test_brain("wiki_create");
        bm.create_page("person", "Alice", "Works at [[companies/acme]].")
            .unwrap();
        bm.create_page("company", "Acme", "A company.").unwrap();

        let links = bm.get_links("people/alice").unwrap();
        assert_eq!(links.outbound.len(), 1);
        assert_eq!(links.outbound[0].to_slug, "companies/acme");

        // Backlink visible from Acme
        let back = bm.get_links("companies/acme").unwrap();
        assert_eq!(back.backlinks.len(), 1);

        cleanup(&dir);
    }

    #[test]
    fn daily_page_is_created_once() {
        let (dir, bm) = test_brain("daily_page");
        let first = bm.daily_page(Some("2026-09-02")).unwrap();
        assert_eq!(first.slug, "daily/2026-09-02");
        assert_eq!(first.page_type, "daily");
        let again = bm.daily_page(Some("2026-09-02")).unwrap();
        assert_eq!(again.slug, first.slug);
        assert_eq!(bm.list_pages(Some("daily"), None).unwrap().len(), 1);
        assert!(bm.daily_page(Some("yesterday")).is_err());
        cleanup(&dir);
    }

    #[test]
    fn capture_lands_in_inbox_and_daily_pages() {
        let (dir, bm) = test_brain("capture");
        let r = bm
            .capture("call the plumber", CaptureTarget::Inbox, None, "test")
            .unwrap();
        assert_eq!(r.slug, "inbox/inbox");
        assert!(r.created_page);
        let r2 = bm
            .capture("and the electrician", CaptureTarget::Inbox, None, "test")
            .unwrap();
        assert!(!r2.created_page);
        let inbox = bm.read_page("inbox/inbox").unwrap().unwrap();
        assert!(inbox.timeline.contains("call the plumber"));
        assert!(inbox.timeline.contains("and the electrician"));
        let d = bm
            .capture(
                "stand-up notes",
                CaptureTarget::Daily,
                Some("2026-09-02"),
                "test",
            )
            .unwrap();
        assert_eq!(d.slug, "daily/2026-09-02");
        assert!(bm
            .capture("   ", CaptureTarget::Inbox, None, "test")
            .is_err());
        assert_eq!(CaptureTarget::parse("Daily").unwrap(), CaptureTarget::Daily);
        assert!(CaptureTarget::parse("later").is_err());
        cleanup(&dir);
    }

    /// A note saved through the notes manager rooted inside the vault is a page of type
    /// `note` once the index has synced (TICKET-014).
    #[test]
    fn notes_written_into_the_vault_index_as_note_pages() {
        let (dir, bm) = test_brain("notes_in_vault");
        bm.ensure_vault().unwrap();
        let notes = crate::notes::NotesManager::with_root(dir.join("notes")).unwrap();
        let rel = notes.create_note("", "hello", false).unwrap();
        notes
            .save_note(&rel, "A note that lives in the vault.")
            .unwrap();
        bm.sync_all().unwrap();
        let page = bm
            .read_page("notes/hello")
            .unwrap()
            .expect("the note is a page");
        assert_eq!(page.page_type, "note");
        assert!(
            page.compiled_truth.contains("lives in the vault"),
            "{}",
            page.compiled_truth
        );
        cleanup(&dir);
    }

    #[test]
    fn page_types_cover_the_type_table() {
        let (dir, bm) = test_brain("page_types");
        bm.create_page("person", "Alice", "").unwrap();
        let types = bm.page_types().unwrap();
        assert_eq!(types.len(), vault::page_types().len());
        let person = types.iter().find(|t| t.page_type == "person").unwrap();
        assert_eq!(person.dir, "people");
        assert_eq!(person.count, 1);
        cleanup(&dir);
    }

    #[test]
    fn migrate_vault_converts_rules_and_link_paths() {
        let (dir, bm) = test_brain("migrate");
        bm.create_page("person", "Bob", "").unwrap();
        bm.create_page("person", "Bob Jones", "").unwrap();
        let alice = dir.join("people").join("alice.md");
        fs::write(
            &alice,
            "---\ntitle: Alice\ntype: person\nrole: CTO\n---\n\nWorks with [[bob]] and [[Bob Jones]] on [[projects/nowhere]].\n\n---\n\n- **2026-01-01** — met [[people/bob|Bob]]\n",
        )
        .unwrap();
        bm.sync_all().unwrap();

        let dry = bm.migrate_vault(true).unwrap();
        assert!(dry.dry_run);
        assert_eq!(dry.pages_changed, 1);
        assert_eq!(dry.timelines_converted, 1);
        assert_eq!(dry.links_rewritten, 2);
        assert_eq!(dry.changed_slugs, vec!["people/alice"]);
        assert!(dry
            .unresolved_links
            .iter()
            .any(|u| u.contains("[[projects/nowhere]]")));
        assert!(fs::read_to_string(&alice)
            .unwrap()
            .contains("\n---\n\n- **"));

        let real = bm.migrate_vault(false).unwrap();
        assert_eq!(real.pages_changed, 1);
        let raw = fs::read_to_string(&alice).unwrap();
        assert!(raw.starts_with("---\ntitle: Alice\ntype: person\nrole: CTO\n---\n"));
        assert!(raw
            .contains("[[people/bob]] and [[people/bob-jones|Bob Jones]] on [[projects/nowhere]]"));
        assert!(raw.contains("\n## Timeline\n\n- **2026-01-01** — met [[people/bob|Bob]]\n"));
        assert_eq!(raw.matches("\n---\n").count(), 1);
        let page = bm.read_page("people/alice").unwrap().unwrap();
        assert!(page.timeline.contains("met"));
        let links = bm.get_links("people/alice").unwrap();
        assert!(links
            .outbound
            .iter()
            .any(|l| l.to_slug == "people/bob-jones"));

        // A second pass has nothing to do.
        let again = bm.migrate_vault(false).unwrap();
        assert_eq!(again.pages_changed, 0);
        cleanup(&dir);
    }

    #[test]
    fn sync_all_drops_orphan_index_rows_without_deadlocking() {
        let (dir, bm) = test_brain("sync_orphans");
        bm.create_page("concept", "Kept", "").unwrap();
        bm.create_page("concept", "Gone", "").unwrap();
        fs::remove_file(dir.join("concepts").join("gone.md")).unwrap();

        // A deadlock here would hang the suite, so run the sync on a thread and wait.
        let bm = Arc::new(bm);
        let worker = Arc::clone(&bm);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(worker.sync_all());
        });
        let synced = rx
            .recv_timeout(std::time::Duration::from_secs(20))
            .expect("sync_all must finish; it deadlocked on the orphan row")
            .unwrap();
        assert_eq!(synced, 1);
        let slugs: Vec<String> = bm
            .list_pages(None, None)
            .unwrap()
            .into_iter()
            .map(|p| p.slug)
            .collect();
        assert_eq!(slugs, vec!["concepts/kept"]);
        cleanup(&dir);
    }

    #[test]
    fn render_text_renders_markdown_that_is_not_a_page() {
        let (dir, bm) = test_brain("render_text");
        let rendered = bm.render_text(
            "# Hello\n\nSome *text* and [[projects/orbit]].\n",
            &render::Style::default(),
        );
        assert_eq!(rendered.slug, "");
        assert!(rendered.raw.starts_with("# Hello"));
        let json = serde_json::to_string(&rendered.rendered).unwrap();
        assert!(json.contains("Hello") && json.contains("text"), "{json}");
        let _ = fs::remove_dir_all(dir);
    }

    fn obsidian_vault(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rusty_obsidian_{}_{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::create_dir_all(dir.join("assets")).unwrap();
        fs::create_dir_all(dir.join(".obsidian")).unwrap();
        fs::create_dir_all(dir.join(".trash")).unwrap();
        fs::write(
            dir.join("Note A.md"),
            "# Note A\n\nSee [[Note B]] and [[Missing One]].\n\n![](assets/pic.png)\n\n#imported\n",
        )
        .unwrap();
        fs::write(
            dir.join("sub/Note B.md"),
            "---\ntags:\n  - alpha\n---\n\nBack to [[Note A]].\n\n## Heading\n\nText.\n",
        )
        .unwrap();
        fs::write(
            dir.join("assets/pic.png"),
            [0x89, b'P', b'N', b'G', 1, 2, 3],
        )
        .unwrap();
        fs::write(dir.join(".obsidian/app.json"), "{}").unwrap();
        fs::write(
            dir.join(".obsidian/bookmarks.json"),
            r##"{"items":[{"type":"file","path":"Note A.md","title":"A"},{"type":"group","title":"g","items":[{"type":"folder","path":"sub"},{"type":"search","query":"tag:alpha","title":"alpha"},{"type":"file","path":"sub/Note B.md","subpath":"#Heading"},{"type":"file","path":"nowhere/Nope.md"},{"type":"url","url":"https://example.invalid"}]}]}"##,
        )
        .unwrap();
        fs::write(dir.join(".trash/old.md"), "gone").unwrap();
        dir
    }

    fn snapshot(dir: &std::path::Path) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        fn walk(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<(String, Vec<u8>)>) {
            for e in fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(root, &p, out);
                } else {
                    out.push((
                        p.strip_prefix(root).unwrap().to_string_lossy().into_owned(),
                        fs::read(&p).unwrap(),
                    ));
                }
            }
        }
        walk(dir, dir, &mut out);
        out.sort();
        out
    }

    #[test]
    fn import_plan_reports_what_it_found() {
        let (dir, bm) = test_brain("import_plan");
        let source = obsidian_vault("plan");
        // A page already in the brain under the same slug: a collision, left alone.
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("sub/Note B.md"), "Mine.\n").unwrap();
        bm.sync_all().unwrap();
        let before = snapshot(&source);
        let plan = bm.import_plan(&source).unwrap();
        assert!(
            plan.name.ends_with("_plan"),
            "the source folder's name: {}",
            plan.name
        );
        assert_eq!(plan.pages, vec!["Note A"]);
        assert_eq!(plan.collisions, vec!["sub/Note B"]);
        assert_eq!(plan.attachments, vec!["assets/pic.png"]);
        assert_eq!(plan.folders, vec!["assets"]);
        assert_eq!(plan.tags, vec!["imported"]);
        assert_eq!(plan.unresolved_links, vec!["Note A: [[Missing One]]"]);
        let kinds: Vec<&str> = plan.bookmarks.iter().map(|b| b.kind.as_str()).collect();
        assert_eq!(kinds, ["file", "folder", "search", "heading"]);
        assert_eq!(
            plan.bookmarks_skipped,
            vec!["file `nowhere/Nope`: no such page or folder"]
        );
        assert_eq!(snapshot(&source), before, "the source is never written");
        assert_eq!(
            fs::read_to_string(dir.join("sub/Note B.md")).unwrap(),
            "Mine.\n"
        );
        assert!(bm.import_plan(&dir).is_err(), "the brain itself is refused");
        assert!(bm.import_plan(&source.join("nowhere")).is_err());
        cleanup(&dir);
        cleanup(&source);
    }

    #[test]
    fn import_skips_what_would_land_in_the_archive() {
        let (dir, bm) = test_brain("import_archive");
        let source = obsidian_vault("archive");
        fs::create_dir_all(source.join("archive")).unwrap();
        fs::write(source.join("archive/Old.md"), "Old.\n").unwrap();
        let plan = bm.import_plan(&source).unwrap();
        assert!(
            plan.collisions.contains(&"archive/Old".to_string()),
            "{plan:?}"
        );
        assert!(!plan.pages.contains(&"archive/Old".to_string()));
        assert!(!plan.folders.contains(&"archive".to_string()));
        let report = bm.import_vault(&source).unwrap();
        assert_eq!(report.imported_pages, report.plan.pages.len());
        assert!(!dir.join("archive/Old.md").exists());
        cleanup(&dir);
        cleanup(&source);
    }

    #[test]
    fn import_vault_brings_pages_attachments_and_a_report() {
        let (dir, bm) = test_brain("import_run");
        let source = obsidian_vault("run");
        let before = snapshot(&source);
        let report = bm.import_vault(&source).unwrap();
        assert_eq!((report.imported_pages, report.imported_attachments), (2, 1));
        // [[Note B]] became [[sub/Note B]]; [[Note A]] already was its vault path.
        assert_eq!(report.links_rewritten, 1);
        let a = fs::read_to_string(dir.join("Note A.md")).unwrap();
        assert!(
            a.contains("[[sub/Note B]]") && a.contains("[[Missing One]]"),
            "{a}"
        );
        let b = fs::read_to_string(dir.join("sub/Note B.md")).unwrap();
        assert!(
            b.starts_with("---\ntags:\n  - alpha\n---\n"),
            "the frontmatter as it was: {b}"
        );
        assert!(b.contains("[[Note A]]"), "{b}");
        assert_eq!(
            fs::read(dir.join("assets/pic.png")).unwrap(),
            [0x89, b'P', b'N', b'G', 1, 2, 3]
        );
        assert!(bm
            .search("Missing", None, None)
            .unwrap()
            .iter()
            .any(|r| r.slug == "Note A"));
        let tags: Vec<String> = bm.tags().unwrap().into_iter().map(|t| t.tag).collect();
        assert!(
            tags.contains(&"alpha".to_string()) && tags.contains(&"imported".to_string()),
            "{tags:?}"
        );
        let links = bm.get_links("sub/Note B").unwrap();
        assert!(
            links
                .outbound
                .iter()
                .any(|l| l.to_slug == "Note A" && l.resolved),
            "{:?}",
            links.outbound
        );
        assert!(
            report.report_slug.starts_with("inbox/import-"),
            "{}",
            report.report_slug
        );
        let page = bm
            .read_page(&report.report_slug)
            .unwrap()
            .expect("the report page");
        assert!(
            page.compiled_truth.contains("[[Note A]]")
                && page.compiled_truth.contains("Missing One"),
            "{}",
            page.compiled_truth
        );
        assert_eq!(report.plan.bookmarks.len(), 4);
        // They land in the vault's bookmarks, not the app's state (TICKET-037).
        assert_eq!(report.bookmarks_added, 4);
        assert_eq!(bm.bookmarks().unwrap(), report.plan.bookmarks);
        assert_eq!(snapshot(&source), before, "the source is never written");
        cleanup(&dir);
        cleanup(&source);
    }

    #[test]
    fn import_vault_rolls_back_when_a_write_fails() {
        let (dir, bm) = test_brain("import_fail");
        let source = obsidian_vault("fail");
        // A file where the attachments' folder must go: the copy fails after the pages
        // were written, and the pages go with it.
        fs::write(dir.join("assets"), "in the way").unwrap();
        let err = bm.import_vault(&source).unwrap_err();
        assert!(err.contains("nothing of it remains"), "{err}");
        assert!(!dir.join("Note A.md").exists() && !dir.join("sub/Note B.md").exists());
        assert!(!bm.page_exists_for_test("Note A"));
        assert!(bm.search("Missing", None, None).unwrap().is_empty());
        assert!(
            fs::read_dir(dir.join("inbox"))
                .unwrap()
                .flatten()
                .next()
                .is_none(),
            "no report page"
        );
        assert_eq!(
            fs::read_to_string(dir.join("assets")).unwrap(),
            "in the way"
        );
        cleanup(&dir);
        cleanup(&source);
    }

    #[test]
    fn capture_writes_updates_and_records_failures() {
        // Its own vault: `capture_lands_in_inbox_and_daily_pages` builds one named
        // "capture", and two tests on one temporary vault wipe each other mid-run.
        let (dir, bm) = test_brain("source_capture");
        let url = "https://www.example.com/posts/orbit-launch";
        let html = |title: &str| {
            sources::Fetched {
            url: url.to_string(),
            content_type: "text/html; charset=utf-8".to_string(),
            bytes: format!("<html><head><title>{title}</title></head><body><main><p>Orbit launched on a Tuesday.</p></main></body></html>").into_bytes(),
        }
        };
        let page = bm.capture_fetched(url, Ok(html("Orbit launch"))).unwrap();
        assert_eq!(page.slug, "sources/example-com-orbit-launch");
        assert_eq!(page.page_type, "source");
        assert_eq!(page.title, "Orbit launch");
        assert_eq!(page.frontmatter.extra["url"], url);
        assert_eq!(page.frontmatter.extra["site"], "example.com");
        assert_eq!(page.frontmatter.extra["kind"], "html");
        assert!(page.frontmatter.extra["captured"]
            .as_str()
            .unwrap()
            .starts_with("20"));
        assert!(
            page.compiled_truth.contains("Orbit launched on a Tuesday."),
            "{}",
            page.compiled_truth
        );
        let raw = fs::read_to_string(dir.join("sources/example-com-orbit-launch.md")).unwrap();
        assert!(
            raw.starts_with("---\n")
                && raw.contains("type: source")
                && raw.contains(&format!("url: {url}")),
            "{raw}"
        );
        let hits = bm.search("Tuesday type:source", None, None).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].page_type, "source");
        // The same URL again: the page, not a second one; the title follows, created stays.
        let created = page.frontmatter.created.clone();
        let again = bm
            .capture_fetched(url, Ok(html("Orbit launch, updated")))
            .unwrap();
        assert_eq!(again.slug, page.slug);
        assert_eq!(again.title, "Orbit launch, updated");
        assert_eq!(again.frontmatter.created, created);
        assert_eq!(bm.list_pages(Some("source"), None).unwrap().len(), 1);
        // A failure on a captured URL keeps the text and records the failure.
        let failed = bm
            .capture_fetched(url, Err("fetch: timed out".to_string()))
            .unwrap();
        assert_eq!(failed.slug, page.slug);
        assert!(failed.compiled_truth.contains("Tuesday"));
        assert_eq!(failed.frontmatter.extra["status"], "failed");
        assert_eq!(failed.frontmatter.extra["error"], "fetch: timed out");
        // A failure on a new URL is a page that says why.
        let fresh = bm
            .capture_fetched(
                "https://other.invalid/paper.pdf",
                Err("pdftotext is not available".to_string()),
            )
            .unwrap();
        assert_eq!(fresh.slug, "sources/other-invalid-paper-pdf");
        assert_eq!(fresh.frontmatter.extra["status"], "failed");
        assert!(
            fresh
                .compiled_truth
                .starts_with("Capture failed: pdftotext"),
            "{}",
            fresh.compiled_truth
        );
        assert!(bm.capture_fetched("ftp://x/y", Err("no".into())).is_err());
        assert_eq!(
            bm.source_for_url(url).unwrap().as_deref(),
            Some(page.slug.as_str())
        );
        assert_eq!(bm.source_for_url("https://nowhere.invalid/").unwrap(), None);
        cleanup(&dir);
    }
}
