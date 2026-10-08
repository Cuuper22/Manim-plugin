"""Short, bounded descriptions of ingested files, one summarizer per source kind."""

from __future__ import annotations

import ast
import csv
import json
import re
import xml.etree.ElementTree as ET
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from . import process
from .errors import DirectorError, dependency_missing, invalid_source, io_error
from .inspection import ParsedFile, scene_class_names
from .media import require_pillow

SUMMARY_CHARS = 2000
TEXT_SAMPLE_BYTES = 256 * 1024


@dataclass(slots=True)
class Summary:
    summary: str = ""
    headings: list[str] = field(default_factory=list)
    columns: list[str] = field(default_factory=list)
    rows: int | None = None
    pages: int | None = None
    scenes: list[str] = field(default_factory=list)
    width: int | None = None
    height: int | None = None
    duration_seconds: float | None = None


def summarize(path: Path, kind: str) -> Summary:
    if kind in ("markdown", "latex", "typst", "text"):
        return _text_summary(_read_text(path), kind)
    summarizers = {
        "csv": _csv_summary,
        "json": _json_summary,
        "python": _python_summary,
        "notebook": _notebook_summary,
        "pdf": _pdf_summary,
        "svg": _svg_summary,
        "image": _image_summary,
        "audio": _media_summary,
        "video": _media_summary,
    }
    if kind in summarizers:
        return summarizers[kind](path)
    size = path.stat().st_size
    return Summary(
        summary=f"{path.suffix.lstrip('.').upper() or 'Extensionless'} file, {size} bytes."
    )


_HEADING_PATTERNS = {
    "markdown": re.compile(r"(?m)^#{1,6}\s+(.+?)\s*#*$"),
    "typst": re.compile(r"(?m)^={1,6}\s+(.+)$"),
    "latex": re.compile(r"\\(?:chapter|section|subsection)\*?\{([^}]*)\}"),
}


def _text_summary(text: str, kind: str) -> Summary:
    pattern = _HEADING_PATTERNS.get(kind)
    headings = [match.strip() for match in pattern.findall(text)] if pattern else []
    if kind == "latex":
        text = re.sub(r"(?m)(?<!\\)%.*$", "", text)
        text = (
            re.sub(r"\\[A-Za-z@]+\*?(?:\[[^]]*\])?", " ", text).replace("{", " ").replace("}", " ")
        )
    if kind == "markdown":
        text = re.sub(r"```.*?```", " ", text, flags=re.DOTALL)
    return Summary(summary=re.sub(r"[`*_>#$=]+", " ", text), headings=headings)


def _csv_summary(path: Path) -> Summary:
    try:
        dialect: Any = csv.Sniffer().sniff(_read_text(path)[:8192])
    except csv.Error:
        dialect = csv.excel_tab if path.suffix.lower() == ".tsv" else csv.excel
    # Rows are counted over the whole file, not just the sample the dialect came from.
    try:
        with path.open(newline="", encoding="utf-8-sig", errors="replace") as handle:
            reader = csv.reader(handle, dialect)
            header = next(reader, [])
            rows = sum(1 for _ in reader)
    except OSError as exc:
        raise io_error(path, exc) from exc
    except csv.Error as exc:
        raise invalid_source(path, f"the table is malformed ({exc})") from exc
    columns = [column.strip()[:120] for column in header]
    return Summary(
        summary=f"Table with {rows} rows and {len(columns)} columns: {', '.join(columns[:12])}.",
        columns=columns,
        rows=rows,
    )


def _json_summary(path: Path) -> Summary:
    value = read_json(path, strict=True)
    if isinstance(value, dict):
        keys = list(value)
        return Summary(
            summary=f"JSON object with {len(keys)} keys: {', '.join(map(str, keys[:20]))}."
        )
    if isinstance(value, list):
        return Summary(summary=f"JSON array of {len(value)} items.", rows=len(value))
    return Summary(summary=f"JSON {type(value).__name__} value.")


def _python_summary(path: Path) -> Summary:
    try:
        tree = ast.parse(path.read_bytes(), filename=str(path))
    except SyntaxError as exc:
        return Summary(
            summary=f"Python source that does not parse: {exc.msg} at line {exc.lineno}."
        )
    classes = [node for node in tree.body if isinstance(node, ast.ClassDef)]
    functions = [n for n in tree.body if isinstance(n, (ast.FunctionDef, ast.AsyncFunctionDef))]
    names = scene_class_names([ParsedFile(path, classes)])
    scenes = [node.name for node in classes if node.name in names]
    return Summary(
        summary=f"Python module with {len(classes)} classes, {len(functions)} functions and "
        f"{len(scenes)} Manim scenes.",
        scenes=scenes,
    )


