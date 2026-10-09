//! Persistent settings storage backed by SQLite key-value table.
//!
//! All settings are stored as strings. Consumers parse values as needed.
//! Defaults are managed by the frontend — the backend is a pure key-value store.

use crate::engine::changes;
use crate::engine::db::Database;
use std::sync::Arc;

/// What a read over MCP returns in place of a credential-looking setting's value.
pub const MASK: &str = "•••";

/// Whether a settings key names something a read must not echo back: a key, token,
/// secret or password. Every MCP read of settings goes through this one rule
/// (TICKET-050).
pub fn looks_secret(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    ["key", "token", "secret", "password", "passwd"]
        .iter()
        .any(|needle| k.contains(needle))
}

/// Refuse to store [`MASK`] under a credential-looking key: it is what a read handed a
/// client, and writing it back would replace the real value with the mask.
pub fn refuse_the_mask(key: &str, value: &str) -> Result<(), String> {
    if looks_secret(key) && value == MASK {
        return Err(format!(
            "{key} holds a hidden value; \"{MASK}\" is the mask a read returns, not a value to store"
        ));
    }
    Ok(())
}

/// Manages persistent settings in SQLite.
pub struct SettingsManager {
    db: Arc<Database>,
}

impl SettingsManager {
    /// Create a new SettingsManager backed by the given database.
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Get a setting value by key. Returns `None` if the key doesn't exist.
    pub fn get(&self, key: &str) -> Result<Option<String>, String> {
        let conn = self.db.conn()?;
        let mut stmt = conn
            .prepare("SELECT value FROM settings WHERE key = ?1")
            .map_err(|e| format!("Query error: {e}"))?;

        let value = stmt
            .query_row(rusqlite::params![key], |row| row.get::<_, String>(0))
            .optional()
            .map_err(|e| format!("Query error: {e}"))?;

        Ok(value)
    }

    /// Get a setting value, returning the provided default if the key doesn't exist.
    pub fn get_or_default(&self, key: &str, default: &str) -> Result<String, String> {
        Ok(self.get(key)?.unwrap_or_else(|| default.to_string()))
    }

    /// [`get`](Self::get) for a client: a credential-looking key's value comes back as
    /// [`MASK`]; an unset key is `None` either way.
    pub fn get_masked(&self, key: &str) -> Result<Option<String>, String> {
        Ok(self.get(key)?.map(|value| {
            if looks_secret(key) {
                MASK.to_string()
            } else {
                value
            }
        }))
    }

    /// Set a setting value. Creates or updates (upsert).
    pub fn set(&self, key: &str, value: &str) -> Result<(), String> {
        let conn = self.db.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            rusqlite::params![key, value],
        )
        .map_err(|e| format!("Failed to set setting: {e}"))?;
        // The key, never the value: a setting may hold something that looks like a
        // credential (TICKET-035).
        changes::record(&conn, "setting", key, "updated", "", "");
        Ok(())
    }

    /// The store this manager reads, for a manager built from it that records changes.
    pub fn database(&self) -> Arc<Database> {
        Arc::clone(&self.db)
    }

    /// Every setting as `(key, value)`, sorted by key.
    pub fn list(&self) -> Result<Vec<(String, String)>, String> {
        let conn = self.db.conn()?;
        let mut stmt = conn
            .prepare("SELECT key, value FROM settings ORDER BY key")
            .map_err(|e| format!("Query error: {e}"))?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| format!("Query error: {e}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Row error: {e}"))
    }

    /// [`list`](Self::list) for a client, credential-looking values masked.
    pub fn list_masked(&self) -> Result<Vec<(String, String)>, String> {
        Ok(self
            .list()?
            .into_iter()
            .map(|(key, value)| {
                let value = if looks_secret(&key) {
                    MASK.to_string()
                } else {
                    value
                };
                (key, value)
            })
            .collect())
    }
}

/// Extension trait for optional query results.
trait OptionalExt<T> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error>;
}

