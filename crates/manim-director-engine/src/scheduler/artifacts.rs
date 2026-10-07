//! The artifact contract (OPS §1.1): where a job's files may live, what each
//! kind must look like, and the probe that measures media.

use crate::{
    confine::{confine, state_dir, Confinement},
    process::wait_bounded,
};
use manim_director_core::{
    files, Artifact, ArtifactKind, ErrorBody, ExportTask, MediaExportFormat, MediaFormat,
    MediaInfo, OperationResult, Task, Timeline, ARTIFACTS_DIR,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs, io,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};
use uuid::Uuid;

const PROBE_TIMEOUT: Duration = Duration::from_secs(15);
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// The artifact's file if it still exists inside the project.
pub fn existing(root: &Path, artifact: &Artifact) -> Option<PathBuf> {
    confine(root, &artifact.path).ok()
}

/// A stored result is reusable only while every artifact is on disk with its
/// recorded size.
pub fn intact(root: &Path, result: &OperationResult) -> bool {
    result.artifacts().iter().all(|artifact| {
        existing(root, artifact)
            .and_then(|path| path.metadata().ok())
            .is_some_and(|metadata| metadata.len() == artifact.bytes)
    })
}

/// `<root>/.manim-director/artifacts/<job>`, the one place a job's files live.
pub fn job_dir(root: &Path, id: Uuid) -> PathBuf {
    root.join(ARTIFACTS_DIR).join(id.to_string())
}

/// Creates a job's out_dir after checking that the state directories are
/// real directories, never symlinks.
pub fn create_out_dir(root: &Path, out_dir: &Path) -> io::Result<()> {
    ensure_state_dirs(root, true)?;
    fs::create_dir_all(out_dir)
}

/// Deletes a job's out_dir without following symlinks. The directory comes
/// from the id, never from a stored task: a database copied along with its
/// project still names the original project's directories.
pub fn remove_out_dir(root: &Path, id: Uuid) -> io::Result<()> {
    ensure_state_dirs(root, false)?;
    let out_dir = job_dir(root, id);
    match fs::symlink_metadata(&out_dir) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(out_dir),
        Ok(_) => fs::remove_file(out_dir),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn ensure_state_dirs(root: &Path, create: bool) -> io::Result<()> {
    state_dir(root, "artifacts", create).map(drop)
}

/// What a job's artifacts are checked against.
pub struct Expectations<'a> {
    pub root: &'a Path,
    pub task: &'a Task,
    /// The probed input of frame, qa and media export jobs.
    pub source: Option<&'a MediaInfo>,
    pub budget_bytes: u64,
}

#[derive(Debug, Clone, Copy)]
enum Check {
    Location,
    Missing,
    Empty,
    Signature,
    Probe,
    Contract,
    Parse,
}

impl Check {
    fn as_str(self) -> &'static str {
        match self {
            Self::Location => "location",
            Self::Missing => "missing",
            Self::Empty => "empty",
            Self::Signature => "signature",
            Self::Probe => "probe",
            Self::Contract => "contract",
            Self::Parse => "parse",
        }
    }
}

struct Mismatch {
    field: &'static str,
    expected: Value,
    actual: Value,
}

fn invalid(path: Option<&str>, check: Check, detail: &str, mismatches: Vec<Mismatch>) -> ErrorBody {
    let (expected, actual): (BTreeMap<_, _>, BTreeMap<_, _>) = mismatches
        .iter()
        .map(|m| ((m.field, m.expected.clone()), (m.field, m.actual.clone())))
        .unzip();
    let listed: Vec<_> = mismatches
        .iter()
        .map(|m| json!({"field": m.field, "expected": m.expected, "actual": m.actual}))
        .collect();
    let subject = path.map_or_else(
        || "The artifacts".to_owned(),
        |path| format!("Artifact {path}"),
    );
    ErrorBody::new(
        "artifact_invalid",
        format!("{subject} failed the {} check: {detail}.", check.as_str()),
        Some(json!({
            "path": path,
            "check": check.as_str(),
            "expected": (!expected.is_empty()).then_some(expected),
            "actual": (!actual.is_empty()).then_some(actual),
            "mismatches": (!listed.is_empty()).then_some(listed),
        })),
    )
}

