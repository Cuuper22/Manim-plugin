mod args;
mod output;
mod serve;

use anyhow::Context;
use args::{Cli, Command, EditArgs, SourceArgs, TargetArgs};
use clap::Parser;
use manim_director_core::{
    find_project, CaptionsParams, ContactSheetParams, DiagnoseParams, DoctorParams, EngineError,
    EngineEvent, ExportParams, FrameParams, IngestParams, IngestSource, InitParams, JobOrigin,
    JobRecord, JobStatus, OperationRequest, Progress, QaParams, RenderParams, SourceRef,
    StillParams, ValidateMathParams,
};
use manim_director_engine::{
    cli_project_path, current_revision, init_project, inspect, run_mcp, shutdown_signal,
    state_db_path, write_source, BridgeConfig, EngineMode, Scheduler, SchedulerConfig, SourceEdit,
    SourceWrite, Store, Submission,
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
    time::{Duration, Instant},
};
use tracing_subscriber::EnvFilter;

const EXIT_JOB_FAILED: u8 = 1;
const EXIT_INVALID_INPUT: u8 = 2;
const EXIT_ENGINE: u8 = 3;
const EXIT_INTERRUPTED: u8 = 130;

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    let machine = cli.json;
    match run(cli).await {
        Ok(code) => code,
        Err(failure) => failure.report(machine),
    }
}

enum Failure {
    Engine(EngineError),
    Other(anyhow::Error),
}

impl From<EngineError> for Failure {
    fn from(error: EngineError) -> Self {
        Self::Engine(error)
    }
}

impl From<anyhow::Error> for Failure {
    fn from(error: anyhow::Error) -> Self {
        Self::Other(error)
    }
}

impl From<std::io::Error> for Failure {
    fn from(error: std::io::Error) -> Self {
        Self::Other(error.into())
    }
}

impl Failure {
    fn report(self, machine: bool) -> ExitCode {
        match self {
            Self::Engine(error) => {
                let body = error.body();
                if machine {
                    output::json(&serde_json::json!({ "error": body }));
                } else {
                    eprintln!("error: {}", error);
                    if let EngineError::Operation(body) = &error {
                        output::failure(body);
                    }
                }
                let code = match body.code.as_str() {
                    "runtime_unavailable" | "engine_lost" | "internal" => EXIT_ENGINE,
                    _ if error.status() < 500 => EXIT_INVALID_INPUT,
                    _ => EXIT_JOB_FAILED,
                };
                ExitCode::from(code)
            }
            Self::Other(error) => {
                eprintln!("error: {error:#}");
                ExitCode::from(EXIT_ENGINE)
            }
        }
    }
}

type Outcome = Result<ExitCode, Failure>;

