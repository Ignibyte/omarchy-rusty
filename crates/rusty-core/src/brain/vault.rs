//! Vault filesystem manager for brain pages.
//!
//! Manages the `~/.rusty/brain/` directory: the type folders, any folder the user adds
//! (the tree is real and nested, as Obsidian shows it), reading and writing markdown
//! files, renames, soft-deletes into `archive/`, and slugs from titles. Dot-folders
//! (`.git`, `.obsidian`, `.templates`) and the root's `archive/` are never part of the
//! tree or the page walk.

use std::path::{Path, PathBuf};

/// The soft-delete bin at the vault root. Nothing in it is a page (TICKET-040).
pub const ARCHIVE_DIR: &str = "archive";

/// Whether a directory entry is left out of the tree and the walks: a dot-name anywhere,
/// or the archive at the root. A folder named `archive` deeper down is an ordinary folder.
/// `rel` is the vault-relative folder holding the entry, empty at the root.
pub(crate) fn hidden(rel: &str, name: &str) -> bool {
    name.starts_with('.') || (rel.is_empty() && name == ARCHIVE_DIR)
}

/// `dir/name`, or `dir/name_2`, `_3`, … when that is taken: a soft delete never
/// replaces what an earlier one put in the archive, however close together they ran.
pub(crate) fn unused_name(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| dir.join(format!("{name}_{n}")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

/// Whether a vault-relative path is the root's archive or inside it.
pub(crate) fn in_archive(rel: &str) -> bool {
    let rel = clean_rel(rel);
    rel == ARCHIVE_DIR || rel.starts_with(&format!("{ARCHIVE_DIR}/"))
}

/// Refuse a write whose vault-relative path lies in the archive: a page put there would
/// be indexed once and dropped by the next full sync. Deleting is how a page gets there.
fn refuse_archive(rel: &str) -> Result<(), String> {
    if in_archive(rel) {
        return Err(format!(
            "{ARCHIVE_DIR}/ holds deleted pages; delete a page to archive it, or move one out to restore it"
        ));
    }
    Ok(())
}

/// Type directories and their corresponding page types.
const TYPE_DIRS: &[(&str, &str)] = &[
    ("person", "people"),
    ("company", "companies"),
    ("project", "projects"),
    ("concept", "concepts"),
    ("meeting", "meetings"),
    ("idea", "ideas"),
    ("daily", "daily"),
    ("inbox", "inbox"),
    ("decision", "decisions"),
    ("conversation", "conversations"),
    ("source", "sources"),
];

/// The type of a page in a folder that is not a type folder, or at the root.
pub const NOTE_TYPE: &str = "note";

/// Every page type with the vault folder it lives in, in display order.
pub fn page_types() -> &'static [(&'static str, &'static str)] {
    TYPE_DIRS
}

/// The page type a folder implies: `people` is `person`; anything else is `note`.
pub fn type_for_folder(folder: &str) -> &'static str {
    TYPE_DIRS
        .iter()
        .find(|(_, d)| *d == folder)
        .map(|(t, _)| *t)
        .unwrap_or(NOTE_TYPE)
}

/// The page type a slug's top folder implies.
pub fn type_for_slug(slug: &str) -> &'static str {
    match slug.split_once('/') {
        Some((top, _)) => type_for_folder(top),
        None => NOTE_TYPE,
    }
}

/// One entry of the vault tree.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VaultNode {
    /// File or folder name as shown (`sarah-chen`, `projects`, `diagram.png`).
    pub name: String,
    /// Vault-relative path: the slug for a page, the folder path, or the file path.
    pub path: String,
    /// `folder`, `page` (markdown) or `file` (anything else).
    pub kind: String,
    /// Pages in this folder and below (0 for files).
    pub pages: usize,
    /// Children, folders first, each group sorted by name.
    pub children: Vec<VaultNode>,
}

/// Manages markdown files in the brain vault directory.
pub struct VaultManager {
    root: PathBuf,
    /// In-flight git-commit threads. The long-running server fires commits and
    /// forgets them; a short-lived process (rusty-cli) calls [`flush_commits`]
    /// before exit so the commit isn't dropped when the process ends.
    ///
    /// [`flush_commits`]: VaultManager::flush_commits
    pending_commits: std::sync::Mutex<Vec<std::thread::JoinHandle<()>>>,
    /// Vault-relative paths this manager's writes touched since its last commit
    /// (TICKET-043); the next [`git_commit`](VaultManager::git_commit) takes them.
    touched: std::sync::Mutex<Vec<String>>,
    /// Paths handed to a commit that has not finished, so a sweep of outside edits leaves
    /// them to it.
    in_flight: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
}