/// Validates every reported artifact and fills in its (engine) `bytes` and
/// `media`. The first violation fails the job.
pub fn validate(expect: &Expectations<'_>, result: &mut OperationResult) -> Result<(), ErrorBody> {
    let root = expect.root;
    let allowed = allowed_kinds(expect.task);
    let location = Location::of(expect.task);
    let transcode = match result {
        OperationResult::Export(export) => Transcode {
            transcoded: export.transcoded,
            effective_fps: export.effective_fps,
        },
        _ => Transcode::default(),
    };
    if matches!(expect.task, Task::Render(_)) {
        let videos = result
            .artifacts()
            .iter()
            .filter(|artifact| artifact.kind == ArtifactKind::Video)
            .count();
        if videos != 1 {
            return Err(invalid(
                None,
                Check::Contract,
                &format!("a render must produce exactly one video, not {videos}"),
                vec![],
            ));
        }
    }
    let mut total = 0_u64;
    for artifact in result.artifacts_mut() {
        if !allowed.contains(&artifact.kind) {
            return Err(invalid(
                Some(&artifact.path),
                Check::Contract,
                &format!(
                    "{} does not produce {:?} artifacts",
                    expect.task.operation(),
                    artifact.kind
                ),
                vec![],
            ));
        }
        let path = confine(root, &artifact.path).map_err(|reason| match reason {
            Confinement::Missing | Confinement::NotFile => {
                invalid(Some(&artifact.path), Check::Missing, "no such file", vec![])
            }
            Confinement::Lexical | Confinement::Outside => invalid(
                Some(&artifact.path),
                Check::Location,
                "it is not a file inside the project",
                vec![],
            ),
        })?;
        if !location.admits(root, &path) {
            return Err(invalid(
                Some(&artifact.path),
                Check::Location,
                "it is outside the job's output location",
                vec![],
            ));
        }
        let bytes = fs::metadata(&path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        if bytes == 0 && artifact.kind != ArtifactKind::File {
            return Err(invalid(
                Some(&artifact.path),
                Check::Empty,
                "the file is empty",
                vec![],
            ));
        }
        artifact.media = inspect(&path, artifact)?;
        artifact.bytes = bytes;
        total = total.saturating_add(bytes);
        let mismatches = contract(expect, artifact, transcode);
        if !mismatches.is_empty() {
            return Err(invalid(
                Some(&artifact.path),
                Check::Contract,
                "it does not match the requested output",
                mismatches,
            ));
        }
    }
    if total > expect.budget_bytes {
        return Err(ErrorBody::new(
            "budget_exceeded",
            format!(
                "The job's artifacts total {total} bytes, over the {} byte budget.",
                expect.budget_bytes
            ),
            Some(json!({"budget": "output_bytes", "limit": expect.budget_bytes, "actual": total})),
        ));
    }
    Ok(())
}

fn allowed_kinds(task: &Task) -> &'static [ArtifactKind] {
    use ArtifactKind::*;
    match task {
        Task::Init(_) | Task::Ingest(_) => &[File],
        Task::Render(_) => &[Video, Section, Captions, Timeline],
        Task::Still(_) | Task::Frame(_) | Task::Qa(_) => &[Image],
        Task::ContactSheet(_) => &[ContactSheet],
        Task::Captions(_) => &[Captions],
        Task::Export(ExportTask::Zip(_)) => &[Archive],
        Task::Export(ExportTask::Media(_)) => &[Video],
        Task::Discover(_) | Task::Doctor(_) | Task::Diagnose(_) | Task::ValidateMath(_) => &[],
    }
}

enum Location {
    Within(PathBuf),
    Exactly(PathBuf),
    /// Inside the project, outside `.manim-director/`.
    ProjectFiles,
    Nowhere,
}

impl Location {
    fn of(task: &Task) -> Self {
        let canonical = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        match task {
            Task::Export(ExportTask::Zip(zip)) => Self::Exactly(canonical(&zip.output)),
            Task::Export(ExportTask::Media(media)) => Self::Exactly(canonical(&media.output)),
            Task::Captions(captions) => captions
                .output
                .as_deref()
                .map_or(Self::Nowhere, |output| Self::Exactly(canonical(output))),
            Task::Init(_) | Task::Ingest(_) => Self::ProjectFiles,
            _ => task
                .out_dir()
                .map_or(Self::Nowhere, |out_dir| Self::Within(canonical(out_dir))),
        }
    }

