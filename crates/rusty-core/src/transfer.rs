//! Moving a whole store to another machine (TICKET-057). [`export`] writes everything
//! Rusty keeps into one zip: a snapshot of the database, the brain vault and the skills
//! store with their git history, a notes folder kept outside the vault, and, only when
//! asked, the secrets file and the PIN. [`import`] unpacks such a zip into a home that
//! holds no store, or, when told to replace, after moving the old home aside whole, and
//! points the path settings at where the parts landed.
//!
//! The zip holds `manifest.json`, `db/rusty.db`, `brain/`, `skills/`, `notes/` (only when
//! the notes folder sat outside the vault) and `secrets/` (only on request). Nothing else
//! of the home travels: the hook scripts are rewritten by `rusty-cli hooks install`, the
//! change sentinel is recreated, and other folders under the home are not the store.

use crate::brain::vault;
use crate::engine::db::Database;
use crate::engine::settings_manager::SettingsManager;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

/// The manifest's `format`.
pub const FORMAT: &str = "rusty-export";
/// The format version this build writes, and the newest it reads.
pub const FORMAT_VERSION: u32 = 1;

const MANIFEST: &str = "manifest.json";
const DB_ENTRY: &str = "db/rusty.db";
const SECRET_ENTRY: &str = "secrets/.secret";
const PIN_ENTRY: &str = "secrets/.pin";
/// The folders an export holds besides the manifest.
const ROOTS: [&str; 5] = ["db", "brain", "skills", "notes", "secrets"];

/// Where the parts of one store live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreLocation {
    /// The store's home, `~/.rusty`.
    pub home: PathBuf,
    /// The SQLite database.
    pub db: PathBuf,
    /// The brain vault.
    pub brain: PathBuf,
    /// The notes folder (inside the vault by default).
    pub notes: PathBuf,
    /// The skills store.
    pub skills: PathBuf,
    /// The secrets file.
    pub secret: PathBuf,
    /// The PIN's hash.
    pub pin: PathBuf,
}

impl StoreLocation {
    /// The layout under `home` before any setting moves a part.
    pub fn defaults(home: &Path) -> Self {
        let brain = home.join("brain");
        Self {
            home: home.to_path_buf(),
            db: home.join("rusty.db"),
            notes: brain.join("notes"),
            brain,
            skills: home.join("skills"),
            secret: home.join(".secret"),
            pin: home.join(".pin"),
        }
    }

    /// The user's own home for the store, `~/.rusty`.
    pub fn user_home() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".rusty")
    }

    /// The layout the store's settings point at: `brain_vault_path`, then `notes_path`
    /// (by default `notes` inside the vault), then the skills root `RUSTY_SKILLS` or
    /// `skills_path` names. The one resolver `Core` and the export share.
    pub fn resolve(home: &Path, settings: &SettingsManager) -> Self {
        let mut loc = Self::defaults(home);
        let default_brain = loc.brain.to_string_lossy().to_string();
        loc.brain = PathBuf::from(
            settings
                .get_or_default("brain_vault_path", &default_brain)
                .unwrap_or(default_brain),
        );
        let default_notes = loc.brain.join("notes").to_string_lossy().to_string();
        loc.notes = PathBuf::from(
            settings
                .get_or_default("notes_path", &default_notes)
                .unwrap_or(default_notes),
        );
        if let Some(skills) = crate::skills::configured_root(settings) {
            loc.skills = skills;
        }
        loc
    }

    /// Whether this home already holds a store: a database, or a vault or a skills
    /// store with anything in it.
    pub fn holds_store(&self) -> bool {
        self.db.exists() || has_entries(&self.brain) || has_entries(&self.skills)
    }
}

/// What an export holds, written as `manifest.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Always [`FORMAT`].
    pub format: String,
    /// The format version; an import refuses a newer one.
    pub format_version: u32,
    /// The Rusty version that wrote the export.
    pub rusty_version: String,
    /// When it was written, local time.
    pub created: String,
    /// Where the notes folder sat inside the vault (`notes` by default), or `None` when
    /// it sat outside and travels as `notes/`.
    pub notes_in_vault: Option<String>,
    /// Whether the secrets file and the PIN are inside.
    pub secrets: bool,
    /// Counts for the reader.
    pub counts: Counts,
    /// What the export left out, and why.
    #[serde(default)]
    pub skipped: Vec<String>,
}

/// The numbers a manifest reports.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    /// Markdown files in the vault, its `.git` left out (a notes folder kept outside the
    /// vault is not counted).
    pub pages: u64,
    /// To-do items.
    pub tasks: u64,
    /// Memories.
    pub memories: u64,
    /// Files in the zip, the manifest left out.
    pub files: u64,
}

