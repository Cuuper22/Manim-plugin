//! Running one job (OPS §1.5, §2): wait for the scene lock, take a runtime
//! worker, key the job on its start fingerprint, converse, validate the
//! artifacts, then record the job's one terminal transition.

use super::{
    active::Active, artifacts, recorder::Recorder, runtime, Inner, Outcome, Queued, RunContext,
    Slot,
};
use crate::{
    cache, request_line, BridgeEvent, BridgeOutcome, Finish, RuntimeIdentity, SpawnKey, Undelivered,
};
use manim_director_core::{
    EngineEvent, ErrorBody, JobRecord, JobStatus, LogLevel, MediaInfo, OperationResult,
    ProgressPhase, ReadyFrame, Task,
};
use serde_json::json;
use std::{path::Path, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const LOCK_RETRY: Duration = Duration::from_millis(250);

/// The start fingerprint and the identity it was computed with, so finish
/// can tell whether project files changed during the run.
struct Started {
    fingerprint: cache::Fingerprint,
    runtime: String,
}

impl Inner {
    pub(super) async fn run(self: Arc<Self>, queued: Queued, slot: &mut Slot) {
        let Queued {
            job,
            context,
            active,
        } = queued;
        active.leave_queue();
        let id = job.id;
        let root = self.root.clone();
        let started =
            self.store
                .blocking(move |store| {
                    let Some(running) = store.set_running(id)? else {
                        return Ok(Err(store.get_job(id)?));
                    };
                    let out_dir = match running.task.out_dir() {
                        Some(out_dir) => artifacts::create_out_dir(&root, out_dir)
                            .map_err(|error| error.to_string()),
                        None => Ok(()),
                    };
                    let key = runtime::spawn_key(&root, &running.limits);
                    Ok(Ok((running, out_dir, key)))
                })
                .await;
        match started {
            Ok(Ok((running, out_dir, key))) => {
                let _ = self
                    .events
                    .send(EngineEvent::Job(Arc::new(running.clone())));
                let recorder = Recorder::start(self.store.clone(), self.events.clone(), id);
                recorder.engine_phase(
                    ProgressPhase::Starting,
                    None,
                    Some("waiting for the runtime"),
                );
                let (outcome, start) = match out_dir {
                    _ if active.token.is_cancelled() => {
                        (Outcome::Cancelled(active.cancelled_by()), None)
                    }
                    Err(error) => (
                        Outcome::Failed(ErrorBody::internal(format!(
                            "Could not create the job's artifact directory: {error}"
                        ))),
                        None,
                    ),
                    Ok(()) => {
                        self.execute(&job, &context, &key, &active, &recorder, slot)
                            .await
                    }
                };
                recorder.close().await;
                self.finish(&job, &context, &active, outcome, start).await;
            }
            // Cancelled while queued, or failed by another engine's reaper.
            Ok(Err(current)) => {
                if let Some(current) = current.filter(|job| job.status.is_terminal()) {
                    self.publish(&active, &current);
                }
            }
            Err(error) => tracing::error!(%id, %error, "could not start the job"),
        }
        self.active.lock().remove(&id);
    }

    /// Everything after `running` counts against the job's timeout,
    /// including waits for the scene lock and the worker's `ready`.
    async fn execute(
        &self,
        job: &JobRecord,
        context: &RunContext,
        key: &SpawnKey,
        active: &Active,
        recorder: &Recorder,
        slot: &mut Slot,
    ) -> (Outcome, Option<Started>) {
        let attempt = active.token.child_token();
        let timeout = Duration::from_secs(job.limits.timeout_seconds);
        let mut start = None;
        let (outcome, timed_out) = {
            let run = async {
                if let Some(lock) = scene_lock_key(&job.task) {
                    match self
                        .lock_scene(job.id, &lock, &attempt, recorder, slot)
                        .await
                    {
                        Ok(true) => {}
                        Ok(false) => return BridgeOutcome::Cancelled,
                        Err(error) => return BridgeOutcome::Failed(error),
                    }
                }
                self.dispatch(job, context, key, &attempt, recorder, &mut start)
                    .await
            };
            tokio::pin!(run);
            tokio::select! {
                outcome = &mut run => (outcome, false),
                _ = tokio::time::sleep(timeout) => {
                    attempt.cancel();
                    ((&mut run).await, true)
                }
            }
        };
        let outcome = match outcome {
            BridgeOutcome::Succeeded(value) => {
                let count = value["artifacts"].as_array().map_or(0, Vec::len) as u64;
                if count > 0 {
                    recorder.engine_phase(ProgressPhase::Validate, Some(count), None);
                }
                let root = self.root.clone();
                let task = job.task.clone();
                let source_media = context.source_media.clone();
                let budget = context.artifact_budget;
                let accepted = tokio::task::spawn_blocking(move || {
                    accept_result(&root, &task, value, source_media.as_ref(), budget)
                })
                .await
                .unwrap_or_else(|error| Err(ErrorBody::internal(error.to_string())));
                match accepted {
                    Ok(mut result) => {
                        if let Some(source) = &context.source {
                            result.set_source(source.clone());
                        }
                        Outcome::Succeeded(Box::new(result))
                    }
                    Err(error) => Outcome::Failed(error),
                }
            }
            BridgeOutcome::Failed(error) => Outcome::Failed(error),
            BridgeOutcome::Cancelled if timed_out => {
                Outcome::Failed(ErrorBody::timeout(job.limits.timeout_seconds))
            }
            BridgeOutcome::Cancelled => Outcome::Cancelled(active.cancelled_by()),
        };
        (outcome, start)
    }

    /// Takes a ready runtime worker, records the start fingerprint, then
    /// hands the worker the request. A worker that died before taking the
    /// request is replaced once.
    async fn dispatch(
        &self,
        job: &JobRecord,
        context: &RunContext,
        key: &SpawnKey,
        cancel: &CancellationToken,
        recorder: &Recorder,
        start: &mut Option<Started>,
    ) -> BridgeOutcome {
        let request_id = job.id.to_string();
        let line = match request_line(&request_id, &self.root, &job.task) {
            Ok(line) => line,
            Err(error) => return BridgeOutcome::Failed(error.body()),
        };
        let mut on_event = |event: BridgeEvent<'_>| recorder.record(event);
        let mut retried = false;
        loop {
            let worker = match self
                .bridge
                .acquire(&self.root, key, cancel, &mut on_event)
                .await
            {
                Ok(worker) => worker,
                Err(outcome) => return outcome,
            };
            *start = self.start_fingerprint(job, context, worker.frame()).await;
            match worker
                .converse(&line, &request_id, cancel, &mut on_event)
                .await
            {
                Ok(outcome) => return outcome,
                Err(Undelivered(error)) if retried => return BridgeOutcome::Failed(error),
                Err(Undelivered(_)) => {
                    retried = true;
                    recorder.engine_log(
                        LogLevel::Warning,
                        "The runtime worker exited before taking the request; retrying with a fresh one.",
                    );
                }
            }
        }
    }

    /// Waits until the job holds its scene lock, and a worker with it;
    /// `Ok(false)` when cancelled.
    async fn lock_scene(
        &self,
        id: Uuid,
        key: &str,
        cancel: &CancellationToken,
        recorder: &Recorder,
        slot: &mut Slot,
    ) -> Result<bool, ErrorBody> {
        let mut announced = false;
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return Ok(false),
                _ = slot.take() => {}
            }
            let (lock, owner) = (key.to_owned(), self.instance_id);
            let held = self
                .store
                .blocking(move |store| store.try_lock_scene(&lock, id, owner))
                .await
                .map_err(|error| ErrorBody::internal(error.to_string()))?;
            if held {
                return Ok(true);
            }
            slot.give_back();
            if !announced {
                announced = true;
                recorder.engine_phase(
                    ProgressPhase::Starting,
                    None,
                    Some(&format!("Waiting for another render of {key}")),
                );
            }
            tokio::select! {
                _ = cancel.cancelled() => return Ok(false),
                _ = tokio::time::sleep(LOCK_RETRY) => {}
            }
        }
    }

    /// `F_start` under the identity of the worker that runs the job, stored
    /// on the job before the request is written. Cacheable jobs only.
    async fn start_fingerprint(
        &self,
        job: &JobRecord,
        context: &RunContext,
        ready: &ReadyFrame,
    ) -> Option<Started> {
        let spec = context.spec.clone()?;
        let runtime = RuntimeIdentity::from_ready(&self.bridge.config().python, ready).cache_key();
        let (root, task, id) = (self.root.clone(), job.task.clone(), job.id);
        let started = self
            .store
            .blocking(move |store| {
                let fingerprint = cache::fingerprint(&root, &spec, &runtime, &task)?;
                store.set_fingerprint(id, &fingerprint.value)?;
                Ok(Started {
                    fingerprint,
                    runtime,
                })
            })
            .await;
        started
            .inspect_err(
                |error| tracing::warn!(%id, %error, "could not compute the start fingerprint"),
            )
            .ok()
    }

    /// Records the job's one terminal transition, frees its scene lock and
    /// caches a success whose inputs did not change during the run. Losing
    /// the race to another engine's reaper is logged and the winner's record
    /// is published instead.
    async fn finish(
        &self,
        job: &JobRecord,
        context: &RunContext,
        active: &Active,
        outcome: Outcome,
        start: Option<Started>,
    ) {
        let id = job.id;
        let root = self.root.clone();
        let job_file = job.scene_file.clone();
        let operation = job.operation;
        let task = job.task.clone();
        let spec = context.spec.clone();
        let recorded = self
            .store
            .blocking(move |store| {
                let revision = |file: Option<&str>| {
                    start
                        .as_ref()
                        .and_then(|start| start.fingerprint.file_hash(file?))
                        .map(str::to_owned)
                };
                let finish = match &outcome {
                    Outcome::Succeeded(result) => {
                        let file = result.scene().map(|scene| scene.file.as_str());
                        let revision = revision(file.or(job_file.as_deref()));
                        store.finish_success(id, result, revision.as_deref())?
                    }
                    Outcome::Failed(error) => {
                        let revision = revision(job_file.as_deref());
                        store.finish_error(id, JobStatus::Failed, error, revision.as_deref())?
                    }
                    Outcome::Cancelled(by) => {
                        let revision = revision(job_file.as_deref());
                        let error = ErrorBody::cancelled(*by);
                        store.finish_error(id, JobStatus::Cancelled, &error, revision.as_deref())?
                    }
                };
                if let Err(error) = store.unlock_scene(id) {
                    tracing::warn!(%id, %error, "could not release the scene lock");
                }
                let kept = matches!(
                    (&finish, &outcome),
                    (Finish::Ended(_), Outcome::Succeeded(_))
                );
                if let (true, Some(start), Some(spec), Outcome::Succeeded(result)) =
                    (kept, &start, &spec, &outcome)
                {
                    match cache::fingerprint(&root, spec, &start.runtime, &task) {
                        Ok(now) if now.value == start.fingerprint.value => {
                            if let Err(error) =
                                store.cache_put(&now.value, Some(id), result, operation)
                            {
                                tracing::warn!(%id, %error, "could not cache the result");
                            }
                        }
                        Ok(_) => tracing::debug!(%id, "inputs changed during the run; not cached"),
                        Err(error) => tracing::warn!(%id, %error, "could not re-hash the inputs"),
                    }
                }
                if !kept && task.out_dir().is_some() {
                    if let Err(error) = artifacts::remove_out_dir(&root, id) {
                        tracing::warn!(%id, %error, "could not remove the job's artifact directory");
                    }
                }
                Ok(finish)
            })
            .await;
        match recorded {
            Ok(Finish::Ended(record)) => self.publish(active, &record),
            Ok(Finish::Superseded(record)) => {
                tracing::warn!(
                    %id,
                    status = %record.status,
                    "the job had already ended elsewhere; this run's outcome was discarded"
                );
                self.publish(active, &record);
            }
            Err(error) => tracing::error!(%id, %error, "could not record the job outcome"),
        }
    }
}