impl<T> OptionalExt<T> for Result<T, rusqlite::Error> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error> {
        match self {
            Ok(val) => Ok(Some(val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::db::Database;
    use rusqlite::Connection;

    fn test_db() -> Arc<Database> {
        let conn = Connection::open_in_memory().unwrap();
        let db = Database::from_conn(conn);
        db.migrate().unwrap();
        Arc::new(db)
    }

    #[test]
    fn set_and_get_setting() {
        let db = test_db();
        let sm = SettingsManager::new(db);

        sm.set("theme", "dark").unwrap();
        let val = sm.get("theme").unwrap();
        assert_eq!(val, Some("dark".to_string()));
    }

    #[test]
    fn credential_values_are_masked() {
        let sm = SettingsManager::new(test_db());
        sm.set("openai_api_key", "sk-test").unwrap();
        sm.set("Service_TOKEN", "abc").unwrap();
        sm.set("smtp_passwd", "hunter2").unwrap();
        sm.set("theme", "dark").unwrap();
        assert_eq!(
            sm.get_masked("openai_api_key").unwrap().as_deref(),
            Some(MASK)
        );
        assert_eq!(
            sm.get_masked("Service_TOKEN").unwrap().as_deref(),
            Some(MASK)
        );
        assert_eq!(sm.get_masked("smtp_passwd").unwrap().as_deref(), Some(MASK));
        assert_eq!(sm.get_masked("theme").unwrap().as_deref(), Some("dark"));
        assert_eq!(sm.get_masked("unset_api_key").unwrap(), None);
        assert_eq!(sm.get_masked("unset").unwrap(), None);
        let listed = sm.list_masked().unwrap();
        assert!(
            listed.iter().all(|(k, v)| (v == MASK) == looks_secret(k)),
            "{listed:?}"
        );
        assert!(listed.contains(&("theme".to_string(), "dark".to_string())));
        assert_eq!(
            sm.get("openai_api_key").unwrap().as_deref(),
            Some("sk-test"),
            "in process the value is still there"
        );
        assert!(refuse_the_mask("openai_api_key", MASK).is_err());
        refuse_the_mask("openai_api_key", "sk-new").unwrap();
        refuse_the_mask("theme", MASK).unwrap();
    }

    #[test]
    fn get_missing_returns_none() {
        let db = test_db();
        let sm = SettingsManager::new(db);

        let val = sm.get("nonexistent").unwrap();
        assert_eq!(val, None);
    }

    #[test]
    fn get_or_default_uses_default() {
        let db = test_db();
        let sm = SettingsManager::new(db);

        let val = sm.get_or_default("missing", "fallback").unwrap();
        assert_eq!(val, "fallback");

        sm.set("missing", "actual").unwrap();
        let val = sm.get_or_default("missing", "fallback").unwrap();
        assert_eq!(val, "actual");
    }

    #[test]
    fn set_overwrites_existing() {
        let db = test_db();
        let sm = SettingsManager::new(db);

        sm.set("key", "first").unwrap();
        sm.set("key", "second").unwrap();
        let val = sm.get("key").unwrap();
        assert_eq!(val, Some("second".to_string()));
    }

    #[test]
    fn multiple_keys_independent() {
        let db = test_db();
        let sm = SettingsManager::new(db);

        sm.set("alpha", "1").unwrap();
        sm.set("beta", "2").unwrap();

        assert_eq!(sm.get("alpha").unwrap(), Some("1".to_string()));
        assert_eq!(sm.get("beta").unwrap(), Some("2".to_string()));
    }

    #[test]
    fn list_returns_every_setting_sorted() {
        let sm = SettingsManager::new(test_db());
        sm.set("zeta", "1").unwrap();
        sm.set("alpha", "2").unwrap();
        let all = sm.list().unwrap();
        assert_eq!(
            all,
            vec![
                ("alpha".to_string(), "2".to_string()),
                ("zeta".to_string(), "1".to_string())
            ]
        );
    }
}