/// How to export.
#[derive(Debug, Clone, Copy, Default)]
pub struct ExportOptions {
    /// Carry the secrets file and the PIN. They are plain text and a hash; leave them
    /// out of anything that is not kept as carefully as the machine itself.
    pub include_secrets: bool,
}

/// What an export wrote.
#[derive(Debug, Clone)]
pub struct ExportReport {
    /// The zip.
    pub path: PathBuf,
    /// Its manifest.
    pub manifest: Manifest,
    /// Its size in bytes.
    pub bytes: u64,
}

/// Write the store at `loc` (whose database `db` is open) into a new zip at `dest`. The
/// database goes in as a `VACUUM INTO` snapshot, so the export is consistent while a
/// server writes; the zip is written beside `dest` and renamed in place when complete,
/// and it is readable by its owner only.
pub fn export(
    loc: &StoreLocation,
    db: &Database,
    dest: &Path,
    opts: ExportOptions,
) -> Result<ExportReport, String> {
    if dest.exists() {
        return Err(format!(
            "{} exists already; name a new file",
            dest.display()
        ));
    }
    let dest_abs = absolute(dest)?;
    for part in [&loc.brain, &loc.skills, &loc.notes] {
        if let Ok(part) = part.canonicalize() {
            if dest_abs.starts_with(&part) {
                return Err(format!(
                    "{} lies inside the store ({}); write the export somewhere else",
                    dest.display(),
                    part.display()
                ));
            }
        }
    }
    let partial = with_suffix(&dest_abs, ".partial");
    let snapshot = with_suffix(&dest_abs, ".db-snapshot");
    let _ = fs::remove_file(&snapshot);
    {
        let conn = db.conn()?;
        conn.execute("VACUUM INTO ?1", [snapshot.to_string_lossy().as_ref()])
            .map_err(|e| format!("snapshot the database: {e}"))?;
    }
    let written = write_zip(loc, &snapshot, &partial, opts);
    let _ = fs::remove_file(&snapshot);
    let manifest = match written {
        Ok(m) => m,
        Err(e) => {
            let _ = fs::remove_file(&partial);
            return Err(e);
        }
    };
    fs::rename(&partial, &dest_abs).map_err(|e| format!("finish {}: {e}", dest.display()))?;
    let bytes = fs::metadata(&dest_abs).map(|m| m.len()).unwrap_or(0);
    Ok(ExportReport {
        path: dest_abs,
        manifest,
        bytes,
    })
}

fn write_zip(
    loc: &StoreLocation,
    snapshot: &Path,
    partial: &Path,
    opts: ExportOptions,
) -> Result<Manifest, String> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(partial)
        .map_err(|e| format!("create {}: {e}", partial.display()))?;
    let mut zip = ZipWriter::new(file);
    let mut walk = Walk::default();
    add_file(&mut zip, snapshot, DB_ENTRY, 0o600, &mut walk)?;
    let notes_in_vault = loc
        .notes
        .strip_prefix(&loc.brain)
        .ok()
        .map(|rel| rel.to_string_lossy().to_string());
    // Pages are the markdown the brain indexes: the vault's, outside its dot-folders and
    // its archive. Templates, deleted pages and the skills store's SKILL.md are not pages.
    add_tree(&mut zip, &loc.brain, "brain", true, &mut walk)?;
    if notes_in_vault.is_none() && loc.notes.is_dir() {
        add_tree(&mut zip, &loc.notes, "notes", false, &mut walk)?;
    }
    add_tree(&mut zip, &loc.skills, "skills", false, &mut walk)?;
    let mut secrets = false;
    if opts.include_secrets {
        for (src, entry) in [(&loc.secret, SECRET_ENTRY), (&loc.pin, PIN_ENTRY)] {
            if src.is_file() {
                add_file(&mut zip, src, entry, 0o600, &mut walk)?;
                secrets = true;
            }
        }
    }
    let (tasks, memories) = count_rows(snapshot);
    let manifest = Manifest {
        format: FORMAT.to_string(),
        format_version: FORMAT_VERSION,
        rusty_version: env!("CARGO_PKG_VERSION").to_string(),
        created: chrono::Local::now()
            .format("%Y-%m-%dT%H:%M:%S%:z")
            .to_string(),
        notes_in_vault,
        secrets,
        counts: Counts {
            pages: walk.pages,
            tasks,
            memories,
            files: walk.files,
        },
        skipped: walk.skipped,
    };
    let json = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
    zip.start_file(MANIFEST, file_options(0o600, json.len() as u64))
        .map_err(|e| format!("write the manifest: {e}"))?;
    zip.write_all(&json)
        .map_err(|e| format!("write the manifest: {e}"))?;
    zip.finish().map_err(|e| format!("finish the zip: {e}"))?;
    Ok(manifest)
}