/// Renders and stills of one scene run one at a time across engines; a
/// scene is named by its class, or by the files searched for it.
fn scene_lock_key(task: &Task) -> Option<String> {
    let (scene, files) = match task {
        Task::Render(task) => (&task.scene, &task.files),
        Task::Still(task) => (&task.scene, &task.files),
        _ => return None,
    };
    if let Some(scene) = scene {
        return Some(scene.clone());
    }
    let mut files: Vec<_> = files.iter().map(|file| file.to_string_lossy()).collect();
    files.sort();
    Some(format!(
        "files:{}",
        blake3::hash(files.join("\0").as_bytes()).to_hex()
    ))
}

/// Parses a runtime result into the operation's type and enforces the
/// artifact contract, filling in each artifact's (engine) fields.
pub(crate) fn accept_result(
    root: &Path,
    task: &Task,
    value: serde_json::Value,
    source_media: Option<&MediaInfo>,
    budget_bytes: u64,
) -> Result<OperationResult, ErrorBody> {
    let operation = task.operation();
    let mut result = OperationResult::from_json(operation, value).map_err(|detail| {
        ErrorBody::new(
            "runtime_protocol",
            format!("The runtime's {operation} result does not match the contract: {detail}."),
            Some(json!({"detail": detail, "stderr_tail": null})),
        )
    })?;
    artifacts::validate(
        &artifacts::Expectations {
            root,
            task,
            source: source_media,
            budget_bytes,
        },
        &mut result,
    )?;
    Ok(result)
}
