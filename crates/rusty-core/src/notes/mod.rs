//! Markdown notes manager backed by the filesystem.
//!
//! Notes are stored as `.md` files in `~/.rusty/notes/`. Supports hierarchical
//! folders, soft-delete to a `.deleted/` directory, and safe file naming.

use std::path::{Path, PathBuf};

/// A node in the note tree (file or folder).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NoteTreeEntry {
    /// Display name (filename without `.md` extension).
    pub name: String,
    /// Relative path from the notes root (e.g., "folder/note.md").
    pub path: String,
    /// Whether this is a "file" or "folder".
    pub entry_type: String,
    /// Child entries (only populated for folders).
    pub children: Vec<NoteTreeEntry>,
}

/// Manages markdown notes on disk.
pub struct NotesManager {
    root: PathBuf,
    /// Where a change is recorded for other processes, when attached (TICKET-035).
    changes: Option<std::sync::Arc<crate::engine::db::Database>>,
}

impl NotesManager {
    /// Create a new NotesManager rooted at `~/.rusty/notes/`.
    ///
    /// Creates the directory if it doesn't exist.
    pub fn new() -> Result<Self, String> {
        let root = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".rusty")
            .join("notes");
        Self::with_root(root)
    }

    /// Create a NotesManager rooted at a custom path.
    pub fn with_root(root: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&root)
            .map_err(|e| format!("Failed to create notes directory: {e}"))?;

        // Ensure .deleted directory exists
        std::fs::create_dir_all(root.join(".deleted"))
            .map_err(|e| format!("Failed to create .deleted directory: {e}"))?;

        Ok(Self {
            root,
            changes: None,
        })
    }

    /// Record every note write in `db`'s change log, by relative path.
    pub fn with_changes(mut self, db: std::sync::Arc<crate::engine::db::Database>) -> Self {
        self.changes = Some(db);
        self
    }

    fn record(&self, path: &str, op: &str, detail: &str) {
        if let Some(db) = &self.changes {
            db.record_change("note", path, op, detail);
        }
    }

    /// Build a tree of all notes and folders.
    pub fn list_tree(&self) -> Result<Vec<NoteTreeEntry>, String> {
        self.build_tree(&self.root, "")
    }

    /// Read the content of a note.
    pub fn read_note(&self, relative_path: &str) -> Result<String, String> {
        let full_path = self.resolve_safe_path(relative_path)?;
        std::fs::read_to_string(&full_path).map_err(|e| format!("Failed to read note: {e}"))
    }

    /// Save content to a note (creates or overwrites).
    pub fn save_note(&self, relative_path: &str, content: &str) -> Result<(), String> {
        let full_path = self.resolve_safe_path(relative_path)?;

        // Ensure parent directory exists
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory: {e}"))?;
        }

        std::fs::write(&full_path, content).map_err(|e| format!("Failed to save note: {e}"))?;
        self.record(relative_path, "updated", "");
        Ok(())
    }

    /// Create a new note or folder.
    ///
    /// For files, creates an empty `.md` file. For folders, creates the directory.
    pub fn create_note(
        &self,
        parent_path: &str,
        name: &str,
        is_folder: bool,
    ) -> Result<String, String> {
        let safe_name = sanitize_name(name)?;

        let parent = if parent_path.is_empty() {
            self.root.clone()
        } else {
            self.resolve_safe_path(parent_path)?
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| self.root.clone())
        };

        if is_folder {
            let folder_path = parent.join(&safe_name);
            if folder_path.exists() {
                return Err("Folder already exists".to_string());
            }
            std::fs::create_dir_all(&folder_path)
                .map_err(|e| format!("Failed to create folder: {e}"))?;

            let relative = folder_path
                .strip_prefix(&self.root)
                .unwrap_or(&folder_path)
                .to_string_lossy()
                .to_string();
            self.record(&relative, "created", "folder");
            Ok(relative)
        } else {
            let file_path = parent.join(format!("{safe_name}.md"));
            if file_path.exists() {
                return Err("Note already exists".to_string());
            }
            std::fs::write(&file_path, "").map_err(|e| format!("Failed to create note: {e}"))?;

            let relative = file_path
                .strip_prefix(&self.root)
                .unwrap_or(&file_path)
                .to_string_lossy()
                .to_string();
            self.record(&relative, "created", "");
            Ok(relative)
        }
    }

    /// Soft-delete a note or folder by moving it to `.deleted/`.
    pub fn delete_note(&self, relative_path: &str) -> Result<(), String> {
        let full_path = self.resolve_safe_path(relative_path)?;
        if !full_path.exists() {
            return Err("Note not found".to_string());
        }

        let deleted_dir = self.root.join(".deleted");
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let file_name = full_path.file_name().unwrap_or_default().to_string_lossy();
        let dest =
            crate::brain::vault::unused_name(&deleted_dir, &format!("{file_name}_{timestamp}"));

        std::fs::rename(&full_path, &dest).map_err(|e| format!("Failed to delete note: {e}"))?;
        self.record(relative_path, "deleted", "");
        Ok(())
    }

    /// Rename a note or folder.
    pub fn rename_note(&self, relative_path: &str, new_name: &str) -> Result<String, String> {
        let full_path = self.resolve_safe_path(relative_path)?;
        if !full_path.exists() {
            return Err("Note not found".to_string());
        }

        let safe_name = sanitize_name(new_name)?;
        let parent = full_path.parent().unwrap_or(&self.root);

        let new_path = if full_path.is_dir() {
            parent.join(&safe_name)
        } else {
            parent.join(format!("{safe_name}.md"))
        };

        if new_path.exists() {
            return Err("A note with that name already exists".to_string());
        }

        std::fs::rename(&full_path, &new_path)
            .map_err(|e| format!("Failed to rename note: {e}"))?;

        let relative = new_path
            .strip_prefix(&self.root)
            .unwrap_or(&new_path)
            .to_string_lossy()
            .to_string();
        self.record(&relative, "moved", relative_path);
        Ok(relative)
    }

    /// Resolve a relative path to an absolute path, validating it stays within root.
    fn resolve_safe_path(&self, relative_path: &str) -> Result<PathBuf, String> {
        if relative_path.contains("..") {
            return Err("Invalid path: directory traversal not allowed".to_string());
        }
        // A note is never a dot-file or inside a dot-folder: that keeps `.deleted/`,
        // `.git` and, wherever the notes folder points, a store's `.secret` and `.pin`
        // out of the note tools' reach.
        let hidden = Path::new(relative_path).components().any(|c| {
            matches!(c, std::path::Component::Normal(name) if name.to_string_lossy().starts_with('.'))
        });
        if hidden {
            return Err("Invalid path: notes cannot be dot-files".to_string());
        }

        let full = self.root.join(relative_path);
        if !full.starts_with(&self.root) {
            return Err("Invalid path: outside notes directory".to_string());
        }

        Ok(full)
    }

    /// Recursively build the file tree.
    fn build_tree(&self, dir: &Path, prefix: &str) -> Result<Vec<NoteTreeEntry>, String> {
        let mut entries = Vec::new();

        let read_dir =
            std::fs::read_dir(dir).map_err(|e| format!("Failed to read directory: {e}"))?;

        let mut items: Vec<_> = read_dir
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                !name.starts_with('.')
            })
            .collect();

        // Sort: folders first, then alphabetical
        items.sort_by(|a, b| {
            let a_dir = a.path().is_dir();
            let b_dir = b.path().is_dir();
            match (a_dir, b_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.file_name().cmp(&b.file_name()),
            }
        });

        for item in items {
            let path = item.path();
            let name = item.file_name().to_string_lossy().to_string();

            if path.is_dir() {
                let rel_path = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}/{name}")
                };
                let children = self.build_tree(&path, &rel_path)?;
                entries.push(NoteTreeEntry {
                    name,
                    path: rel_path,
                    entry_type: "folder".to_string(),
                    children,
                });
            } else if name.ends_with(".md") {
                let display_name = name.trim_end_matches(".md").to_string();
                let rel_path = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}/{name}")
                };
                entries.push(NoteTreeEntry {
                    name: display_name,
                    path: rel_path,
                    entry_type: "file".to_string(),
                    children: Vec::new(),
                });
            }
        }

        Ok(entries)
    }
}