async fn run(cli: Cli) -> Outcome {
    let cwd = std::env::current_dir()?;
    let machine = cli.json;
    let project: PathBuf = cwd.join(&cli.project).components().collect();
    match cli.command {
        Command::Init(args) => {
            let params = InitParams {
                name: args.name,
                template: args.template,
                scene_template: args.scene_template,
                theme: args.theme,
                seed: args.seed,
                force: args.force,
            };
            let result =
                init_project(&BridgeConfig::default(), &cwd.join(args.path), params).await?;
            output::init(&result, machine);
            Ok(ExitCode::SUCCESS)
        }
        Command::Mcp => {
            fs::create_dir_all(&project)?;
            let root = match find_project(&project) {
                Ok(root) => root,
                Err(_) => project.canonicalize()?,
            };
            let served = run_mcp(start_scheduler(&root, EngineMode::Mcp).await?).await;
            // stdin is read on a thread nothing can cancel, so a server that
            // stopped on a signal exits rather than wait for more input.
            if let Err(error) = served {
                eprintln!("error: {error:#}");
                std::process::exit(EXIT_ENGINE.into());
            }
            std::process::exit(0)
        }
        command => {
            let root = find_project(&project).map_err(EngineError::from)?;
            let paths = CliPaths {
                root: &root,
                cwd: &cwd,
            };
            let request = match command {
                Command::Doctor => OperationRequest::Doctor(DoctorParams {}),
                Command::Render(args) => {
                    let TargetParts {
                        scene,
                        file,
                        profile,
                        fresh,
                    } = paths.target(args.target)?;
                    OperationRequest::Render(RenderParams {
                        scene,
                        file,
                        profile,
                        sections: args.sections,
                        fresh,
                    })
                }
                Command::Still(args) => {
                    let TargetParts {
                        scene,
                        file,
                        profile,
                        fresh,
                    } = paths.target(args.target)?;
                    OperationRequest::Still(StillParams {
                        scene,
                        file,
                        profile,
                        fresh,
                    })
                }
                Command::Frame(args) => {
                    let SourceParts {
                        source,
                        scene,
                        profile,
                    } = paths.source(args.source)?;
                    OperationRequest::Frame(FrameParams {
                        at_seconds: args.at_seconds,
                        source,
                        scene,
                        profile,
                    })
                }
                Command::ContactSheet(args) => {
                    let SourceParts {
                        source,
                        scene,
                        profile,
                    } = paths.source(args.source)?;
                    OperationRequest::ContactSheet(ContactSheetParams {
                        source,
                        scene,
                        profile,
                        count: args.count,
                        columns: args.columns,
                    })
                }
                Command::Qa(args) => {
                    let SourceParts {
                        source,
                        scene,
                        profile,
                    } = paths.source(args.source)?;
                    OperationRequest::Qa(QaParams {
                        source,
                        scene,
                        profile,
                        frames: args.frames,
                    })
                }
                Command::Diagnose(args) => {
                    let text = match args.text_file {
                        Some(path) => Some(
                            fs::read_to_string(cwd.join(&path))
                                .with_context(|| format!("reading {}", path.display()))?,
                        ),
                        None => args.text,
                    };
                    OperationRequest::Diagnose(DiagnoseParams {
                        job_id: args.job,
                        text,
                    })
                }
                Command::ValidateMath(args) => OperationRequest::ValidateMath(ValidateMathParams {
                    steps: args.steps,
                    ranges: parse_ranges(&args.ranges)?,
                    samples: args.samples,
                    tolerance: args.tolerance,
                    seed: args.seed,
                }),
                Command::Captions(args) => OperationRequest::Captions(CaptionsParams {
                    path: paths.relative(&args.path)?,
                    shift_seconds: args.shift_seconds,
                    scale: args.scale,
                    output: args.output.map(|path| paths.relative(&path)).transpose()?,
                }),
                Command::Ingest(args) => {
                    if args.ids.len() > args.paths.len() {
                        return Err(EngineError::invalid("id", "more ids than paths").into());
                    }
                    let sources = args
                        .paths
                        .iter()
                        .enumerate()
                        .map(|(index, path)| IngestSource {
                            path: cwd.join(path).to_string_lossy().into_owned(),
                            id: args.ids.get(index).cloned(),
                            license: args.license.clone(),
                            attribution: args.attribution.clone(),
                        })
                        .collect();
                    OperationRequest::Ingest(IngestParams {
                        sources,
                        normalize: args.normalize,
                        force: args.force,
                    })
                }
                Command::Export(args) => {
                    let SourceParts {
                        source,
                        scene,
                        profile,
                    } = paths.source(args.source)?;
                    OperationRequest::Export(ExportParams {
                        format: args.format,
                        source,
                        scene,
                        profile,
                        output: args.output.map(|path| paths.relative(&path)).transpose()?,
                        gif_fps: args.gif_fps,
                        gif_width: args.gif_width,
                    })
                }
                Command::Inspect => {
                    let scheduler = start_scheduler(&root, EngineMode::Cli).await?;
                    let summary = inspect(&scheduler).await;
                    scheduler.shutdown().await;
                    output::inspect(&summary?, machine);
                    return Ok(ExitCode::SUCCESS);
                }
                Command::Edit(args) => return edit(paths, args, machine).await,
                Command::Serve(args) => {
                    serve::serve(&root, args.server, machine).await?;
                    return Ok(ExitCode::SUCCESS);
                }
                Command::Open(args) => {
                    serve::open(&root, args, machine).await?;
                    return Ok(ExitCode::SUCCESS);
                }
                Command::Init(_) | Command::Mcp => unreachable!("handled above"),
            };
            submit_and_wait(&root, request, machine).await
        }
    }
}

/// Converts CLI path arguments into the project-relative form requests carry.
struct CliPaths<'a> {
    root: &'a Path,
    cwd: &'a Path,
}

struct SourceParts {
    source: Option<SourceRef>,
    scene: Option<String>,
    profile: Option<String>,
}

struct TargetParts {
    scene: Option<String>,
    file: Option<String>,
    profile: Option<String>,
    fresh: bool,
}

