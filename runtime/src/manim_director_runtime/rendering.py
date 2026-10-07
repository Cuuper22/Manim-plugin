"""The `render` and `still` operations: Manim runs in-process inside the bridge worker.

Cancellation needs no cooperation: the engine kills the worker's process group, which also
takes down any LaTeX or encoder children Manim started.
"""

from __future__ import annotations

import ast
import configparser
import importlib.util
import json
import shutil
import sys
import uuid
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Any, Literal

from . import timeline
from .cachelock import guard_shared_caches
from .diagnostics import exception_findings
from .errors import DirectorError, dependency_missing, io_error
from .inspection import ParsedFile, scene_class_names
from .jsonio import write_json
from .model import ArtifactKind, RuntimeArtifact, SceneRef, SourceLocation
from .paths import ensure_dir, slug
from .process import pip_install
from .tasks import RenderSettings, RenderTask, StillTask

if TYPE_CHECKING:
    from .protocol import Context

Stage = Literal["setup", "import", "construct", "write"]
_TRACEBACK_CHARS = 16 * 1024


@dataclass(frozen=True, slots=True)
class RenderResult:
    scene: SceneRef
    duration_seconds: float
    animations: int
    artifacts: list[RuntimeArtifact]


@dataclass(frozen=True, slots=True)
class StillResult:
    scene: SceneRef
    artifacts: list[RuntimeArtifact]


@dataclass(frozen=True, slots=True)
class SceneTarget:
    name: str
    path: Path
    line: int | None = None  # of the class statement


@dataclass(slots=True)
class _Run:
    """What a finished in-process render left behind."""

    scene: Any
    recorder: timeline.BeatRecorder
    stage: Stage = "setup"
    plays: int = 0
    scene_seconds: float = 0.0


def render(task: RenderTask, ctx: Context) -> RenderResult:
    out_dir = ensure_dir(ctx.require_inside(task.out_dir, "out_dir"))
    target = resolve_scene(task.scene, task.files, ctx)
    s = task.settings
    ctx.log("info", f"Rendering {target.name} at {s.width}x{s.height}@{s.fps} ({s.renderer})")
    run = _run_manim(
        target, s, task.media_dir, ctx, movie=True, sections=task.sections, fresh=task.fresh
    )
    writer = run.scene.renderer.file_writer
    movie = Path(writer.gif_file_path if s.format == "gif" else writer.movie_file_path)
    if run.plays == 0 or not movie.is_file():
        message = (
            f"{target.name} has no animations, so there is no video; add a wait or render a still."
        )
        raise _render_failed("write", target, ctx, message=message)
    video = _move(movie, out_dir / f"{target.name}{movie.suffix}")
    artifacts = [ctx.artifact(ArtifactKind.VIDEO, video)]
    if task.sections:
        artifacts += _collect_sections(writer, out_dir, ctx)
    subtitles = movie.with_suffix(".srt")
    if writer.subcaptions and subtitles.is_file():
        srt = _move(subtitles, out_dir / f"{target.name}.srt")
        artifacts.append(ctx.artifact(ArtifactKind.CAPTIONS, srt))
    if run.recorder.attached:
        path = out_dir / f"{target.name}.timeline.json"
        write_json(path, run.recorder.timeline(target.name, run.scene_seconds))
        artifacts.append(ctx.artifact(ArtifactKind.TIMELINE, path))
    return RenderResult(
        scene=SceneRef(target.name, ctx.relative(target.path)),
        duration_seconds=run.scene_seconds,
        animations=run.plays,
        artifacts=artifacts,
    )


def still(task: StillTask, ctx: Context) -> StillResult:
    out_dir = ensure_dir(ctx.require_inside(task.out_dir, "out_dir"))
    target = resolve_scene(task.scene, task.files, ctx)
    s = task.settings
    ctx.log(
        "info", f"Rendering the last frame of {target.name} at {s.width}x{s.height} ({s.renderer})"
    )
    run = _run_manim(target, s, task.media_dir, ctx, movie=False, sections=False, fresh=task.fresh)
    image = Path(run.scene.renderer.file_writer.image_file_path)
    if not image.is_file():
        raise _render_failed(
            "write", target, ctx, message=f"Manim wrote no image for {target.name}."
        )
    path = _move(image, out_dir / f"{target.name}.png")
    return StillResult(
        scene=SceneRef(target.name, ctx.relative(target.path)),
        artifacts=[ctx.artifact(ArtifactKind.IMAGE, path)],
    )