    fn admits(&self, root: &Path, path: &Path) -> bool {
        match self {
            Self::Within(dir) => path.starts_with(dir),
            Self::Exactly(expected) => path == expected,
            Self::ProjectFiles => {
                path.starts_with(root) && !path.starts_with(root.join(".manim-director"))
            }
            Self::Nowhere => false,
        }
    }
}

/// Kind-specific checks; returns the probe for visual kinds.
fn inspect(path: &Path, artifact: &Artifact) -> Result<Option<MediaInfo>, ErrorBody> {
    let fail = |check: Check, detail: &str| invalid(Some(&artifact.path), check, detail, vec![]);
    match artifact.kind {
        ArtifactKind::Video | ArtifactKind::Section => {
            if !files::has_extension(path, files::VIDEO) {
                return Err(fail(
                    Check::Contract,
                    "a video must be mp4, mov, webm or gif",
                ));
            }
            probe_media(path)
                .map(Some)
                .map_err(|detail| fail(Check::Probe, &detail))
        }
        ArtifactKind::Image | ArtifactKind::ContactSheet => {
            if !starts_with(path, PNG_SIGNATURE) {
                return Err(fail(Check::Signature, "not a PNG file"));
            }
            probe_media(path)
                .map(Some)
                .map_err(|detail| fail(Check::Probe, &detail))
        }
        ArtifactKind::Archive => {
            if starts_with(path, b"PK") {
                Ok(None)
            } else {
                Err(fail(Check::Signature, "not a zip archive"))
            }
        }
        ArtifactKind::Captions => match fs::read(path).map(String::from_utf8) {
            Ok(Ok(_)) => Ok(None),
            _ => Err(fail(Check::Parse, "captions must be UTF-8 text")),
        },
        ArtifactKind::Timeline => {
            let timeline = fs::read(path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Timeline>(&bytes).ok());
            match timeline {
                Some(timeline) if timeline.version == 1 => Ok(None),
                _ => Err(fail(Check::Parse, "not a version 1 beat timeline")),
            }
        }
        ArtifactKind::File => Ok(None),
    }
}

fn starts_with(path: &Path, signature: &[u8]) -> bool {
    let mut head = vec![0_u8; signature.len()];
    fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut head))
        .is_ok()
        && head == signature
}

/// What a media export reports about its output.
#[derive(Debug, Clone, Copy, Default)]
struct Transcode {
    transcoded: bool,
    effective_fps: Option<f64>,
}

/// Per-operation media contract for one artifact.
fn contract(expect: &Expectations<'_>, artifact: &Artifact, transcode: Transcode) -> Vec<Mismatch> {
    let Some(media) = &artifact.media else {
        return Vec::new();
    };
    let mut mismatches = Vec::new();
    let mut size = |width: u32, height: u32| {
        if media.width != width {
            mismatches.push(Mismatch {
                field: "width",
                expected: json!(width),
                actual: json!(media.width),
            });
        }
        if media.height != height {
            mismatches.push(Mismatch {
                field: "height",
                expected: json!(height),
                actual: json!(media.height),
            });
        }
    };
    match (expect.task, artifact.kind) {
        (Task::Render(render), ArtifactKind::Video) => {
            let settings = &render.settings;
            size(settings.width, settings.height);
            check_container(&mut mismatches, media, settings.format.as_str());
            let fps = match settings.format {
                MediaFormat::Gif => gif_rate(settings.fps, media.fps),
                _ => f64::from(settings.fps),
            };
            check_fps(&mut mismatches, media, fps);
            check_alpha(&mut mismatches, media, settings.transparent);
        }
        (Task::Render(render), ArtifactKind::Section) => {
            size(render.settings.width, render.settings.height);
        }
        (Task::Still(still), ArtifactKind::Image) => {
            size(still.settings.width, still.settings.height);
            check_alpha(&mut mismatches, media, still.settings.transparent);
        }
        (Task::Frame(_) | Task::Qa(_), ArtifactKind::Image) => {
            if let Some(source) = expect.source {
                size(source.width, source.height);
            }
        }
        (Task::Export(ExportTask::Media(export)), ArtifactKind::Video) => {
            if let Some(source) = expect.source {
                match export.format {
                    // A gif source is delivered byte for byte; gif settings
                    // shape only a transcode.
                    MediaExportFormat::Gif if !transcode.transcoded => {}
                    MediaExportFormat::Gif => {
                        let width = export
                            .gif
                            .map_or(source.width, |gif| gif.width.min(source.width));
                        if media.width != width {
                            mismatches.push(Mismatch {
                                field: "width",
                                expected: json!(width),
                                actual: json!(media.width),
                            });
                        }
                        if let Some(fps) = transcode.effective_fps {
                            check_fps(&mut mismatches, media, fps);
                        }
                    }
                    MediaExportFormat::Mp4 | MediaExportFormat::Webm => {
                        size(source.width, source.height);
                        if let Some(fps) = source.fps {
                            check_fps(&mut mismatches, media, fps);
                        }
                        check_alpha(&mut mismatches, media, export.alpha);
                    }
                }
            }
        }
        _ => {}
    }
    mismatches
}