impl VaultManager {
    /// Create a new VaultManager rooted at the given path.
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            pending_commits: std::sync::Mutex::new(Vec::new()),
            touched: std::sync::Mutex::new(Vec::new()),
            in_flight: std::sync::Arc::default(),
        }
    }

    /// Note that a write changed `rel` (vault-relative), for the next commit. Every write
    /// through this manager notes its own paths; a caller that writes a vault file
    /// directly calls this.
    pub fn touch(&self, rel: &str) {
        let rel = clean_rel(rel);
        if rel.is_empty() {
            return;
        }
        if let Ok(mut touched) = self.touched.lock() {
            if !touched.contains(&rel) {
                touched.push(rel);
            }
        }
    }

    fn touch_path(&self, path: &Path) {
        if let Ok(rel) = path.strip_prefix(&self.root) {
            self.touch(&rel.to_string_lossy());
        }
    }

    /// Ensure all type directories, `.templates/`, and `archive/` exist.
    /// Also initializes a git repo in the vault if one doesn't exist.
    pub fn ensure_dirs(&self) -> Result<(), String> {
        for (_, dir) in TYPE_DIRS {
            std::fs::create_dir_all(self.root.join(dir))
                .map_err(|e| format!("Failed to create {dir}/ directory: {e}"))?;
        }
        std::fs::create_dir_all(self.root.join(".templates"))
            .map_err(|e| format!("Failed to create .templates/ directory: {e}"))?;
        std::fs::create_dir_all(self.root.join(ARCHIVE_DIR))
            .map_err(|e| format!("Failed to create archive/ directory: {e}"))?;

        // Initialize git repo if not present
        if !self.root.join(".git").exists() {
            self.git_init();
        }

        Ok(())
    }

    /// Commit the paths this manager's writes touched since its last commit, and only
    /// those, with a descriptive message (TICKET-043). An edit made outside the tools is
    /// left for [`commit_outside_edits`](VaultManager::commit_outside_edits).
    ///
    /// Runs git in a background thread so callers (especially the async server)
    /// don't block on the subprocess. The handle is tracked so a short-lived
    /// process can [`flush_commits`] before exiting; finished handles are pruned
    /// on each call to keep the tracking vector bounded in a long-running server.
    /// A failure is printed with git's message; the paths stay changed on disk, so the
    /// next sweep of outside edits commits them.
    ///
    /// [`flush_commits`]: VaultManager::flush_commits
    pub fn git_commit(&self, message: &str) {
        let paths: Vec<String> = match self.touched.lock() {
            Ok(mut touched) => touched.drain(..).collect(),
            Err(_) => return,
        };
        if paths.is_empty() {
            return;
        }
        if let Ok(mut in_flight) = self.in_flight.lock() {
            in_flight.extend(paths.iter().cloned());
        }
        let root = self.root.clone();
        let msg = message.to_string();
        let in_flight = std::sync::Arc::clone(&self.in_flight);
        let handle = std::thread::spawn(move || {
            if let Err(e) = commit_paths(&root, &msg, &paths) {
                eprintln!("rusty: vault commit \"{msg}\": {e}");
            }
            if let Ok(mut in_flight) = in_flight.lock() {
                for path in &paths {
                    in_flight.remove(path);
                }
            }
        });
        if let Ok(mut pending) = self.pending_commits.lock() {
            pending.retain(|h| !h.is_finished());
            pending.push(handle);
        }
    }

    /// Commit what changed in the vault that no write of this manager is about to commit:
    /// an edit made in Obsidian, an editor or by an agent's file write, in a commit of its
    /// own naming the paths (TICKET-043). Returns the paths committed. Any process may
    /// call it; a second finds the tree clean.
    pub fn commit_outside_edits(&self) -> Result<Vec<String>, String> {
        let mine: std::collections::HashSet<String> = {
            let mut mine: std::collections::HashSet<String> = self
                .touched
                .lock()
                .map(|t| t.iter().cloned().collect())
                .unwrap_or_default();
            if let Ok(in_flight) = self.in_flight.lock() {
                mine.extend(in_flight.iter().cloned());
            }
            mine
        };
        let claimed = |path: &str| {
            mine.iter()
                .any(|m| path == m || path.starts_with(&format!("{m}/")))
        };
        let changed: Vec<String> = changed_paths(&self.root, &[])?
            .into_iter()
            .filter(|p| !claimed(p))
            .collect();
        if changed.is_empty() {
            return Ok(Vec::new());
        }
        let names: Vec<&str> = changed.iter().take(5).map(String::as_str).collect();
        let more = changed.len().saturating_sub(names.len());
        let mut message = format!("edit outside the tools: {}", names.join(", "));
        if more > 0 {
            message.push_str(&format!(" (and {more} more)"));
        }
        commit_paths(&self.root, &message, &changed)?;
        Ok(changed)
    }

    /// Wait for all in-flight git-commit threads to finish.
    ///
    /// Call this before a process exits (rusty-cli, and a stdio server when its agent
    /// closes it) so an auto-commit isn't lost when the process ends.
    pub fn flush_commits(&self) {
        let handles: Vec<_> = match self.pending_commits.lock() {
            Ok(mut pending) => pending.drain(..).collect(),
            Err(_) => return,
        };
        for handle in handles {
            let _ = handle.join();
        }
    }

    /// Initialize a git repo in the vault.
    fn git_init(&self) {
        let _ = std::process::Command::new("git")
            .args(["init"])
            .current_dir(&self.root)
            .output();
        // Create .gitignore
        let gitignore = self.root.join(".gitignore");
        if !gitignore.exists() {
            let _ = std::fs::write(&gitignore, ".templates/\narchive/\n.obsidian/\n");
        }
        // Initial commit
        let _ = std::process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(&self.root)
            .output();
        let _ = std::process::Command::new("git")
            .args([
                "commit",
                "-m",
                "init: brain vault",
                "--allow-empty-message",
                "--no-gpg-sign",
            ])
            .current_dir(&self.root)
            .output();
    }

    /// Write a page's content to its markdown file.
    ///
    /// The slug determines the file path (e.g., `people/sarah-chen` → `people/sarah-chen.md`).
    pub fn write_page(&self, slug: &str, content: &str) -> Result<(), String> {
        refuse_archive(slug)?;
        let path = self.resolve_path(slug)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory: {e}"))?;
        }
        std::fs::write(&path, content).map_err(|e| format!("Failed to write page: {e}"))?;
        self.touch_path(&path);
        Ok(())
    }

    /// Write a file that is not a page (`.rusty/bookmarks.json`) at a vault-relative
    /// path, making its folders; noted for the next commit like any write.
    pub fn write_file(&self, rel: &str, content: &str) -> Result<(), String> {
        refuse_archive(rel)?;
        let path = self.resolve_rel(&clean_rel(rel))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory: {e}"))?;
        }
        std::fs::write(&path, content).map_err(|e| format!("Failed to write {rel}: {e}"))?;
        self.touch_path(&path);
        Ok(())
    }

    /// Read a file that is not a page; `None` when it does not exist.
    pub fn read_file(&self, rel: &str) -> Result<Option<String>, String> {
        let path = self.resolve_rel(&clean_rel(rel))?;
        if !path.is_file() {
            return Ok(None);
        }
        std::fs::read_to_string(&path)
            .map(Some)
            .map_err(|e| format!("Failed to read {rel}: {e}"))
    }

    /// Read a page's raw markdown content from disk.
    pub fn read_page(&self, slug: &str) -> Result<Option<String>, String> {
        let path = self.resolve_path(slug)?;
        if !path.exists() {
            return Ok(None);
        }
        let content =
            std::fs::read_to_string(&path).map_err(|e| format!("Failed to read page: {e}"))?;
        Ok(Some(content))
    }

    /// Soft-delete a page by moving it to `archive/` with a timestamp suffix.
    pub fn delete_page(&self, slug: &str) -> Result<(), String> {
        let path = self.resolve_path(slug)?;
        if !path.exists() {
            return Err(format!("Page not found: {slug}"));
        }
        self.archive(&path).map(|_| ())
    }

    /// Move a file or folder into `archive/` under `<name>_<timestamp>`; returns the
    /// vault-relative path it now has.
    fn archive(&self, path: &Path) -> Result<String, String> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let file_name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let archive_dir = self.root.join(ARCHIVE_DIR);
        std::fs::create_dir_all(&archive_dir)
            .map_err(|e| format!("Failed to create archive/: {e}"))?;
        let dest = unused_name(&archive_dir, &format!("{file_name}_{timestamp}"));
        std::fs::rename(path, &dest).map_err(|e| format!("Failed to archive: {e}"))?;
        self.touch_path(path);
        Ok(format!(
            "archive/{}",
            dest.file_name().unwrap_or_default().to_string_lossy()
        ))
    }

    /// Soft-delete a folder (and everything in it) into `archive/`.
    pub fn delete_folder(&self, folder: &str) -> Result<String, String> {
        let path = self.resolve_rel(folder)?;
        if !path.is_dir() {
            return Err(format!("Folder not found: {folder}"));
        }
        if path == self.root {
            return Err("Refusing to delete the vault root".to_string());
        }
        self.archive(&path)
    }

    /// Create a folder (and its parents) inside the vault.
    pub fn create_folder(&self, folder: &str) -> Result<String, String> {
        let rel = clean_rel(folder);
        if rel.is_empty() {
            return Err("A folder needs a name".to_string());
        }
        refuse_archive(&rel)?;
        let path = self.resolve_rel(&rel)?;
        if path.exists() {
            return Err(format!("Already exists: {rel}"));
        }
        std::fs::create_dir_all(&path).map_err(|e| format!("Failed to create folder: {e}"))?;
        Ok(rel)
    }

    /// Rename or move a file or folder. Both are vault-relative paths; a file keeps its
    /// extension (`people/x.md` to `archive/x.md`). The target's parents are created; an
    /// existing target is refused.
    pub fn rename_path(&self, from: &str, to: &str) -> Result<(), String> {
        refuse_archive(to)?;
        let from_path = self.resolve_rel(from)?;
        let to_path = self.resolve_rel(to)?;
        if !from_path.exists() {
            return Err(format!("Not found: {from}"));
        }
        if to_path.exists() {
            return Err(format!("Already exists: {to}"));
        }
        if from_path == self.root || to_path.starts_with(&from_path) && from_path.is_dir() {
            return Err("Cannot move a folder into itself".to_string());
        }
        if let Some(parent) = to_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory: {e}"))?;
        }
        std::fs::rename(&from_path, &to_path).map_err(|e| format!("Failed to move: {e}"))?;
        self.touch_path(&from_path);
        self.touch_path(&to_path);
        Ok(())
    }

    /// Whether a vault-relative path (file or folder) exists.
    pub fn exists(&self, rel: &str) -> bool {
        self.resolve_rel(rel).map(|p| p.exists()).unwrap_or(false)
    }

    /// Whether a vault-relative path is a folder.
    pub fn is_folder(&self, rel: &str) -> bool {
        self.resolve_rel(rel).map(|p| p.is_dir()).unwrap_or(false)
    }

    /// Check if a page file exists on disk.
    pub fn page_exists(&self, slug: &str) -> bool {
        self.resolve_path(slug)
            .map(|p| p.is_file())
            .unwrap_or(false)
    }

    /// The vault as a tree: folders first, then pages and other files, each group by
    /// name, dot-entries left out.
    pub fn tree(&self) -> Result<VaultNode, String> {
        let mut root = VaultNode {
            name: self
                .root
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "vault".to_string()),
            path: String::new(),
            kind: "folder".to_string(),
            pages: 0,
            children: Vec::new(),
        };
        self.fill_tree(&self.root, "", &mut root)?;
        Ok(root)
    }

    fn fill_tree(&self, dir: &Path, rel: &str, node: &mut VaultNode) -> Result<(), String> {
        let entries = std::fs::read_dir(dir).map_err(|e| format!("Failed to read {rel}: {e}"))?;
        let mut folders = Vec::new();
        let mut files = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if hidden(rel, &name) {
                continue;
            }
            let path = entry.path();
            let child_rel = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            if path.is_dir() {
                let mut child = VaultNode {
                    name,
                    path: child_rel.clone(),
                    kind: "folder".to_string(),
                    pages: 0,
                    children: Vec::new(),
                };
                self.fill_tree(&path, &child_rel, &mut child)?;
                folders.push(child);
            } else if path.is_file() {
                let is_page = path.extension().and_then(|e| e.to_str()) == Some("md");
                files.push(VaultNode {
                    name: if is_page {
                        name.strip_suffix(".md").unwrap_or(&name).to_string()
                    } else {
                        name
                    },
                    path: if is_page {
                        child_rel
                            .strip_suffix(".md")
                            .unwrap_or(&child_rel)
                            .to_string()
                    } else {
                        child_rel
                    },
                    kind: if is_page { "page" } else { "file" }.to_string(),
                    pages: usize::from(is_page),
                    children: Vec::new(),
                });
            }
        }
        let by_name =
            |a: &VaultNode, b: &VaultNode| a.name.to_lowercase().cmp(&b.name.to_lowercase());
        folders.sort_by(by_name);
        files.sort_by(by_name);
        node.pages = folders.iter().map(|f| f.pages).sum::<usize>()
            + files.iter().map(|f| f.pages).sum::<usize>();
        node.children = folders;
        node.children.extend(files);
        Ok(())
    }

    /// Every markdown file in the vault, in any folder, as (slug, path) pairs, sorted by
    /// slug. Dot-folders are skipped.
    pub fn list_all_files(&self) -> Result<Vec<(String, PathBuf)>, String> {
        let mut files = Vec::new();
        self.walk_pages(&self.root, "", &mut files)?;
        files.sort();
        Ok(files)
    }

    fn walk_pages(
        &self,
        dir: &Path,
        rel: &str,
        out: &mut Vec<(String, PathBuf)>,
    ) -> Result<(), String> {
        let entries = std::fs::read_dir(dir).map_err(|e| format!("Failed to read {rel}: {e}"))?;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if hidden(rel, &name) {
                continue;
            }
            let path = entry.path();
            let child_rel = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            if path.is_dir() {
                self.walk_pages(&path, &child_rel, out)?;
            } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                out.push((
                    child_rel
                        .strip_suffix(".md")
                        .unwrap_or(&child_rel)
                        .to_string(),
                    path,
                ));
            }
        }
        Ok(())
    }

    /// Find a non-markdown file (an image, say) by its vault path or, failing that, by
    /// its file name anywhere in the vault; the first match by path order wins.
    pub fn find_file(&self, target: &str) -> Option<PathBuf> {
        let rel = clean_rel(target);
        if let Ok(path) = self.resolve_rel(&rel) {
            if path.is_file() {
                return Some(path);
            }
        }
        let wanted = rel.rsplit('/').next()?.to_lowercase();
        let mut found = Vec::new();
        self.walk_files(&self.root, "", &wanted, &mut found);
        found.sort();
        found.into_iter().next()
    }

    fn walk_files(&self, dir: &Path, rel: &str, wanted: &str, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if hidden(rel, &name) {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                let child_rel = if rel.is_empty() {
                    name.clone()
                } else {
                    format!("{rel}/{name}")
                };
                self.walk_files(&path, &child_rel, wanted, out);
            } else if name.to_lowercase() == wanted {
                out.push(path);
            }
        }
    }

    /// Resolve a slug to its absolute filesystem path.
    ///
    /// Validates the path stays within the vault root.
    pub fn resolve_path(&self, slug: &str) -> Result<PathBuf, String> {
        if slug.contains("..") {
            return Err("Invalid slug: path traversal not allowed".to_string());
        }
        // A slug written with its `.md` names the same page, never `<name>.md.md`.
        let slug = slug.strip_suffix(".md").unwrap_or(slug);
        let path = self.root.join(format!("{slug}.md"));
        if !path.starts_with(&self.root) {
            return Err("Invalid slug: outside vault directory".to_string());
        }
        Ok(path)
    }

    /// Resolve a vault-relative path (file with extension, or folder) to an absolute
    /// path inside the vault.
    pub fn resolve_rel(&self, rel: &str) -> Result<PathBuf, String> {
        if rel.contains("..") {
            return Err("Invalid path: path traversal not allowed".to_string());
        }
        let rel = clean_rel(rel);
        let path = if rel.is_empty() {
            self.root.clone()
        } else {
            self.root.join(&rel)
        };
        if !path.starts_with(&self.root) {
            return Err("Invalid path: outside vault directory".to_string());
        }
        Ok(path)
    }

    /// Get the vault root path.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// The vault-relative paths `git status` reports as changed, untracked ones included,