/// What a walk of the store has added and left out.
#[derive(Default)]
struct Walk {
    files: u64,
    pages: u64,
    skipped: Vec<String>,
}

fn file_options(mode: u32, len: u64) -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(mode & 0o777)
        .large_file(len >= u64::from(u32::MAX))
}

fn add_file(
    zip: &mut ZipWriter<File>,
    src: &Path,
    entry: &str,
    mode: u32,
    walk: &mut Walk,
) -> Result<(), String> {
    let mut input = File::open(src).map_err(|e| format!("read {}: {e}", src.display()))?;
    let len = input.metadata().map(|m| m.len()).unwrap_or(0);
    zip.start_file(entry, file_options(mode, len))
        .map_err(|e| format!("add {entry}: {e}"))?;
    io::copy(&mut input, zip).map_err(|e| format!("add {entry}: {e}"))?;
    walk.files += 1;
    Ok(())
}

/// Add `root` as `prefix/` and everything under it, in name order. Symbolic links and
/// git's lock files are left out and named in the walk's `skipped`. With `vault`, the
/// markdown the brain would index is counted as pages.
fn add_tree(
    zip: &mut ZipWriter<File>,
    root: &Path,
    prefix: &str,
    vault: bool,
    walk: &mut Walk,
) -> Result<(), String> {
    if !root.is_dir() {
        return Ok(());
    }
    let mode = fs::metadata(root)
        .map(|m| m.permissions().mode())
        .unwrap_or(0o755);
    zip.add_directory(format!("{prefix}/"), dir_options(mode))
        .map_err(|e| format!("add {prefix}/: {e}"))?;
    let pages_rel = vault.then_some("");
    add_dir(zip, root, prefix, pages_rel, false, walk)
}

fn dir_options(mode: u32) -> SimpleFileOptions {
    SimpleFileOptions::default().unix_permissions(mode & 0o777)
}

/// `pages_rel` is this folder's vault-relative path while its markdown counts as pages,
/// and `None` once the walk is outside the vault's pages.
fn add_dir(
    zip: &mut ZipWriter<File>,
    dir: &Path,
    prefix: &str,
    pages_rel: Option<&str>,
    in_git: bool,
    walk: &mut Walk,
) -> Result<(), String> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|e| format!("read {}: {e}", dir.display()))?
        .flatten()
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            walk.skipped
                .push(format!("{} (a name that is not UTF-8)", path.display()));
            continue;
        };
        let entry_name = format!("{prefix}/{name}");
        let meta =
            fs::symlink_metadata(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        if meta.file_type().is_symlink() {
            walk.skipped.push(format!("{entry_name} (a symbolic link)"));
        } else if meta.is_dir() {
            zip.add_directory(
                format!("{entry_name}/"),
                dir_options(meta.permissions().mode()),
            )
            .map_err(|e| format!("add {entry_name}/: {e}"))?;
            let child_rel = pages_rel
                .filter(|rel| !vault::hidden(rel, &name))
                .map(|rel| {
                    if rel.is_empty() {
                        name.clone()
                    } else {
                        format!("{rel}/{name}")
                    }
                });
            add_dir(
                zip,
                &path,
                &entry_name,
                child_rel.as_deref(),
                in_git || name == ".git",
                walk,
            )?;
        } else if in_git && name.ends_with(".lock") {
            walk.skipped.push(format!("{entry_name} (a git lock file)"));
        } else if meta.is_file() {
            if pages_rel.is_some() && name.ends_with(".md") {
                walk.pages += 1;
            }
            add_file(zip, &path, &entry_name, meta.permissions().mode(), walk)?;
        } else {
            walk.skipped
                .push(format!("{entry_name} (not a regular file)"));
        }
    }
    Ok(())
}

/// To-do items and memories in a database file; zeros when it cannot be read.
fn count_rows(db: &Path) -> (u64, u64) {
    let Ok(conn) =
        rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    else {
        return (0, 0);
    };
    let count = |table: &str| -> u64 {
        conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| {
            r.get::<_, i64>(0)
        })
        .map(|n| n.max(0) as u64)
        .unwrap_or(0)
    };
    (count("user_tasks"), count("memories"))
}

/// How to import.
#[derive(Debug, Clone, Copy, Default)]
pub struct ImportOptions {
    /// Move a home that already holds a store aside, whole, and import in its place.
    pub replace: bool,
    /// Read the zip and report the plan; write nothing.
    pub dry_run: bool,
}