fn check_container(mismatches: &mut Vec<Mismatch>, media: &MediaInfo, expected: &str) {
    if media.container != expected {
        mismatches.push(Mismatch {
            field: "container",
            expected: json!(expected),
            actual: json!(media.container),
        });
    }
}

fn check_fps(mismatches: &mut Vec<Mismatch>, media: &MediaInfo, expected: f64) {
    let tolerance = (expected.abs() * 0.001).max(0.01);
    if media
        .fps
        .is_none_or(|actual| (actual - expected).abs() > tolerance)
    {
        mismatches.push(Mismatch {
            field: "fps",
            expected: json!(expected),
            actual: json!(media.fps),
        });
    }
}

/// GIF frame delays are whole centiseconds, so a GIF rendered at `fps`
/// probes at 100/n for a delay n either side of 100/fps: the one of those
/// two rates nearest `actual`.
fn gif_rate(fps: u32, actual: Option<f64>) -> f64 {
    let delay = 100.0 / f64::from(fps);
    let [fast, slow] = [delay.floor(), delay.ceil()].map(|delay| 100.0 / delay.max(1.0));
    let distance = |rate: f64| actual.map_or(0.0, |actual| (rate - actual).abs());
    if distance(fast) <= distance(slow) {
        fast
    } else {
        slow
    }
}

fn check_alpha(mismatches: &mut Vec<Mismatch>, media: &MediaInfo, required: bool) {
    if required && !media.has_alpha {
        mismatches.push(Mismatch {
            field: "has_alpha",
            expected: json!(true),
            actual: json!(false),
        });
    }
}

#[derive(Debug, Deserialize)]
struct ProbeOutput {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    format: Option<ProbeFormat>,
}

#[derive(Debug, Deserialize)]
struct ProbeStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    pix_fmt: Option<String>,
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
    #[serde(default)]
    tags: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct ProbeFormat {
    format_name: Option<String>,
    duration: Option<String>,
}

/// Measures a media file with ffprobe (blocking, bounded by a timeout).
pub fn probe_media(path: &Path) -> Result<MediaInfo, String> {
    let mut child = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_entries",
            "format=format_name,duration:stream=codec_type,codec_name,width,height,pix_fmt,avg_frame_rate,r_frame_rate:stream_tags=alpha_mode",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("ffprobe could not start ({error})"))?;
    let status = match wait_bounded(&mut child, PROBE_TIMEOUT) {
        Ok(Some(status)) => status,
        Ok(None) => return Err("ffprobe timed out".into()),
        Err(error) => return Err(format!("ffprobe failed ({error})")),
    };
    let mut output = Vec::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_end(&mut output);
    }
    if !status.success() {
        return Err("ffprobe could not read the file".into());
    }
    let probe: ProbeOutput = serde_json::from_slice(&output)
        .map_err(|error| format!("ffprobe returned unreadable output ({error})"))?;
    media_info(path, probe)
}

