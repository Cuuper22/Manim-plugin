use clap::{Args, Parser, Subcommand};
use manim_director_core::{
    default_export_format, default_qa_frames, default_samples, default_scale,
    default_sheet_columns, default_sheet_count, default_tolerance, ExportFormat,
};
use std::{net::IpAddr, path::PathBuf};
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(
    name = "manim-director",
    version,
    about = "Render, check and diagnose Manim scenes; serve the workbench and MCP"
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
    Serve(ServerArgs),
    /// Open the local workbench and serve its API.
    Open(OpenArgs),
    /// Serve MCP over stdio.
    Mcp,
}

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Directory for the new project, relative to --project.
    #[arg(default_value = ".")]
    pub path: PathBuf,
    /// Project name [default: the directory's name].
    #[arg(long)]
    pub name: Option<String>,
    /// Template to start from [default: explainer]; a wrong name lists them all.
    #[arg(long)]
    pub template: Option<String>,
    /// Add this template's scene to the existing project instead.
    #[arg(long)]
    pub scene_template: Option<String>,
    /// Theme for director.yaml [default: midnight].
    #[arg(long)]
    pub theme: Option<String>,
    /// The project's random seed [default: derived from the name].
    #[arg(long, value_parser = clap::value_parser!(u32).range(0..=2_147_483_647))]
    pub seed: Option<u32>,
    /// Write into a non-empty directory, replacing files of the same names without a copy.
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct TargetArgs {
    /// Scene class or director.yaml scene id [default: engine.main_scene].
    #[arg(long)]
    pub scene: Option<String>,
    /// The file that defines the scene, when two files define the class.
    #[arg(long)]
    pub file: Option<PathBuf>,
    /// Render profile [default: render.profile, else preview].
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
    /// Read this job's output instead of the latest render.
    #[arg(long, conflicts_with = "path")]
    pub job: Option<Uuid>,
    /// Read this file instead of the latest render.
    #[arg(long)]
    pub path: Option<PathBuf>,
    /// Use the latest render of this scene [default: engine.main_scene].
    #[arg(long)]
    pub scene: Option<String>,
    /// Use the latest render at this profile.
    #[arg(long)]
    pub profile: Option<String>,
}

#[derive(Debug, Args)]
pub struct FrameArgs {
    /// The moment to grab, in seconds from the start.
    #[arg(long = "at", value_name = "SECONDS")]
    pub at_seconds: f64,
    #[command(flatten)]
    pub source: SourceArgs,
}

#[derive(Debug, Args)]
pub struct ContactSheetArgs {
    #[command(flatten)]
    pub source: SourceArgs,
    /// Frames on the sheet (1 to 24).
    #[arg(long, default_value_t = default_sheet_count())]
    pub count: u8,
    /// Columns of the sheet (1 to 8).
    #[arg(long, default_value_t = default_sheet_columns())]
    pub columns: u8,
}

#[derive(Debug, Args)]
pub struct QaArgs {
    #[command(flatten)]
    pub source: SourceArgs,
    /// Frames to check (1 to 40).
    #[arg(long, default_value_t = default_qa_frames())]
    pub frames: u8,
}

