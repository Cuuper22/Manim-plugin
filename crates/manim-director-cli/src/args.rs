use clap::{Args, Parser, Subcommand};
use manim_director_core::ExportFormat;
use std::{net::IpAddr, path::PathBuf};
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(
    name = "manim-director",
    version,
    about = "Fast control plane for authored Manim projects"
)]
pub struct Cli {
    #[arg(
        long,
        global = true,
        default_value = ".",
        help = "Project directory or a path inside it"
    )]
    pub project: PathBuf,
    #[arg(long, global = true, help = "Emit machine-readable JSON")]
    pub json: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create a project from a template, or add a scene template to one.
    Init(InitArgs),
    /// Check Python, Manim, LaTeX and the video tools.
    Doctor,
    /// Render one scene to video.
    Render(RenderArgs),
    /// Render one scene's last frame to PNG.
    Still(StillArgs),
    /// Grab one frame of a rendered video.
    Frame(FrameArgs),
    /// Lay evenly spaced frames of a video out as one image.
    ContactSheet(ContactSheetArgs),
    /// Check a render for blank frames, low contrast and safe-area violations.
    Qa(QaArgs),
    /// Explain a failed job or a traceback with file:line findings.
    Diagnose(DiagnoseArgs),
    /// Check that consecutive derivation steps are equal.
    ValidateMath(ValidateMathArgs),
    /// Validate, retime or convert a caption file.
    Captions(CaptionsArgs),
    /// Copy external files into the project with provenance.
    Ingest(IngestArgs),
    /// Bundle the project as a zip, or deliver a render as mp4, webm or gif.
    Export(ExportArgs),
    /// Summarize scenes, profiles, latest renders and recent jobs.
    Inspect,
    /// Atomically replace, line-edit or merge-patch project source.
    Edit(EditArgs),
    /// Serve the REST/SSE API and workbench.
    Serve(ServeArgs),
    /// Open the local workbench and serve its API.
    Open(OpenArgs),
    /// Serve MCP over stdio.
    Mcp,
}

#[derive(Debug, Args)]
pub struct InitArgs {
    #[arg(default_value = ".")]
    pub path: PathBuf,
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long)]
    pub template: Option<String>,
    #[arg(long)]
    pub scene_template: Option<String>,
    #[arg(long)]
    pub theme: Option<String>,
    #[arg(long, value_parser = clap::value_parser!(u32).range(0..=2_147_483_647))]
    pub seed: Option<u32>,
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct TargetArgs {
    #[arg(long)]
    pub scene: Option<String>,
    #[arg(long)]
    pub file: Option<PathBuf>,
    #[arg(long)]
    pub profile: Option<String>,
    #[arg(long, help = "Skip the cache and Manim's partial-movie cache")]
    pub fresh: bool,
}

#[derive(Debug, Args)]
pub struct RenderArgs {
    #[command(flatten)]
    pub target: TargetArgs,
    #[arg(long, help = "Also write one video per Manim section")]
    pub sections: bool,
}

#[derive(Debug, Args)]
pub struct StillArgs {
    #[command(flatten)]
    pub target: TargetArgs,
}

/// Picks the media to read; without `--job`/`--path`, the latest render.
#[derive(Debug, Args)]
pub struct SourceArgs {
    #[arg(long, conflicts_with = "path")]
    pub job: Option<Uuid>,
    #[arg(long)]
    pub path: Option<PathBuf>,
    #[arg(long)]
    pub scene: Option<String>,
    #[arg(long)]
    pub profile: Option<String>,
}

#[derive(Debug, Args)]
pub struct FrameArgs {
    #[arg(long = "at", value_name = "SECONDS")]
    pub at_seconds: f64,
    #[command(flatten)]
    pub source: SourceArgs,
}

#[derive(Debug, Args)]
pub struct ContactSheetArgs {
    #[command(flatten)]
    pub source: SourceArgs,
    #[arg(long, default_value_t = 6)]
    pub count: u8,
    #[arg(long, default_value_t = 3)]
    pub columns: u8,
}