/// within `pathspecs` (the whole vault when empty).
fn changed_paths(root: &Path, pathspecs: &[String]) -> Result<Vec<String>, String> {
    // Literal pathspecs: a page named with `*`, `?` or `[` is that file, not a pattern.
    let out = std::process::Command::new("git")
        .args([
            "--literal-pathspecs",
            "status",
            "--porcelain=v1",
            "-z",
            "--no-renames",
            "--untracked-files=all",
            "--",
        ])
        .args(pathspecs)
        .current_dir(root)
        .output()
        .map_err(|e| format!("git status: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git status: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter_map(|entry| entry.get(3..))
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect())
}

/// Stage and commit exactly the changed paths among `paths`, nothing else; git's own
/// message on a failure. Nothing changed is not an error.
fn commit_paths(root: &Path, message: &str, paths: &[String]) -> Result<(), String> {
    let changed = changed_paths(root, paths)?;
    if changed.is_empty() {
        return Ok(());
    }
    // Several processes commit to one vault; git's index lock turns them away while another
    // holds it, so a locked attempt waits and tries again before it gives up.
    let run = |args: &[&str]| -> Result<(), String> {
        let mut attempt = 0;
        loop {
            let out = std::process::Command::new("git")
                .arg("--literal-pathspecs")
                .args(args)
                .args(&changed)
                .current_dir(root)
                .output()
                .map_err(|e| format!("git {}: {e}", args[0]))?;
            if out.status.success() {
                return Ok(());
            }
            let stderr = String::from_utf8_lossy(&out.stderr);
            attempt += 1;
            if stderr.contains("index.lock") && attempt < 8 {
                std::thread::sleep(std::time::Duration::from_millis(100 * attempt));
                continue;
            }
            return Err(format!("git {}: {}", args[0], stderr.trim()));
        }
    };
    run(&["add", "-A", "--"])?;
    run(&[
        "commit",
        "-m",
        message,
        "--allow-empty-message",
        "--no-gpg-sign",
        "--",
    ])
}

/// A vault-relative path without leading or trailing slashes or `./`.
pub fn clean_rel(rel: &str) -> String {
    let mut r = rel.trim().trim_matches('/');
    while let Some(rest) = r.strip_prefix("./") {
        r = rest;
    }
    r.to_string()
}

/// Convert a title string to a URL-safe slug.
///
/// "Sarah Chen" → "sarah-chen"
/// "O'Brien (CEO)" → "obrien-ceo"
pub fn title_to_slug(title: &str) -> Result<String, String> {
    let slug: String = title
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c
            } else if c == ' ' || c == '_' || c == '-' {
                '-'
            } else {
                // Strip other characters
                '\0'
            }
        })
        .filter(|c| *c != '\0')
        .collect();

    // Collapse multiple hyphens
    let mut collapsed = String::with_capacity(slug.len());
    let mut last_was_hyphen = false;
    for c in slug.chars() {
        if c == '-' {
            if !last_was_hyphen {
                collapsed.push(c);
            }
            last_was_hyphen = true;
        } else {
            collapsed.push(c);
            last_was_hyphen = false;
        }
    }

    let result = collapsed.trim_matches('-').to_string();
    if result.is_empty() {
        return Err("Title produces empty slug".to_string());
    }
    Ok(result)
}

