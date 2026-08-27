use anyhow::{anyhow, bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use manim_director_core::{
    find_project, JobRecord, JobRequest, JobStatus, Operation, ProjectInventory,
};
use manim_director_engine::{
    apply_source_mutation, run_mcp, scaffold_params, scaffold_project, serve, Scheduler,
    SchedulerConfig, ServeConfig, SourceMutation, Store,
};
use serde_json::{json, Map, Value};
use std::{
    fs,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, Stdio},
    sync::Arc,
    time::Duration,
};
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(
    name = "manim-director",
    version,
    about = "Fast control plane for authored Manim projects"
)]
struct Cli {
    #[arg(
        long,
        global = true,
        default_value = ".",
        help = "Project directory or a path inside it"
    )]
    project: PathBuf,
    #[arg(long, global = true, help = "Emit machine-readable JSON")]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create a complete project skeleton.
    Init(InitArgs),
    /// Open the local workbench and serve its API.
    Open(OpenArgs),
    /// Inspect source, assets, outputs, and the parsed spec.
    Inspect(InspectArgs),
    /// Atomically replace, line-edit, or merge-patch project source.
    Edit(EditArgs),
    /// Ingest notes, data, code, documents, and media metadata.
    Ingest(IngestArgs),
    /// Check Python, Manim, renderers, fonts, and codecs.
    Doctor(WaitArgs),
    /// Queue a full render.
    Render(RenderArgs),
    /// Queue a low-latency preview.
    Preview(PreviewArgs),
    /// Run visual, mathematical, caption, and artifact QA.
    Qa(QaArgs),
    /// Diagnose a scene or failed job.
    Debug(DebugArgs),
    /// Package source and selected deliverables.
    Export(ExportArgs),
    /// Serve the REST/SSE API and workbench.
    Serve(ServeArgs),
    /// Serve the compact MCP protocol over stdio.
    Mcp,
}

#[derive(Debug, Args)]
struct InitArgs {
    #[arg(default_value = ".")]
    path: PathBuf,
    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    force: bool,
    #[arg(long, value_parser = clap::value_parser!(u32).range(0..=2_147_483_647))]
    seed: Option<u32>,
}

#[derive(Debug, Args)]
struct OpenArgs {
    #[command(flatten)]
    server: ServerArgs,
    #[arg(long)]
    no_browser: bool,
}

#[derive(Debug, Args)]
struct ServeArgs {
    #[command(flatten)]
    server: ServerArgs,
}

#[derive(Debug, Args)]
struct ServerArgs {
    #[arg(long, default_value = "127.0.0.1")]
    host: IpAddr,
    #[arg(long, default_value_t = 4177)]
    port: u16,
    #[arg(long, env = "MANIM_DIRECTOR_WORKBENCH")]
    workbench_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct InspectArgs {
    #[arg(
        long,
        help = "Also ask the Python runtime to discover scenes and capabilities"
    )]
    deep: bool,
}

#[derive(Debug, Args)]
struct WaitArgs {
    #[arg(long = "set", value_name = "KEY=JSON")]
    values: Vec<String>,
}

#[derive(Debug, Args)]
struct EditArgs {
    path: String,
    #[arg(long, conflicts_with_all = ["content_file", "line", "merge_patch", "merge_patch_file"])]
    content: Option<String>,
    #[arg(long, conflicts_with_all = ["content", "line", "merge_patch", "merge_patch_file"])]
    content_file: Option<PathBuf>,
    #[arg(long, value_name = "START:END", conflicts_with_all = ["content", "content_file", "merge_patch", "merge_patch_file"])]
    line: Option<String>,
    #[arg(long, conflicts_with = "replacement_file")]
    replacement: Option<String>,
    #[arg(long, conflicts_with = "replacement")]
    replacement_file: Option<PathBuf>,
    #[arg(long, value_name = "JSON", conflicts_with_all = ["content", "content_file", "line", "merge_patch_file"])]
    merge_patch: Option<String>,
    #[arg(long, conflicts_with_all = ["content", "content_file", "line", "merge_patch"])]
    merge_patch_file: Option<PathBuf>,
    #[arg(long)]
    expected_revision: Option<String>,
}

