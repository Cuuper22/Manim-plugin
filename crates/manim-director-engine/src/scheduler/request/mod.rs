//! The one place requests are built and resolved (OPS §1.2). CLI, HTTP and
//! MCP parse through here; resolution turns public params into the absolute,
//! fully defaulted task the runtime executes, using the spec loaded once for
//! the submission.

mod export;
mod ingest;
mod paths;
mod source;

pub use paths::cli_project_path;
pub(crate) use paths::{project_path, PathUse};
pub(crate) use source::SelectedSource;

use crate::{cache, scheduler::artifacts, Store};
use manim_director_core::{
    files, python_sources, relative_posix, Budget, CaptionsTask, ContactSheetTask, DiagnoseTask,
    DirectorSpec, DoctorTask, EngineError, FrameTask, Limits, LogStream, MediaFormat, Operation,
    OperationRequest, QaTask, RenderTask, SceneSpec, StillTask, Task, ValidateMathTask,
    MAX_PYTHON_SOURCES,
};
use serde_json::Value;
use source::Consumer;
use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
};
use uuid::Uuid;

const DEFAULT_TIMEOUT_SECONDS: u64 = 1800;
const DEFAULT_OUTPUT_MB: u64 = 2048;
const DEFAULT_SEED: u64 = 1729;
const MAX_DIAGNOSE_TEXT_BYTES: usize = 64 * 1024;
const DEFAULT_MEDIA_DIR: &str = ".manim-director/media";

/// A frontend that accepts tagged `OperationRequest` bodies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frontend {
    /// `POST /api/jobs`: job operations except `ingest`, which reads host paths.
    Http,
    /// The MCP `submit` tool: every job operation.
    McpSubmit,
}

impl Frontend {
    pub fn accepts(self, operation: Operation) -> bool {
        match self {
            Self::Http => operation.http_allowed(),
            Self::McpSubmit => !operation.is_direct(),
        }
    }
}

/// Parses a tagged request body, rejecting operations the frontend does not
/// accept.
pub fn parse_request(frontend: Frontend, body: Value) -> Result<OperationRequest, EngineError> {
    let named = body
        .get("operation")
        .and_then(Value::as_str)
        .and_then(|name| name.parse::<Operation>().ok());
    if let Some(operation) = named.filter(|operation| !frontend.accepts(*operation)) {
        return Err(EngineError::OperationNotAllowed {
            operation,
            allowed: Operation::ALL
                .iter()
                .copied()
                .filter(|operation| frontend.accepts(*operation))
                .collect(),
        });
    }
    OperationRequest::from_json(body)
}

/// Parses the params of an operation the caller already chose (a dedicated
/// route or tool).
pub fn parse_params(operation: Operation, params: Value) -> Result<OperationRequest, EngineError> {
    OperationRequest::from_params(operation, params)
}

pub(crate) struct ProjectContext<'a> {
    /// Canonical project root.
    pub root: &'a Path,
    /// `director.yaml` as loaded once for this submission.
    pub spec: &'a Result<DirectorSpec, EngineError>,
    pub store: &'a Store,
}

impl ProjectContext<'_> {
    pub(crate) fn spec(&self) -> Result<&DirectorSpec, EngineError> {
        self.spec.as_ref().map_err(Clone::clone)
    }

    fn valid_spec(&self) -> Option<&DirectorSpec> {
        self.spec.as_ref().ok()
    }
}

/// A request resolved at submit; queued jobs run with exactly this.
pub(crate) struct ResolvedJob {
    pub task: Task,
    pub limits: Limits,
    pub scene_class: Option<String>,
    pub scene_file: Option<String>,
    /// Inherited from a source job; render/still revisions come from the fingerprint.
    pub scene_revision: Option<String>,
    pub profile: Option<String>,
    pub source: Option<SelectedSource>,
    pub artifact_budget: u64,
}

impl ResolvedJob {
    fn new(task: Task, spec: Option<&DirectorSpec>) -> Self {
        Self {
            task,
            limits: limits(spec),
            scene_class: None,
            scene_file: None,
            scene_revision: None,
            profile: None,
            source: None,
            artifact_budget: artifact_budget(spec),
        }
    }

    /// Jobs reading another job's media inherit its scene association.
    fn reading(mut self, source: SelectedSource) -> Self {
        if let Some(job) = &source.job {
            self.scene_class = job.scene_class.clone();
            self.scene_file = job.scene_file.clone();
            self.scene_revision = job.scene_revision.clone();
        }
        self.source = Some(source);
        self
    }
}

