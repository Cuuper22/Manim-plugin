//! What this engine holds for each job it owns until the job ends.

use manim_director_core::CancelledBy;
use parking_lot::Mutex;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    OnceLock,
};
use tokio::sync::OwnedSemaphorePermit;
use tokio_util::sync::CancellationToken;

pub(super) struct Active {
    pub token: CancellationToken,
    cancelled_by: OnceLock<CancelledBy>,
    /// The job's place in this engine's bounded queue, released as soon as it
    /// stops waiting (started or cancelled).
    slot: Mutex<Option<OwnedSemaphorePermit>>,
    published: AtomicBool,
}

impl Active {
    pub fn new(slot: OwnedSemaphorePermit) -> Self {
        Self {
            token: CancellationToken::new(),
            cancelled_by: OnceLock::new(),
            slot: Mutex::new(Some(slot)),
            published: AtomicBool::new(false),
        }
    }

    /// The first reason given wins.
    pub fn cancel(&self, by: CancelledBy) {
        let _ = self.cancelled_by.set(by);
        self.token.cancel();
    }

    pub fn cancelled_by(&self) -> CancelledBy {
        self.cancelled_by
            .get()
            .copied()
            .unwrap_or(CancelledBy::Client)
    }

    pub fn leave_queue(&self) {
        self.slot.lock().take();
    }

    /// True exactly once: whoever observes the terminal state first publishes it.
    pub fn claim_publication(&self) -> bool {
        !self.published.swap(true, Ordering::SeqCst)
    }
}