#[derive(Debug, Args)]
struct IngestArgs {
    #[arg(required = true, num_args = 1..)]
    paths: Vec<PathBuf>,
    #[arg(long = "set", value_name = "KEY=JSON")]
    values: Vec<String>,
}

#[derive(Debug, Args)]
struct RenderArgs {
    #[command(flatten)]
    target: TargetArgs,
    #[arg(long)]
    renderer: Option<String>,
    #[arg(long)]
    transparent: bool,
}

#[derive(Debug, Args)]
struct PreviewArgs {
    #[command(flatten)]
    target: TargetArgs,
    #[arg(long, help = "Produce representative keyframes in addition to video")]
    contact_sheet: bool,
}

#[derive(Debug, Args)]
struct TargetArgs {
    #[arg(long)]
    scene: Option<String>,
    #[arg(long)]
    profile: Option<String>,
    #[arg(long)]
    section: Option<String>,
    #[arg(long = "set", value_name = "KEY=JSON")]
    values: Vec<String>,
}

#[derive(Debug, Args)]
struct QaArgs {
    #[arg(long)]
    scene: Option<String>,
    #[arg(long)]
    artifact: Option<PathBuf>,
    #[arg(long)]
    job_id: Option<Uuid>,
    #[arg(long = "set", value_name = "KEY=JSON")]
    values: Vec<String>,
}

#[derive(Debug, Args)]
struct DebugArgs {
    #[arg(long)]
    scene: Option<String>,
    #[arg(long)]
    job_id: Option<Uuid>,
    #[arg(long = "set", value_name = "KEY=JSON")]
    values: Vec<String>,
}

#[derive(Debug, Args)]
struct ExportArgs {
    #[arg(long, default_value = "zip")]
    format: String,
    #[arg(long)]
    output: Option<PathBuf>,
    #[arg(long)]
    job_id: Option<Uuid>,
    #[arg(long = "set", value_name = "KEY=JSON")]
    values: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    match cli.command {
        Command::Init(args) => init(args, cli.json).await,
        Command::Mcp => {
            let candidate = if cli.project.is_absolute() {
                cli.project
            } else {
                std::env::current_dir()?.join(cli.project)
            };
            fs::create_dir_all(&candidate)?;
            let root = find_project(&candidate).unwrap_or(candidate.canonicalize()?);
            let scheduler = scheduler(&root)?;
            run_mcp(root, scheduler).await
        }
        command => {
            let root = find_project(&cli.project).map_err(|error| anyhow!(error))?;
            match command {
                Command::Open(args) => open(root, args).await,
                Command::Inspect(args) => inspect(root, args, cli.json).await,
                Command::Edit(args) => edit(root, args, cli.json).await,
                Command::Ingest(args) => {
                    let mut params = object_values(args.values)?;
                    params.insert(
                        "sources".into(),
                        json!(args
                            .paths
                            .iter()
                            .map(|path| path.to_string_lossy().into_owned())
                            .collect::<Vec<_>>()),
                    );
                    execute(root, Operation::Ingest, Value::Object(params), cli.json).await
                }
                Command::Doctor(args) => {
                    execute(root, Operation::Doctor, values(args.values)?, cli.json).await
                }
                Command::Render(args) => {
                    let mut params = target_params(args.target)?;
                    insert_some(&mut params, "renderer", args.renderer);
                    if args.transparent {
                        params.insert("transparent".into(), Value::Bool(true));
                    }
                    execute(root, Operation::Render, Value::Object(params), cli.json).await
                }
                Command::Preview(args) => {
                    let mut params = target_params(args.target)?;
                    if args.contact_sheet {
                        params.insert("contact_sheet".into(), Value::Bool(true));
                    }
                    execute(root, Operation::Preview, Value::Object(params), cli.json).await
                }
                Command::Qa(args) => {
                    let mut params = object_values(args.values)?;
                    insert_some(&mut params, "scene", args.scene);
                    if let Some(path) = args.artifact {
                        params.insert(
                            "source".into(),
                            Value::String(path.to_string_lossy().into_owned()),
                        );
                    }
                    if let Some(id) = args.job_id {
                        params.insert("job_id".into(), Value::String(id.to_string()));
                    }
                    execute(root, Operation::Qa, Value::Object(params), cli.json).await
                }
                Command::Debug(args) => {
                    let mut params = object_values(args.values)?;
                    insert_some(&mut params, "scene", args.scene);
                    if let Some(id) = args.job_id {
                        params.insert("job_id".into(), Value::String(id.to_string()));
                    }
                    execute(root, Operation::Debug, Value::Object(params), cli.json).await
                }
                Command::Export(args) => {
                    let mut params = object_values(args.values)?;
                    params.insert("format".into(), Value::String(args.format));
                    if let Some(path) = args.output {
                        params.insert(
                            "output".into(),
                            Value::String(path.to_string_lossy().into_owned()),
                        );
                    }
                    if let Some(id) = args.job_id {
                        params.insert("job_id".into(), Value::String(id.to_string()));
                    }
                    execute(root, Operation::Export, Value::Object(params), cli.json).await
                }
                Command::Serve(args) => serve_command(root, args.server).await,
                Command::Mcp | Command::Init(_) => unreachable!(),
            }
        }
    }
}