/// What an import did, or (with `dry_run`) would do.
#[derive(Debug, Clone)]
pub struct ImportReport {
    /// The zip's manifest.
    pub manifest: Manifest,
    /// The home the store went to.
    pub home: PathBuf,
    /// Whether that home held a store before.
    pub held_store: bool,
    /// Where the old home went, when there was one.
    pub moved_aside: Option<PathBuf>,
    /// Whether the old home's hook scripts were carried over.
    pub hooks_carried: bool,
    /// The settings rewritten for the new layout, as `key: what happened`.
    pub settings: Vec<String>,
    /// Files written (or, in a dry run, in the zip).
    pub files: u64,
}

/// Read and check a zip's manifest without unpacking anything.
pub fn read_manifest(zip_path: &Path) -> Result<Manifest, String> {
    let mut archive = open_archive(zip_path)?;
    manifest_of(&mut archive)
}

fn open_archive(zip_path: &Path) -> Result<ZipArchive<File>, String> {
    let file = File::open(zip_path).map_err(|e| format!("open {}: {e}", zip_path.display()))?;
    ZipArchive::new(file)
        .map_err(|e| format!("{} is not a zip Rusty can read: {e}", zip_path.display()))
}

fn manifest_of(archive: &mut ZipArchive<File>) -> Result<Manifest, String> {
    let mut text = String::new();
    {
        let mut entry = archive
            .by_name(MANIFEST)
            .map_err(|_| "no manifest.json: not a Rusty export".to_string())?;
        entry
            .read_to_string(&mut text)
            .map_err(|e| format!("read manifest.json: {e}"))?;
    }
    let manifest: Manifest =
        serde_json::from_str(&text).map_err(|e| format!("manifest.json does not parse: {e}"))?;
    if manifest.format != FORMAT {
        return Err(format!(
            "manifest.json says format {:?}, not {FORMAT:?}",
            manifest.format
        ));
    }
    if manifest.format_version > FORMAT_VERSION {
        return Err(format!(
            "the export is format version {} and this Rusty reads up to {FORMAT_VERSION}; update Rusty first",
            manifest.format_version
        ));
    }
    Ok(manifest)
}

/// Where an entry of the zip goes under the store `loc`, or why it may not.
fn target_of(name: &str, rel: &Path, is_dir: bool, loc: &StoreLocation) -> Result<PathBuf, String> {
    let mut parts = rel.components();
    let top = match parts.next() {
        Some(Component::Normal(top)) => top.to_string_lossy().to_string(),
        _ => return Err(format!("{name}: not a path an export holds")),
    };
    let rest: PathBuf = parts.collect();
    match top.as_str() {
        "brain" => Ok(loc.brain.join(rest)),
        "skills" => Ok(loc.skills.join(rest)),
        "notes" => Ok(loc.home.join("notes").join(rest)),
        "db" if is_dir && rest.as_os_str().is_empty() => Ok(loc.home.clone()),
        "db" if name == DB_ENTRY => Ok(loc.db.clone()),
        "secrets" if is_dir && rest.as_os_str().is_empty() => Ok(loc.home.clone()),
        "secrets" if name == SECRET_ENTRY => Ok(loc.secret.clone()),
        "secrets" if name == PIN_ENTRY => Ok(loc.pin.clone()),
        _ if ROOTS.contains(&top.as_str()) => Err(format!("{name}: not a file an export holds")),
        _ => Err(format!("{name}: outside the folders an export holds")),
    }
}

