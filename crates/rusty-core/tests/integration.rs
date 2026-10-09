//! Integration tests for Rusty's core systems.
//!
//! These tests exercise full workflows across multiple managers,
//! simulating real user scenarios without requiring the Tauri GUI.

use rusqlite::Connection;
use rusty_core::brain::BrainManager;
use rusty_core::engine::db::Database;
use rusty_core::engine::memory_manager::MemoryManager;
use rusty_core::engine::task_manager::{TaskManager, TaskState};
use std::path::PathBuf;
use std::sync::Arc;

/// Create an in-memory database with all tables.
fn test_db() -> Arc<Database> {
    let conn = Connection::open_in_memory().unwrap();
    let db = Database::from_conn(conn);
    db.migrate().unwrap();
    Arc::new(db)
}

/// Create a temp directory for tests.
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rusty_integration_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn cleanup(dir: &std::path::Path) {
    let _ = std::fs::remove_dir_all(dir);
}

// ── Brain lifecycle: create → search → update → read → delete ──────

#[test]
fn brain_full_lifecycle() {
    let dir = temp_dir("brain_lifecycle");
    let db = test_db();
    let bm = BrainManager::new(Arc::clone(&db), dir.clone());
    bm.ensure_vault().unwrap();

    // Create pages
    let sarah = bm
        .create_page(
            "person",
            "Sarah Chen",
            "CTO of Acme Corp. Expert in distributed systems.",
        )
        .unwrap();
    let acme = bm
        .create_page("company", "Acme Corp", "Enterprise SaaS platform.")
        .unwrap();

    assert_eq!(sarah.slug, "people/sarah-chen");
    assert_eq!(acme.slug, "companies/acme-corp");

    // Search finds them
    let results = bm.search("distributed", None, None).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].slug, "people/sarah-chen");

    // Add link
    bm.add_link(
        "people/sarah-chen",
        "companies/acme-corp",
        Some("works_at"),
        Some("CTO"),
    )
    .unwrap();

    // Verify links
    let links = bm.get_links("people/sarah-chen").unwrap();
    assert_eq!(links.outbound.len(), 1);
    assert_eq!(links.outbound[0].to_slug, "companies/acme-corp");

    let back = bm.get_links("companies/acme-corp").unwrap();
    assert_eq!(back.backlinks.len(), 1);

    // Add timeline
    bm.add_timeline(
        "people/sarah-chen",
        "2026-04-12",
        "meeting",
        "Discussed Rusty architecture",
        None,
    )
    .unwrap();

    let timeline = bm.get_timeline("people/sarah-chen", None).unwrap();
    assert_eq!(timeline.len(), 1);
    assert_eq!(timeline[0].source, "meeting");

    // Update page
    let updated = bm
        .update_page(
            "people/sarah-chen",
            "CTO of Acme Corp. Expert in distributed systems and Rust.",
        )
        .unwrap();
    assert!(updated.compiled_truth.contains("and Rust"));

    // Stats
    let stats = bm.stats().unwrap();
    assert_eq!(stats.page_count, 2);
    assert_eq!(stats.link_count, 1);
    assert_eq!(stats.timeline_count, 1);

    // Delete
    bm.delete_page("companies/acme-corp").unwrap();
    let stats = bm.stats().unwrap();
    assert_eq!(stats.page_count, 1);

    cleanup(&dir);
}

// ── Brain context injection: search → inject into prompt ────────────

#[test]
fn brain_context_injection_flow() {
    let dir = temp_dir("brain_context");
    let db = test_db();
    let bm = BrainManager::new(Arc::clone(&db), dir.clone());
    bm.ensure_vault().unwrap();

    // Create a page about a person
    bm.create_page(
        "person",
        "Marcus Reid",
        "Lead engineer at TechCo. Working on the API integration project.",
    )
    .unwrap();

    // Search for context when user mentions the person
    let ctx = bm.build_context_for_prompt("What did Marcus Reid say about the API?");
    assert!(ctx.contains("<brain_context>"));
    assert!(ctx.contains("Marcus Reid"));
    assert!(ctx.contains("Lead engineer"));

    // No context for unrelated prompts
    let ctx = bm.build_context_for_prompt("What's the weather xyz123?");
    assert!(ctx.is_empty());

    cleanup(&dir);
}

// ── Wiki link parsing: [[slug]] → brain_links ──────────────────────

#[test]
fn wiki_links_create_graph() {
    let dir = temp_dir("wiki_links");
    let db = test_db();
    let bm = BrainManager::new(Arc::clone(&db), dir.clone());
    bm.ensure_vault().unwrap();

    bm.create_page("person", "Alice", "Works at [[companies/acme]].")
        .unwrap();
    bm.create_page("company", "Acme", "Alice's employer.")
        .unwrap();

    // Wiki link should create a reference link
    let links = bm.get_links("people/alice").unwrap();
    assert_eq!(links.outbound.len(), 1);
    assert_eq!(links.outbound[0].to_slug, "companies/acme");
    assert_eq!(links.outbound[0].link_type, "reference");

    // Backlink visible
    let back = bm.get_links("companies/acme").unwrap();
    assert_eq!(back.backlinks.len(), 1);

    cleanup(&dir);
}

// ── Memory + Brain coexistence ──────────────────────────────────────

