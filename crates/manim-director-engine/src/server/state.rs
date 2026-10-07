//! What every handler and background task of one server shares: the engine,
//! the session, the event hub and the live inputs of the workspace views.

use super::{
    auth::Session,
    events::{EventHub, ServerEvent},
    workbench::Workbench,
};
use crate::{
    edit::{MAX_PAGE_LINES, MAX_SOURCE_BYTES},
    workspace::{self, SceneIndex, Section, Sections, SpecSnapshot, SpecTracker, ViewInputs},
    Scheduler,
};
use manim_director_core::{Catalog, DirectorSpec, EngineError, Timestamp};
use parking_lot::Mutex;
use serde::Serialize;
use std::{
    collections::HashMap,
    ops::Deref,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub const REQUEST_BODY_BYTES: usize = 256 * 1024;
pub const SOURCE_BODY_BYTES: usize = 3 * 1024 * 1024;

#[derive(Clone)]
pub struct AppState(Arc<Shared>);

pub struct Shared {
    pub scheduler: Scheduler,
    pub hub: Arc<EventHub>,
    pub session: Session,
    pub port: u16,
    pub allow_remote: bool,
    pub workbench: Workbench,
    /// Cancelled when the server stops: background tasks and streams end.
    pub closing: CancellationToken,
    started_at: Timestamp,
    spec: Mutex<SpecTracker>,
    index: Mutex<SceneIndex>,
    /// Revisions of indexed files and `director.yaml` as last announced.
    revisions: Mutex<HashMap<String, Option<String>>>,
    reindex_requested: AtomicBool,
    reindex: Notify,
}

impl Deref for AppState {
    type Target = Shared;

    fn deref(&self) -> &Shared {
        &self.0
    }
}

impl AppState {
    /// Derives `wanted` off the async runtime and publishes them as one
    /// `workspace` event.
    pub async fn publish_sections(&self, wanted: &[Section]) {
        let state = self.clone();
        let wanted = wanted.to_vec();
        match tokio::task::spawn_blocking(move || state.sections(&wanted)).await {
            Ok(Ok(sections)) => self.hub.publish(&ServerEvent::Workspace {
                sections: Box::new(sections),
            }),
            Ok(Err(error)) => tracing::warn!(%error, "could not derive workspace sections"),
            Err(error) => tracing::warn!(%error, "workspace derivation stopped"),
        }
    }

    pub fn new(
        scheduler: Scheduler,
        session: Session,
        port: u16,
        allow_remote: bool,
        workbench: Workbench,
        hub: Arc<EventHub>,
    ) -> Self {
        Self(Arc::new(Shared {
            scheduler,
            hub,
            session,
            port,
            allow_remote,
            workbench,
            closing: CancellationToken::new(),
            started_at: Timestamp::now(),
            spec: Mutex::new(SpecTracker::default()),
            index: Mutex::new(SceneIndex::default()),
            revisions: Mutex::new(HashMap::new()),
            reindex_requested: AtomicBool::new(false),
            reindex: Notify::new(),
        }))
    }
}

impl Shared {
    pub fn root(&self) -> &Path {
        self.scheduler.root()
    }

    pub fn catalog(&self) -> Option<Catalog> {
        self.scheduler
            .runtime()
            .borrow()
            .as_ref()
            .map(|identity| identity.catalog.clone())
    }

    /// Re-reads `director.yaml`. Blocking.
    pub fn load_spec(&self) -> SpecSnapshot {
        self.spec.lock().load(self.root())
    }

    /// The spec the views and the file watcher work from, without re-reading.
    pub fn current_spec(&self) -> Arc<DirectorSpec> {
        self.spec.lock().current()
    }

    pub fn index(&self) -> &Mutex<SceneIndex> {
        &self.index
    }

    /// Derives the wanted sections from fresh inputs. Blocking.
    pub fn sections(&self, wanted: &[Section]) -> Result<Sections, EngineError> {
        let spec = self.load_spec();
        let index = self.index.lock().clone();
        let catalog = self.catalog();
        let store = self.scheduler.store();
        workspace::sections(
            &ViewInputs {
                root: self.root(),
                spec: &spec,
                index: &index,
                catalog: catalog.as_ref(),
                store,
            },
            wanted,
        )
        .map_err(EngineError::internal)
    }

    /// Records a file's revision; `true` when it differs from the last one
    /// announced, in which case a `file` event goes out.
    pub fn file_changed(&self, path: &str, revision: Option<String>) -> bool {
        let changed = {
            let mut known = self.revisions.lock();
            let previous = known.insert(path.to_owned(), revision.clone());
            previous.as_ref() != Some(&revision)
        };
        if changed {
            self.hub.publish(&ServerEvent::File {
                path: path.to_owned(),
                revision,
            });
        }
        changed
    }

    /// Remembers a revision without announcing it (the watcher's first look).
    pub fn file_seen(&self, path: &str, revision: Option<String>) {
        self.revisions.lock().insert(path.to_owned(), revision);
    }

    /// Asks for one scene-index refresh; requests during a refresh coalesce
    /// into a single follow-up.
    pub fn request_reindex(&self) {
        self.reindex_requested.store(true, Ordering::SeqCst);
        self.reindex.notify_one();
    }

    pub async fn reindex_wanted(&self) {
        self.reindex.notified().await;
    }

    /// Claims every request made so far; `false` when none is pending.
    pub fn claim_reindex(&self) -> bool {
        self.reindex_requested.swap(false, Ordering::SeqCst)
    }

    pub fn engine_info(&self) -> EngineInfo {
        EngineInfo {
            version: env!("CARGO_PKG_VERSION"),
            api_version: 2,
            instance_id: self.scheduler.instance_id(),
            started_at: self.started_at,
            limits: EngineLimits {
                request_body_bytes: REQUEST_BODY_BYTES,
                source_body_bytes: SOURCE_BODY_BYTES,
                source_file_bytes: MAX_SOURCE_BYTES,
                source_page_lines: MAX_PAGE_LINES,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct EngineInfo {
    pub version: &'static str,
    pub api_version: u8,
    pub instance_id: Uuid,
    pub started_at: Timestamp,
    pub limits: EngineLimits,
}

#[derive(Debug, Clone, Serialize)]
pub struct EngineLimits {
    pub request_body_bytes: usize,
    pub source_body_bytes: usize,
    pub source_file_bytes: u64,
    pub source_page_lines: u64,
}
