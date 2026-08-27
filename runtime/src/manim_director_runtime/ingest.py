from __future__ import annotations

import ast
import csv
import json
import mimetypes
import re
import shutil
import xml.etree.ElementTree as ET
from pathlib import Path
from typing import Any, Mapping, Sequence

from .assets import AUDIO, RASTER, VIDEO, describe_asset
from .errors import DirectorError
from .util import Emit, atomic_write, confined_path, noop_emit, project_root


TEXT_LIMIT = 256_000
SUMMARY_LIMIT = 2_000
SUPPORTED = {
    ".md": "markdown", ".markdown": "markdown", ".tex": "latex", ".latex": "latex",
    ".typ": "typst", ".csv": "csv", ".json": "json", ".py": "python",
    ".ipynb": "notebook", ".pdf": "pdf", ".svg": "svg",
}


def _read_text(path: Path, limit: int = TEXT_LIMIT) -> tuple[str, bool]:
    with path.open("rb") as handle:
        raw = handle.read(limit + 1)
    truncated = len(raw) > limit
    return raw[:limit].decode("utf-8", errors="replace"), truncated


def _compact_text(value: str, limit: int = SUMMARY_LIMIT) -> str:
    value = re.sub(r"```.*?```", " ", value, flags=re.DOTALL)
    value = re.sub(r"[`*_>#]+", " ", value)
    value = re.sub(r"\s+", " ", value).strip()
    if len(value) <= limit:
        return value
    return value[: max(0, limit - 1)].rstrip() + "…"