impl CliPaths<'_> {
    fn relative(&self, path: &Path) -> Result<String, EngineError> {
        cli_project_path(self.root, self.cwd, path)
    }

    fn target(&self, args: TargetArgs) -> Result<TargetParts, EngineError> {
        Ok(TargetParts {
            scene: args.scene,
            file: args.file.map(|file| self.relative(&file)).transpose()?,
            profile: args.profile,
            fresh: args.fresh,
        })
    }

    fn source(&self, args: SourceArgs) -> Result<SourceParts, EngineError> {
        let source = match (args.job, args.path) {
            (Some(id), _) => Some(SourceRef::JobId(id)),
            (None, Some(path)) => Some(SourceRef::Path(self.relative(&path)?)),
            (None, None) => None,
        };
        Ok(SourceParts {
            source,
            scene: args.scene,
            profile: args.profile,
        })
    }
}

fn parse_ranges(values: &[String]) -> Result<BTreeMap<String, [f64; 2]>, EngineError> {
    values
        .iter()
        .map(|value| {
            let invalid = || EngineError::invalid("range", format!("{value:?} is not VAR=LO:HI"));
            let (name, bounds) = value.split_once('=').ok_or_else(invalid)?;
            let (low, high) = bounds.split_once(':').ok_or_else(invalid)?;
            let low = low.trim().parse().map_err(|_| invalid())?;
            let high = high.trim().parse().map_err(|_| invalid())?;
            Ok((name.trim().to_owned(), [low, high]))
        })
        .collect()
}

async fn start_scheduler(root: &Path, mode: EngineMode) -> anyhow::Result<Scheduler> {
    Scheduler::open(root, SchedulerConfig::new(mode)).await
}

async fn submit_and_wait(root: &Path, request: OperationRequest, machine: bool) -> Outcome {
    let scheduler = start_scheduler(root, EngineMode::Cli).await?;
    let outcome = submit_and_report(&scheduler, request, machine).await;
    scheduler.shutdown().await;
    outcome
}

async fn submit_and_report(
    scheduler: &Scheduler,
    request: OperationRequest,
    machine: bool,
) -> Outcome {
    let submission = scheduler.submit(JobOrigin::Cli, request).await?;
    // A coalesced job belongs to the client that started it.
    let mine = matches!(submission, Submission::Queued(_));
    let job = submission.into_job();
    if job.status.is_terminal() {
        output::job(&job, machine);
        return Ok(exit_code(&job));
    }
    let progress = (!machine).then(|| tokio::spawn(print_progress(scheduler.clone(), job.id)));
    let finished = tokio::select! {
        finished = scheduler.wait(job.id) => Some(finished?),
        _ = shutdown_signal() => None,
    };
    if let Some(task) = progress {
        task.abort();
    }
    let Some(finished) = finished else {
        if !mine {
            let id = job.id;
            let current = scheduler.store().blocking(move |store| store.get_job(id));
            eprintln!("Stopped waiting; job {id} keeps running for the client that started it.");
            output::job(&current.await?.unwrap_or(job), machine);
            return Ok(ExitCode::from(EXIT_INTERRUPTED));
        }
        scheduler.cancel(job.id).await?;
        let cancelled = tokio::select! {
            finished = scheduler.wait(job.id) => finished?,
            _ = shutdown_signal() => std::process::exit(EXIT_INTERRUPTED.into()),
        };
        output::job(&cancelled, machine);
        return Ok(ExitCode::from(EXIT_INTERRUPTED));
    };
    output::job(&finished, machine);
    Ok(exit_code(&finished))
}

fn exit_code(job: &JobRecord) -> ExitCode {
    match (
        job.status,
        job.error.as_ref().map(|error| error.code.as_str()),
    ) {
        (JobStatus::Succeeded, _) => ExitCode::SUCCESS,
        (_, Some("runtime_unavailable" | "engine_lost" | "internal")) => {
            ExitCode::from(EXIT_ENGINE)
        }
        _ => ExitCode::from(EXIT_JOB_FAILED),
    }
}