/// Unpack the zip at `zip_path` into `home` (`~/.rusty`). Every entry is checked before
/// anything is written; the store is built in a staging folder beside `home` and renamed
/// into place, and a `home` that exists already is renamed to
/// `<home>.before-import-<time>` first, never deleted. A home that holds a store is
/// replaced only with `replace`. Its `hooks/` folder is carried into the new home.
pub fn import(zip_path: &Path, home: &Path, opts: ImportOptions) -> Result<ImportReport, String> {
    let mut archive = open_archive(zip_path)?;
    let manifest = manifest_of(&mut archive)?;
    let target = StoreLocation::defaults(home);
    // Check every entry before writing anything.
    let mut files = 0u64;
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|e| format!("read the zip: {e}"))?;
        let name = entry
            .name()
            .map_err(|e| format!("read the zip: {e}"))?
            .to_string();
        if name == MANIFEST {
            continue;
        }
        if entry.is_symlink() {
            return Err(format!("{name}: a symbolic link; refusing the export"));
        }
        let rel = entry
            .enclosed_name()
            .ok_or_else(|| format!("{name}: escapes the store; refusing the export"))?;
        target_of(&name, &rel, entry.is_dir(), &target)?;
        if !entry.is_dir() {
            files += 1;
        }
    }
    let held_store = target.holds_store();
    let mut report = ImportReport {
        manifest: manifest.clone(),
        home: home.to_path_buf(),
        held_store,
        moved_aside: None,
        hooks_carried: false,
        settings: Vec::new(),
        files,
    };
    if opts.dry_run {
        return Ok(report);
    }
    if held_store && !opts.replace {
        return Err(format!(
            "{} holds a Rusty store already; import --replace moves it aside to {}.before-import-<time> first",
            home.display(),
            home.display()
        ));
    }
    let (parent, base) = match (home.parent(), home.file_name()) {
        (Some(p), Some(b)) => (p.to_path_buf(), b.to_string_lossy().to_string()),
        _ => {
            return Err(format!(
                "{}: not a folder an import can replace",
                home.display()
            ))
        }
    };
    fs::create_dir_all(&parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let staging = parent.join(format!("{base}.importing-{stamp}"));
    let staged = StoreLocation::defaults(&staging);
    let unpacked = unpack(&mut archive, &staged)
        .and_then(|_| rewrite_settings(&staged.db, &manifest, &target));
    let settings = match unpacked {
        Ok(s) => s,
        Err(e) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(e);
        }
    };
    report.settings = settings;
    if home.exists() {
        let aside = parent.join(format!("{base}.before-import-{stamp}"));
        fs::rename(home, &aside).map_err(|e| {
            let _ = fs::remove_dir_all(&staging);
            format!("move {} aside: {e}", home.display())
        })?;
        let hooks = aside.join("hooks");
        if hooks.is_dir() && copy_tree(&hooks, &staging.join("hooks")).is_ok() {
            report.hooks_carried = true;
        }
        report.moved_aside = Some(aside);
    }
    if let Err(e) = fs::rename(&staging, home) {
        if let Some(aside) = &report.moved_aside {
            let _ = fs::rename(aside, home);
        }
        return Err(format!(
            "move the imported store into {}: {e}",
            home.display()
        ));
    }
    Ok(report)
}

fn unpack(archive: &mut ZipArchive<File>, staged: &StoreLocation) -> Result<(), String> {
    create_private_dir(&staged.home)?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("read the zip: {e}"))?;
        let name = entry
            .name()
            .map_err(|e| format!("read the zip: {e}"))?
            .to_string();
        if name == MANIFEST {
            continue;
        }
        let rel = entry
            .enclosed_name()
            .ok_or_else(|| format!("{name}: escapes the store"))?;
        let out = target_of(&name, &rel, entry.is_dir(), staged)?;
        let mode = entry.unix_mode().map(|m| m & 0o777);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|e| format!("create {}: {e}", out.display()))?;
            if let Some(mode) = mode {
                let _ = fs::set_permissions(&out, fs::Permissions::from_mode(mode | 0o700));
            }
            continue;
        }
        if let Some(dir) = out.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
        }
        let secret = name == SECRET_ENTRY || name == PIN_ENTRY || name == DB_ENTRY;
        let mode = if secret { 0o600 } else { mode.unwrap_or(0o644) };
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&out)
            .map_err(|e| format!("write {}: {e}", out.display()))?;
        io::copy(&mut entry, &mut file).map_err(|e| format!("write {}: {e}", out.display()))?;
        let _ = fs::set_permissions(&out, fs::Permissions::from_mode(mode));
    }
    Ok(())
}

/// Point the imported database's path settings at the new layout: the vault and the
/// skills store at their defaults under the new home, the notes folder where it landed.
fn rewrite_settings(
    db: &Path,
    manifest: &Manifest,
    target: &StoreLocation,
) -> Result<Vec<String>, String> {
    if !db.is_file() {
        return Ok(Vec::new());
    }
    let conn =
        rusqlite::Connection::open(db).map_err(|e| format!("open the imported database: {e}"))?;
    let mut changed = Vec::new();
    for key in ["brain_vault_path", "skills_path"] {
        let n = conn
            .execute("DELETE FROM settings WHERE key = ?1", [key])
            .map_err(|e| format!("rewrite {key}: {e}"))?;
        if n > 0 {
            changed.push(format!(
                "{key}: removed (the default under the new home applies)"
            ));
        }
    }
    let notes = match manifest.notes_in_vault.as_deref() {
        Some("notes") => None,
        Some(rel) => Some(target.brain.join(rel)),
        None => Some(target.home.join("notes")),
    };
    match notes {
        None => {
            let n = conn
                .execute("DELETE FROM settings WHERE key = 'notes_path'", [])
                .map_err(|e| format!("rewrite notes_path: {e}"))?;
            if n > 0 {
                changed.push("notes_path: removed (notes in the vault, the default)".to_string());
            }
        }
        Some(path) => {
            let value = path.to_string_lossy().to_string();
            conn.execute(
                "INSERT INTO settings (key, value) VALUES ('notes_path', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [&value],
            )
            .map_err(|e| format!("rewrite notes_path: {e}"))?;
            changed.push(format!("notes_path: {value}"));
        }
    }
    Ok(changed)
}

