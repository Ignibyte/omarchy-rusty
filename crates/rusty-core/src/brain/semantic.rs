//! Semantic search over the brain: an embedding per page chunk, stored in `sqlite-vec`
//! inside `rusty.db`, merged with the full-text hits at query time.
//!
//! Vectors need an embedding provider. Ollama is used when it is running locally
//! (`embedding_provider = auto`, the default) or asked for; OpenAI is used only when the
//! setting says `openai` and the secrets vault holds `openai_api_key`, because that sends
//! page text off the machine. With no provider there are no vectors and search stays
//! full-text. The provider and model are stored with the vectors, so changing either
//! rebuilds the index.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::engine::db::Database;
use crate::engine::secrets_manager::SecretsManager;
use crate::engine::settings_manager::SettingsManager;

/// Setting: `auto` (Ollama when it answers), `ollama`, `openai`, or `off`.
pub const SETTING_PROVIDER: &str = "embedding_provider";
/// Setting: the model name; empty means the provider's default.
pub const SETTING_MODEL: &str = "embedding_model";
/// Setting: where Ollama listens.
pub const SETTING_OLLAMA_URL: &str = "ollama_url";
/// Secret: the OpenAI key, read from the vault and never returned by any tool. The
/// upper-case spelling many vaults already hold is accepted too.
pub const SECRET_OPENAI_KEY: &str = "openai_api_key";
/// The other spelling of [`SECRET_OPENAI_KEY`].
pub const SECRET_OPENAI_KEY_UPPER: &str = "OPENAI_API_KEY";
/// Ollama's default address.
pub const DEFAULT_OLLAMA_URL: &str = "http://127.0.0.1:11434";
/// Ollama's default embedding model.
pub const DEFAULT_OLLAMA_MODEL: &str = "nomic-embed-text";
/// OpenAI's default embedding model.
pub const DEFAULT_OPENAI_MODEL: &str = "text-embedding-3-small";

const CHUNK_CHARS: usize = 900;
const CHUNK_OVERLAP: usize = 120;
const BATCH: usize = 16;
const RRF_K: f64 = 60.0;

/// Something that turns text into vectors.
pub trait Embedder: Send + Sync {
    /// `provider:model`. Stored with the vectors, so a change rebuilds them.
    fn id(&self) -> String;
    /// One vector per input, all the same length.
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String>;
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .build()
        .into()
}

/// Ollama's `/api/embed`.
pub struct OllamaEmbedder {
    url: String,
    model: String,
    agent: ureq::Agent,
}

#[derive(Deserialize)]
struct OllamaEmbedResponse {
    embeddings: Vec<Vec<f32>>,
}

impl OllamaEmbedder {
    /// Bind to an Ollama address and model.
    pub fn new(url: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            url: url.into().trim_end_matches('/').to_string(),
            model: model.into(),
            agent: agent(Duration::from_secs(120)),
        }
    }

    /// Whether an Ollama server answers at `url` (one quick request).
    pub fn is_up(url: &str) -> bool {
        let url = format!("{}/api/tags", url.trim_end_matches('/'));
        agent(Duration::from_millis(800)).get(&url).call().is_ok()
    }
}

impl Embedder for OllamaEmbedder {
    fn id(&self) -> String {
        format!("ollama:{}", self.model)
    }

    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let url = format!("{}/api/embed", self.url);
        let body = serde_json::json!({ "model": self.model, "input": texts });
        let parsed: OllamaEmbedResponse = self
            .agent
            .post(&url)
            .send_json(&body)
            .map_err(|e| format!("ollama embed: {e}"))?
            .body_mut()
            .read_json()
            .map_err(|e| format!("ollama embed: bad reply: {e}"))?;
        check_count(parsed.embeddings, texts.len())
    }
}

/// OpenAI's `/v1/embeddings`.
pub struct OpenAiEmbedder {
    key: String,
    model: String,
    agent: ureq::Agent,
}

#[derive(Deserialize)]
struct OpenAiEmbedResponse {
    data: Vec<OpenAiEmbedding>,
}

#[derive(Deserialize)]
struct OpenAiEmbedding {
    embedding: Vec<f32>,
    index: usize,
}

