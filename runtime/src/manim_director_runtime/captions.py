from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Mapping, Sequence

from .errors import DirectorError
from .media import probe_media
from .util import atomic_write, confined_path, project_root


TIMING = re.compile(r"(?P<start>\d{1,3}:\d{2}:\d{2}[,.]\d{3})\s*-->\s*(?P<end>\d{1,3}:\d{2}:\d{2}[,.]\d{3})(?:\s+.*)?")


@dataclass(slots=True)
class Cue:
    start: float
    end: float
    text: str
    identifier: str | None = None

    def as_dict(self) -> dict[str, Any]:
        value = {"start": round(self.start, 6), "end": round(self.end, 6), "text": self.text}
        if self.identifier:
            value["identifier"] = self.identifier
        return value


def _seconds(value: str) -> float:
    hours, minutes, rest = value.replace(",", ".").split(":")
    return int(hours) * 3600 + int(minutes) * 60 + float(rest)


def _timestamp(value: float, separator: str) -> str:
    milliseconds = max(0, round(value * 1000))
    hours, remainder = divmod(milliseconds, 3_600_000)
    minutes, remainder = divmod(remainder, 60_000)
    seconds, millis = divmod(remainder, 1000)
    return f"{hours:02d}:{minutes:02d}:{seconds:02d}{separator}{millis:03d}"


def parse_captions(text: str) -> list[Cue]:
    normalized = text.replace("\r\n", "\n").lstrip("\ufeff")
    if normalized.startswith("WEBVTT"):
        normalized = normalized.split("\n", 1)[1] if "\n" in normalized else ""
    blocks = re.split(r"\n\s*\n", normalized.strip()) if normalized.strip() else []
    cues: list[Cue] = []
    for block in blocks:
        lines = [line.rstrip() for line in block.splitlines()]
        if not lines:
            continue
        timing_index = next((index for index, line in enumerate(lines) if TIMING.fullmatch(line.strip())), None)
        if timing_index is None:
            continue
        match = TIMING.fullmatch(lines[timing_index].strip())
        assert match
        identifier = lines[timing_index - 1].strip() if timing_index > 0 else None
        if identifier and identifier.isdigit():
            identifier = None
        body = "\n".join(lines[timing_index + 1:]).strip()
        cues.append(Cue(_seconds(match.group("start")), _seconds(match.group("end")), body, identifier))
    return cues


def cues_from_values(values: Sequence[Mapping[str, Any]]) -> list[Cue]:
    cues = []
    for index, value in enumerate(values):
        try:
            cue = Cue(float(value["start"]), float(value["end"]), str(value["text"]), str(value["identifier"]) if value.get("identifier") else None)
        except (KeyError, TypeError, ValueError) as exc:
            raise DirectorError("invalid_cue", f"Cue {index} requires numeric start/end and text") from exc
        cues.append(cue)
    return cues


def format_captions(cues: Sequence[Cue], fmt: str) -> str:
    fmt = fmt.lower()
    if fmt not in {"srt", "vtt"}:
        raise DirectorError("invalid_caption_format", "Caption format must be srt or vtt")
    separator = "," if fmt == "srt" else "."
    blocks = []
    for index, cue in enumerate(cues, 1):
        identifier = str(index) if fmt == "srt" else cue.identifier
        lines = ([identifier] if identifier else []) + [
            f"{_timestamp(cue.start, separator)} --> {_timestamp(cue.end, separator)}", cue.text,
        ]
        blocks.append("\n".join(lines))
    prefix = "WEBVTT\n\n" if fmt == "vtt" else ""
    return prefix + "\n\n".join(blocks) + ("\n" if blocks else "")


def validate_cues(cues: Sequence[Cue], *, max_cps: float = 24.0, max_lines: int = 2, max_chars_per_line: int = 48) -> dict[str, Any]:
    issues: list[dict[str, Any]] = []
    previous_end = 0.0
    for index, cue in enumerate(cues):
        duration = cue.end - cue.start
        if cue.start < 0 or duration <= 0:
            issues.append({"code": "invalid_timing", "severity": "error", "cue": index, "start": cue.start, "end": cue.end})
        if cue.start < previous_end:
            issues.append({"code": "overlap", "severity": "error", "cue": index, "previous_end": previous_end, "start": cue.start})
        if not cue.text.strip():
            issues.append({"code": "empty_caption", "severity": "warning", "cue": index})
        lines = cue.text.splitlines() or [""]
        if len(lines) > max_lines:
            issues.append({"code": "too_many_lines", "severity": "warning", "cue": index, "lines": len(lines), "maximum": max_lines})
        longest = max(map(len, lines))
        if longest > max_chars_per_line:
            issues.append({"code": "line_too_long", "severity": "warning", "cue": index, "characters": longest, "maximum": max_chars_per_line})
        cps = len(cue.text.replace("\n", " ")) / max(duration, 0.001)
        if cps > max_cps:
            issues.append({"code": "reading_speed", "severity": "warning", "cue": index, "characters_per_second": round(cps, 2), "maximum": max_cps})
        previous_end = max(previous_end, cue.end)
    errors = sum(issue["severity"] == "error" for issue in issues)
    return {"valid": errors == 0, "issues": issues, "duration": max((cue.end for cue in cues), default=0.0), "cue_count": len(cues)}