fn media_info(path: &Path, probe: ProbeOutput) -> Result<MediaInfo, String> {
    let extension = files::extension(path).unwrap_or_default();
    let (container, families, image): (&str, &[&str], bool) = match extension.as_str() {
        "mp4" => ("mp4", &["mp4", "mov"], false),
        "mov" => ("mov", &["mov"], false),
        "webm" => ("webm", &["webm", "matroska"], false),
        "gif" => ("gif", &["gif"], false),
        "png" => ("png", &["png_pipe", "image2", "apng"], true),
        "jpg" | "jpeg" => ("jpeg", &["jpeg_pipe", "image2", "mjpeg"], true),
        "webp" => ("webp", &["webp_pipe", "webp"], true),
        other => return Err(format!("unsupported media extension {other:?}")),
    };
    let format_name = probe
        .format
        .as_ref()
        .and_then(|format| format.format_name.as_deref())
        .unwrap_or_default();
    if !format_name
        .split(',')
        .any(|name| families.contains(&name.trim()))
    {
        return Err(format!(
            "ffprobe reads a {format_name:?} file, not {container}"
        ));
    }
    let stream = probe
        .streams
        .iter()
        .find(|stream| stream.codec_type.as_deref() == Some("video"))
        .ok_or("no video stream")?;
    let (Some(width), Some(height)) = (stream.width, stream.height) else {
        return Err("no frame size".into());
    };
    let fps = if image {
        None
    } else {
        stream
            .avg_frame_rate
            .as_deref()
            .and_then(parse_rate)
            .or_else(|| stream.r_frame_rate.as_deref().and_then(parse_rate))
    };
    let duration_seconds = if image {
        None
    } else {
        probe
            .format
            .as_ref()
            .and_then(|format| format.duration.as_deref())
            .and_then(|duration| duration.parse::<f64>().ok())
            .filter(|duration| duration.is_finite())
    };
    let has_alpha = stream.pix_fmt.as_deref().is_some_and(alpha_pixel_format)
        || stream
            .tags
            .get("alpha_mode")
            .is_some_and(|mode| !mode.is_empty() && mode != "0");
    Ok(MediaInfo {
        container: container.into(),
        codec: stream.codec_name.clone(),
        width,
        height,
        fps,
        duration_seconds,
        has_alpha,
    })
}

fn parse_rate(value: &str) -> Option<f64> {
    let rate = match value.split_once('/') {
        Some((numerator, denominator)) => {
            numerator.parse::<f64>().ok()? / denominator.parse::<f64>().ok()?
        }
        None => value.parse().ok()?,
    };
    (rate.is_finite() && rate > 0.0).then_some(rate)
}

fn alpha_pixel_format(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    ["rgba", "bgra", "argb", "abgr"]
        .iter()
        .any(|alpha| value.contains(alpha))
        || ["yuva", "gbrap", "ya"]
            .iter()
            .any(|prefix| value.starts_with(prefix))
        || value == "pal8"
}

#[cfg(test)]
mod tests {
    use super::*;
    use manim_director_core::{
        CaptionsResult, CaptionsTask, MediaFormat, RenderResult, RenderSettings, RenderTask,
        Renderer, SceneRef, ARTIFACTS_DIR,
    };