/// Map a page type to its directory name.
///
/// "person" → "people", "company" → "companies"
pub fn type_to_dir(page_type: &str) -> Result<&'static str, String> {
    TYPE_DIRS
        .iter()
        .find(|(t, _)| *t == page_type)
        .map(|(_, d)| *d)
        .ok_or_else(|| {
            format!(
                "Unknown page type: {page_type}. Valid types: {}",
                TYPE_DIRS
                    .iter()
                    .map(|(t, _)| *t)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Two pages with one file name deleted in the same second both stay in the archive,
    /// and a slug written with `.md` names the page, not `<name>.md.md`.
    #[test]
    fn soft_deletes_never_replace_each_other_and_md_slugs_are_one_page() {
        let (dir, vm) = test_vault("archive_names");
        vm.write_page("people/plan", "A").unwrap();
        vm.write_page("projects/plan", "B").unwrap();
        vm.delete_page("people/plan").unwrap();
        vm.delete_page("projects/plan").unwrap();
        let mut kept: Vec<String> = fs::read_dir(dir.join(ARCHIVE_DIR))
            .unwrap()
            .flatten()
            .map(|e| fs::read_to_string(e.path()).unwrap())
            .collect();
        kept.sort();
        assert_eq!(kept, vec!["A".to_string(), "B".to_string()]);

        vm.write_page("ideas/x.md", "X").unwrap();
        assert!(dir.join("ideas/x.md").is_file());
        assert!(!dir.join("ideas/x.md.md").exists());
        assert_eq!(vm.read_page("ideas/x").unwrap().as_deref(), Some("X"));
        let _ = fs::remove_dir_all(dir);
    }

    fn test_vault(name: &str) -> (PathBuf, VaultManager) {
        let dir =
            std::env::temp_dir().join(format!("rusty_brain_vault_{}_{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let vm = VaultManager::new(dir.clone());
        vm.ensure_dirs().unwrap();
        (dir, vm)
    }

    fn cleanup(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn title_to_slug_basic() {
        assert_eq!(title_to_slug("Sarah Chen").unwrap(), "sarah-chen");
        assert_eq!(title_to_slug("hello world").unwrap(), "hello-world");
        assert_eq!(title_to_slug("MCP Protocol").unwrap(), "mcp-protocol");
    }

    #[test]
    fn title_to_slug_special_chars() {
        assert_eq!(title_to_slug("O'Brien (CEO)").unwrap(), "obrien-ceo");
        assert_eq!(title_to_slug("hello---world").unwrap(), "hello-world");
        assert_eq!(title_to_slug("  spaces  ").unwrap(), "spaces");
    }

    #[test]
    fn title_to_slug_empty_rejected() {
        assert!(title_to_slug("!!!").is_err());
        assert!(title_to_slug("").is_err());
    }

    #[test]
    fn type_to_dir_pluralization() {
        assert_eq!(type_to_dir("person").unwrap(), "people");
        assert_eq!(type_to_dir("company").unwrap(), "companies");
        assert_eq!(type_to_dir("project").unwrap(), "projects");
        assert_eq!(type_to_dir("daily").unwrap(), "daily");
        assert!(type_to_dir("invalid").is_err());
        assert_eq!(type_for_folder("people"), "person");
        assert_eq!(type_for_folder("notes"), "note");
        assert_eq!(type_for_slug("2026-09-02"), "note");
        assert_eq!(type_for_slug("projects/deep/one"), "project");
    }

    #[test]
    fn ensure_dirs_creates_all() {
        let (dir, _vm) = test_vault("ensure_dirs");
        assert!(dir.join("people").is_dir());
        assert!(dir.join("companies").is_dir());
        assert!(dir.join("projects").is_dir());
        assert!(dir.join("concepts").is_dir());
        assert!(dir.join("meetings").is_dir());
        assert!(dir.join("ideas").is_dir());
        assert!(dir.join("daily").is_dir());
        assert!(dir.join("inbox").is_dir());
        assert!(dir.join(".templates").is_dir());
        assert!(dir.join("archive").is_dir());
        cleanup(&dir);
    }

    #[test]
    fn write_and_read_page() {
        let (dir, vm) = test_vault("write_read");
        vm.write_page("people/test-person", "# Test Person\n\nContent here.\n")
            .unwrap();
        let content = vm.read_page("people/test-person").unwrap().unwrap();
        assert_eq!(content, "# Test Person\n\nContent here.\n");
        assert!(vm.page_exists("people/test-person"));
        assert!(!vm.page_exists("people/nonexistent"));
        cleanup(&dir);
    }

    #[test]
    fn delete_moves_to_archive() {
        let (dir, vm) = test_vault("delete_archive");
        vm.write_page("people/to-delete", "content").unwrap();
        assert!(vm.page_exists("people/to-delete"));

        vm.delete_page("people/to-delete").unwrap();
        assert!(!vm.page_exists("people/to-delete"));

        let archive_files: Vec<_> = fs::read_dir(dir.join("archive"))
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(archive_files.len(), 1);
        cleanup(&dir);
    }

    #[test]
    fn walks_skip_the_root_archive_only() {
        let (dir, vm) = test_vault("archive_skip");
        fs::create_dir_all(dir.join("archive/zones_1")).unwrap();
        fs::write(dir.join("archive/zones_1/plan.md"), "Archived.").unwrap();
        fs::write(dir.join("archive/only-here.png"), "png").unwrap();
        fs::create_dir_all(dir.join("projects/archive")).unwrap();
        fs::write(dir.join("projects/archive/old.md"), "Kept.").unwrap();

        let slugs: Vec<String> = vm
            .list_all_files()
            .unwrap()
            .into_iter()
            .map(|f| f.0)
            .collect();
        assert!(
            slugs.contains(&"projects/archive/old".to_string()),
            "{slugs:?}"
        );
        assert!(
            slugs.iter().all(|s| !s.starts_with("archive/")),
            "{slugs:?}"
        );

        let tree = vm.tree().unwrap();
        assert!(tree.children.iter().all(|c| c.path != "archive"));
        let projects = tree.children.iter().find(|c| c.path == "projects").unwrap();
        assert!(projects
            .children
            .iter()
            .any(|c| c.path == "projects/archive"));

        assert_eq!(vm.find_file("only-here.png"), None);
        cleanup(&dir);
    }

    #[test]
    fn nothing_is_written_or_moved_into_the_archive() {
        let (dir, vm) = test_vault("archive_refuse");
        assert!(vm.write_page("archive/x", "x").is_err());
        assert!(vm.create_folder("archive/new").is_err());
        vm.write_page("ideas/x", "x").unwrap();
        assert!(vm.rename_path("ideas/x.md", "archive/x.md").is_err());
        assert!(vm.rename_path("ideas/x.md", "ideas/archive/x.md").is_ok());
        assert!(
            vm.delete_page("ideas/archive/x").is_ok(),
            "deleting still archives"
        );
        cleanup(&dir);
    }

    #[test]
    fn list_all_files_walks_every_folder() {
        let (dir, vm) = test_vault("list_all");
        vm.write_page("people/alice", "# Alice").unwrap();
        vm.write_page("concepts/rust", "# Rust").unwrap();
        vm.write_page("projects/deep/nested", "# Nested").unwrap();
        vm.write_page("loose", "# Loose").unwrap();
        fs::write(dir.join(".templates/person.md"), "tpl").unwrap();

        let files = vm.list_all_files().unwrap();
        let slugs: Vec<&str> = files.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(
            slugs,
            vec![
                "concepts/rust",
                "loose",
                "people/alice",
                "projects/deep/nested"
            ]
        );
        cleanup(&dir);
    }

    #[test]
    fn tree_is_folders_first_with_counts() {
        let (dir, vm) = test_vault("tree");
        vm.write_page("people/alice", "# Alice").unwrap();
        vm.write_page("projects/deep/nested", "# Nested").unwrap();
        vm.write_page("projects/Zed", "# Z").unwrap();
        vm.write_page("2026-09-02", "daily").unwrap();
        fs::write(dir.join("projects/data.json"), "{}").unwrap();
        let tree = vm.tree().unwrap();
        assert_eq!(tree.pages, 4);
        let names: Vec<&str> = tree.children.iter().map(|c| c.name.as_str()).collect();
        // Folders (the nine type folders plus archive) first, then the loose page.
        assert_eq!(names.last().copied(), Some("2026-09-02"));
        assert_eq!(tree.children.last().unwrap().kind, "page");
        let projects = tree.children.iter().find(|c| c.name == "projects").unwrap();
        assert_eq!(projects.pages, 2);
        let kinds: Vec<(&str, &str)> = projects
            .children
            .iter()
            .map(|c| (c.name.as_str(), c.kind.as_str()))
            .collect();
        assert_eq!(
            kinds,
            vec![("deep", "folder"), ("data.json", "file"), ("Zed", "page")]
        );
        assert_eq!(projects.children[2].path, "projects/Zed");
        assert_eq!(projects.children[1].path, "projects/data.json");
        cleanup(&dir);
    }

    #[test]
    fn folders_and_renames() {
        let (dir, vm) = test_vault("folders");
        assert_eq!(vm.create_folder("/areas/health/").unwrap(), "areas/health");
        assert!(vm.is_folder("areas/health"));
        assert!(vm.create_folder("areas/health").is_err());
        vm.write_page("areas/health/run", "# Run").unwrap();
        vm.rename_path("areas/health/run.md", "projects/run.md")
            .unwrap();
        assert!(vm.page_exists("projects/run"));
        assert!(vm
            .rename_path("projects/run.md", "projects/run.md")
            .is_err());
        vm.rename_path("areas", "zones").unwrap();
        assert!(vm.is_folder("zones/health"));
        assert!(vm.rename_path("zones", "zones/inner").is_err());
        let archived = vm.delete_folder("zones").unwrap();
        assert!(archived.starts_with("archive/zones_"));
        assert!(!vm.exists("zones"));
        assert!(vm.delete_folder("").is_err());
        fs::write(dir.join("projects/pic.png"), "png").unwrap();
        assert!(vm
            .find_file("pic.png")
            .unwrap()
            .ends_with("projects/pic.png"));
        assert!(vm.find_file("projects/pic.png").is_some());
        assert!(vm.find_file("nope.png").is_none());
        cleanup(&dir);
    }

    #[test]
    fn path_traversal_rejected() {
        let (dir, vm) = test_vault("traversal");
        assert!(vm.resolve_path("../etc/passwd").is_err());
        assert!(vm.write_page("../../evil", "hack").is_err());
        assert!(vm.resolve_rel("../x").is_err());
        assert!(vm.create_folder("../x").is_err());
        cleanup(&dir);
    }
}