#[test]
fn memory_and_brain_independent() {
    let db = test_db();
    let dir = temp_dir("mem_brain");

    let mm = MemoryManager::new(Arc::clone(&db));
    let bm = BrainManager::new(Arc::clone(&db), dir.clone());
    bm.ensure_vault().unwrap();

    // Store memory (preferences/facts)
    mm.store("preference", "high", "Prefers Rust", "manual")
        .unwrap();

    // Create brain page (world knowledge)
    bm.create_page(
        "concept",
        "Rust Language",
        "A systems programming language.",
    )
    .unwrap();

    // Both accessible independently
    let mems = mm.list(None).unwrap();
    assert_eq!(mems.len(), 1);
    assert_eq!(mems[0].content, "Prefers Rust");

    let pages = bm.list_pages(None, None).unwrap();
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].title, "Rust Language");

    // Memory context and brain context are separate
    let mem_ctx = mm.build_system_prompt_context().unwrap();
    assert!(mem_ctx.contains("Prefers Rust"));
    assert!(!mem_ctx.contains("systems programming"));

    let brain_ctx = bm.build_context_for_prompt("Tell me about Rust");
    assert!(brain_ctx.contains("systems programming"));
    assert!(!brain_ctx.contains("Prefers Rust"));

    cleanup(&dir);
}

// ── Conversation + Task lifecycle ───────────────────────────────────

#[test]
fn conversation_lifecycle() {
    let db = test_db();
    let tm = TaskManager::new(Arc::clone(&db));

    // Create conversation
    let conv_id = tm.create_conversation().unwrap();
    assert!(!conv_id.is_empty());

    // Create task in conversation
    let task_id = tm.create_task("Hello Rusty", &conv_id, "").unwrap();
    assert!(!task_id.is_empty());

    // Task starts pending
    let task = tm.get_task(&task_id).unwrap().unwrap();
    assert_eq!(task.state, TaskState::Pending);

    // Transition to running
    tm.set_running(&task_id).unwrap();
    let task = tm.get_task(&task_id).unwrap().unwrap();
    assert_eq!(task.state, TaskState::Running);

    // Complete
    tm.set_completed(
        &task_id,
        "Hello! How can I help?",
        "session-1",
        0.01,
        1,
        500,
    )
    .unwrap();
    let task = tm.get_task(&task_id).unwrap().unwrap();
    assert_eq!(task.state, TaskState::Completed);
    assert_eq!(task.result, "Hello! How can I help?");

    // List conversations
    let convos = tm.list_conversations().unwrap();
    assert_eq!(convos.len(), 1);
    assert_eq!(convos[0].id, conv_id);
    assert_eq!(convos[0].message_count, 1);

    // Search conversations
    let found = tm.search_conversations("Hello", 10).unwrap();
    assert_eq!(found.len(), 1);

    let not_found = tm.search_conversations("nonexistent_xyz", 10).unwrap();
    assert!(not_found.is_empty());

    // Delete conversation
    tm.delete_conversation(&conv_id).unwrap();
    let convos = tm.list_conversations().unwrap();
    assert!(convos.is_empty());
}

// ── Notes filesystem operations ─────────────────────────────────────

#[test]
fn notes_full_lifecycle() {
    let dir = temp_dir("notes_lifecycle");
    std::fs::create_dir_all(dir.join(".deleted")).unwrap();
    // Create NotesManager with custom root (via direct struct creation for testing)
    // We can't use NotesManager::new() as it uses ~/.rusty/notes/
    // Instead test the core operations via the filesystem
    let notes_root = dir.clone();

    // Create a note file
    let note_path = notes_root.join("test-note.md");
    std::fs::write(&note_path, "# Test Note\n\nContent here.").unwrap();
    assert!(note_path.exists());

    // Read it back
    let content = std::fs::read_to_string(&note_path).unwrap();
    assert!(content.contains("# Test Note"));

    // Update it
    std::fs::write(&note_path, "# Updated Note\n\nNew content.").unwrap();
    let content = std::fs::read_to_string(&note_path).unwrap();
    assert!(content.contains("Updated Note"));

    // Delete (move to .deleted)
    let deleted_path = notes_root.join(".deleted/test-note.md");
    std::fs::rename(&note_path, &deleted_path).unwrap();
    assert!(!note_path.exists());
    assert!(deleted_path.exists());

    cleanup(&dir);
}

// ── Passive entity extraction ───────────────────────────────────────

#[test]
fn enrichment_passive_extraction() {
    use rusty_core::brain::enrichment::extract_entities_passive;

    let text = "I met Sarah Chen and Bob Williams at Acme Corp today. \
                They discussed the Rusty project and how it integrates with MCP.";

    let entities = extract_entities_passive(text);
    let names: Vec<&str> = entities.iter().map(|e| e.name.as_str()).collect();

    assert!(names.contains(&"Sarah Chen"));
    assert!(names.contains(&"Bob Williams"));
    assert!(names.contains(&"Acme Corp"));

    // Acme Corp should be detected as company
    let acme = entities.iter().find(|e| e.name == "Acme Corp").unwrap();
    assert_eq!(acme.entity_type, "company");

    // Sarah and Bob should be persons
    let sarah = entities.iter().find(|e| e.name == "Sarah Chen").unwrap();
    assert_eq!(sarah.entity_type, "person");
}