/// Validate and sanitize a file/folder name.
fn sanitize_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Name cannot be empty".to_string());
    }
    if trimmed.contains("..") || trimmed.contains('/') || trimmed.contains('\\') {
        return Err("Name contains invalid characters".to_string());
    }
    // Allow alphanumeric, spaces, dashes, underscores
    let safe: String = trimmed
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_')
        .collect();
    if safe.is_empty() {
        return Err("Name contains only invalid characters".to_string());
    }
    Ok(safe)
}

/// The file [`adopt`] leaves behind in the old folder, naming the new place.
pub const ADOPT_README: &str = "README.md";

/// What [`adopt`] moved, or would move under `dry_run`.
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct AdoptReport {
    /// The folder the files came from.
    pub from: String,
    /// The folder they went into.
    pub into: String,
    /// The relative paths moved, in walk order.
    pub moved: Vec<String>,
    /// True when the old folder was missing or held nothing but the README.
    pub nothing_to_do: bool,
}

/// Move every file under `from` into `into`, keeping names and folders. Refuses, moving
/// nothing, when any destination already exists or when the two folders are the same
/// or nested; deletes nothing (empty folders stay); skips a README of its own from an
/// earlier run; and writes that README into `from` naming the new place. `dry_run`
/// reports without touching a file.
pub fn adopt(from: &Path, into: &Path, dry_run: bool) -> Result<AdoptReport, String> {
    let mut report = AdoptReport {
        from: from.display().to_string(),
        into: into.display().to_string(),
        ..AdoptReport::default()
    };
    if !from.is_dir() {
        report.nothing_to_do = true;
        return Ok(report);
    }
    let from_c = from
        .canonicalize()
        .map_err(|e| format!("{}: {e}", from.display()))?;
    if let Ok(into_c) = into.canonicalize() {
        if into_c == from_c {
            return Err(format!(
                "{} is already the notes folder; nothing to adopt",
                into.display()
            ));
        }
        if into_c.starts_with(&from_c) || from_c.starts_with(&into_c) {
            return Err(format!(
                "{} and {} are nested; refusing to move a folder into itself",
                from.display(),
                into.display()
            ));
        }
    }
    let mut files = Vec::new();
    collect_files(&from_c, &from_c, &mut files)?;
    files.retain(|rel| !(rel == ADOPT_README && is_adopt_readme(&from_c.join(rel))));
    if files.is_empty() {
        report.nothing_to_do = true;
        return Ok(report);
    }
    let clashes: Vec<&String> = files.iter().filter(|rel| into.join(rel).exists()).collect();
    if !clashes.is_empty() {
        let list: Vec<&str> = clashes.iter().map(|s| s.as_str()).collect();
        return Err(format!(
            "refusing to move anything: {} already exist under {}: {}",
            clashes.len(),
            into.display(),
            list.join(", ")
        ));
    }
    report.moved.clone_from(&files);
    if dry_run {
        return Ok(report);
    }
    for rel in &files {
        let src = from_c.join(rel);
        let dst = into.join(rel);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create {}: {e}", parent.display()))?;
        }
        if std::fs::rename(&src, &dst).is_err() {
            std::fs::copy(&src, &dst).map_err(|e| format!("copy {rel}: {e}"))?;
            std::fs::remove_file(&src).map_err(|e| format!("remove {rel} after copying: {e}"))?;
        }
    }
    let readme = format!(
        "Notes moved to {} on {} by `rusty-cli notes adopt`. The vault's explorer, search,\nlinks and graph cover them there; the notes tools and the `/note` skill read that folder.\n",
        into.display(),
        crate::brain::frontmatter::today_iso()
    );
    std::fs::write(from_c.join(ADOPT_README), readme)
        .map_err(|e| format!("write {}: {e}", ADOPT_README))?;
    Ok(report)
}

