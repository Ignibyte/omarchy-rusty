//! The wired-up manager layer: one [`Core`] holds every manager, the event bus
//! and the paths the file watcher observes. Transport-agnostic by design: the
//! MCP server and the CLI build the same `Core`.

use crate::brain::semantic::{resolve_embedder, Embedder};
use crate::brain::BrainManager;
use crate::engine::db::Database;
use crate::engine::memory_manager::MemoryManager;
use crate::engine::pin_lock::PinLock;
use crate::engine::secrets_manager::SecretsManager;
use crate::engine::settings_manager::SettingsManager;
use crate::engine::task_manager::TaskManager;
use crate::engine::user_tasks::UserTaskManager;
use crate::events::EventBus;
use crate::notes::NotesManager;
use crate::skills::SkillsManager;
use crate::transfer::StoreLocation;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The last resolved embedding provider and when it was resolved.
type EmbedderCache = Mutex<Option<(Instant, Option<Arc<dyn Embedder>>)>>;

/// Every manager Rusty has, ready to use.
pub struct Core {
    /// Broadcasts [`crate::events::AppEvent`]s to whoever is listening.
    pub events: EventBus,
    /// The conversations of earlier versions' agent runs, which `search_conversations`
    /// reads.
    pub task_manager: Arc<TaskManager>,
    /// Long-term memories.
    pub memory_manager: Arc<MemoryManager>,
    /// Markdown notes.
    pub notes_manager: Arc<NotesManager>,
    /// The to-do lists.
    pub user_task_manager: Arc<UserTaskManager>,
    /// Key/value settings.
    pub settings_manager: Arc<SettingsManager>,
    /// The secrets vault.
    pub secrets_manager: Arc<SecretsManager>,
    /// The PIN behind the Secrets tab and its unlock.
    pub pin_lock: Arc<PinLock>,
    /// The brain vault and its index.
    pub brain_manager: Arc<BrainManager>,
    /// The skills store.
    pub skills_manager: Arc<SkillsManager>,
    /// The store every manager shares; the change log is read from it (TICKET-035).
    pub db: Arc<Database>,
    /// Resolved notes path (watched for changes).
    pub notes_path: PathBuf,
    /// Resolved brain vault path (watched for changes).
    pub brain_path: PathBuf,
    /// Resolved skills root (watched for changes).
    pub skills_root: PathBuf,
    /// The embedding provider last resolved from settings, and when.
    embedder_cache: EmbedderCache,
}

impl Core {
    /// Open the database, resolve the configured paths and build every manager.
    /// Panics only when the database or the brain vault cannot be initialized, which
    /// nothing can recover from.
    pub fn init() -> Self {
        let db = Arc::new(Database::open().expect("Failed to open database"));
        let settings_manager = Arc::new(SettingsManager::new(Arc::clone(&db)));

        // One resolver for the store's parts, shared with the export (TICKET-057). Notes
        // live inside the vault since TICKET-014; an explicit `notes_path` still wins.
        let loc = StoreLocation::resolve(&StoreLocation::user_home(), &settings_manager);
        let brain_path = loc.brain.to_string_lossy().to_string();
        let notes_path = loc.notes.to_string_lossy().to_string();
        let secrets_path = loc.secret.clone();

        crate::skills::bootstrap(&settings_manager);
        let skills_root = loc.skills.clone();

        let events = EventBus::new();
        let task_manager = Arc::new(TaskManager::new(Arc::clone(&db)));
        let memory_manager = Arc::new(MemoryManager::new(Arc::clone(&db)));
        let notes_manager = Arc::new(
            NotesManager::with_root(PathBuf::from(&notes_path))
                .expect("Failed to init notes")
                .with_changes(Arc::clone(&db)),
        );
        let user_task_manager = Arc::new(UserTaskManager::new(Arc::clone(&db)));
        let brain_manager = {
            let bm = BrainManager::new(Arc::clone(&db), PathBuf::from(&brain_path));
            bm.ensure_vault().expect("Failed to initialize brain vault");
            Arc::new(bm)
        };
        let skills_manager =
            Arc::new(SkillsManager::new(skills_root.clone()).with_changes(Arc::clone(&db)));
        let secrets_manager =
            Arc::new(SecretsManager::new(secrets_path).with_changes(Arc::clone(&db)));
        let pin_lock = Arc::new(PinLock::new(loc.pin.clone()));

        Core {
            events,
            task_manager,
            memory_manager,
            notes_manager,
            user_task_manager,
            settings_manager,
            secrets_manager,
            pin_lock,
            brain_manager,
            skills_manager,
            db: Arc::clone(&db),
            notes_path: PathBuf::from(&notes_path),
            brain_path: PathBuf::from(&brain_path),
            skills_root,
            embedder_cache: Mutex::new(None),
        }
    }

    /// The embedding provider the settings point at, resolved at most once a minute
    /// (resolving may probe Ollama). `None` means full-text search only.
    pub fn embedder(&self) -> Option<Arc<dyn Embedder>> {
        if let Ok(cache) = self.embedder_cache.lock() {
            if let Some((when, embedder)) = cache.as_ref() {
                if when.elapsed() < Duration::from_secs(60) {
                    return embedder.clone();
                }
            }
        }
        let resolved = resolve_embedder(&self.settings_manager, &self.secrets_manager);
        if let Ok(mut cache) = self.embedder_cache.lock() {
            *cache = Some((Instant::now(), resolved.clone()));
        }
        resolved
    }
}