impl OpenAiEmbedder {
    /// Bind to a key and model.
    pub fn new(key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            model: model.into(),
            agent: agent(Duration::from_secs(120)),
        }
    }
}

impl Embedder for OpenAiEmbedder {
    fn id(&self) -> String {
        format!("openai:{}", self.model)
    }

    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let body = serde_json::json!({ "model": self.model, "input": texts });
        let parsed: OpenAiEmbedResponse = self
            .agent
            .post("https://api.openai.com/v1/embeddings")
            .header("Authorization", &format!("Bearer {}", self.key))
            .send_json(&body)
            .map_err(|e| format!("openai embeddings: {e}"))?
            .body_mut()
            .read_json()
            .map_err(|e| format!("openai embeddings: bad reply: {e}"))?;
        let mut data = parsed.data;
        data.sort_by_key(|d| d.index);
        check_count(data.into_iter().map(|d| d.embedding).collect(), texts.len())
    }
}

fn check_count(vectors: Vec<Vec<f32>>, wanted: usize) -> Result<Vec<Vec<f32>>, String> {
    if vectors.len() != wanted {
        return Err(format!(
            "embedding provider returned {} vectors for {wanted} texts",
            vectors.len()
        ));
    }
    if let Some(first) = vectors.first() {
        if first.is_empty() || vectors.iter().any(|v| v.len() != first.len()) {
            return Err("embedding provider returned vectors of uneven length".to_string());
        }
    }
    Ok(vectors)
}

/// The provider the settings and secrets point at, or `None` for none.
pub fn resolve_embedder(
    settings: &SettingsManager,
    secrets: &SecretsManager,
) -> Option<Arc<dyn Embedder>> {
    let provider = settings
        .get_or_default(SETTING_PROVIDER, "auto")
        .unwrap_or_else(|_| "auto".to_string());
    let model = settings
        .get(SETTING_MODEL)
        .ok()
        .flatten()
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty());
    let ollama_url = settings
        .get_or_default(SETTING_OLLAMA_URL, DEFAULT_OLLAMA_URL)
        .unwrap_or_else(|_| DEFAULT_OLLAMA_URL.to_string());
    let ollama = |model: Option<String>| -> Arc<dyn Embedder> {
        Arc::new(OllamaEmbedder::new(
            ollama_url.clone(),
            model.unwrap_or_else(|| DEFAULT_OLLAMA_MODEL.to_string()),
        ))
    };
    match provider.trim().to_ascii_lowercase().as_str() {
        "off" | "none" | "false" => None,
        "ollama" => Some(ollama(model)),
        "openai" => secrets
            .get(SECRET_OPENAI_KEY)
            .or_else(|| secrets.get(SECRET_OPENAI_KEY_UPPER))
            .map(|key| {
                Arc::new(OpenAiEmbedder::new(
                    key,
                    model.unwrap_or_else(|| DEFAULT_OPENAI_MODEL.to_string()),
                )) as Arc<dyn Embedder>
            }),
        _ => OllamaEmbedder::is_up(&ollama_url).then(|| ollama(model)),
    }
}

/// Split page text into chunks of roughly `CHUNK_CHARS` characters on paragraph
/// boundaries; a paragraph longer than that is cut into overlapping windows.
pub fn chunk_text(text: &str) -> Vec<String> {
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();
    let flush = |current: &mut String, chunks: &mut Vec<String>| {
        let t = current.trim();
        if !t.is_empty() {
            chunks.push(t.to_string());
        }
        current.clear();
    };
    for paragraph in text.split("\n\n").map(str::trim).filter(|p| !p.is_empty()) {
        if paragraph.chars().count() > CHUNK_CHARS {
            flush(&mut current, &mut chunks);
            let chars: Vec<char> = paragraph.chars().collect();
            let mut start = 0;
            while start < chars.len() {
                let end = (start + CHUNK_CHARS).min(chars.len());
                chunks.push(
                    chars[start..end]
                        .iter()
                        .collect::<String>()
                        .trim()
                        .to_string(),
                );
                if end == chars.len() {
                    break;
                }
                start = end.saturating_sub(CHUNK_OVERLAP);
            }
            continue;
        }
        if current.chars().count() + paragraph.chars().count() + 2 > CHUNK_CHARS {
            flush(&mut current, &mut chunks);
        }
        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(paragraph);
    }
    flush(&mut current, &mut chunks);
    chunks
}