#[derive(Debug, Args)]
pub struct QaArgs {
    #[command(flatten)]
    pub source: SourceArgs,
    #[arg(long, default_value_t = 8)]
    pub frames: u8,
}

#[derive(Debug, Args)]
#[group(required = true, multiple = false)]
pub struct DiagnoseArgs {
    #[arg(long)]
    pub job: Option<Uuid>,
    #[arg(long)]
    pub text: Option<String>,
    #[arg(long)]
    pub text_file: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct ValidateMathArgs {
    #[arg(required = true, num_args = 2..)]
    pub steps: Vec<String>,
    #[arg(long = "range", value_name = "VAR=LO:HI")]
    pub ranges: Vec<String>,
    #[arg(long, default_value_t = 200)]
    pub samples: u32,
    #[arg(long, default_value_t = 1e-9)]
    pub tolerance: f64,
    #[arg(long)]
    pub seed: Option<u64>,
}

#[derive(Debug, Args)]
pub struct CaptionsArgs {
    pub path: PathBuf,
    #[arg(long = "shift", default_value_t = 0.0, allow_negative_numbers = true)]
    pub shift_seconds: f64,
    #[arg(long, default_value_t = 1.0)]
    pub scale: f64,
    #[arg(long)]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct IngestArgs {
    #[arg(required = true, num_args = 1..)]
    pub paths: Vec<PathBuf>,
    /// Ids, paired with the paths in order.
    #[arg(long = "id")]
    pub ids: Vec<String>,
    #[arg(long)]
    pub license: Option<String>,
    #[arg(long)]
    pub attribution: Option<String>,
    #[arg(long)]
    pub normalize: bool,
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct ExportArgs {
    #[arg(long, default_value = "zip")]
    pub format: ExportFormat,
    #[command(flatten)]
    pub source: SourceArgs,
    #[arg(long)]
    pub output: Option<PathBuf>,
    #[arg(long)]
    pub gif_fps: Option<u32>,
    #[arg(long)]
    pub gif_width: Option<u32>,
}

#[derive(Debug, Args)]
pub struct EditArgs {
    pub path: String,
    #[arg(long, conflicts_with_all = ["content_file", "line", "merge_patch", "merge_patch_file"])]
    pub content: Option<String>,
    #[arg(long, conflicts_with_all = ["content", "line", "merge_patch", "merge_patch_file"])]
    pub content_file: Option<PathBuf>,
    #[arg(long, value_name = "START:END", conflicts_with_all = ["content", "content_file", "merge_patch", "merge_patch_file"])]
    pub line: Option<String>,
    #[arg(long, conflicts_with = "replacement_file")]
    pub replacement: Option<String>,
    #[arg(long, conflicts_with = "replacement")]
    pub replacement_file: Option<PathBuf>,
    #[arg(long, value_name = "JSON", conflicts_with_all = ["content", "content_file", "line", "merge_patch_file"])]
    pub merge_patch: Option<String>,
    #[arg(long, conflicts_with_all = ["content", "content_file", "line", "merge_patch"])]
    pub merge_patch_file: Option<PathBuf>,
    #[arg(long)]
    pub expected_revision: Option<String>,
}

#[derive(Debug, Args)]
pub struct ServeArgs {
    #[command(flatten)]
    pub server: ServerArgs,
}

#[derive(Debug, Args)]
pub struct OpenArgs {
    #[command(flatten)]
    pub server: ServerArgs,
    #[arg(long)]
    pub no_browser: bool,
}

#[derive(Debug, Args)]
pub struct ServerArgs {
    #[arg(long, default_value = "127.0.0.1")]
    pub host: IpAddr,
    #[arg(long, default_value_t = 4177)]
    pub port: u16,
    #[arg(long, env = "MANIM_DIRECTOR_WORKBENCH")]
    pub workbench_dir: Option<PathBuf>,
    /// Accept other machines: binds non-loopback addresses and any Host header.
    #[arg(long)]
    pub allow_remote: bool,
}
