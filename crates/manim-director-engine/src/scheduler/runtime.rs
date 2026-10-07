//! What the scheduler knows about the runtime: the identity every `ready`
//! frame reports (OPS §1.5) and the spawn key a job's worker needs (§2.1).

use super::Inner;
use crate::{cache, ReadySink, RuntimeIdentity, SpawnKey, Store};
use manim_director_core::{EngineError, Limits, ReadyFrame};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::watch;

/// Records every `ready` frame: in memory at once, so this engine's next
/// submit keys its fingerprint on it, and in the store in the background.
pub(super) fn ready_sink(
    store: Arc<Store>,
    python: PathBuf,
    current: watch::Sender<Option<RuntimeIdentity>>,
) -> ReadySink {
    Arc::new(move |frame: &ReadyFrame| {
        let identity = RuntimeIdentity::from_ready(&python, frame);
        current.send_if_modified(|known| {
            let changed = known.as_ref() != Some(&identity);
            *known = Some(identity.clone());
            changed
        });
        let store = store.clone();
        tokio::spawn(async move {
            let recorded = store
                .blocking(move |store| store.record_runtime(&identity))
                .await;
            if let Err(error) = recorded {
                tracing::warn!(%error, "could not record the runtime identity");
            }
        });
    })
}

/// The key a job's preloaded worker must have been spawned with. Blocking.
pub(super) fn spawn_key(root: &Path, limits: &Limits) -> SpawnKey {
    SpawnKey {
        memory_mb: limits.memory_mb,
        manim_cfg: cache::file_revision(&root.join("manim.cfg")).ok(),
    }
}

impl Inner {
    /// The runtime identity cache keys use: this engine's latest `ready`,
    /// else the one any engine recorded for the same interpreter. Blocking.
    pub(super) fn known_runtime(&self) -> Result<Option<String>, EngineError> {
        if let Some(identity) = self.runtime.borrow().as_ref() {
            return Ok(Some(identity.cache_key()));
        }
        let stored = self
            .store
            .runtime(&self.bridge.config().python)
            .map_err(EngineError::internal)?;
        Ok(stored.map(|stored| stored.identity.cache_key()))
    }
}