/// Reciprocal rank fusion of two ranked slug lists; best first.
pub fn fuse(fts: &[String], vectors: &[String]) -> Vec<(String, f64)> {
    let mut scores: HashMap<&str, f64> = HashMap::new();
    for list in [fts, vectors] {
        for (rank, slug) in list.iter().enumerate() {
            *scores.entry(slug.as_str()).or_insert(0.0) += 1.0 / (RRF_K + rank as f64 + 1.0);
        }
    }
    let mut out: Vec<(String, f64)> = scores
        .into_iter()
        .map(|(s, v)| (s.to_string(), v))
        .collect();
    out.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    out
}

/// What an indexing pass did.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexReport {
    /// The provider and model the vectors came from.
    pub model: String,
    /// Pages embedded in this pass.
    pub pages_indexed: usize,
    /// Chunks written in this pass.
    pub chunks_written: usize,
    /// Pages whose chunks were dropped because the page is gone.
    pub pages_removed: usize,
    /// Pages that could not be embedded, with the reason.
    pub pages_failed: Vec<String>,
}

/// The state of the vector index.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SemanticStats {
    /// The model the stored vectors came from, when any exist.
    pub model: Option<String>,
    /// Vector length.
    pub dims: usize,
    /// Pages with vectors.
    pub pages: usize,
    /// Chunks with vectors.
    pub chunks: usize,
}

/// One nearest chunk.
#[derive(Debug, Clone)]
pub struct VecHit {
    /// The page the chunk belongs to.
    pub slug: String,
    /// The chunk text.
    pub text: String,
    /// Smaller is closer.
    pub distance: f64,
}

/// The chunk and vector tables in `rusty.db`.
pub struct SemanticIndex {
    db: Arc<Database>,
}

fn f32_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