def _text_summary(path: Path, kind: str) -> dict[str, Any]:
    text, truncated = _read_text(path)
    lines = text.splitlines()
    if kind == "markdown":
        headings = [match.group(2).strip() for line in lines if (match := re.match(r"^(#{1,6})\s+(.+)", line))][:30]
        summary = _compact_text(text)
        details = {"headings": headings, "code_blocks": text.count("```") // 2, "links": len(re.findall(r"\[[^]]+\]\([^)]+\)", text))}
    elif kind == "latex":
        cleaned = re.sub(r"(?m)(?<!\\)%.*$", "", text)
        headings = [match.group(2).strip() for match in re.finditer(r"\\(section|subsection|chapter)\*?\{([^}]*)\}", cleaned)][:30]
        prose = re.sub(r"\\[A-Za-z@]+\*?(?:\[[^]]*\])?", " ", cleaned)
        prose = prose.replace("{", " ").replace("}", " ").replace("$", " ")
        summary = _compact_text(prose)
        details = {"headings": headings, "equation_environments": len(re.findall(r"\\begin\{(?:equation|align|gather|multline)\*?\}", cleaned)), "document_classes": re.findall(r"\\documentclass(?:\[[^]]*\])?\{([^}]+)\}", cleaned)[:5]}
    else:
        headings = [match.group(2).strip() for line in lines if (match := re.match(r"^(={1,6})\s+(.+)", line))][:30]
        prose = re.sub(r"#(?:import|include)\s+[^\n]+", " ", text)
        summary = _compact_text(prose.replace("$", " "))
        details = {"headings": headings, "equation_delimiters": text.count("$") // 2, "imports": re.findall(r"#(?:import|include)\s+([^\n]+)", text)[:20]}
    return {"summary": summary, "lines_sampled": len(lines), "words_sampled": len(re.findall(r"\b\w+\b", text)), "text_sample_truncated": truncated, **details}


def _csv_summary(path: Path) -> dict[str, Any]:
    rows = 0
    columns: list[str] = []
    widths: set[int] = set()
    dialect_name = "unknown"
    with path.open("r", encoding="utf-8-sig", errors="replace", newline="") as handle:
        sample = handle.read(8192)
        handle.seek(0)
        try:
            dialect = csv.Sniffer().sniff(sample)
            dialect_name = {",": "comma", "\t": "tab", ";": "semicolon", "|": "pipe"}.get(dialect.delimiter, dialect.delimiter)
        except csv.Error:
            dialect = csv.excel
        reader = csv.reader(handle, dialect)
        try:
            columns = [str(value)[:120] for value in next(reader)]
        except StopIteration:
            return {"summary": "Empty tabular source.", "columns": [], "rows": 0, "consistent_width": True, "dialect": dialect_name}
        for row in reader:
            rows += 1
            widths.add(len(row))
    return {
        "summary": f"Tabular source with {rows} data rows and {len(columns)} named columns.",
        "columns": columns[:100], "column_count": len(columns), "rows": rows,
        "consistent_width": not widths or widths == {len(columns)}, "observed_widths": sorted(widths)[:20], "dialect": dialect_name,
    }


def _json_shape(value: Any, depth: int = 0) -> dict[str, Any]:
    if depth >= 3:
        return {"type": type(value).__name__}
    if isinstance(value, dict):
        keys = [str(key) for key in list(value)[:50]]
        return {"type": "object", "keys": keys, "key_count": len(value), "sample": {key: _json_shape(value[key], depth + 1) for key in list(value)[:8]}}
    if isinstance(value, list):
        return {"type": "array", "length": len(value), "item": _json_shape(value[0], depth + 1) if value else None}
    return {"type": type(value).__name__}


def _json_summary(path: Path) -> dict[str, Any]:
    if path.stat().st_size > 64 * 1024 * 1024:
        raise DirectorError("json_too_large", "JSON ingestion is limited to 64 MiB; convert larger sources to CSV or split them")
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise DirectorError("invalid_json_source", f"JSON source is invalid: {path}", {"error": str(exc)}) from exc
    shape = _json_shape(value)
    return {"summary": f"JSON {shape['type']} source.", "shape": shape}


def _python_summary(path: Path) -> dict[str, Any]:
    text, truncated = _read_text(path)
    if truncated:
        functions = re.findall(r"(?m)^\s*(?:async\s+)?def\s+([A-Za-z_]\w*)\s*\(", text)[:100]
        classes = re.findall(r"(?m)^\s*class\s+([A-Za-z_]\w*)\s*(?:\([^\n]*\))?\s*:", text)[:100]
        scenes = [match.group(1) for match in re.finditer(r"(?m)^\s*class\s+([A-Za-z_]\w*)\s*\([^\n]*(?:Scene|Slide)[^\n]*\)\s*:", text)][:100]
        imports = re.findall(r"(?m)^\s*(?:from\s+([A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*)\s+import|import\s+([A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*))", text)
        modules = sorted({left or right for left, right in imports})[:100]
        valid_python: bool | None = None
        syntax_error = None
    else:
        try:
            tree = ast.parse(text, filename=str(path))
        except SyntaxError as exc:
            return {"summary": "Python source with a syntax error.", "valid_python": False, "syntax_error": {"message": exc.msg, "line": exc.lineno, "column": exc.offset}}
        functions = [node.name for node in tree.body if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))][:100]
        classes = [node.name for node in tree.body if isinstance(node, ast.ClassDef)][:100]
        scenes = [node.name for node in tree.body if isinstance(node, ast.ClassDef) and any((base.id if isinstance(base, ast.Name) else base.attr if isinstance(base, ast.Attribute) else "").endswith(("Scene", "Slide")) for base in node.bases)][:100]
        modules = sorted({node.module for node in tree.body if isinstance(node, ast.ImportFrom) and node.module} | {alias.name for node in tree.body if isinstance(node, ast.Import) for alias in node.names})[:100]
        valid_python = True
        syntax_error = None
    return {
        "summary": f"Python module with {len(classes)} classes, {len(functions)} functions, and {len(scenes)} apparent Manim scenes.",
        "valid_python": valid_python, "syntax_error": syntax_error, "classes": classes, "functions": functions,
        "scenes": scenes, "imports": modules,
        "text_sample_truncated": truncated,
    }