/// Every file under `dir`, as paths relative to `root`, folders first by name.
fn collect_files(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), String> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("read {}: {e}", dir.display()))?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, out)?;
        } else if let Ok(rel) = path.strip_prefix(root) {
            out.push(rel.to_string_lossy().to_string());
        }
    }
    Ok(())
}

/// True for the README [`adopt`] wrote, so a second run does not carry it along.
fn is_adopt_readme(path: &Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|s| s.starts_with("Notes moved to "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_notes_dir(test_name: &str) -> (PathBuf, NotesManager) {
        let dir = std::env::temp_dir().join(format!(
            "rusty_notes_test_{}_{test_name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::create_dir_all(dir.join(".deleted")).unwrap();
        let manager = NotesManager {
            root: dir.clone(),
            changes: None,
        };
        (dir, manager)
    }

    fn cleanup(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn list_tree_empty() {
        let (dir, nm) = test_notes_dir("list_tree_empty");
        let tree = nm.list_tree().unwrap();
        assert!(tree.is_empty());
        cleanup(&dir);
    }

    #[test]
    fn create_and_read_note() {
        let (dir, nm) = test_notes_dir("create_and_read");
        let path = nm.create_note("", "Test Note", false).unwrap();
        assert_eq!(path, "Test Note.md");

        nm.save_note(&path, "# Hello World").unwrap();
        let content = nm.read_note(&path).unwrap();
        assert_eq!(content, "# Hello World");
        cleanup(&dir);
    }

    #[test]
    fn create_folder_and_note_inside() {
        let (dir, nm) = test_notes_dir("folder_and_note");
        let folder = nm.create_note("", "My Folder", true).unwrap();
        assert_eq!(folder, "My Folder");

        nm.save_note("My Folder/child.md", "child content").unwrap();
        let content = nm.read_note("My Folder/child.md").unwrap();
        assert_eq!(content, "child content");

        let tree = nm.list_tree().unwrap();
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].entry_type, "folder");
        assert_eq!(tree[0].children.len(), 1);
        cleanup(&dir);
    }

    #[test]
    fn delete_note_soft_deletes() {
        let (dir, nm) = test_notes_dir("delete_soft");
        nm.create_note("", "To Delete", false).unwrap();
        assert!(dir.join("To Delete.md").exists());

        nm.delete_note("To Delete.md").unwrap();
        assert!(!dir.join("To Delete.md").exists());
        let deleted_files: Vec<_> = fs::read_dir(dir.join(".deleted"))
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(deleted_files.len(), 1);
        cleanup(&dir);
    }

    #[test]
    fn rename_note() {
        let (dir, nm) = test_notes_dir("rename");
        nm.create_note("", "Original", false).unwrap();
        let new_path = nm.rename_note("Original.md", "Renamed").unwrap();
        assert_eq!(new_path, "Renamed.md");
        assert!(!dir.join("Original.md").exists());
        assert!(dir.join("Renamed.md").exists());
        cleanup(&dir);
    }

    #[test]
    fn create_duplicate_fails() {
        let (dir, nm) = test_notes_dir("duplicate");
        nm.create_note("", "Unique", false).unwrap();
        let result = nm.create_note("", "Unique", false);
        assert!(result.is_err());
        cleanup(&dir);
    }

    #[test]
    fn path_traversal_rejected() {
        let (dir, nm) = test_notes_dir("traversal");
        assert!(nm.read_note("../etc/passwd").is_err());
        assert!(nm.save_note("../../evil.md", "hack").is_err());
        cleanup(&dir);
    }

    #[test]
    fn sanitize_name_valid() {
        assert_eq!(sanitize_name("My Note").unwrap(), "My Note");
        assert_eq!(sanitize_name("hello-world_2").unwrap(), "hello-world_2");
    }

    #[test]
    fn sanitize_name_strips_special() {
        assert_eq!(sanitize_name("hello!@#world").unwrap(), "helloworld");
    }

    #[test]
    fn sanitize_name_rejects_empty() {
        assert!(sanitize_name("").is_err());
        assert!(sanitize_name("!!!").is_err());
    }

    #[test]
    fn tree_sorts_folders_first() {
        let (dir, nm) = test_notes_dir("sort_order");
        fs::write(dir.join("zebra.md"), "").unwrap();
        fs::create_dir(dir.join("alpha")).unwrap();

        let tree = nm.list_tree().unwrap();
        assert_eq!(tree[0].name, "alpha");
        assert_eq!(tree[0].entry_type, "folder");
        assert_eq!(tree[1].name, "zebra");
        assert_eq!(tree[1].entry_type, "file");
        cleanup(&dir);
    }

    #[test]
    fn adopt_moves_files_and_folders_and_leaves_a_readme() {
        let base = std::env::temp_dir().join(format!("rusty_notes_adopt_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let from = base.join("old");
        let into = base.join("vault").join("notes");
        fs::create_dir_all(from.join("sub")).unwrap();
        fs::create_dir_all(from.join(".deleted")).unwrap();
        fs::write(from.join("A.md"), "a").unwrap();
        fs::write(from.join("sub").join("B.md"), "b").unwrap();
        fs::write(from.join(".deleted").join("C.md_1"), "c").unwrap();

        let report = adopt(&from, &into, false).unwrap();
        assert_eq!(report.moved, vec![".deleted/C.md_1", "A.md", "sub/B.md"]);
        assert!(!report.nothing_to_do);
        assert_eq!(fs::read_to_string(into.join("A.md")).unwrap(), "a");
        assert_eq!(
            fs::read_to_string(into.join("sub").join("B.md")).unwrap(),
            "b"
        );
        assert!(into.join(".deleted").join("C.md_1").exists());
        assert!(!from.join("A.md").exists());
        assert!(
            from.join("sub").is_dir(),
            "empty folders stay; nothing is deleted"
        );
        let readme = fs::read_to_string(from.join(ADOPT_README)).unwrap();
        assert!(readme.starts_with("Notes moved to "), "{readme}");

        // A second run finds only the README and carries nothing along.
        let again = adopt(&from, &into, false).unwrap();
        assert!(again.nothing_to_do);
        assert!(!into.join(ADOPT_README).exists());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn adopt_refuses_a_clash_and_moves_nothing() {
        let base = std::env::temp_dir().join(format!("rusty_notes_clash_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let from = base.join("old");
        let into = base.join("notes");
        fs::create_dir_all(&from).unwrap();
        fs::create_dir_all(&into).unwrap();
        fs::write(from.join("A.md"), "old a").unwrap();
        fs::write(from.join("B.md"), "b").unwrap();
        fs::write(into.join("A.md"), "new a").unwrap();

        let err = adopt(&from, &into, false).unwrap_err();
        assert!(err.contains("A.md"), "{err}");
        assert_eq!(fs::read_to_string(from.join("B.md")).unwrap(), "b");
        assert!(!into.join("B.md").exists());
        assert_eq!(fs::read_to_string(into.join("A.md")).unwrap(), "new a");
        assert!(!from.join(ADOPT_README).exists());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn adopt_dry_run_moves_nothing() {
        let base = std::env::temp_dir().join(format!("rusty_notes_dry_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let from = base.join("old");
        let into = base.join("notes");
        fs::create_dir_all(&from).unwrap();
        fs::write(from.join("A.md"), "a").unwrap();
        let report = adopt(&from, &into, true).unwrap();
        assert_eq!(report.moved, vec!["A.md"]);
        assert!(from.join("A.md").exists());
        assert!(!into.exists());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn adopt_refuses_the_same_or_a_nested_folder() {
        let base = std::env::temp_dir().join(format!("rusty_notes_same_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let from = base.join("old");
        fs::create_dir_all(from.join("inner")).unwrap();
        fs::write(from.join("A.md"), "a").unwrap();
        assert!(adopt(&from, &from, false).is_err());
        assert!(adopt(&from, &from.join("inner"), false).is_err());
        assert!(from.join("A.md").exists());
        let _ = fs::remove_dir_all(&base);
    }
}