    fn probe(json: Value) -> ProbeOutput {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn probe_output_becomes_media_info() {
        let video = media_info(
            Path::new("a.webm"),
            probe(json!({"streams":[{"codec_type":"video","codec_name":"vp9","width":1920,"height":1080,
                "pix_fmt":"yuv420p","avg_frame_rate":"60000/1001","tags":{"alpha_mode":"1"}}],
                "format":{"format_name":"matroska,webm","duration":"3.5"}})),
        )
        .unwrap();
        assert_eq!(video.container, "webm");
        assert!((video.fps.unwrap() - 59.94).abs() < 0.01);
        assert_eq!(video.duration_seconds, Some(3.5));
        assert!(video.has_alpha);

        let image = media_info(
            Path::new("a.png"),
            probe(json!({"streams":[{"codec_type":"video","codec_name":"png","width":8,"height":4,"pix_fmt":"rgb24","avg_frame_rate":"0/0"}],
                "format":{"format_name":"png_pipe"}})),
        )
        .unwrap();
        assert_eq!(
            (image.fps, image.duration_seconds, image.has_alpha),
            (None, None, false)
        );

        let wrong = media_info(
            Path::new("a.mov"),
            probe(
                json!({"streams":[{"codec_type":"video","width":8,"height":4}],"format":{"format_name":"gif"}}),
            ),
        );
        assert!(wrong.unwrap_err().contains("not mov"));
    }

    fn render_task(root: &Path, transparent: bool) -> Task {
        Task::Render(RenderTask {
            scene: Some("A".into()),
            files: vec![],
            settings: RenderSettings {
                profile: "draft".into(),
                width: 854,
                height: 480,
                fps: 15,
                renderer: Renderer::Cairo,
                format: MediaFormat::Mp4,
                transparent,
            },
            media_dir: root.join("media"),
            out_dir: root.join(".manim-director/artifacts/job"),
            sections: false,
            fresh: false,
        })
    }

    fn artifact(kind: ArtifactKind, path: &str) -> Artifact {
        Artifact {
            kind,
            path: path.into(),
            label: None,
            bytes: 0,
            media: None,
        }
    }

    #[test]
    fn render_video_contract_lists_every_mismatch() {
        let root = Path::new("/p");
        let task = render_task(root, true);
        let expect = Expectations {
            root,
            task: &task,
            source: None,
            budget_bytes: u64::MAX,
        };
        let mut video = artifact(ArtifactKind::Video, "v.mov");
        video.media = Some(MediaInfo {
            container: "mov".into(),
            codec: None,
            width: 1280,
            height: 720,
            fps: Some(30.0),
            duration_seconds: Some(1.0),
            has_alpha: false,
        });
        let fields: Vec<_> = contract(&expect, &video, Transcode::default())
            .iter()
            .map(|mismatch| mismatch.field)
            .collect();
        assert_eq!(fields, ["width", "height", "container", "fps", "has_alpha"]);
        video.media = Some(MediaInfo {
            container: "mp4".into(),
            codec: None,
            width: 854,
            height: 480,
            fps: Some(15.0001),
            duration_seconds: None,
            has_alpha: true,
        });
        assert!(contract(&expect, &video, Transcode::default()).is_empty());
    }

    #[test]
    fn a_gif_render_may_probe_at_the_centisecond_rate_next_to_its_fps() {
        let root = Path::new("/p");
        let probed = |fps: u32, actual: f64| {
            let mut task = render_task(root, false);
            if let Task::Render(render) = &mut task {
                render.settings.fps = fps;
                render.settings.format = MediaFormat::Gif;
            }
            let expect = Expectations {
                root,
                task: &task,
                source: None,
                budget_bytes: u64::MAX,
            };
            let mut video = artifact(ArtifactKind::Video, "v.gif");
            video.media = Some(MediaInfo {
                container: "gif".into(),
                codec: Some("gif".into()),
                width: 854,
                height: 480,
                fps: Some(actual),
                duration_seconds: Some(1.0),
                has_alpha: false,
            });
            contract(&expect, &video, Transcode::default())
                .iter()
                .map(|mismatch| mismatch.field)
                .collect::<Vec<_>>()
        };
        for (fps, actual) in [(15, 50.0 / 3.0), (30, 100.0 / 3.0), (24, 25.0), (60, 100.0)] {
            assert!(
                probed(fps, actual).is_empty(),
                "{fps} fps probed at {actual}"
            );
        }
        assert!(probed(15, 100.0 / 7.0).is_empty(), "the slower neighbour");
        assert!(probed(25, 25.0).is_empty());
        assert_eq!(probed(15, 25.0), ["fps"]);
        assert_eq!(probed(25, 50.0 / 3.0), ["fps"]);
    }

    #[test]
    fn gif_settings_constrain_only_a_transcoded_export() {
        let task = Task::Export(ExportTask::Media(manim_director_core::MediaExportTask {
            format: MediaExportFormat::Gif,
            output: "/p/output/A.gif".into(),
            source: "/p/a.gif".into(),
            alpha: false,
            gif: Some(manim_director_core::GifSettings {
                fps: 15,
                width: 960,
            }),
        }));
        let source = MediaInfo {
            container: "gif".into(),
            codec: Some("gif".into()),
            width: 1280,
            height: 720,
            fps: Some(30.0),
            duration_seconds: Some(1.0),
            has_alpha: false,
        };
        let expect = Expectations {
            root: Path::new("/p"),
            task: &task,
            source: Some(&source),
            budget_bytes: u64::MAX,
        };
        let mut gif = artifact(ArtifactKind::Video, "output/A.gif");
        gif.media = Some(source.clone());
        let copied = Transcode {
            transcoded: false,
            effective_fps: None,
        };
        assert!(contract(&expect, &gif, copied).is_empty());
        let transcoded = Transcode {
            transcoded: true,
            effective_fps: Some(100.0 / 7.0),
        };
        let fields: Vec<_> = contract(&expect, &gif, transcoded)
            .iter()
            .map(|mismatch| mismatch.field)
            .collect();
        assert_eq!(fields, ["width", "fps"]);
    }

    #[test]
    fn artifacts_outside_their_location_or_kind_fail_validation() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let task = render_task(&root, false);
        create_out_dir(&root, task.out_dir().unwrap()).unwrap();
        std::fs::write(root.join("stray.srt"), "1\n").unwrap();
        let expect = Expectations {
            root: &root,
            task: &task,
            source: None,
            budget_bytes: u64::MAX,
        };
        let mut result = OperationResult::Render(RenderResult {
            scene: SceneRef {
                name: "A".into(),
                file: "a.py".into(),
            },
            duration_seconds: 1.0,
            animations: 1,
            artifacts: vec![
                artifact(ArtifactKind::Captions, "stray.srt"),
                artifact(ArtifactKind::Video, ".manim-director/artifacts/job/A.mp4"),
            ],
        });
        let error = validate(&expect, &mut result).unwrap_err();
        assert_eq!(error.data.unwrap()["check"], "location");

        result.artifacts_mut()[0] = artifact(ArtifactKind::Archive, "stray.srt");
        let error = validate(&expect, &mut result).unwrap_err();
        assert_eq!(error.data.unwrap()["check"], "contract");

        result.artifacts_mut()[0] = artifact(
            ArtifactKind::Captions,
            ".manim-director/artifacts/job/gone.srt",
        );
        let error = validate(&expect, &mut result).unwrap_err();
        assert_eq!(error.data.unwrap()["check"], "missing");

        result.artifacts_mut().pop();
        let error = validate(&expect, &mut result).unwrap_err();
        let data = error.data.unwrap();
        assert_eq!(
            (data["check"].as_str(), &data["path"]),
            (Some("contract"), &Value::Null)
        );
    }