/// A job's timeout and opt-in memory ceiling: the environment overrides
/// `budgets` in `director.yaml` (OPS §1.2).
pub(crate) fn limits(spec: Option<&DirectorSpec>) -> Limits {
    let budgets = spec.map(|spec| &spec.budgets);
    let timeout_seconds = env_u64("MANIM_DIRECTOR_TIMEOUT_SECONDS")
        .or(budgets.and_then(|budgets| budgets.render_seconds))
        .unwrap_or(DEFAULT_TIMEOUT_SECONDS)
        .clamp(10, 86_400);
    let memory_mb = env_u64("MANIM_DIRECTOR_MEMORY_MB")
        .or(budgets.and_then(|budgets| budgets.memory_mb))
        .filter(|megabytes| *megabytes > 0);
    Limits {
        timeout_seconds,
        memory_mb,
    }
}

fn artifact_budget(spec: Option<&DirectorSpec>) -> u64 {
    spec.and_then(|spec| spec.budgets.output_mb)
        .unwrap_or(DEFAULT_OUTPUT_MB)
        .saturating_mul(1024 * 1024)
}

fn env_u64(name: &str) -> Option<u64> {
    std::env::var(name).ok()?.trim().parse().ok()
}

/// Resolves a job request. Direct operations never reach here.
pub(crate) fn resolve(
    ctx: &ProjectContext<'_>,
    job_id: Uuid,
    request: &OperationRequest,
) -> Result<ResolvedJob, EngineError> {
    let out_dir = artifacts::job_dir(ctx.root, job_id);
    let spec = ctx.valid_spec();
    Ok(match request {
        OperationRequest::Init(_) | OperationRequest::Discover(_) => {
            return Err(EngineError::OperationNotAllowed {
                operation: request.operation(),
                allowed: Operation::job_operations().collect(),
            })
        }
        OperationRequest::Doctor(_) => ResolvedJob::new(Task::Doctor(DoctorTask {}), spec),
        OperationRequest::Render(params) => {
            let spec = ctx.spec()?;
            let target = scene_target(ctx, params.scene.as_deref(), params.file.as_deref())?;
            let settings = spec.select_profile(params.profile.as_deref())?.clone();
            let profile = settings.profile.clone();
            let task = Task::Render(RenderTask {
                scene: target.class.clone(),
                files: target.files,
                settings,
                media_dir: ctx.root.join(&spec.project.media_dir),
                out_dir,
                sections: params.sections,
                fresh: params.fresh,
            });
            ResolvedJob {
                scene_class: target.class,
                scene_file: target.file,
                profile: Some(profile),
                ..ResolvedJob::new(task, Some(spec))
            }
        }
        OperationRequest::Still(params) => {
            let spec = ctx.spec()?;
            let target = scene_target(ctx, params.scene.as_deref(), params.file.as_deref())?;
            let mut settings = spec.select_profile(params.profile.as_deref())?.clone();
            settings.format = MediaFormat::Png;
            let profile = settings.profile.clone();
            let task = Task::Still(StillTask {
                scene: target.class.clone(),
                files: target.files,
                settings,
                media_dir: ctx.root.join(&spec.project.media_dir),
                out_dir,
                fresh: params.fresh,
            });
            ResolvedJob {
                scene_class: target.class,
                scene_file: target.file,
                profile: Some(profile),
                ..ResolvedJob::new(task, Some(spec))
            }
        }
        OperationRequest::Frame(params) => {
            let source = source::select(
                ctx,
                params.source.as_ref(),
                params.scene.as_deref(),
                params.profile.as_deref(),
                Consumer::Video,
            )?;
            if let Some(duration) = source.media.duration_seconds {
                if params.at_seconds > duration {
                    return Err(EngineError::invalid(
                        "at_seconds",
                        format!("is past the end of the source ({duration} s)"),
                    ));
                }
            }
            let task = Task::Frame(FrameTask {
                video: source.absolute.clone(),
                at_seconds: params.at_seconds,
                out_dir,
            });
            ResolvedJob::new(task, spec).reading(source)
        }
        OperationRequest::ContactSheet(params) => {
            let source = source::select(
                ctx,
                params.source.as_ref(),
                params.scene.as_deref(),
                params.profile.as_deref(),
                Consumer::Video,
            )?;
            let task = Task::ContactSheet(ContactSheetTask {
                video: source.absolute.clone(),
                count: params.count,
                columns: params.columns,
                timeline: source.timeline.clone(),
                out_dir,
            });
            ResolvedJob::new(task, spec).reading(source)
        }
        OperationRequest::Qa(params) => {
            let spec = ctx.spec()?;
            let source = source::select(
                ctx,
                params.source.as_ref(),
                params.scene.as_deref(),
                params.profile.as_deref(),
                Consumer::VideoOrImage,
            )?;
            let task = Task::Qa(QaTask {
                source: source.absolute.clone(),
                source_kind: source.kind,
                frames: params.frames,
                safe_area: spec.safe_area,
                timeline: source.timeline.clone(),
                out_dir,
            });
            ResolvedJob::new(task, Some(spec)).reading(source)
        }
        OperationRequest::Diagnose(params) => {
            let text = match (params.job_id, &params.text) {
                (Some(id), _) => failure_text(ctx, id)?,
                (None, Some(text)) => text.clone(),
                (None, None) => return Err(EngineError::invalid("text", "missing")),
            };
            let task = Task::Diagnose(DiagnoseTask {
                text: keep_tail(&text, MAX_DIAGNOSE_TEXT_BYTES).to_owned(),
            });
            ResolvedJob::new(task, spec)
        }
        OperationRequest::ValidateMath(params) => {
            let seed = params
                .seed
                .or(spec.and_then(|spec| spec.project.seed))
                .unwrap_or(DEFAULT_SEED);
            let task = Task::ValidateMath(ValidateMathTask {
                steps: params.steps.clone(),
                ranges: params.ranges.clone(),
                samples: params.samples,
                tolerance: params.tolerance,
                seed,
            });
            ResolvedJob::new(task, spec)
        }
        OperationRequest::Captions(params) => {
            let path = project_path(
                ctx.root,
                "path",
                &params.path,
                PathUse::Input {
                    allow_artifacts: true,
                    extensions: files::CAPTIONS,
                },
            )?;
            let output = params
                .output
                .as_deref()
                .map(|output| {
                    project_path(
                        ctx.root,
                        "output",
                        output,
                        PathUse::Output {
                            media_dir: media_dir(spec),
                            extensions: files::CAPTIONS,
                        },
                    )
                })
                .transpose()?;
            let task = Task::Captions(CaptionsTask {
                path,
                shift_seconds: params.shift_seconds,
                scale: params.scale,
                output,
            });
            ResolvedJob::new(task, spec)
        }
        OperationRequest::Ingest(params) => {
            let spec = ctx.spec()?;
            ResolvedJob::new(ingest::task(ctx.root, spec, params)?, Some(spec))
        }
        OperationRequest::Export(params) => {
            let (task, source) = export::task(ctx, params, artifact_budget(spec))?;
            let job = ResolvedJob::new(task, spec);
            match source {
                Some(source) => job.reading(source),
                None => job,
            }
        }
    })
}