impl SemanticIndex {
    /// Over the shared database.
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    fn meta(conn: &rusqlite::Connection) -> Option<(String, usize)> {
        conn.query_row(
            "SELECT model, dims FROM brain_vec_meta WHERE id = 1",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize)),
        )
        .ok()
    }

    /// Make sure the vector table matches the model and dimension; a change drops
    /// every stored vector and chunk, because they would not be comparable.
    fn ensure_vec_table(
        conn: &rusqlite::Connection,
        model: &str,
        dims: usize,
    ) -> Result<(), String> {
        match Self::meta(conn) {
            Some((m, d)) if m == model && d == dims => return Ok(()),
            Some(_) => {
                conn.execute_batch("DROP TABLE IF EXISTS brain_vec; DELETE FROM brain_chunks;")
                    .map_err(|e| format!("Failed to reset vector table: {e}"))?;
            }
            None => {}
        }
        conn.execute_batch(&format!(
            "CREATE VIRTUAL TABLE IF NOT EXISTS brain_vec USING vec0(chunk_id INTEGER PRIMARY KEY, embedding float[{dims}]);"
        ))
        .map_err(|e| format!("Failed to create vector table: {e}"))?;
        conn.execute(
            "INSERT OR REPLACE INTO brain_vec_meta (id, model, dims) VALUES (1, ?1, ?2)",
            rusqlite::params![model, dims as i64],
        )
        .map_err(|e| format!("Failed to record vector model: {e}"))?;
        Ok(())
    }

    /// Pages whose vectors are missing, older than the page, or from another model, and
    /// pages that still have chunks but no longer exist.
    pub fn stale_slugs(&self, model: &str) -> Result<(Vec<String>, Vec<String>), String> {
        let conn = self.db.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT p.slug FROM brain_pages p \
                 LEFT JOIN (SELECT slug, MIN(content_hash) AS h, MIN(model) AS m FROM brain_chunks GROUP BY slug) c \
                 ON c.slug = p.slug \
                 WHERE c.slug IS NULL OR c.h IS NOT p.content_hash OR c.m != ?1 \
                 ORDER BY p.slug",
            )
            .map_err(|e| format!("Query error: {e}"))?;
        let stale = stmt
            .query_map(rusqlite::params![model], |row| row.get(0))
            .map_err(|e| format!("Query error: {e}"))?
            .flatten()
            .collect();
        let mut stmt = conn
            .prepare("SELECT DISTINCT slug FROM brain_chunks WHERE slug NOT IN (SELECT slug FROM brain_pages)")
            .map_err(|e| format!("Query error: {e}"))?;
        let orphaned = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| format!("Query error: {e}"))?
            .flatten()
            .collect();
        Ok((stale, orphaned))
    }

    fn remove_with(conn: &rusqlite::Connection, slug: &str) -> Result<(), String> {
        if Self::meta(conn).is_some() {
            conn.execute(
                "DELETE FROM brain_vec WHERE chunk_id IN (SELECT id FROM brain_chunks WHERE slug = ?1)",
                rusqlite::params![slug],
            )
            .map_err(|e| format!("Failed to drop vectors: {e}"))?;
        }
        conn.execute(
            "DELETE FROM brain_chunks WHERE slug = ?1",
            rusqlite::params![slug],
        )
        .map_err(|e| format!("Failed to drop chunks: {e}"))?;
        Ok(())
    }

    /// Drop a page's chunks and vectors.
    pub fn remove(&self, slug: &str) -> Result<(), String> {
        let conn = self.db.conn()?;
        Self::remove_with(&conn, slug)
    }

    /// Embed a page's text and store it, replacing whatever the page had. `hash` is the
    /// page's content hash, kept so a later pass can tell the vectors are current.
    /// Returns the number of chunks written.
    pub fn index_page(
        &self,
        embedder: &dyn Embedder,
        slug: &str,
        text: &str,
        hash: &str,
    ) -> Result<usize, String> {
        let chunks = chunk_text(text);
        if chunks.is_empty() {
            self.remove(slug)?;
            return Ok(0);
        }
        let mut vectors: Vec<Vec<f32>> = Vec::with_capacity(chunks.len());
        for batch in chunks.chunks(BATCH) {
            vectors.extend(embedder.embed(batch)?);
        }
        let dims = vectors.first().map(|v| v.len()).unwrap_or(0);
        if dims == 0 {
            return Err("embedding provider returned empty vectors".to_string());
        }
        let model = embedder.id();
        let conn = self.db.conn()?;
        Self::ensure_vec_table(&conn, &model, dims)?;
        Self::remove_with(&conn, slug)?;
        for (i, (chunk, vector)) in chunks.iter().zip(vectors.iter()).enumerate() {
            conn.execute(
                "INSERT INTO brain_chunks (slug, chunk_index, text, content_hash, model) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![slug, i as i64, chunk, hash, model],
            )
            .map_err(|e| format!("Failed to store chunk: {e}"))?;
            let id = conn.last_insert_rowid();
            conn.execute(
                "INSERT INTO brain_vec (chunk_id, embedding) VALUES (?1, ?2)",
                rusqlite::params![id, f32_blob(vector)],
            )
            .map_err(|e| format!("Failed to store vector: {e}"))?;
        }
        Ok(chunks.len())
    }

    /// The `k` nearest chunks to a query, closest first. Empty when nothing is indexed.
    pub fn search(
        &self,
        embedder: &dyn Embedder,
        query: &str,
        k: usize,
    ) -> Result<Vec<VecHit>, String> {
        let query = query.trim();
        if query.is_empty() || k == 0 {
            return Ok(Vec::new());
        }
        {
            let conn = self.db.conn()?;
            match Self::meta(&conn) {
                Some((model, _)) if model == embedder.id() => {}
                _ => return Ok(Vec::new()),
            }
        }
        let vector = embedder
            .embed(&[query.to_string()])?
            .into_iter()
            .next()
            .ok_or_else(|| "embedding provider returned nothing for the query".to_string())?;
        let conn = self.db.conn()?;
        let mut stmt = conn
            .prepare("SELECT chunk_id, distance FROM brain_vec WHERE embedding MATCH ?1 AND k = ?2 ORDER BY distance")
            .map_err(|e| format!("Vector query error: {e}"))?;
        let nearest: Vec<(i64, f64)> = stmt
            .query_map(rusqlite::params![f32_blob(&vector), k as i64], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .map_err(|e| format!("Vector query error: {e}"))?
            .flatten()
            .collect();
        let mut hits = Vec::with_capacity(nearest.len());
        for (id, distance) in nearest {
            let row: Option<(String, String)> = conn
                .query_row(
                    "SELECT slug, text FROM brain_chunks WHERE id = ?1",
                    rusqlite::params![id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .ok();
            if let Some((slug, text)) = row {
                hits.push(VecHit {
                    slug,
                    text,
                    distance,
                });
            }
        }
        Ok(hits)
    }

    /// How much is indexed, and with what.
    pub fn stats(&self) -> Result<SemanticStats, String> {
        let conn = self.db.conn()?;
        let (model, dims) = match Self::meta(&conn) {
            Some((m, d)) => (Some(m), d),
            None => (None, 0),
        };
        let (pages, chunks): (i64, i64) = conn
            .query_row(
                "SELECT COUNT(DISTINCT slug), COUNT(*) FROM brain_chunks",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| format!("Query error: {e}"))?;
        Ok(SemanticStats {
            model,
            dims,
            pages: pages as usize,
            chunks: chunks as usize,
        })
    }

    /// Forget every vector and chunk.
    pub fn clear(&self) -> Result<(), String> {
        let conn = self.db.conn()?;
        conn.execute_batch(
            "DROP TABLE IF EXISTS brain_vec; DELETE FROM brain_chunks; DELETE FROM brain_vec_meta;",
        )
        .map_err(|e| format!("Failed to clear the vector index: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::io::{Read, Write};

    /// A deterministic bag-of-words embedder: enough to prove storage and ranking.
    struct HashEmbedder;

    impl Embedder for HashEmbedder {
        fn id(&self) -> String {
            "test:hash".into()
        }
        fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
            Ok(texts
                .iter()
                .map(|t| {
                    let mut v = [0.0f32; 32];
                    for word in t
                        .split(|c: char| !c.is_alphanumeric())
                        .filter(|w| !w.is_empty())
                    {
                        let mut h: u32 = 2166136261;
                        for b in word.to_lowercase().bytes() {
                            h = (h ^ b as u32).wrapping_mul(16777619);
                        }
                        v[(h % 32) as usize] += 1.0;
                    }
                    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
                    v.iter().map(|x| x / norm).collect()
                })
                .collect())
        }
    }

    fn test_db() -> Arc<Database> {
        Database::register_extensions();
        let conn = Connection::open_in_memory().unwrap();
        let db = Database::from_conn(conn);
        db.migrate().unwrap();
        Arc::new(db)
    }

    fn add_page(db: &Database, slug: &str, hash: &str) {
        let conn = db.conn().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO brain_pages (slug, page_type, title, frontmatter, content_hash, created_at, updated_at) VALUES (?1, 'concept', ?1, '{}', ?2, 1, 1)",
            rusqlite::params![slug, hash],
        )
        .unwrap();
    }

    #[test]
    fn chunks_follow_paragraphs_and_cut_long_ones() {
        assert!(chunk_text("   \n\n  ").is_empty());
        let short = chunk_text("one\n\ntwo\n\nthree");
        assert_eq!(short, vec!["one\n\ntwo\n\nthree"]);
        let long_paragraph = "word ".repeat(400);
        let chunks = chunk_text(&long_paragraph);
        assert!(chunks.len() >= 2, "{}", chunks.len());
        assert!(chunks.iter().all(|c| c.chars().count() <= CHUNK_CHARS));
        let many: String = (0..30)
            .map(|i| format!("paragraph {i} {}", "x".repeat(100)))
            .collect::<Vec<_>>()
            .join("\n\n");
        let grouped = chunk_text(&many);
        assert!(grouped.len() > 1 && grouped.len() < 30);
    }

    #[test]
    fn fusion_rewards_agreement() {
        let fts = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let vec = vec!["b".to_string(), "d".to_string()];
        let fused = fuse(&fts, &vec);
        assert_eq!(fused[0].0, "b");
        assert!(fused.iter().any(|(s, _)| s == "d"));
        assert_eq!(fused.len(), 4);
    }

    #[test]
    fn index_search_and_rebuild_on_model_change() {
        let db = test_db();
        add_page(&db, "fruit/apples", "h1");
        add_page(&db, "tools/hammers", "h2");
        let index = SemanticIndex::new(Arc::clone(&db));
        let embedder = HashEmbedder;

        let n = index
            .index_page(
                &embedder,
                "fruit/apples",
                "Apples are sweet fruit.\n\nOrchards grow apples.",
                "h1",
            )
            .unwrap();
        assert_eq!(n, 1);
        index
            .index_page(
                &embedder,
                "tools/hammers",
                "Hammers drive nails into wood.",
                "h2",
            )
            .unwrap();

        let hits = index.search(&embedder, "sweet apples", 5).unwrap();
        assert_eq!(hits[0].slug, "fruit/apples", "{hits:?}");
        let stats = index.stats().unwrap();
        assert_eq!(stats.pages, 2);
        assert_eq!(stats.model.as_deref(), Some("test:hash"));
        assert_eq!(stats.dims, 32);

        let (stale, orphaned) = index.stale_slugs("test:hash").unwrap();
        assert!(
            stale.is_empty() && orphaned.is_empty(),
            "{stale:?} {orphaned:?}"
        );
        add_page(&db, "fruit/apples", "h1-changed");
        let (stale, _) = index.stale_slugs("test:hash").unwrap();
        assert_eq!(stale, vec!["fruit/apples"]);
        let (stale, _) = index.stale_slugs("other:model").unwrap();
        assert_eq!(stale.len(), 2);

        // A page that disappears leaves orphaned chunks, which remove() clears.
        db.conn()
            .unwrap()
            .execute("DELETE FROM brain_pages WHERE slug = 'tools/hammers'", [])
            .unwrap();
        let (_, orphaned) = index.stale_slugs("test:hash").unwrap();
        assert_eq!(orphaned, vec!["tools/hammers"]);
        index.remove("tools/hammers").unwrap();
        assert_eq!(index.stats().unwrap().pages, 1);

        // Another model means a rebuild: the old chunks go.
        struct Other;
        impl Embedder for Other {
            fn id(&self) -> String {
                "test:other".into()
            }
            fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
                Ok(texts.iter().map(|_| vec![1.0, 0.0, 0.0]).collect())
            }
        }
        index
            .index_page(&Other, "fruit/apples", "Apples.", "h1-changed")
            .unwrap();
        let stats = index.stats().unwrap();
        assert_eq!(stats.model.as_deref(), Some("test:other"));
        assert_eq!(stats.dims, 3);
        assert_eq!(stats.chunks, 1);
        assert!(
            index.search(&embedder, "apples", 3).unwrap().is_empty(),
            "old model gets nothing"
        );
        index.clear().unwrap();
        assert_eq!(index.stats().unwrap().chunks, 0);
    }

    /// A one-shot HTTP server that answers any request with the given JSON.
    fn mock_server(body: &'static str) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().take(1) {
                let mut stream = stream.unwrap();
                let mut buf = [0u8; 8192];
                let _ = stream.read(&mut buf);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        format!("http://{addr}")
    }

    #[test]
    fn ollama_replies_parse_and_absence_is_detected() {
        let url = mock_server(r#"{"embeddings":[[0.1,0.2,0.3],[0.4,0.5,0.6]]}"#);
        let embedder = OllamaEmbedder::new(url, "nomic-embed-text");
        assert_eq!(embedder.id(), "ollama:nomic-embed-text");
        let vectors = embedder.embed(&["a".into(), "b".into()]).unwrap();
        assert_eq!(vectors, [vec![0.1, 0.2, 0.3], vec![0.4, 0.5, 0.6]]);
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = closed.local_addr().unwrap();
        drop(closed);
        assert!(!OllamaEmbedder::is_up(&format!("http://{addr}")));
        assert!(check_count(vec![vec![1.0], vec![1.0, 2.0]], 2).is_err());
        assert!(check_count(vec![vec![1.0]], 2).is_err());
    }
}