def _notebook_summary(path: Path) -> Summary:
    notebook = read_json(path, strict=True)
    if not isinstance(notebook, dict) or not isinstance(notebook.get("cells"), list):
        raise invalid_source(path, "it is not a Jupyter notebook")
    cells = [cell for cell in notebook["cells"] if isinstance(cell, dict)]
    sources = [cell.get("source", "") for cell in cells if cell.get("cell_type") == "markdown"]
    # nbformat: a cell's source is a string or a list of strings.
    if not all(isinstance(s, str) or _strings(s) for s in sources):
        raise invalid_source(path, "it is not a Jupyter notebook")
    markdown = "\n".join("".join(source) for source in sources)
    code = sum(cell.get("cell_type") == "code" for cell in cells)
    summary = _text_summary(markdown, "markdown")
    summary.summary = f"Notebook with {len(cells)} cells ({code} code). {summary.summary}"
    return summary


def _pdf_summary(path: Path) -> Summary:
    try:
        from pypdf import PdfReader
    except ImportError:
        raise dependency_missing("pypdf", "Install pypdf in the runtime environment.") from None
    try:
        reader = PdfReader(str(path))
        text: list[str] = []
        for page in reader.pages:
            text.append(page.extract_text() or "")
            if sum(map(len, text)) >= SUMMARY_CHARS:
                break
        pages = len(reader.pages)
    except OSError as exc:
        raise io_error(path, exc) from exc
    except Exception as exc:  # malformed PDFs surface as arbitrary errors deep inside pypdf
        raise invalid_source(path, f"the PDF is malformed ({type(exc).__name__}: {exc})") from exc
    return Summary(summary=" ".join(text), pages=pages)


def _svg_summary(path: Path) -> Summary:
    root = parse_svg(path)
    elements = sum(1 for _ in root.iter())
    return Summary(
        summary=f"SVG drawing with {elements} elements.",
        width=_svg_length(root.get("width")),
        height=_svg_length(root.get("height")),
    )


def _image_summary(path: Path) -> Summary:
    pil = require_pillow()
    try:
        with pil.open(path) as image:
            width, height, mode = image.width, image.height, image.mode
    except (OSError, ValueError, pil.DecompressionBombError) as exc:
        raise invalid_source(path, "it is not a readable image") from exc
    return Summary(summary=f"{width}x{height} {mode} image.", width=width, height=height)


def _media_summary(path: Path) -> Summary:
    entries = "format=duration:stream=codec_type,width,height"
    output = media_tool(
        path, "ffprobe", ["-v", "error", "-show_entries", entries, "-of", "json", str(path)]
    )
    payload = json.loads(output)
    video = next((s for s in payload.get("streams", []) if s.get("codec_type") == "video"), {})
    duration = payload.get("format", {}).get("duration")
    seconds = round(float(duration), 3) if duration else None
    width, height = video.get("width"), video.get("height")
    shape = f"{width}x{height} video" if width else "Audio"
    length = f", {seconds:g} s" if seconds is not None else ""
    return Summary(
        summary=f"{shape}{length}.", width=width, height=height, duration_seconds=seconds
    )


def media_tool(path: Path, tool: str, args: list[str]) -> bytes:
    try:
        return process.run(tool, args)
    except DirectorError as error:
        if error.code == "media_error":
            raise invalid_source(path, f"{tool} cannot read it") from error
        raise


def parse_svg(path: Path) -> ET.Element:
    try:
        return ET.parse(path).getroot()
    except ET.ParseError as exc:
        raise invalid_source(path, f"the SVG is malformed ({exc})") from exc
    except OSError as exc:
        raise io_error(path, exc) from exc


def _svg_length(value: str | None) -> int | None:
    match = re.fullmatch(r"\s*(\d+(?:\.\d+)?)\s*(?:px)?\s*", value or "")
    return round(float(match.group(1))) if match else None


def local_name(tag: str) -> str:
    return tag.rsplit("}", 1)[-1]


def _read_text(path: Path) -> str:
    try:
        with path.open("rb") as handle:
            return handle.read(TEXT_SAMPLE_BYTES).decode("utf-8", errors="replace")
    except OSError as exc:
        raise io_error(path, exc) from exc


def read_json(path: Path, *, strict: bool = False) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError, RecursionError) as exc:
        if strict:
            reason = "it nests too deeply" if isinstance(exc, RecursionError) else exc
            raise invalid_source(path, f"it is not valid JSON ({reason})") from None
        return None
    except OSError as exc:
        raise io_error(path, exc) from exc


def _strings(value: Any) -> bool:
    return isinstance(value, list) and all(isinstance(item, str) for item in value)


def compact(text: str) -> str:
    text = re.sub(r"\s+", " ", text).strip()
    return text if len(text) <= SUMMARY_CHARS else text[: SUMMARY_CHARS - 1].rstrip() + "…"