def _notebook_summary(path: Path) -> dict[str, Any]:
    if path.stat().st_size > 64 * 1024 * 1024:
        raise DirectorError("notebook_too_large", "Notebook ingestion is limited to 64 MiB")
    try:
        notebook = json.loads(path.read_text(encoding="utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise DirectorError("invalid_notebook", f"Notebook JSON is invalid: {path}") from exc
    cells = notebook.get("cells", []) if isinstance(notebook, dict) else []
    counts: dict[str, int] = {}
    headings = []
    output_count = 0
    for cell in cells:
        kind = str(cell.get("cell_type", "unknown"))
        counts[kind] = counts.get(kind, 0) + 1
        source = "".join(cell.get("source", []))
        if kind == "markdown":
            headings.extend(match.group(2).strip() for line in source.splitlines() if (match := re.match(r"^(#{1,6})\s+(.+)", line)))
        output_count += len(cell.get("outputs", []))
    language = ((notebook.get("metadata") or {}).get("language_info") or {}).get("name") if isinstance(notebook, dict) else None
    return {"summary": f"Notebook with {len(cells)} cells ({counts.get('code', 0)} code, {counts.get('markdown', 0)} markdown).", "cells": len(cells), "cell_types": counts, "language": language, "headings": headings[:30], "outputs": output_count}


def _pdf_summary(path: Path, max_pages: int) -> dict[str, Any]:
    try:
        from pypdf import PdfReader
    except ImportError as exc:
        raise DirectorError("pdf_dependency_missing", "PDF ingestion requires the optional `ingest` extra (`pip install manim-director-runtime[ingest]`).") from exc
    try:
        reader = PdfReader(str(path), strict=False)
        text_parts = []
        for page in reader.pages[:max_pages]:
            if sum(map(len, text_parts)) >= TEXT_LIMIT:
                break
            text_parts.append((page.extract_text() or "")[:TEXT_LIMIT])
        metadata = reader.metadata or {}
    except Exception as exc:
        raise DirectorError("pdf_read_failed", f"Could not read PDF: {path}", {"error": str(exc)}) from exc
    safe_metadata = {str(key).lstrip("/"): str(value)[:500] for key, value in list(metadata.items())[:30]}
    return {"summary": _compact_text(" ".join(text_parts)), "pages": len(reader.pages), "pages_sampled": min(len(reader.pages), max_pages), "metadata": safe_metadata, "text_sample_truncated": len(reader.pages) > max_pages or sum(map(len, text_parts)) >= TEXT_LIMIT}


def _svg_summary(path: Path) -> dict[str, Any]:
    try:
        root = ET.parse(path).getroot()
    except ET.ParseError as exc:
        raise DirectorError("invalid_svg", f"SVG is malformed: {path}", {"error": str(exc)}) from exc
    counts: dict[str, int] = {}
    for node in root.iter():
        name = node.tag.rsplit("}", 1)[-1]
        counts[name] = counts.get(name, 0) + 1
    return {"summary": f"SVG vector with {sum(counts.values())} elements.", "view_box": root.attrib.get("viewBox"), "width": root.attrib.get("width"), "height": root.attrib.get("height"), "elements": dict(sorted(counts.items())[:50])}


def _kind(path: Path, explicit: Any = None) -> str:
    if explicit:
        return str(explicit).lower()
    suffix = path.suffix.lower()
    if suffix in RASTER:
        return "image"
    if suffix in AUDIO:
        return "audio"
    if suffix in VIDEO:
        return "video"
    return SUPPORTED.get(suffix, "binary")


def _destination(directory: Path, source: Path, identifier: str | None, force: bool) -> Path:
    stem = re.sub(r"[^A-Za-z0-9_.-]+", "-", identifier or source.stem).strip(".-") or "source"
    candidate = directory / f"{stem}{source.suffix.lower()}"
    if force or not candidate.exists():
        return candidate
    for index in range(2, 10_000):
        candidate = directory / f"{stem}-{index}{source.suffix.lower()}"
        if not candidate.exists():
            return candidate
    raise DirectorError("source_collision", f"Could not allocate a destination for {source.name}")


def _summarize(path: Path, kind: str, root: Path, params: Mapping[str, Any]) -> dict[str, Any]:
    if kind in {"markdown", "latex", "typst"}:
        return _text_summary(path, kind)
    if kind == "csv":
        return _csv_summary(path)
    if kind == "json":
        return _json_summary(path)
    if kind == "python":
        return _python_summary(path)
    if kind == "notebook":
        return _notebook_summary(path)
    if kind == "pdf":
        return _pdf_summary(path, max(1, min(100, int(params.get("pdf_pages", 12)))))
    if kind == "svg":
        return _svg_summary(path)
    if kind in {"image", "audio", "video"}:
        metadata = describe_asset(path, root, hashes=False)
        return {"summary": f"{kind.title()} source ({metadata.get('mime_type')}, {metadata['bytes']} bytes).", "metadata": metadata}
    mime, _ = mimetypes.guess_type(path.name)
    return {"summary": f"Binary source ({mime or 'application/octet-stream'}, {path.stat().st_size} bytes)."}


def ingest(params: Mapping[str, Any], emit: Emit = noop_emit) -> dict[str, Any]:
    root = project_root(params)
    raw_sources = params.get("sources", params.get("paths"))
    if raw_sources is None and params.get("source") is not None:
        raw_sources = [params["source"]]
    if not isinstance(raw_sources, Sequence) or isinstance(raw_sources, (str, bytes)) or not raw_sources:
        raise DirectorError("sources_required", "Provide a non-empty sources list")
    destination_dir = confined_path(root, str(params.get("destination_dir", "sources")))
    destination_dir.mkdir(parents=True, exist_ok=True)
    per_source_limit = int(params.get("max_source_bytes", 256 * 1024 * 1024))
    total_limit = int(params.get("max_total_bytes", 1024 * 1024 * 1024))
    summary_chars = int(params.get("summary_chars", SUMMARY_LIMIT))
    if not (128 <= summary_chars <= 20_000):
        raise DirectorError("invalid_summary_limit", "summary_chars must be between 128 and 20000")
    total = 0
    items = []
    for index, raw in enumerate(raw_sources):
        descriptor = raw if isinstance(raw, Mapping) else {"path": raw}
        if not descriptor.get("path"):
            raise DirectorError("invalid_source", f"Source {index} has no path")
        source = Path(str(descriptor["path"])).expanduser().resolve()
        if not source.exists() or not source.is_file():
            raise DirectorError("source_not_found", f"Source does not exist: {source}")
        size = source.stat().st_size
        if size > per_source_limit or total + size > total_limit:
            raise DirectorError("ingest_budget_exceeded", "Source ingestion exceeds the configured byte budget", {"source": str(source), "bytes": size, "total_before": total})
        total += size
        identifier = str(descriptor.get("id")) if descriptor.get("id") else None
        destination = _destination(destination_dir, source, identifier, bool(params.get("force", False)))
        if source != destination:
            shutil.copy2(source, destination)
        kind = _kind(destination, descriptor.get("kind"))
        emit("ingest_progress", {"completed": index + 1, "total": len(raw_sources), "source": source.name, "kind": kind})
        summary = _summarize(destination, kind, root, params)
        item = {
            "id": identifier or destination.stem, "kind": kind,
            "path": destination.relative_to(root).as_posix(), "bytes": size,
            "source": str(descriptor.get("provenance", source)), **summary,
        }
        for key in ("license", "attribution", "notes"):
            if descriptor.get(key) is not None:
                item[key] = descriptor[key]
        # Hard bound all prose returned or persisted, even if a backend emitted more.
        item["summary"] = _compact_text(str(item.get("summary", "")), summary_chars)
        items.append(item)
    manifest_path = confined_path(root, str(params.get("manifest", "sources/manifest.json")))
    manifest = {"version": 1, "sources": items, "total_bytes": total}
    atomic_write(manifest_path, json.dumps(manifest, indent=2, ensure_ascii=False) + "\n")
    return {"manifest": str(manifest_path), "sources": items, "count": len(items), "total_bytes": total}