#[derive(Debug, Args)]
#[group(required = true, multiple = false)]
pub struct DiagnoseArgs {
    /// A failed job.
    #[arg(long)]
    pub job: Option<Uuid>,
    /// A traceback or TeX log.
    #[arg(long)]
    pub text: Option<String>,
    /// A file holding a traceback or TeX log.
    #[arg(long)]
    pub text_file: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct ValidateMathArgs {
    /// Consecutive steps in Python syntax (`^` is a power); an equation is checked as lhs - rhs.
    #[arg(required = true, num_args = 2..)]
    pub steps: Vec<String>,
    /// Where to sample a variable [default: -10:10]; LO >= 0 also makes it nonnegative.
    #[arg(long = "range", value_name = "VAR=LO:HI")]
    pub ranges: Vec<String>,
    /// Sample points per pair of steps.
    #[arg(long, default_value_t = default_samples())]
    pub samples: u32,
    /// The largest absolute and relative difference that still counts as equal.
    #[arg(long, default_value_t = default_tolerance())]
    pub tolerance: f64,
    /// Seed for the sample points.
    #[arg(long)]
    pub seed: Option<u64>,
}

#[derive(Debug, Args)]
pub struct CaptionsArgs {
    /// A WebVTT (.vtt) or SRT (.srt) file.
    pub path: PathBuf,
    /// Seconds added to every time, after scaling.
    #[arg(long = "shift", default_value_t = 0.0, allow_negative_numbers = true)]
    pub shift_seconds: f64,
    /// Factor every time is multiplied by.
    #[arg(long, default_value_t = default_scale())]
    pub scale: f64,
    /// Where to write the result; its extension picks the format.
    #[arg(long)]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct IngestArgs {
    /// Files to copy in, from anywhere you can read.
    #[arg(required = true, num_args = 1..)]
    pub paths: Vec<PathBuf>,
    /// Ids, paired with the paths in order.
    #[arg(long = "id")]
    pub ids: Vec<String>,
    /// License recorded for every file in sources/manifest.json.
    #[arg(long)]
    pub license: Option<String>,
    /// Attribution recorded for every file.
    #[arg(long)]
    pub attribution: Option<String>,
    /// Strip scripts from SVG, scale images over 4096 px down, even out audio loudness.
    #[arg(long)]
    pub normalize: bool,
    /// Replace a file of the same name instead of adding a numbered copy.
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct ExportArgs {
    /// zip bundles the project; mp4, webm and gif deliver a render.
    #[arg(long, default_value_t = default_export_format())]
    pub format: ExportFormat,
    #[command(flatten)]
    pub source: SourceArgs,
    /// Path inside the project [default: output/<scene>.<format> or output/<project>.zip].
    #[arg(long)]
    pub output: Option<PathBuf>,
    /// GIF frame rate.
    #[arg(long)]
    pub gif_fps: Option<u32>,
    /// GIF width in pixels.
    #[arg(long)]
    pub gif_width: Option<u32>,
}

#[derive(Debug, Args)]
pub struct EditArgs {
    /// The project file to change.
    pub path: String,
    /// New content for the whole file.
    #[arg(long, conflicts_with_all = ["content_file", "line", "merge_patch", "merge_patch_file"])]
    pub content: Option<String>,
    /// Read the new content from this file.
    #[arg(long, conflicts_with_all = ["content", "line", "merge_patch", "merge_patch_file"])]
    pub content_file: Option<PathBuf>,
    /// Replace these lines (1-based, inclusive; END = START - 1 inserts before START).
    #[arg(long, value_name = "START:END", conflicts_with_all = ["content", "content_file", "merge_patch", "merge_patch_file"])]
    pub line: Option<String>,
    /// The text for --line.
    #[arg(long, conflicts_with = "replacement_file")]
    pub replacement: Option<String>,
    /// Read the text for --line from this file.
    #[arg(long, conflicts_with = "replacement")]
    pub replacement_file: Option<PathBuf>,
    /// A JSON merge patch for director.yaml.
    #[arg(long, value_name = "JSON", conflicts_with_all = ["content", "content_file", "line", "merge_patch_file"])]
    pub merge_patch: Option<String>,
    /// Read the merge patch from this file.
    #[arg(long, conflicts_with_all = ["content", "content_file", "line", "merge_patch"])]
    pub merge_patch_file: Option<PathBuf>,
    /// Refuse unless the file is still at this revision (the hash an earlier edit printed).
    #[arg(long)]
    pub expected_revision: Option<String>,
}

#[derive(Debug, Args)]
pub struct OpenArgs {
    #[command(flatten)]
    pub server: ServerArgs,
    /// Print the sign-in link without opening a browser.
    #[arg(long)]
    pub no_browser: bool,
}

#[derive(Debug, Args)]
pub struct ServerArgs {
    /// Address to bind; only 127.0.0.1 or ::1 without --allow-remote.
    #[arg(long, default_value = "127.0.0.1")]
    pub host: IpAddr,
    /// Port to listen on; 0 picks a free one.
    #[arg(long, default_value_t = 4177)]
    pub port: u16,
    /// Serve the workbench from this directory instead of the built-in one.
    #[arg(long, env = "MANIM_DIRECTOR_WORKBENCH")]
    pub workbench_dir: Option<PathBuf>,
    /// Accept other machines: binds any address and accepts any Host header.
    #[arg(long)]
    pub allow_remote: bool,
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::CommandFactory;

    #[test]
    fn every_argument_says_what_it_does() {
        Cli::command().debug_assert();
        let cli = Cli::command();
        for command in cli.get_subcommands() {
            for argument in command.get_arguments() {
                assert!(
                    argument.get_help().is_some(),
                    "{} {} has no help",
                    command.get_name(),
                    argument.get_id()
                );
            }
        }
    }
}