fn create_private_dir(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("set the mode of {}: {e}", dir.display()))
}

/// Copy a folder of plain files and folders (the hook scripts), keeping file modes.
fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)?.flatten() {
        let path = entry.path();
        let meta = fs::symlink_metadata(&path)?;
        let dest = to.join(entry.file_name());
        if meta.is_dir() {
            copy_tree(&path, &dest)?;
        } else if meta.is_file() {
            fs::copy(&path, &dest)?;
        }
    }
    Ok(())
}

fn has_entries(dir: &Path) -> bool {
    fs::read_dir(dir).is_ok_and(|mut d| d.next().is_some())
}

/// `path` made absolute against the current directory, its parent resolved.
fn absolute(path: &Path) -> Result<PathBuf, String> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| format!("the current directory: {e}"))?
            .join(path)
    };
    let name = path
        .file_name()
        .ok_or_else(|| format!("{}: name a file", path.display()))?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("/"));
    let parent = parent
        .canonicalize()
        .map_err(|e| format!("{}: {e}", parent.display()))?;
    Ok(parent.join(name))
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::memory_manager::MemoryManager;
    use crate::engine::user_tasks::UserTaskManager;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            static N: AtomicUsize = AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!(
                "rusty_transfer_{}_{}_{name}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write(path: &Path, text: &str, mode: u32) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    /// A store under `home/.rusty` with a page, an empty git folder, a git lock file, a
    /// symbolic link, an executable store script, a task, a memory, the secrets and an
    /// absolute `notes_path`, as an older install leaves it.
    fn store(root: &Path) -> (StoreLocation, Arc<Database>) {
        let home = root.join(".rusty");
        let loc = StoreLocation::defaults(&home);
        fs::create_dir_all(&home).unwrap();
        let db = Arc::new(Database::open_path(&loc.db).unwrap());
        let settings = SettingsManager::new(Arc::clone(&db));
        settings
            .set("notes_path", &loc.brain.join("notes").to_string_lossy())
            .unwrap();
        settings
            .set("skills_path", &loc.skills.to_string_lossy())
            .unwrap();
        let tasks = UserTaskManager::new(Arc::clone(&db));
        let list = tasks.create_header("Errands").unwrap();
        tasks.create_task(list, "Buy stamps").unwrap();
        MemoryManager::new(Arc::clone(&db))
            .store("fact", "normal", "Prefers tea", "manual")
            .unwrap();
        write(&loc.brain.join("projects/orbit.md"), "# Orbit\n", 0o644);
        write(&loc.brain.join("notes/idea.md"), "an idea\n", 0o644);
        // Neither a template nor a deleted page is a page; `archive/` deeper down is a folder.
        write(
            &loc.brain.join(".templates/concept.md"),
            "# {{title}}\n",
            0o644,
        );
        write(&loc.brain.join("archive/old.md"), "# Old\n", 0o644);
        write(
            &loc.brain.join("projects/archive/kept.md"),
            "# Kept\n",
            0o644,
        );
        write(
            &loc.brain.join(".git/HEAD"),
            "ref: refs/heads/main\n",
            0o644,
        );
        fs::create_dir_all(loc.brain.join(".git/refs/tags")).unwrap();
        write(&loc.brain.join(".git/index.lock"), "", 0o644);
        std::os::unix::fs::symlink("/etc/hostname", loc.brain.join("link.md")).unwrap();
        write(
            &loc.skills.join(".claude/skills/tidy/SKILL.md"),
            "---\nname: tidy\ndescription: d\n---\n",
            0o644,
        );
        write(
            &loc.skills.join(".claude/skills/tidy/tidy.sh"),
            "echo hi\n",
            0o755,
        );
        write(&loc.secret, "KEY='value'\n", 0o600);
        write(&loc.pin, "$argon2id$hash\n", 0o600);
        write(
            &home.join("hooks/brain-ask-before-write.sh"),
            "exit 0\n",
            0o755,
        );
        (loc, db)
    }

    fn entries(zip: &Path) -> Vec<String> {
        let mut archive = open_archive(zip).unwrap();
        (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().unwrap().to_string())
            .collect()
    }

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn export_then_import_round_trips_the_store() {
        let src = Scratch::new("src");
        let (loc, db) = store(src.path());
        let zip = src.path().join("out.zip");
        let report = export(&loc, &db, &zip, ExportOptions::default()).unwrap();
        assert_eq!(mode(&zip), 0o600);
        let names = entries(&zip);
        assert!(names.contains(&"db/rusty.db".to_string()), "{names:?}");
        assert!(
            names.contains(&"brain/.git/refs/tags/".to_string()),
            "{names:?}"
        );
        assert!(!names.iter().any(|n| n.starts_with("secrets")), "{names:?}");
        assert!(
            !names.iter().any(|n| n.ends_with("index.lock")),
            "{names:?}"
        );
        assert!(!names.iter().any(|n| n.ends_with("link.md")), "{names:?}");
        assert!(!names.iter().any(|n| n.contains("hooks")), "{names:?}");
        let m = &report.manifest;
        assert_eq!(m.notes_in_vault.as_deref(), Some("notes"));
        assert!(!m.secrets);
        assert_eq!(
            (m.counts.pages, m.counts.tasks, m.counts.memories),
            (3, 1, 1)
        );
        assert!(
            names.contains(&"brain/.templates/concept.md".to_string()),
            "{names:?}"
        );
        assert!(
            m.skipped.iter().any(|s| s.contains("a git lock file")),
            "{m:?}"
        );
        assert!(
            m.skipped.iter().any(|s| s.contains("a symbolic link")),
            "{m:?}"
        );

        let dst = Scratch::new("dst");
        let home = dst.path().join(".rusty");
        let done = import(&zip, &home, ImportOptions::default()).unwrap();
        assert!(!done.held_store && done.moved_aside.is_none());
        let new = StoreLocation::defaults(&home);
        assert_eq!(
            fs::read_to_string(new.brain.join("projects/orbit.md")).unwrap(),
            "# Orbit\n"
        );
        assert_eq!(
            fs::read_to_string(new.brain.join("notes/idea.md")).unwrap(),
            "an idea\n"
        );
        assert!(new.brain.join(".git/refs/tags").is_dir());
        assert_eq!(mode(&new.skills.join(".claude/skills/tidy/tidy.sh")), 0o755);
        assert_eq!(mode(&home), 0o700);
        assert!(!new.secret.exists() && !new.pin.exists());
        let db2 = Arc::new(Database::open_path(&new.db).unwrap());
        let settings = SettingsManager::new(Arc::clone(&db2));
        assert_eq!(settings.get("notes_path").unwrap(), None);
        assert_eq!(settings.get("skills_path").unwrap(), None);
        let tasks = UserTaskManager::new(Arc::clone(&db2));
        let lists = tasks.list_headers().unwrap();
        assert_eq!(lists.len(), 1);
        assert_eq!(
            tasks.list_tasks(lists[0].id, false).unwrap()[0].title,
            "Buy stamps"
        );
        let resolved = StoreLocation::resolve(&home, &settings);
        assert_eq!(resolved.brain, new.brain);
        assert_eq!(resolved.notes, new.brain.join("notes"));
    }

    #[test]
    fn secrets_travel_only_when_asked_and_stay_private() {
        let src = Scratch::new("secrets");
        let (loc, db) = store(src.path());
        let zip = src.path().join("with-secrets.zip");
        let report = export(
            &loc,
            &db,
            &zip,
            ExportOptions {
                include_secrets: true,
            },
        )
        .unwrap();
        assert!(report.manifest.secrets);
        let dst = Scratch::new("secrets-dst");
        let home = dst.path().join(".rusty");
        import(&zip, &home, ImportOptions::default()).unwrap();
        let new = StoreLocation::defaults(&home);
        assert_eq!(fs::read_to_string(&new.secret).unwrap(), "KEY='value'\n");
        assert_eq!(mode(&new.secret), 0o600);
        assert_eq!(mode(&new.pin), 0o600);
    }

    #[test]
    fn an_existing_store_is_replaced_only_when_asked_and_moved_aside_whole() {
        let src = Scratch::new("replace-src");
        let (loc, db) = store(src.path());
        let zip = src.path().join("out.zip");
        export(&loc, &db, &zip, ExportOptions::default()).unwrap();
        let dst = Scratch::new("replace-dst");
        let (old, _db) = store(dst.path());
        write(&old.brain.join("mine.md"), "keep me\n", 0o644);
        let err = import(&zip, &old.home, ImportOptions::default()).unwrap_err();
        assert!(err.contains("--replace"), "{err}");
        assert!(
            old.brain.join("mine.md").is_file(),
            "a refusal changed nothing"
        );
        let plan = import(
            &zip,
            &old.home,
            ImportOptions {
                replace: true,
                dry_run: true,
            },
        )
        .unwrap();
        assert!(plan.held_store && plan.moved_aside.is_none());
        assert!(
            old.brain.join("mine.md").is_file(),
            "a dry run wrote nothing"
        );
        let done = import(
            &zip,
            &old.home,
            ImportOptions {
                replace: true,
                dry_run: false,
            },
        )
        .unwrap();
        let aside = done.moved_aside.unwrap();
        assert_eq!(
            fs::read_to_string(aside.join("brain/mine.md")).unwrap(),
            "keep me\n"
        );
        assert!(done.hooks_carried);
        assert!(old.home.join("hooks/brain-ask-before-write.sh").is_file());
        assert!(!old.brain.join("mine.md").exists());
    }

    fn zip_with(dir: &Path, name: &str, manifest: Option<&str>, entry: Option<&str>) -> PathBuf {
        let path = dir.join(name);
        let mut zip = ZipWriter::new(File::create(&path).unwrap());
        if let Some(text) = manifest {
            zip.start_file(MANIFEST, SimpleFileOptions::default())
                .unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        if let Some(entry) = entry {
            zip.start_file(entry, SimpleFileOptions::default()).unwrap();
            zip.write_all(b"x").unwrap();
        }
        zip.finish().unwrap();
        path
    }

    #[test]
    fn unsafe_or_foreign_zips_are_refused_before_anything_is_written() {
        let dir = Scratch::new("unsafe");
        let good = serde_json::to_string(&Manifest {
            format: FORMAT.into(),
            format_version: FORMAT_VERSION,
            rusty_version: "0".into(),
            created: "now".into(),
            notes_in_vault: Some("notes".into()),
            secrets: false,
            counts: Counts::default(),
            skipped: Vec::new(),
        })
        .unwrap();
        let newer = good.replace(
            &format!("\"format_version\":{FORMAT_VERSION}"),
            "\"format_version\":99",
        );
        let cases = [
            (
                "no-manifest.zip",
                None,
                Some("brain/a.md"),
                "no manifest.json",
            ),
            (
                "foreign.zip",
                Some("{\"format\":\"x\"}"),
                None,
                "does not parse",
            ),
            (
                "newer.zip",
                Some(newer.as_str()),
                None,
                "update Rusty first",
            ),
            (
                "escape.zip",
                Some(good.as_str()),
                Some("../../evil.md"),
                "escapes the store",
            ),
            (
                "outside.zip",
                Some(good.as_str()),
                Some("etc/passwd"),
                "outside the folders",
            ),
            (
                "odd-db.zip",
                Some(good.as_str()),
                Some("db/other.db"),
                "not a file an export holds",
            ),
        ];
        for (name, manifest, entry, expect) in cases {
            let zip = zip_with(dir.path(), name, manifest, entry);
            let home = dir.path().join(format!("{name}.home"));
            let err = import(&zip, &home, ImportOptions::default()).unwrap_err();
            assert!(err.contains(expect), "{name}: {err}");
            assert!(!home.exists(), "{name}: wrote {}", home.display());
        }
    }

    #[test]
    fn export_refuses_an_existing_file_or_one_inside_the_store() {
        let src = Scratch::new("dest");
        let (loc, db) = store(src.path());
        let taken = src.path().join("taken.zip");
        fs::write(&taken, "").unwrap();
        let err = export(&loc, &db, &taken, ExportOptions::default()).unwrap_err();
        assert!(err.contains("exists already"), "{err}");
        let inside = loc.brain.join("backup.zip");
        let err = export(&loc, &db, &inside, ExportOptions::default()).unwrap_err();
        assert!(err.contains("inside the store"), "{err}");
        assert!(!inside.exists());
    }

    #[test]
    fn notes_outside_the_vault_travel_as_their_own_folder() {
        let src = Scratch::new("notes");
        let (mut loc, db) = store(src.path());
        loc.notes = src.path().join("elsewhere/notes");
        write(&loc.notes.join("far.md"), "far away\n", 0o644);
        let zip = src.path().join("out.zip");
        let report = export(&loc, &db, &zip, ExportOptions::default()).unwrap();
        assert_eq!(report.manifest.notes_in_vault, None);
        let dst = Scratch::new("notes-dst");
        let home = dst.path().join(".rusty");
        let done = import(&zip, &home, ImportOptions::default()).unwrap();
        assert_eq!(
            fs::read_to_string(home.join("notes/far.md")).unwrap(),
            "far away\n"
        );
        let expected = home.join("notes").to_string_lossy().to_string();
        assert!(
            done.settings.iter().any(|s| s.contains(&expected)),
            "{:?}",
            done.settings
        );
    }
}
