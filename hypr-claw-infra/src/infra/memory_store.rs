use parking_lot::Mutex;
use rusqlite::{params, Connection};
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum MemoryStoreError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),
}

pub struct MemoryStore {
    conn: Mutex<Connection>,
}

impl MemoryStore {
    pub fn new<P: AsRef<Path>>(db_path: P) -> Result<Self, MemoryStoreError> {
        let conn = Connection::open(db_path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS memory (
                id INTEGER PRIMARY KEY,
                key TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
            [],
        )?;

        conn.execute("CREATE INDEX IF NOT EXISTS idx_key ON memory(key)", [])?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn save_memory(&self, key: &str, content: &str) -> Result<(), MemoryStoreError> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn.lock();

        let existing: Option<i64> = conn
            .query_row(
                "SELECT id FROM memory WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .ok();

        if let Some(id) = existing {
            conn.execute(
                "UPDATE memory SET content = ?1, updated_at = ?2 WHERE id = ?3",
                params![content, now, id],
            )?;
        } else {
            conn.execute(
                "INSERT INTO memory (key, content, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
                params![key, content, now, now],
            )?;
        }

        Ok(())
    }

    pub fn search_memory(&self, query: &str) -> Result<Vec<(String, String)>, MemoryStoreError> {
        let pattern = format!("%{}%", query);
        let conn = self.conn.lock();

        let mut stmt = conn.prepare(
            "SELECT key, content FROM memory WHERE key LIKE ?1 OR content LIKE ?1 ORDER BY updated_at DESC"
        )?;

        let results = stmt.query_map(params![pattern], |row| Ok((row.get(0)?, row.get(1)?)))?;

        let mut memories = Vec::new();
        for result in results {
            memories.push(result?);
        }

        Ok(memories)
    }
}
