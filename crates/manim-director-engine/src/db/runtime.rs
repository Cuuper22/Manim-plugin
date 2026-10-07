//! The runtime identity and catalog each `ready` frame reports, per
//! configured interpreter (OPS §1.5, §1.8).

use super::{parsed, Store};
use anyhow::Result;
use manim_director_core::{Catalog, ReadyFrame, Timestamp};
use rusqlite::{params, OptionalExtension, Row};
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeIdentity {
    /// The interpreter path the lookup resolved, not the version it reports.
    pub python: String,
    pub runtime_version: String,
    pub manim: Option<String>,
    pub catalog: Catalog,
}

impl RuntimeIdentity {
    pub fn from_ready(python: &Path, frame: &ReadyFrame) -> Self {
        Self {
            python: python.to_string_lossy().into_owned(),
            runtime_version: frame.runtime_version.clone(),
            manim: frame.manim.clone(),
            catalog: frame.catalog.clone(),
        }
    }

    /// What cache fingerprints key on.
    pub fn cache_key(&self) -> String {
        format!(
            "{}\0{}\0{}",
            self.python,
            self.runtime_version,
            self.manim.as_deref().unwrap_or_default()
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoredRuntime {
    pub identity: RuntimeIdentity,
    /// Moves only when the version, Manim or the catalog change.
    pub changed_at: Timestamp,
    pub seen_at: Timestamp,
}

const COLUMNS: &str = "python, runtime_version, manim, catalog, changed_at, seen_at";

impl Store {
    pub fn record_runtime(&self, identity: &RuntimeIdentity) -> Result<StoredRuntime> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO runtime_identity(python, runtime_version, manim, catalog, changed_at, seen_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)
             ON CONFLICT(python) DO UPDATE SET
                changed_at = CASE WHEN runtime_version IS excluded.runtime_version
                                   AND manim IS excluded.manim AND catalog IS excluded.catalog
                             THEN changed_at ELSE excluded.changed_at END,
                runtime_version = excluded.runtime_version, manim = excluded.manim,
                catalog = excluded.catalog, seen_at = excluded.seen_at",
            params![
                identity.python,
                identity.runtime_version,
                identity.manim,
                serde_json::to_string(&identity.catalog)?,
                Timestamp::now().to_string(),
            ],
        )?;
        Ok(conn.query_row(
            &format!("SELECT {COLUMNS} FROM runtime_identity WHERE python=?1"),
            [&identity.python],
            stored,
        )?)
    }

    pub fn runtime(&self, python: &Path) -> Result<Option<StoredRuntime>> {
        Ok(self
            .conn
            .lock()
            .query_row(
                &format!("SELECT {COLUMNS} FROM runtime_identity WHERE python=?1"),
                [python.to_string_lossy()],
                stored,
            )
            .optional()?)
    }
}

fn stored(row: &Row<'_>) -> rusqlite::Result<StoredRuntime> {
    Ok(StoredRuntime {
        identity: RuntimeIdentity {
            python: row.get("python")?,
            runtime_version: row.get("runtime_version")?,
            manim: row.get("manim")?,
            catalog: super::json_column(row, "catalog")?,
        },
        changed_at: parsed(row, "changed_at")?,
        seen_at: parsed(row, "seen_at")?,
    })
}

#[cfg(test)]
mod tests {
    use super::super::testing::store;
    use super::*;
    use manim_director_core::CatalogTheme;

    fn identity(manim: Option<&str>) -> RuntimeIdentity {
        RuntimeIdentity {
            python: "/venv/bin/python".into(),
            runtime_version: "2.0.0".into(),
            manim: manim.map(str::to_owned),
            catalog: Catalog {
                themes: vec![CatalogTheme {
                    name: "midnight".into(),
                    tokens: vec![("background".into(), "#0B1020".into())],
                }],
                project_templates: vec!["explainer".into()],
                scene_templates: vec![],
            },
        }
    }

    #[test]
    fn changed_at_moves_only_when_the_identity_changes() {
        let (_dir, store) = store();
        assert!(store
            .runtime(Path::new("/venv/bin/python"))
            .unwrap()
            .is_none());
        let first = store.record_runtime(&identity(Some("0.21.0"))).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let again = store.record_runtime(&identity(Some("0.21.0"))).unwrap();
        assert_eq!(again.changed_at, first.changed_at);
        assert!(again.seen_at > first.seen_at);
        std::thread::sleep(std::time::Duration::from_millis(5));
        let upgraded = store.record_runtime(&identity(None)).unwrap();
        assert!(upgraded.changed_at > first.changed_at);
        assert_eq!(
            store.runtime(Path::new("/venv/bin/python")).unwrap(),
            Some(upgraded)
        );
    }
}
