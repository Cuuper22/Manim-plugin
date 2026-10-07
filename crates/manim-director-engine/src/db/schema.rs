//! Schema versions, applied through `PRAGMA user_version`.

use anyhow::{bail, Context, Result};
use rusqlite::{Connection, TransactionBehavior};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// The version this engine reads and writes.
pub(super) const SCHEMA_VERSION: i64 = 2;

/// `(version, script)` in ascending order; a database at version `v` runs
/// every script whose version is above `v`. Version 2 is the 2.0 baseline;
/// 1.x databases are retired instead of migrated.
const MIGRATIONS: &[(i64, &str)] = &[(2, V2)];

const V2: &str = "
CREATE TABLE jobs (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    operation TEXT NOT NULL,
    status TEXT NOT NULL,
    origin TEXT NOT NULL,
    owner TEXT NOT NULL,
    request TEXT NOT NULL,
    task TEXT NOT NULL,
    limits TEXT NOT NULL,
    fingerprint TEXT,
    cached INTEGER NOT NULL DEFAULT 0,
    cached_from TEXT,
    source_job_id TEXT,
    scene_file TEXT,
    scene_class TEXT,
    scene_revision TEXT,
    profile TEXT,
    cancel_requested INTEGER NOT NULL DEFAULT 0,
    progress TEXT,
    result TEXT,
    error TEXT,
    created_at TEXT NOT NULL,
    started_at TEXT,
    finished_at TEXT
);
CREATE INDEX jobs_status_sequence ON jobs(status, sequence DESC);
CREATE INDEX jobs_owner_status ON jobs(owner, status);
CREATE INDEX jobs_scene ON jobs(scene_class, scene_file, operation, status, sequence DESC);
CREATE INDEX jobs_fingerprint ON jobs(fingerprint);
CREATE TABLE logs (
    cursor INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    stream TEXT NOT NULL,
    level TEXT NOT NULL,
    message TEXT NOT NULL,
    data TEXT
);
CREATE INDEX logs_job_cursor ON logs(job_id, cursor);
CREATE TABLE log_usage (
    job_id TEXT PRIMARY KEY,
    records INTEGER NOT NULL DEFAULT 0,
    bytes INTEGER NOT NULL DEFAULT 0,
    truncated INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE cache (
    fingerprint TEXT PRIMARY KEY,
    operation TEXT NOT NULL,
    job_id TEXT,
    result TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE TABLE engine_leases (
    instance_id TEXT PRIMARY KEY,
    pid INTEGER NOT NULL,
    mode TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    heartbeat_at INTEGER NOT NULL
);
CREATE TABLE scene_locks (
    key TEXT PRIMARY KEY,
    job_id TEXT NOT NULL,
    owner TEXT NOT NULL,
    acquired_at TEXT NOT NULL
);
CREATE TABLE runtime_identity (
    python TEXT PRIMARY KEY,
    runtime_version TEXT NOT NULL,
    manim TEXT,
    catalog TEXT NOT NULL,
    changed_at TEXT NOT NULL,
    seen_at TEXT NOT NULL
);";

/// Brings the database to `SCHEMA_VERSION`. The version is re-read inside an
/// immediate transaction so two engines opening a fresh file cannot both run
/// the same script.
pub(super) fn migrate(conn: &mut Connection) -> Result<()> {
    let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current: i64 = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if current > SCHEMA_VERSION {
        bail!(
            "state.db uses schema {current}, newer than this engine's {SCHEMA_VERSION}; \
             upgrade manim-director"
        );
    }
    for (version, script) in MIGRATIONS {
        if *version > current {
            transaction
                .execute_batch(script)
                .with_context(|| format!("migrating state.db to schema {version}"))?;
            transaction.pragma_update(None, "user_version", version)?;
        }
    }
    transaction.commit()?;
    Ok(())
}

/// A 1.x database cannot be migrated meaningfully; it is kept beside the new
/// one as `state.v1.db` and history starts fresh.
pub(super) fn retire_one_x(path: &Path) -> Result<()> {
    if !path.is_file() {
        return Ok(());
    }
    let (version, tables): (i64, i64) = {
        let conn = Connection::open(path)?;
        (
            conn.pragma_query_value(None, "user_version", |row| row.get(0))?,
            conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
                [],
                |row| row.get(0),
            )?,
        )
    };
    if version >= SCHEMA_VERSION || tables == 0 {
        return Ok(());
    }
    let retired = path.with_file_name("state.v1.db");
    for suffix in ["", "-wal", "-shm"] {
        let from = PathBuf::from(format!("{}{suffix}", path.display()));
        if from.exists() {
            fs::rename(&from, format!("{}{suffix}", retired.display()))
                .with_context(|| format!("retiring {}", from.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_is_idempotent_and_refuses_newer_schemas() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = Connection::open(dir.path().join("state.db")).unwrap();
        migrate(&mut conn).unwrap();
        migrate(&mut conn).unwrap();
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .unwrap();
        let error = migrate(&mut conn).unwrap_err().to_string();
        assert!(error.contains("newer than this engine"), "{error}");
    }

    #[test]
    fn a_one_x_database_is_retired_beside_the_new_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        Connection::open(&path)
            .unwrap()
            .execute_batch("CREATE TABLE jobs (sequence INTEGER PRIMARY KEY, params TEXT);")
            .unwrap();
        retire_one_x(&path).unwrap();
        assert!(dir.path().join("state.v1.db").is_file());
        assert!(!path.exists());
    }
}