fn media_dir(spec: Option<&DirectorSpec>) -> &str {
    spec.map_or(DEFAULT_MEDIA_DIR, |spec| &spec.project.media_dir)
}

/// Which scene a name refers to, per `director.yaml` (no disk access).
pub(crate) struct SceneLookup<'a> {
    pub class: Option<String>,
    entry: Option<(usize, &'a SceneSpec)>,
    spec: &'a DirectorSpec,
}

impl<'a> SceneLookup<'a> {
    pub fn new(spec: &'a DirectorSpec, scene: Option<&str>) -> Self {
        let entry = scene.and_then(|scene| {
            spec.scenes
                .iter()
                .enumerate()
                .find(|(_, entry)| entry.id == scene || entry.class_name.as_deref() == Some(scene))
        });
        let class = entry
            .map(|(_, entry)| entry.class_name.clone().unwrap_or_else(|| entry.id.clone()))
            .or_else(|| scene.map(str::to_owned))
            .or_else(|| spec.engine.main_scene.clone());
        Self { class, entry, spec }
    }

    /// The spec key and value that name this scene's file, if any.
    pub fn spec_file(&self) -> Option<(String, &'a str)> {
        if let Some((index, entry)) = self.entry {
            if let Some(file) = entry.file.as_deref() {
                return Some((format!("scenes[{index}].file"), file));
            }
        }
        let main = self.spec.engine.main_scene.as_deref();
        match (&self.class, main, self.spec.engine.source.as_deref()) {
            (Some(class), Some(main), Some(source)) if class == main => {
                Some(("engine.source".into(), source))
            }
            _ => None,
        }
    }
}

