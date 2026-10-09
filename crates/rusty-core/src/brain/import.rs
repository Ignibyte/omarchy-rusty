//! Bringing an Obsidian vault into the brain (TICKET-026): the pure parts. The source
//! is walked and never written; pages keep their paths; bookmarks are read from
//! `.obsidian/bookmarks.json`; the report is a page. `BrainManager::import_plan` and
//! `import_vault` in the parent module do the reading of the brain, the link rewrite
//! and the writes.

use std::path::{Path, PathBuf};

/// What the source vault holds.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Scan {
    /// Pages: the slug (the path without `.md`) and the file.
    pub pages: Vec<(String, PathBuf)>,
    /// Everything else: the vault-relative path and the file.
    pub attachments: Vec<(String, PathBuf)>,
}

/// A bookmark from Obsidian, already in the store's shape (TICKET-037).
pub use super::bookmarks::Bookmark as ImportedBookmark;

/// What an import would do, or did.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct ImportPlan {
    /// The source vault.
    pub source: String,
    /// The vault folder's name.
    pub name: String,
    /// The slugs that come in.
    pub pages: Vec<String>,
    /// The folders they and the attachments live in, distinct.
    pub folders: Vec<String>,
    /// The attachments that come in, vault-relative.
    pub attachments: Vec<String>,
    /// Slugs and attachment paths already in the brain: skipped, never overwritten.
    pub collisions: Vec<String>,
    /// The tags of the incoming pages, frontmatter and inline, as first seen.
    pub tags: Vec<String>,
    /// Links in the incoming pages that resolve to nothing, as `slug: [[target]]`.
    pub unresolved_links: Vec<String>,
    /// The bookmarks that come across.
    pub bookmarks: Vec<ImportedBookmark>,
    /// The bookmarks that do not, and why.
    pub bookmarks_skipped: Vec<String>,
}

/// What an import did.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct ImportReport {
    pub plan: ImportPlan,
    pub imported_pages: usize,
    pub imported_attachments: usize,
    pub links_rewritten: usize,
    /// Bookmarks the store did not have yet, added from the vault's own.
    #[serde(default)]
    pub bookmarks_added: usize,
    /// The report page under `inbox/`.
    pub report_slug: String,
}

/// Every page and attachment under `source`, dot-entries skipped (`.obsidian`,
/// `.trash` and `.git` among them), sorted by path.
pub fn scan_vault(source: &Path) -> Result<Scan, String> {
    if !source.is_dir() {
        return Err(format!("{} is not a folder", source.display()));
    }
    let mut scan = Scan::default();
    walk(source, "", &mut scan)?;
    scan.pages.sort();
    scan.attachments.sort();
    Ok(scan)
}

fn walk(dir: &Path, rel: &str, out: &mut Scan) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("read {}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let child = if rel.is_empty() {
            name.clone()
        } else {
            format!("{rel}/{name}")
        };
        if path.is_dir() {
            walk(&path, &child, out)?;
        } else if let Some(slug) = child.strip_suffix(".md") {
            out.pages.push((slug.to_string(), path));
        } else {
            out.attachments.push((child, path));
        }
    }
    Ok(())
}

/// The distinct folders above a set of vault-relative paths, sorted.
pub fn folders_of(paths: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for p in paths {
        let mut dir = p.as_str();
        while let Some(i) = dir.rfind('/') {
            dir = &dir[..i];
            if !out.iter().any(|d| d == dir) {
                out.push(dir.to_string());
            }
        }
    }
    out.sort();
    out
}

/// The bookmarks in Obsidian's `bookmarks.json`: groups flattened, `file` (a heading
/// when it carries a `#subpath`), `folder` and `search` kept, the rest left out.
pub fn parse_bookmarks(json: &str) -> Vec<ImportedBookmark> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(items) = value["items"].as_array() {
        collect_bookmarks(items, &mut out);
    }
    out
}

