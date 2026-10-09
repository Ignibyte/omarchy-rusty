//! Bookmarks: the user's saved files, folders, searches and headings, kept in a file in
//! the vault that git tracks, `.rusty/bookmarks.json`, so every client reads the same
//! list. The file and folder bookmarks are the favourites.

use serde::{Deserialize, Serialize};

use super::BrainManager;

/// Where the bookmarks live, relative to the vault. A dot-folder, so the page walk and
/// the tree leave it out; not ignored, so the vault's history keeps it.
pub const FILE: &str = ".rusty/bookmarks.json";

/// The kinds a bookmark may be.
pub const KINDS: &[&str] = &["file", "folder", "search", "heading"];

/// One bookmark: `path` (a slug for a file or
/// heading, a vault path for a folder) for all but a search, `query` for a search,
/// `heading` for a heading. Empty fields are left out of the file.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Bookmark {
    pub kind: String,
    #[serde(default)]
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub query: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub heading: String,
}

impl Bookmark {
    /// What makes two bookmarks the same one, as the app's `bookmarkKey` has it: the
    /// kind and the query, the path and heading, or the path.
    pub fn key(&self) -> String {
        match self.kind.as_str() {
            "search" => format!("search:{}", self.query),
            "heading" => format!("heading:{}#{}", self.path, self.heading),
            kind => format!("{kind}:{}", self.path),
        }
    }

    /// Whether the bookmark is well formed: a known kind and the field that kind needs.
    pub fn check(&self) -> Result<(), String> {
        if !KINDS.contains(&self.kind.as_str()) {
            return Err(format!(
                "a bookmark's kind is one of {}, not {:?}",
                KINDS.join(", "),
                self.kind
            ));
        }
        let missing = match self.kind.as_str() {
            "search" => self.query.trim().is_empty().then_some("query"),
            "heading" if self.heading.trim().is_empty() => Some("heading"),
            _ => self.path.trim().is_empty().then_some("path"),
        };
        match missing {
            Some(field) => Err(format!("a {} bookmark needs a {field}", self.kind)),
            None => Ok(()),
        }
    }

    /// Whether the bookmark points at `path` or, for a folder move or delete, anything
    /// under it.
    fn points_into(&self, path: &str, folder: bool) -> bool {
        self.kind != "search"
            && (self.path == path || (folder && self.path.starts_with(&format!("{path}/"))))
    }
}

impl BrainManager {
    /// The bookmarks, in their order; empty when there are none yet.
    pub fn bookmarks(&self) -> Result<Vec<Bookmark>, String> {
        match self.vault.read_file(FILE)? {
            Some(text) if !text.trim().is_empty() => {
                serde_json::from_str(&text).map_err(|e| format!("{FILE}: {e}"))
            }
            _ => Ok(Vec::new()),
        }
    }

    /// Replace the bookmarks with `list` (reordered, retitled, whatever the caller did),
    /// each checked, a repeated one kept once. Returns the list as stored.
    pub fn set_bookmarks(&self, list: Vec<Bookmark>) -> Result<Vec<Bookmark>, String> {
        let mut kept: Vec<Bookmark> = Vec::with_capacity(list.len());
        for mut bookmark in list {
            bookmark.check()?;
            if bookmark.title.trim().is_empty() {
                bookmark.title = match bookmark.kind.as_str() {
                    "search" => bookmark.query.clone(),
                    "heading" => bookmark.heading.clone(),
                    _ => bookmark.path.rsplit('/').next().unwrap_or("").to_string(),
                };
            }
            if !kept.iter().any(|b| b.key() == bookmark.key()) {
                kept.push(bookmark);
            }
        }
        self.write_bookmarks(&kept, "bookmarks: update")?;
        Ok(kept)
    }

    /// Add one bookmark at the end; one already there is left as it is. Returns the list.
    pub fn add_bookmark(&self, bookmark: Bookmark) -> Result<Vec<Bookmark>, String> {
        let mut list = self.bookmarks()?;
        list.push(bookmark);
        self.set_bookmarks(list)
    }

    /// Remove the bookmark with `bookmark`'s key; an error when there is none.
    pub fn remove_bookmark(&self, bookmark: &Bookmark) -> Result<Vec<Bookmark>, String> {
        let key = bookmark.key();
        let mut list = self.bookmarks()?;
        let before = list.len();
        list.retain(|b| b.key() != key);
        if list.len() == before {
            return Err(format!("No bookmark {key}"));
        }
        self.write_bookmarks(&list, "bookmarks: remove")?;
        Ok(list)
    }

    /// Point the bookmarks on `from` (and, for a folder, under it) at `to`.
    pub(crate) fn follow_bookmarks(
        &self,
        from: &str,
        to: &str,
        folder: bool,
    ) -> Result<(), String> {
        let mut list = self.bookmarks()?;
        let mut moved = false;
        for b in list.iter_mut().filter(|b| b.points_into(from, folder)) {
            b.path = format!("{to}{}", &b.path[from.len()..]);
            moved = true;
        }
        if moved {
            self.write_bookmarks(&list, &format!("bookmarks: follow {from} to {to}"))?;
        }
        Ok(())
    }

    /// Drop the bookmarks on `path` (and, for a folder, under it).
    pub(crate) fn drop_bookmarks(&self, path: &str, folder: bool) -> Result<(), String> {
        let mut list = self.bookmarks()?;
        let before = list.len();
        list.retain(|b| !b.points_into(path, folder));
        if list.len() != before {
            self.write_bookmarks(&list, &format!("bookmarks: drop {path}"))?;
        }
        Ok(())
    }

    fn write_bookmarks(&self, list: &[Bookmark], message: &str) -> Result<(), String> {
        let text = serde_json::to_string_pretty(list).map_err(|e| e.to_string())? + "\n";
        self.vault.write_file(FILE, &text)?;
        self.vault.git_commit(message);
        self.db.record_change("bookmarks", FILE, "updated", "");
        Ok(())
    }
}
