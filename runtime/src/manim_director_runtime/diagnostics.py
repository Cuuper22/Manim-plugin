from __future__ import annotations

import re
from typing import Any, Mapping


RULES: tuple[tuple[str, str, re.Pattern[str], str], ...] = (
    ("python_syntax", "python", re.compile(r"SyntaxError:\s*(?P<message>.+)"), "Fix the reported Python syntax before rendering."),
    ("python_import", "dependency", re.compile(r"ModuleNotFoundError:\s*No module named ['\"](?P<module>[^'\"]+)"), "Install the missing package in the render environment or remove the import."),
    ("python_name", "python", re.compile(r"NameError:\s*(?P<message>.+)"), "Define or import the referenced name."),
    ("python_attribute", "version", re.compile(r"AttributeError:\s*(?P<message>.+)"), "Check the installed Manim API version and replace the unavailable attribute."),
    ("api_signature", "version", re.compile(r"TypeError:\s*(?P<message>.*(?:unexpected keyword argument|missing .* required positional argument).*)"), "The code and installed API likely disagree; inspect the installed Manim signature."),
    ("latex_missing", "latex", re.compile(r"(?:latex|pdflatex|xelatex): (?:not found|command not found)|No such file or directory: ['\"](?:latex|pdflatex|xelatex)"), "Install a LaTeX distribution and dvisvgm."),
    ("latex_error", "latex", re.compile(r"!\s*(?P<message>LaTeX Error:.+|Undefined control sequence\..*)"), "Open the generated .log near this error and correct the expression or preamble."),
    ("latex_package", "latex", re.compile(r"File [`'](?P<package>[^`']+\.sty)['`] not found"), "Install the missing TeX package or remove it from the template."),
    ("typst_error", "typst", re.compile(r"(?:error:|typst: error:)\s*(?P<message>.+)", re.IGNORECASE), "Correct the reported Typst markup or install the required Typst package."),
    ("ffmpeg_encoder", "ffmpeg", re.compile(r"Unknown encoder ['\"]?(?P<encoder>[^'\"\s]+)"), "Select a codec included in this FFmpeg build or install a build containing it."),
    ("ffmpeg_mux", "ffmpeg", re.compile(r"Could not write header.*|Invalid argument.*(?:mp4|webm|mov)", re.IGNORECASE), "Use a codec/container pair compatible with the requested format."),
    ("font_missing", "font", re.compile(r"font(?: family)? ['\"]?(?P<font>[^'\"\n]+)['\"]? not found", re.IGNORECASE), "Install the font or choose an available fallback."),
    ("opengl_context", "renderer", re.compile(r"(?:OpenGL|GLX|EGL).*(?:context|display).*(?:fail|error|unavailable)", re.IGNORECASE), "Use the Cairo renderer or provide a working OpenGL context/display."),
    ("asset_missing", "asset", re.compile(r"FileNotFoundError:\s*\[Errno 2\].*['\"](?P<path>[^'\"]+)['\"]"), "Restore the asset or correct its project-relative path."),
)

TRACE_LOCATION = re.compile(r'File "(?P<file>[^"]+)", line (?P<line>\d+)(?:, in (?P<function>.+))?')


def diagnose_text(text: str, *, limit: int = 20) -> dict[str, Any]:
    text = str(text or "")
    locations = [
        {"file": match.group("file"), "line": int(match.group("line")), "function": match.group("function")}
        for match in TRACE_LOCATION.finditer(text)
    ]
    issues: list[dict[str, Any]] = []
    seen: set[tuple[str, str]] = set()
    for code, category, pattern, hint in RULES:
        for match in pattern.finditer(text):
            captured = {k: v for k, v in match.groupdict().items() if v is not None}
            message = captured.get("message") or match.group(0).strip()
            key = (code, message)
            if key in seen:
                continue
            seen.add(key)
            issue = {"code": code, "category": category, "message": message, "hint": hint}
            issue.update(captured)
            if locations:
                issue["location"] = locations[-1]
            issues.append(issue)
            if len(issues) >= limit:
                break
        if len(issues) >= limit:
            break
    if not issues and text.strip():
        lines = [line.strip() for line in text.splitlines() if line.strip()]
        issues.append({
            "code": "unclassified_failure", "category": "unknown",
            "message": lines[-1][:500] if lines else "The process failed without diagnostic output.",
            "hint": "Inspect the command log near its final traceback or error line.",
            **({"location": locations[-1]} if locations else {}),
        })
    return {"issues": issues, "locations": locations[-10:], "recognized": bool(issues and issues[0]["code"] != "unclassified_failure")}


def diagnose(params: Mapping[str, Any]) -> dict[str, Any]:
    return diagnose_text(str(params.get("text", params.get("log", ""))), limit=int(params.get("limit", 20)))