async fn init(args: InitArgs, machine: bool) -> Result<()> {
    let root = if args.path.is_absolute() {
        args.path
    } else {
        std::env::current_dir()?.join(args.path)
    };
    let name = args.name.unwrap_or_else(|| {
        root.file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("manim-project")
            .to_owned()
    });
    let params = scaffold_params(&root, &name, args.force, args.seed)?;
    let result = scaffold_project(&root, params).await?;
    print_value(&result, machine);
    Ok(())
}

async fn inspect(root: PathBuf, args: InspectArgs, machine: bool) -> Result<()> {
    let inventory = ProjectInventory::scan(&root).map_err(|error| anyhow!(error))?;
    let local = json!({
        "project_root": inventory.root,
        "spec": inventory.spec,
        "source_files": inventory.source_files,
        "asset_files": inventory.asset_files,
        "output_files": inventory.output_files,
    });
    if !args.deep {
        print_value(&local, machine);
        return Ok(());
    }
    let scheduler = scheduler(&root)?;
    let job = scheduler
        .submit(
            &root,
            JobRequest {
                operation: Operation::Inspect,
                params: json!({}),
                priority: 0,
            },
        )
        .await?;
    let job = scheduler.wait(job.id, Duration::from_millis(100)).await?;
    print_value(&json!({"local":local,"runtime":job}), machine);
    ensure_success(&job)
}

async fn edit(root: PathBuf, args: EditArgs, machine: bool) -> Result<()> {
    let content = match (args.content, args.content_file) {
        (Some(content), None) => Some(content),
        (None, Some(path)) => Some(fs::read_to_string(path)?),
        (None, None) => None,
        _ => unreachable!(),
    };
    let replacement = match (args.replacement, args.replacement_file) {
        (Some(content), None) => Some(content),
        (None, Some(path)) => Some(fs::read_to_string(path)?),
        (None, None) => None,
        _ => unreachable!(),
    };
    let (start_line, end_line) = if let Some(range) = args.line {
        let (start, end) = range
            .split_once(':')
            .ok_or_else(|| anyhow!("--line requires START:END"))?;
        (
            Some(start.parse().context("invalid start line")?),
            Some(end.parse().context("invalid end line")?),
        )
    } else {
        (None, None)
    };
    let merge_source = match (args.merge_patch, args.merge_patch_file) {
        (Some(value), None) => Some(value),
        (None, Some(path)) => Some(fs::read_to_string(path)?),
        (None, None) => None,
        _ => unreachable!(),
    };
    let merge_patch = merge_source
        .map(|value| serde_json::from_str(&value).context("invalid JSON merge patch"))
        .transpose()?;
    let result = apply_source_mutation(
        &root,
        SourceMutation {
            path: args.path,
            content,
            start_line,
            end_line,
            replacement,
            merge_patch,
            expected_revision: args.expected_revision,
        },
    )
    .await?;
    print_value(&serde_json::to_value(result)?, machine);
    Ok(())
}

async fn execute(root: PathBuf, operation: Operation, params: Value, machine: bool) -> Result<()> {
    let scheduler = scheduler(&root)?;
    let job = scheduler
        .submit(
            &root,
            JobRequest {
                operation,
                params,
                priority: 0,
            },
        )
        .await?;
    if job.cached {
        print_job(&job, machine);
        return ensure_success_or_active(&job);
    }
    let finished = scheduler.wait(job.id, Duration::from_millis(150)).await?;
    print_job(&finished, machine);
    ensure_success(&finished)
}