    #[test]
    fn captions_output_is_measured_and_budgeted() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let captions = "1\n00:00:00,000 --> 00:00:01,000\nHi\n";
        std::fs::write(root.join("en.srt"), captions).unwrap();
        let task = Task::Captions(CaptionsTask {
            path: root.join("en.vtt"),
            shift_seconds: 0.0,
            scale: 1.0,
            output: Some(root.join("en.srt")),
        });
        let mut result = OperationResult::Captions(CaptionsResult {
            cue_count: 1,
            duration_seconds: 1.0,
            valid: true,
            findings: vec![],
            artifacts: vec![artifact(ArtifactKind::Captions, "en.srt")],
        });
        let mut expect = Expectations {
            root: &root,
            task: &task,
            source: None,
            budget_bytes: u64::MAX,
        };
        validate(&expect, &mut result).unwrap();
        assert_eq!(result.artifacts()[0].bytes, captions.len() as u64);
        expect.budget_bytes = 10;
        assert_eq!(
            validate(&expect, &mut result).unwrap_err().code,
            "budget_exceeded"
        );
    }

    #[test]
    fn out_dirs_refuse_symlinked_state_directories() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let id = Uuid::new_v4();
        let out_dir = job_dir(&root, id);
        create_out_dir(&root, &out_dir).unwrap();
        std::fs::write(out_dir.join("a.png"), "x").unwrap();
        remove_out_dir(&root, id).unwrap();
        assert!(!out_dir.exists());
        #[cfg(unix)]
        {
            let elsewhere = tempfile::tempdir().unwrap();
            std::fs::remove_dir(root.join(ARTIFACTS_DIR)).unwrap();
            std::os::unix::fs::symlink(elsewhere.path(), root.join(ARTIFACTS_DIR)).unwrap();
            assert!(create_out_dir(&root, &out_dir).is_err());
            assert!(remove_out_dir(&root, id).is_err());
        }
    }

    #[test]
    fn real_media_is_probed_when_ffprobe_is_installed() {
        let directory = tempfile::tempdir().unwrap();
        let clip = directory.path().join("clip.mp4");
        let made = Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=64x32:r=24:d=0.5",
            ])
            .args(["-pix_fmt", "yuv420p"])
            .arg(&clip)
            .status();
        if !made.is_ok_and(|status| status.success()) {
            eprintln!("skipping: ffmpeg is not installed");
            return;
        }
        let media = probe_media(&clip).unwrap();
        assert_eq!(
            (media.container.as_str(), media.width, media.height),
            ("mp4", 64, 32)
        );
        assert!((media.fps.unwrap() - 24.0).abs() < 0.01);
        assert!(!media.has_alpha);
    }
}