fn collect_bookmarks(items: &[serde_json::Value], out: &mut Vec<ImportedBookmark>) {
    for item in items {
        let title = item["title"].as_str().unwrap_or("").trim().to_string();
        match item["type"].as_str().unwrap_or("") {
            "group" => {
                if let Some(inner) = item["items"].as_array() {
                    collect_bookmarks(inner, out);
                }
            }
            "file" => {
                let path = item["path"].as_str().unwrap_or("").trim_matches('/');
                if path.is_empty() {
                    continue;
                }
                let slug = path.strip_suffix(".md").unwrap_or(path).to_string();
                let name = slug.rsplit('/').next().unwrap_or(&slug).to_string();
                let heading = item["subpath"]
                    .as_str()
                    .and_then(|s| s.strip_prefix('#'))
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if heading.is_empty() {
                    out.push(ImportedBookmark {
                        kind: "file".to_string(),
                        title: if title.is_empty() { name } else { title },
                        path: slug,
                        ..Default::default()
                    });
                } else {
                    out.push(ImportedBookmark {
                        kind: "heading".to_string(),
                        title: if title.is_empty() {
                            format!("{name} › {heading}")
                        } else {
                            title
                        },
                        path: slug,
                        heading,
                        ..Default::default()
                    });
                }
            }
            "folder" => {
                let path = item["path"].as_str().unwrap_or("").trim_matches('/');
                if path.is_empty() {
                    continue;
                }
                let name = path.rsplit('/').next().unwrap_or(path).to_string();
                out.push(ImportedBookmark {
                    kind: "folder".to_string(),
                    title: if title.is_empty() { name } else { title },
                    path: path.to_string(),
                    ..Default::default()
                });
            }
            "search" => {
                let query = item["query"].as_str().unwrap_or("").trim().to_string();
                if query.is_empty() {
                    continue;
                }
                out.push(ImportedBookmark {
                    kind: "search".to_string(),
                    title: if title.is_empty() {
                        query.clone()
                    } else {
                        title
                    },
                    query,
                    ..Default::default()
                });
            }
            _ => {}
        }
    }
}

fn list(lines: &[String], empty: &str) -> String {
    if lines.is_empty() {
        format!("- {empty}\n")
    } else {
        lines.iter().map(|l| format!("- {l}\n")).collect()
    }
}