def _load_cues(params: Mapping[str, Any], root: Path) -> list[Cue]:
    if params.get("cues") is not None:
        return cues_from_values(params["cues"])
    source = params.get("source")
    if not source:
        raise DirectorError("captions_required", "Provide cues or a caption source file")
    path = confined_path(root, str(source), must_exist=True)
    return parse_captions(path.read_text(encoding="utf-8"))


def _from_words(words: Sequence[Mapping[str, Any]], max_chars: int, max_duration: float) -> list[Cue]:
    cues: list[Cue] = []
    group: list[Mapping[str, Any]] = []
    for word in words:
        candidate = " ".join(str(item["text"]) for item in [*group, word])
        duration = float(word["end"]) - float((group[0] if group else word)["start"])
        if group and (len(candidate) > max_chars or duration > max_duration):
            cues.append(Cue(float(group[0]["start"]), float(group[-1]["end"]), " ".join(str(item["text"]) for item in group)))
            group = []
        group.append(word)
    if group:
        cues.append(Cue(float(group[0]["start"]), float(group[-1]["end"]), " ".join(str(item["text"]) for item in group)))
    return cues


def _reconcile(beats: Sequence[Mapping[str, Any]], cues: Sequence[Cue], minimum_hold: float) -> list[dict[str, Any]]:
    result = []
    cursor = 0.0
    for index, beat in enumerate(beats):
        original = float(beat.get("duration", 0))
        narration_duration = (cues[index].end - cues[index].start) if index < len(cues) else 0.0
        duration = max(original, narration_duration, minimum_hold)
        result.append({**beat, "start": round(cursor, 6), "duration": round(duration, 6), "end": round(cursor + duration, 6), "extended_by": round(max(0.0, duration - original), 6)})
        cursor += duration
    return result


def captions(params: Mapping[str, Any]) -> dict[str, Any]:
    root = project_root(params)
    operation = str(params.get("operation", "validate"))
    if operation == "from_words":
        cues = _from_words(params.get("words", []), int(params.get("max_chars", 76)), float(params.get("max_duration", 5.5)))
    else:
        cues = _load_cues(params, root)
    if operation == "retime":
        offset, scale = float(params.get("offset", 0)), float(params.get("scale", 1))
        if scale <= 0:
            raise DirectorError("invalid_scale", "Caption time scale must be positive")
        cues = [Cue(max(0.0, offset + cue.start * scale), max(0.0, offset + cue.end * scale), cue.text, cue.identifier) for cue in cues]
    validation = validate_cues(cues, max_cps=float(params.get("max_cps", 24)), max_lines=int(params.get("max_lines", 2)), max_chars_per_line=int(params.get("max_chars_per_line", 48)))
    result: dict[str, Any] = {"operation": operation, "cues": [cue.as_dict() for cue in cues], "validation": validation}
    if operation == "reconcile":
        result["beats"] = _reconcile(params.get("beats", []), cues, float(params.get("minimum_hold", 0.2)))
        result["duration"] = max((beat["end"] for beat in result["beats"]), default=0.0)
    audio = params.get("audio")
    if audio:
        info = probe_media(confined_path(root, str(audio), must_exist=True))
        result["audio"] = info
        result["tail_gap_seconds"] = round((info.get("duration_seconds") or 0) - validation["duration"], 6)
    if params.get("output"):
        fmt = str(params.get("format", Path(str(params["output"])).suffix.lstrip(".") or "vtt"))
        destination = confined_path(root, str(params["output"]))
        atomic_write(destination, format_captions(cues, fmt))
        result["output"] = str(destination)
        result["format"] = fmt
    elif operation in {"generate", "retime", "from_words"}:
        fmt = str(params.get("format", "vtt"))
        result["text"] = format_captions(cues, fmt)
        result["format"] = fmt
    return result