/// One stderr line per change of phase or message; within one, at most one
/// per second.
async fn print_progress(scheduler: Scheduler, id: uuid::Uuid) {
    let mut events = scheduler.subscribe();
    let mut last: Option<(Progress, Instant)> = None;
    while let Ok(event) = events.recv().await {
        let EngineEvent::Progress { job_id, progress } = event else {
            continue;
        };
        if job_id != id {
            continue;
        }
        let repeat = last.as_ref().is_some_and(|(shown, at)| {
            (shown.phase, &shown.message) == (progress.phase, &progress.message)
                && at.elapsed() < Duration::from_secs(1)
        });
        if !repeat {
            eprintln!("{}", progress_line(&progress));
            last = Some((progress, Instant::now()));
        }
    }
}

/// `animate 3 (4.1 s)`, `extract 2/8`, `starting: waiting for the runtime`.
fn progress_line(progress: &Progress) -> String {
    let mut line = progress.phase.to_string();
    match progress.total {
        Some(total) => line += &format!(" {}/{total}", progress.current),
        None if progress.current > 0 => line += &format!(" {}", progress.current),
        None => {}
    }
    if let Some(seconds) = progress.scene_seconds {
        line += &format!(" ({seconds:.1} s)");
    }
    if let Some(message) = &progress.message {
        line += &format!(": {message}");
    }
    line
}

async fn edit(paths: CliPaths<'_>, args: EditArgs, machine: bool) -> Outcome {
    let read = |path: Option<PathBuf>| -> anyhow::Result<Option<String>> {
        path.map(|path| {
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))
        })
        .transpose()
    };
    let content = args.content.or(read(args.content_file)?);
    let replacement = args.replacement.or(read(args.replacement_file)?);
    let merge_patch = args.merge_patch.or(read(args.merge_patch_file)?);
    let edit = match (content, args.line, merge_patch) {
        (Some(content), _, _) => SourceEdit::ReplaceAll { content },
        (None, Some(range), _) => {
            let invalid = || EngineError::invalid("line", "expected START:END");
            let (start, end) = range.split_once(':').ok_or_else(invalid)?;
            SourceEdit::ReplaceLines {
                start_line: start.trim().parse().map_err(|_| invalid())?,
                end_line: end.trim().parse().map_err(|_| invalid())?,
                replacement: replacement.unwrap_or_default(),
            }
        }
        (None, None, Some(patch)) => SourceEdit::MergePatch {
            patch: serde_json::from_str(&patch)
                .map_err(|error| EngineError::invalid("merge_patch", error.to_string()))?,
        },
        (None, None, None) => {
            return Err(
                EngineError::invalid("edit", "pass --content, --line or --merge-patch").into(),
            )
        }
    };
    let root = paths.root.to_path_buf();
    let path = paths.relative(Path::new(&args.path))?;
    let expected_revision = args.expected_revision;
    let result = tokio::task::spawn_blocking(move || {
        // Without --expected-revision the edit applies to whatever is on disk now.
        let expected_revision = match expected_revision {
            Some(revision) => Some(revision),
            None => {
                let current = current_revision(&root, &path)?;
                if current.is_none() && !matches!(edit, SourceEdit::ReplaceAll { .. }) {
                    return Err(EngineError::NotFound {
                        resource: manim_director_core::Resource::File,
                        key: path,
                    });
                }
                current
            }
        };
        let index = match state_db_path(&root).is_file() {
            true => Store::open(state_db_path(&root))
                .and_then(|store| store.newest_discover())
                .ok()
                .flatten(),
            false => None,
        };
        let write = SourceWrite {
            path,
            expected_revision,
            edit,
        };
        write_source(
            &root,
            &BridgeConfig::default().python,
            &write,
            index.as_ref(),
        )
    })
    .await
    .map_err(EngineError::internal)??;
    if machine {
        output::json(&result);
    } else {
        println!("{} @ {}", result.path, result.revision);
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use manim_director_core::{ProgressPhase::*, Timestamp};

    #[test]
    fn progress_lines_read_as_phase_count_and_message() {
        let line = |phase, current, total, scene_seconds, message: Option<&str>| {
            progress_line(&Progress {
                phase,
                current,
                total,
                scene_seconds,
                message: message.map(str::to_owned),
                updated_at: Timestamp::now(),
            })
        };
        assert_eq!(
            line(Starting, 0, None, None, Some("waiting for the runtime")),
            "starting: waiting for the runtime"
        );
        assert_eq!(
            line(Animate, 3, None, Some(4.07), None),
            "animate 3 (4.1 s)"
        );
        assert_eq!(line(Extract, 2, Some(8), None, None), "extract 2/8");
        assert_eq!(line(Validate, 0, None, None, None), "validate");
    }
}