def resolve_scene(scene: str | None, files: list[Path], ctx: Context) -> SceneTarget:
    """Pick the scene class by AST alone, before any user code runs (contract §1.3 step 1)."""

    parsed: list[ParsedFile] = []
    broken: list[tuple[Path, SyntaxError]] = []
    for path in files:
        try:
            tree = ast.parse(path.read_bytes(), filename=str(path))
        except SyntaxError as exc:
            broken.append((path, exc))
            continue
        except OSError as exc:
            raise io_error(path, exc) from exc
        parsed.append(ParsedFile(path, [n for n in tree.body if isinstance(n, ast.ClassDef)]))
    scene_names = scene_class_names(parsed)
    scenes = [
        SceneTarget(node.name, item.path, node.lineno)
        for item in parsed
        for node in item.classes
        if node.name in scene_names
    ]
    available = sorted({target.name for target in scenes})
    if scene is not None:
        # The last definition in a file wins, as in Python.
        defining = {
            item.path: node.lineno for item in parsed for node in item.classes if node.name == scene
        }
        if len(defining) == 1:
            ((path, line),) = defining.items()
            return SceneTarget(scene, path, line)
        if len(defining) > 1:
            paths = [ctx.relative(path) for path in defining]
            raise DirectorError(
                "scene_ambiguous",
                f"{scene} is defined in {len(defining)} files; pass the file to render.",
                {"scene": scene, "files": paths},
            )
    elif len(scenes) == 1 and not broken:
        return scenes[0]
    if broken:
        # The wanted class may be in a file that does not parse: report the syntax error.
        path, error = broken[0]
        raise _render_failed("import", SceneTarget(scene or path.stem, path), ctx, error)
    if scene is None:
        raise DirectorError(
            "scene_required",
            f"Found {len(scenes)} scene classes; name the one to render.",
            {"available": available},
        )
    raise DirectorError(
        "scene_not_found",
        f"No scene class named {scene} in the given files.",
        {"scene": scene, "available": available},
    )


def _run_manim(
    target: SceneTarget,
    settings: RenderSettings,
    media_dir: Path,
    ctx: Context,
    *,
    movie: bool,
    sections: bool,
    fresh: bool,
) -> _Run:
    def apply_task_values() -> None:
        _apply_settings(target, settings, media_dir, movie=movie, sections=sections, fresh=fresh)

    ctx.progress("import", 0, message=ctx.relative(target.path))
    manim = _import_manim(target, ctx)
    with manim.tempconfig({}), timeline.recording(ctx.project_root) as recorder:
        try:
            _digest_config_files(ctx.project_root, target.path.parent, settings)
            apply_task_values()
        except Exception as exc:  # a malformed manim.cfg is the project's error, like its code
            raise _render_failed("setup", target, ctx, exc) from None
        try:
            module = _import_module(target.path, ctx.project_root)
        except (Exception, SystemExit) as exc:
            raise _render_failed("import", target, ctx, exc) from None
        scene_class = getattr(module, target.name, None)
        if not (isinstance(scene_class, type) and issubclass(scene_class, manim.Scene)):
            raise DirectorError(
                "scene_not_found",
                f"{target.name} in {ctx.relative(target.path)} is not a Manim Scene subclass.",
                {"scene": target.name, "reason": "not a Scene subclass"},
            )
        # Module-level `config.x = ...` behaves as under plain manim, but task values still win.
        apply_task_values()
        run = _Run(scene=None, recorder=recorder)
        try:
            run.scene = scene_class()
            _instrument(run, ctx)
            run.scene.render()
        except (Exception, SystemExit) as exc:
            raise _render_failed(run.stage, target, ctx, exc) from None
    return run


def _import_manim(target: SceneTarget, ctx: Context) -> Any:
    try:
        import manim
    except ModuleNotFoundError as exc:
        if exc.name != "manim":
            raise _render_failed("setup", target, ctx, exc) from None
        hint = f"Install Manim Community Edition: {pip_install('manim>=0.21,<0.22')}"
        raise dependency_missing("manim", hint) from None
    except Exception as exc:  # importing Manim reads manim.cfg from the project root
        raise _render_failed("setup", target, ctx, exc) from None
    guard_shared_caches()  # other workers render this project's scenes at the same time
    _plain_logs()
    return manim


def _plain_logs() -> None:
    """One plain stderr line per Manim log record. Rich wraps records at 80 columns into
    fragments (paths and TeX split mid-token), and each fragment becomes a job log line."""

    import logging

    from rich.logging import RichHandler

    plain = logging.StreamHandler(sys.stderr)
    plain.setFormatter(logging.Formatter("%(levelname)s %(message)s"))
    for logger in (logging.getLogger("manim"), logging.getLogger()):
        for handler in [h for h in logger.handlers if isinstance(h, RichHandler)]:
            logger.removeHandler(handler)
            logger.addHandler(plain)


