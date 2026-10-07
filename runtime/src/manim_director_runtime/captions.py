"""The `captions` operation: parse, retime, check and convert VTT/SRT files."""

from __future__ import annotations

import re
from collections.abc import Iterator
from dataclasses import dataclass, replace
from typing import TYPE_CHECKING, NoReturn

from .errors import DirectorError, invalid_params, io_error
from .model import ArtifactKind, Finding, RuntimeArtifact, Severity, SourceLocation
from .paths import atomic_target
from .tasks import CaptionsTask

if TYPE_CHECKING:
    from .protocol import Context

MAX_LINES = 2
MAX_LINE_CHARS = 48
MAX_CHARS_PER_SECOND = 24.0
MAX_FINDINGS = 200
_TIME = r"(?:(\d+):)?(\d{2}):(\d{2})[.,](\d{3})"
_TIMING = re.compile(rf"\s*{_TIME}\s*-->\s*{_TIME}(?:\s+.*)?")
_VTT_METADATA_BLOCKS = ("NOTE", "STYLE", "REGION")


@dataclass(frozen=True, slots=True)
class Cue:
    start: float
    end: float
    text: str
    identifier: str | None
    line: int  # 1-based line of the timing line in the source file


@dataclass(frozen=True, slots=True)
class CaptionsResult:
    cue_count: int
    duration_seconds: float
    valid: bool
    findings: list[Finding]
    artifacts: list[RuntimeArtifact]


def captions(task: CaptionsTask, ctx: Context) -> CaptionsResult:
    try:
        text = task.path.read_text(encoding="utf-8-sig")
    except UnicodeDecodeError:
        raise DirectorError(
            "invalid_captions", f"{task.path.name} is not UTF-8 text.", {"line": None}
        ) from None
    except OSError as exc:
        raise io_error(task.path, exc) from exc
    cues = [
        replace(cue, start=_retime(cue.start, task), end=_retime(cue.end, task))
        for cue in parse(text, vtt=task.path.suffix.lower() == ".vtt")
    ]
    findings = check(cues, ctx.relative(task.path))
    artifacts = []
    if task.output is not None:
        output = ctx.require_inside(task.output, "output")
        suffix = output.suffix.lower()
        if suffix not in (".vtt", ".srt"):
            raise invalid_params("output", "must end in .vtt or .srt", allowed=[".vtt", ".srt"])
        with atomic_target(output) as temp:
            temp.write_text(render(cues, vtt=suffix == ".vtt"), encoding="utf-8")
        artifacts.append(ctx.artifact(ArtifactKind.CAPTIONS, output))
    return CaptionsResult(
        cue_count=len(cues),
        duration_seconds=max((cue.end for cue in cues), default=0.0),
        valid=all(finding.severity is not Severity.ERROR for finding in findings),
        findings=findings[:MAX_FINDINGS],
        artifacts=artifacts,
    )


def parse(text: str, *, vtt: bool) -> list[Cue]:
    lines = text.replace("\r\n", "\n").replace("\r", "\n").split("\n")
    blocks: list[list[tuple[int, str]]] = [[]]
    for number, line in enumerate(lines, start=1):
        if line.strip():
            blocks[-1].append((number, line.rstrip()))
        elif blocks[-1]:
            blocks.append([])
    blocks = [block for block in blocks if block]
    if vtt:
        if not blocks or not blocks[0][0][1].startswith("WEBVTT"):
            _invalid(1, "a WebVTT file starts with WEBVTT")
        blocks = blocks[1:]
    cues = []
    for block in blocks:
        timing = next((i for i, (_, line) in enumerate(block) if "-->" in line), None)
        if timing is None:
            if vtt and block[0][1].startswith(_VTT_METADATA_BLOCKS):
                continue
            _invalid(block[0][0], "this block has no timing line")
        number, line = block[timing]
        match = _TIMING.fullmatch(line)
        if match is None:
            _invalid(number, "the timing line is malformed")
        identifier = block[timing - 1][1].strip() if timing > 0 else None
        cues.append(
            Cue(
                start=_seconds(*match.groups()[:4]),
                end=_seconds(*match.groups()[4:]),
                text="\n".join(text for _, text in block[timing + 1 :]),
                identifier=identifier,
                line=number,
            )
        )
    return cues


_RULES = {  # code -> (severity, hint)
    "invalid_timing": (Severity.ERROR, "Make the end time later than the start time."),
    "overlap": (Severity.ERROR, "Shorten the previous cue or start this one later."),
    "empty_cue": (Severity.WARNING, "Remove the cue or add text."),
    "too_many_lines": (Severity.WARNING, f"Keep cues to {MAX_LINES} lines; split long cues."),
    "line_too_long": (Severity.WARNING, f"Keep lines to {MAX_LINE_CHARS} characters."),
    "reading_speed": (
        Severity.WARNING,
        f"Show the cue longer; aim for {MAX_CHARS_PER_SECOND:.0f} characters per second.",
    ),
}


def check(cues: list[Cue], file: str) -> list[Finding]:
    findings: list[Finding] = []
    previous_end = 0.0
    for cue in cues:
        for code, message in _problems(cue, previous_end):
            severity, hint = _RULES[code]
            location = SourceLocation(file, cue.line)
            findings.append(
                Finding(code, severity, message, hint, location=location, at_seconds=cue.start)
            )
        previous_end = max(previous_end, cue.end)
    return findings


def _problems(cue: Cue, previous_end: float) -> Iterator[tuple[str, str]]:
    duration = cue.end - cue.start
    if duration <= 0:
        yield "invalid_timing", "The cue ends before it starts."
    if cue.start < previous_end:
        yield "overlap", "The cue starts before the previous cue ends."
    lines = cue.text.splitlines()
    if not cue.text.strip():
        yield "empty_cue", "The cue has no text."
        return
    if len(lines) > MAX_LINES:
        yield "too_many_lines", f"The cue has {len(lines)} lines."
    longest = max(len(line) for line in lines)
    if longest > MAX_LINE_CHARS:
        yield "line_too_long", f"A line has {longest} characters."
    speed = len(cue.text.replace("\n", " ")) / duration if duration > 0 else 0.0
    if speed > MAX_CHARS_PER_SECOND:
        yield "reading_speed", f"The cue needs {speed:.0f} characters per second of reading."


def render(cues: list[Cue], *, vtt: bool) -> str:
    blocks = []
    for number, cue in enumerate(cues, start=1):
        identifier = cue.identifier if vtt else str(number)
        timing = f"{_clock(cue.start, vtt)} --> {_clock(cue.end, vtt)}"
        blocks.append("\n".join([*([identifier] if identifier else []), timing, cue.text]))
    return ("WEBVTT\n\n" if vtt else "") + "\n\n".join(blocks) + ("\n" if blocks else "")


def _retime(seconds: float, task: CaptionsTask) -> float:
    return max(0.0, task.shift_seconds + seconds * task.scale)


def _seconds(hours: str | None, minutes: str, seconds: str, millis: str) -> float:
    return int(hours or 0) * 3600 + int(minutes) * 60 + int(seconds) + int(millis) / 1000


def _clock(seconds: float, vtt: bool) -> str:
    total = round(seconds * 1000)
    hours, rest = divmod(total, 3_600_000)
    minutes, rest = divmod(rest, 60_000)
    whole, millis = divmod(rest, 1000)
    return f"{hours:02d}:{minutes:02d}:{whole:02d}{'.' if vtt else ','}{millis:03d}"


def _invalid(line: int, reason: str) -> NoReturn:
    raise DirectorError("invalid_captions", f"Line {line}: {reason}.", {"line": line})
