use rusqlite::{Connection, Result};

/// Wrapper around a SQLite connection for the file-sharing metadata store.
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open (or create) a SQLite database at the given path.
    pub fn new(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let db = Self { conn };
        db.run_migrations()?;
        Ok(db)
    }

    /// Return a reference to the underlying connection.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Create all tables if they don't already exist.
    fn run_migrations(&self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS users (
                user_id       TEXT PRIMARY KEY,
                username      TEXT UNIQUE NOT NULL,
                password_hash TEXT NOT NULL,
                role          TEXT NOT NULL CHECK(role IN ('admin', 'standard')),
                created_at    TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS sessions (
                token       TEXT PRIMARY KEY,
                user_id     TEXT NOT NULL REFERENCES users(user_id),
                device_name TEXT,
                issued_at   TEXT NOT NULL,
                expires_at  TEXT NOT NULL,
                last_active TEXT NOT NULL,
                is_revoked  INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS files (
                file_id         TEXT PRIMARY KEY,
                filename        TEXT NOT NULL,
                size            INTEGER NOT NULL,
                mime_type       TEXT,
                chunk_size      INTEGER NOT NULL,
                total_chunks    INTEGER NOT NULL,
                whole_file_hash TEXT NOT NULL,
                hash_algorithm  TEXT NOT NULL,
                uploaded_by     TEXT NOT NULL REFERENCES users(user_id),
                uploaded_at     TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS transfer_sessions (
                session_id      TEXT PRIMARY KEY,
                file_id         TEXT NOT NULL REFERENCES files(file_id),
                user_id         TEXT NOT NULL REFERENCES users(user_id),
                direction       TEXT NOT NULL CHECK(direction IN ('upload', 'download')),
                chunk_size      INTEGER NOT NULL,
                total_chunks    INTEGER NOT NULL,
                status          TEXT NOT NULL,
                created_at      TEXT NOT NULL,
                updated_at      TEXT NOT NULL,
                source_size     INTEGER,
                source_modified TEXT
            );

            CREATE TABLE IF NOT EXISTS chunk_progress (
                session_id   TEXT NOT NULL REFERENCES transfer_sessions(session_id),
                chunk_index  INTEGER NOT NULL,
                hash         TEXT NOT NULL,
                completed_at TEXT NOT NULL,
                PRIMARY KEY (session_id, chunk_index)
            );

            CREATE TABLE IF NOT EXISTS transfer_history (
                session_id     TEXT PRIMARY KEY,
                file_id        TEXT NOT NULL,
                filename       TEXT NOT NULL,
                direction      TEXT NOT NULL,
                file_size      INTEGER NOT NULL,
                status         TEXT NOT NULL,
                started_at     TEXT NOT NULL,
                completed_at   TEXT,
                avg_throughput REAL
            );
            ",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_in_memory_and_migrate() {
        let db = Database::new(":memory:").expect("should open in-memory db");

        // Verify all six tables exist
        let tables: Vec<String> = db
            .conn()
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(tables.contains(&"users".to_string()));
        assert!(tables.contains(&"sessions".to_string()));
        assert!(tables.contains(&"files".to_string()));
        assert!(tables.contains(&"transfer_sessions".to_string()));
        assert!(tables.contains(&"chunk_progress".to_string()));
        assert!(tables.contains(&"transfer_history".to_string()));
    }

    #[test]
    fn migrations_are_idempotent() {
        let db = Database::new(":memory:").expect("first open");
        // Running migrations again via a second call should not fail
        db.run_migrations().expect("second migration run should succeed");
    }
}