/// The report page: what came in, what was skipped and why, what did not resolve.
pub fn report_page(report: &ImportReport, date: &str) -> String {
    let p = &report.plan;
    let mut skipped: Vec<String> = p
        .collisions
        .iter()
        .map(|c| format!("`{c}`: already in the brain; left as it was"))
        .collect();
    skipped.extend(p.bookmarks_skipped.iter().map(|b| format!("bookmark {b}")));
    let bookmarks: Vec<String> = p
        .bookmarks
        .iter()
        .map(|b| match b.kind.as_str() {
            "search" => format!("search `{}` ({})", b.query, b.title),
            "heading" => format!("heading `{}#{}` ({})", b.path, b.heading, b.title),
            kind => format!("{kind} `{}` ({})", b.path, b.title),
        })
        .collect();
    format!(
        "---\ntitle: Import of {name}\ntype: note\n---\n\nImported from `{source}` on {date}: {pages} pages in {folders} folders, {attachments} attachments, {tags} tags, {links} links rewritten to vault paths, {bookmarks_n} bookmarks (the app adds them to Bookmarks). The source vault was read and not changed.\n\n## Pages\n\n{page_list}\n## Attachments\n\n{attachment_list}\n## Tags\n\n{tag_list}\n## Skipped\n\n{skipped_list}\n## Unresolved links\n\n{unresolved_list}\n## Bookmarks\n\n{bookmark_list}",
        name = p.name,
        source = p.source,
        pages = report.imported_pages,
        folders = p.folders.len(),
        attachments = report.imported_attachments,
        tags = p.tags.len(),
        links = report.links_rewritten,
        bookmarks_n = p.bookmarks.len(),
        page_list = list(&p.pages.iter().map(|s| format!("[[{s}]]")).collect::<Vec<_>>(), "none"),
        attachment_list = list(&p.attachments.iter().map(|a| format!("`{a}`")).collect::<Vec<_>>(), "none"),
        tag_list = list(&p.tags.iter().map(|t| format!("#{t}")).collect::<Vec<_>>(), "none"),
        skipped_list = list(&skipped, "nothing"),
        unresolved_list = list(&p.unresolved_links.iter().map(|u| format!("`{u}`")).collect::<Vec<_>>(), "none"),
        bookmark_list = list(&bookmarks, "none"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_vault_skips_dot_entries() {
        let dir = std::env::temp_dir().join(format!("rusty_import_scan_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::create_dir_all(dir.join(".obsidian")).unwrap();
        std::fs::create_dir_all(dir.join(".trash")).unwrap();
        std::fs::write(dir.join("Note A.md"), "a").unwrap();
        std::fs::write(dir.join("sub/Note B.md"), "b").unwrap();
        std::fs::write(dir.join("sub/pic.png"), [1, 2, 3]).unwrap();
        std::fs::write(dir.join(".obsidian/app.json"), "{}").unwrap();
        std::fs::write(dir.join(".trash/old.md"), "gone").unwrap();
        let scan = scan_vault(&dir).unwrap();
        let pages: Vec<&str> = scan.pages.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(pages, ["Note A", "sub/Note B"]);
        let files: Vec<&str> = scan.attachments.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(files, ["sub/pic.png"]);
        assert_eq!(
            folders_of(&["sub/Note B".into(), "a/b/c.png".into()]),
            ["a", "a/b", "sub"]
        );
        assert!(scan_vault(&dir.join("nowhere")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_bookmarks_flattens_groups_and_maps_kinds() {
        let json = r##"{"items":[
            {"type":"file","ctime":1,"path":"Note A.md","title":"A"},
            {"type":"group","title":"g","items":[
                {"type":"folder","path":"sub"},
                {"type":"search","query":"tag:alpha","title":"alpha"},
                {"type":"file","path":"sub/Note B.md","subpath":"#Heading"},
                {"type":"url","url":"https://example.invalid"},
                {"type":"graph"}
            ]},
            {"type":"file","path":""}
        ]}"##;
        let got = parse_bookmarks(json);
        let kinds: Vec<(&str, &str, &str)> = got
            .iter()
            .map(|b| (b.kind.as_str(), b.title.as_str(), b.path.as_str()))
            .collect();
        assert_eq!(
            kinds,
            [
                ("file", "A", "Note A"),
                ("folder", "sub", "sub"),
                ("search", "alpha", ""),
                ("heading", "Note B › Heading", "sub/Note B"),
            ]
        );
        assert_eq!(got[2].query, "tag:alpha");
        assert_eq!(got[3].heading, "Heading");
        assert!(parse_bookmarks("not json").is_empty());
    }

    #[test]
    fn report_page_names_everything() {
        let report = ImportReport {
            plan: ImportPlan {
                source: "/v".into(),
                name: "v".into(),
                pages: vec!["Note A".into()],
                folders: vec![],
                attachments: vec!["pic.png".into()],
                collisions: vec!["sub/Note B".into()],
                tags: vec!["alpha".into()],
                unresolved_links: vec!["Note A: [[Missing One]]".into()],
                bookmarks: vec![ImportedBookmark {
                    kind: "search".into(),
                    title: "alpha".into(),
                    query: "tag:alpha".into(),
                    ..Default::default()
                }],
                bookmarks_skipped: vec!["url https://example.invalid".into()],
            },
            imported_pages: 1,
            imported_attachments: 1,
            links_rewritten: 2,
            bookmarks_added: 1,
            report_slug: String::new(),
        };
        let page = report_page(&report, "2026-09-05");
        for needle in [
            "title: Import of v",
            "1 pages in 0 folders, 1 attachments, 1 tags, 2 links rewritten",
            "- [[Note A]]",
            "- `pic.png`",
            "- #alpha",
            "`sub/Note B`: already in the brain; left as it was",
            "bookmark url https://example.invalid",
            "- `Note A: [[Missing One]]`",
            "search `tag:alpha` (alpha)",
        ] {
            assert!(page.contains(needle), "{needle}\n{page}");
        }
    }
}