async fn open(root: PathBuf, args: OpenArgs) -> Result<()> {
    let url = format!(
        "http://{}:{}",
        display_host(args.server.host),
        args.server.port
    );
    if !args.no_browser {
        let url_clone = url.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(180)).await;
            if let Err(error) = launch_browser(&url_clone) {
                tracing::warn!(%error, "could not open browser");
            }
        });
    }
    eprintln!("{url}");
    serve_command(root, args.server).await
}

async fn serve_command(root: PathBuf, args: ServerArgs) -> Result<()> {
    let workbench_dir = resolve_workbench(args.workbench_dir);
    let scheduler = scheduler(&root)?;
    serve(
        ServeConfig {
            address: SocketAddr::new(args.host, args.port),
            project_root: root,
            workbench_dir,
        },
        scheduler,
    )
    .await
}

fn scheduler(root: &Path) -> Result<Scheduler> {
    let store = Arc::new(Store::open(root.join(".manim-director/state.db"))?);
    Ok(Scheduler::start(store, SchedulerConfig::default()))
}

fn target_params(args: TargetArgs) -> Result<Map<String, Value>> {
    let mut params = object_values(args.values)?;
    insert_some(&mut params, "scene", args.scene);
    insert_some(&mut params, "profile", args.profile);
    insert_some(&mut params, "section", args.section);
    Ok(params)
}

fn values(values: Vec<String>) -> Result<Value> {
    Ok(Value::Object(object_values(values)?))
}

fn object_values(values: Vec<String>) -> Result<Map<String, Value>> {
    let mut object = Map::new();
    for item in values {
        let (key, raw) = item
            .split_once('=')
            .ok_or_else(|| anyhow!("--set requires KEY=JSON, got {item}"))?;
        if key.is_empty() {
            bail!("--set key cannot be empty");
        }
        let value = serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_owned()));
        object.insert(key.to_owned(), value);
    }
    Ok(object)
}

fn insert_some(object: &mut Map<String, Value>, key: &str, value: Option<String>) {
    if let Some(value) = value {
        object.insert(key.into(), Value::String(value));
    }
}

fn print_job(job: &JobRecord, machine: bool) {
    if machine {
        print_value(&serde_json::to_value(job).unwrap(), true);
    } else if let Some(result) = &job.result {
        println!(
            "{} {} {}\n{}",
            job.status,
            job.operation,
            job.id,
            serde_json::to_string_pretty(result).unwrap()
        );
    } else if let Some(error) = &job.error {
        println!(
            "{} {} {}: {}",
            job.status, job.operation, job.id, error.message
        );
    } else {
        println!("{} {} {}", job.status, job.operation, job.id);
    }
}

fn print_value(value: &Value, machine: bool) {
    if machine {
        println!("{}", serde_json::to_string(value).unwrap());
    } else {
        println!("{}", serde_json::to_string_pretty(value).unwrap());
    }
}

fn ensure_success(job: &JobRecord) -> Result<()> {
    match job.status {
        JobStatus::Succeeded => Ok(()),
        JobStatus::Failed | JobStatus::Cancelled => Err(anyhow!(job
            .error
            .as_ref()
            .map(|v| v.message.clone())
            .unwrap_or_else(|| "job failed".into()))),
        _ => Err(anyhow!("job did not reach a terminal state")),
    }
}

fn ensure_success_or_active(job: &JobRecord) -> Result<()> {
    if matches!(job.status, JobStatus::Failed | JobStatus::Cancelled) {
        ensure_success(job)
    } else {
        Ok(())
    }
}

fn resolve_workbench(explicit: Option<PathBuf>) -> Option<PathBuf> {
    explicit
        .or_else(|| std::env::var_os("MANIM_DIRECTOR_WORKBENCH").map(PathBuf::from))
        .or_else(|| {
            let development =
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../workbench/dist");
            development.is_dir().then_some(development)
        })
}

fn launch_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut c = ProcessCommand::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut c = ProcessCommand::new("open");
        c.arg(url);
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut c = ProcessCommand::new("xdg-open");
        c.arg(url);
        c
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

fn display_host(host: IpAddr) -> String {
    if host.is_unspecified() {
        "127.0.0.1".into()
    } else {
        host.to_string()
    }
}