struct SceneTarget {
    class: Option<String>,
    /// Project-relative association for the job (never changes `files`).
    file: Option<String>,
    files: Vec<PathBuf>,
}

fn scene_target(
    ctx: &ProjectContext<'_>,
    scene: Option<&str>,
    file: Option<&str>,
) -> Result<SceneTarget, EngineError> {
    let spec = ctx.spec()?;
    let lookup = SceneLookup::new(spec, scene);
    let chosen = match file {
        Some(file) => Some(project_path(
            ctx.root,
            "file",
            file,
            PathUse::Input {
                allow_artifacts: false,
                extensions: &["py"],
            },
        )?),
        None => lookup
            .spec_file()
            .map(|(key, value)| spec_scene_file(ctx.root, &key, value))
            .transpose()?,
    };
    let files = match &chosen {
        Some(path) => vec![path.clone()],
        None => {
            let sources = python_sources(ctx.root, spec);
            if sources.truncated() {
                return Err(EngineError::BudgetExceeded {
                    budget: Budget::PythonSources,
                    limit: MAX_PYTHON_SOURCES as u64,
                    actual: sources.total as u64,
                });
            }
            sources.files
        }
    };
    let file = match chosen {
        Some(path) => Some(relative_posix(ctx.root, &path)),
        None => indexed_file(ctx, lookup.class.as_deref()),
    };
    Ok(SceneTarget {
        class: lookup.class,
        file,
        files,
    })
}

fn spec_scene_file(root: &Path, key: &str, value: &str) -> Result<PathBuf, EngineError> {
    root.join(value)
        .canonicalize()
        .ok()
        .filter(|path| path.starts_with(root) && path.is_file())
        .ok_or_else(|| EngineError::InvalidSpec {
            reason: format!("{key} {value:?} does not exist in the project"),
            line: None,
            column: None,
        })
}

/// The file of the unique scene named `class` in the newest discover index.
fn indexed_file(ctx: &ProjectContext<'_>, class: Option<&str>) -> Option<String> {
    let class = class?;
    let index = ctx.store.newest_discover().ok()??;
    let mut matches = index.scenes.iter().filter(|scene| scene.name == class);
    let only = matches.next()?;
    matches.next().is_none().then(|| only.file.clone())
}

/// The failure a `diagnose` of a job explains: its error and last log lines.
fn failure_text(ctx: &ProjectContext<'_>, id: Uuid) -> Result<String, EngineError> {
    let job = ctx
        .store
        .get_job(id)
        .map_err(EngineError::internal)?
        .ok_or_else(|| EngineError::job_not_found(id))?;
    let Some(error) = job.error.as_ref() else {
        return Err(EngineError::invalid(
            "job_id",
            format!("job {id} is {}, not failed or cancelled", job.status),
        ));
    };
    let mut text = format!("{}: {}\n", error.code, error.message);
    if let Some(traceback) = error
        .data
        .as_ref()
        .and_then(|data| data.get("traceback"))
        .and_then(Value::as_str)
    {
        text.push_str(traceback);
        text.push('\n');
    }
    let logs = ctx
        .store
        .tail_logs(id, 400)
        .map_err(EngineError::internal)?;
    for record in logs {
        if record.stream != LogStream::Engine {
            let _ = writeln!(text, "{}", record.message);
        }
    }
    Ok(text)
}

/// The last `max` bytes of `text`, starting on a character boundary.
fn keep_tail(text: &str, max: usize) -> &str {
    &text[text.ceil_char_boundary(text.len().saturating_sub(max))..]
}

/// blake3 of the scene file, from the fingerprint when it was hashed there.
pub(crate) fn scene_revision(
    root: &Path,
    scene_file: Option<&str>,
    fingerprint: Option<&cache::Fingerprint>,
) -> Option<String> {
    let file = scene_file?;
    fingerprint
        .and_then(|fingerprint| fingerprint.file_hash(file).map(str::to_owned))
        .or_else(|| cache::file_revision(&root.join(file)).ok())
}

#[cfg(test)]
mod tests;