def _digest_config_files(project_root: Path, scene_dir: Path, settings: RenderSettings) -> None:
    """Manim defaults, then project and scene-directory manim.cfg, then the task's frame size."""

    from manim import config
    from manim._config.utils import config_file_paths

    library, user, _cwd = config_file_paths()
    parser = configparser.ConfigParser()
    with library.open(encoding="utf-8") as handle:
        parser.read_file(handle)
    parser.read([user, *dict.fromkeys([project_root / "manim.cfg", scene_dir / "manim.cfg"])])
    # Size goes through the parser so Manim derives frame_width from the task's aspect ratio.
    parser["CLI"]["pixel_width"] = str(settings.width)
    parser["CLI"]["pixel_height"] = str(settings.height)
    parser["CLI"]["frame_rate"] = str(settings.fps)
    config.digest_parser(parser)


def _apply_settings(
    target: SceneTarget,
    settings: RenderSettings,
    media_dir: Path,
    *,
    movie: bool,
    sections: bool,
    fresh: bool,
) -> None:
    from manim import config

    config.input_file = target.path
    config.output_file = target.name
    config.media_dir = media_dir
    config.pixel_width = settings.width
    config.pixel_height = settings.height
    config.frame_rate = settings.fps
    config.renderer = settings.renderer
    config.transparent = settings.transparent  # before format: both pick the movie extension
    if movie:
        config.format = settings.format
    config.write_to_movie = movie
    config.save_last_frame = not movie
    config.save_sections = sections
    config.disable_caching = fresh
    config.preview = False
    config.progress_bar = "none"


def _import_module(path: Path, project_root: Path) -> Any:
    for entry in reversed(dict.fromkeys([str(path.parent), str(project_root)])):
        sys.path.insert(0, entry)
    name = f"_manim_director_scene_{uuid.uuid4().hex}"
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


def _instrument(run: _Run, ctx: Context) -> None:
    """Track the render stage and report progress after every play and wait."""

    scene = run.scene
    renderer = scene.renderer
    construct, play, finished = scene.construct, renderer.play, renderer.scene_finished

    def construct_hook() -> None:
        run.stage = "construct"
        construct()

    def play_hook(*args: Any, **kwargs: Any) -> None:
        play(*args, **kwargs)
        run.plays, run.scene_seconds = renderer.num_plays, float(renderer.time)
        ctx.progress("animate", run.plays, scene_seconds=run.scene_seconds)

    def finished_hook(*args: Any, **kwargs: Any) -> None:
        run.stage = "write"
        run.plays, run.scene_seconds = renderer.num_plays, float(renderer.time)
        ctx.progress("encode", 0)
        finished(*args, **kwargs)

    scene.construct = construct_hook
    renderer.play = play_hook
    renderer.scene_finished = finished_hook


def _collect_sections(writer: Any, out_dir: Path, ctx: Context) -> list[RuntimeArtifact]:
    sections_dir = Path(writer.sections_output_dir)
    index_path = sections_dir / f"{writer.output_name}.json"
    try:
        index = json.loads(index_path.read_text(encoding="utf-8"))
    except OSError as exc:
        raise io_error(index_path, exc) from exc
    artifacts = []
    for number, entry in enumerate(index, start=1):
        source = sections_dir / entry["video"]
        name = str(entry["name"])
        destination = out_dir / "sections" / f"{number:04d}-{slug(name, 'section')}{source.suffix}"
        artifacts.append(ctx.artifact(ArtifactKind.SECTION, _move(source, destination), label=name))
    return artifacts


def _move(source: Path, destination: Path) -> Path:
    try:
        ensure_dir(destination.parent)
        shutil.move(source, destination)
    except OSError as exc:
        raise io_error(source, exc) from exc
    return destination


def _render_failed(
    stage: Stage,
    target: SceneTarget,
    ctx: Context,
    exc: BaseException | None = None,
    *,
    message: str | None = None,
) -> DirectorError:
    # Without a project frame (a bad theme, an encoder failure) the scene class is the place.
    scene_line = SourceLocation(ctx.relative(target.path), target.line) if target.line else None
    findings, trace = exception_findings(exc, ctx.project_root, scene_line) if exc else ([], "")
    if message is None:
        assert exc is not None
        subject = {
            "import": f"Importing {ctx.relative(target.path)}",
            "setup": f"Setting up {target.name}",
            "construct": f"{target.name}.construct",
            "write": f"Writing {target.name}",
        }[stage]
        detail = (str(exc).strip().splitlines() or [""])[0][:300].rstrip(".")
        message = f"{subject} raised {type(exc).__name__}" + (f": {detail}" if detail else "") + "."
    return DirectorError(
        "render_failed",
        message,
        {
            "stage": stage,
            "exception": type(exc).__name__ if exc else None,
            "findings": findings,
            "traceback": trace[-_TRACEBACK_CHARS:],
        },
    )
