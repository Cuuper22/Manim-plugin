"""One cross-process lock around Manim's shared TeX and SVG scratch files.

Concurrent renders of one project share `<media_dir>/Tex`. Manim checks for `<hash>.tex/.dvi/.svg`
and then writes them with no lock, deletes every other job's `.dvi`/`.log` after each compile,
and parses an SVG through a fixed `<name>_.svg` temp file beside it. Two workers that typeset the
same formula read each other's half-written files. Holding one lock for a compile (a cache hit is
only an `exists()` check) and for an SVG parse makes both safe while renders stay parallel.
"""

from __future__ import annotations

import functools
import os
from collections.abc import Callable, Iterator
from contextlib import contextmanager
from pathlib import Path
from typing import Any

_LOCK_NAME = ".manim-director-tex.lock"  # beside Tex/, which Manim's cleanup sweeps
_held = False  # the lock is per file descriptor, so a nested call in this process must not wait


def guard_shared_caches() -> None:
    """Wrap the Manim functions that touch the shared files; idempotent per process."""

    from manim.mobject.svg import svg_mobject
    from manim.mobject.text import tex_mobject
    from manim.utils import tex_file_writing

    if getattr(tex_mobject.tex_to_svg_file, "__wrapped__", None) is None:
        # tex_mobject binds the name at import, so both modules need the wrapper.
        guarded = _locked(tex_file_writing.tex_to_svg_file)
        tex_file_writing.tex_to_svg_file = guarded
        tex_mobject.tex_to_svg_file = guarded
    svg = svg_mobject.SVGMobject
    if getattr(svg.generate_mobject, "__wrapped__", None) is None:
        svg.generate_mobject = _locked(svg.generate_mobject)


def _locked(function: Callable[..., Any]) -> Callable[..., Any]:
    @functools.wraps(function)
    def wrapper(*args: Any, **kwargs: Any) -> Any:
        with media_lock():
            return function(*args, **kwargs)

    return wrapper


@contextmanager
def media_lock() -> Iterator[None]:
    """Hold the project's lock; its path follows the config, as tempconfig may change it."""

    global _held
    if _held:
        yield
        return
    from manim import config

    tex_dir = Path(config.get_dir("tex_dir"))
    tex_dir.parent.mkdir(parents=True, exist_ok=True)
    fd = os.open(tex_dir.parent / _LOCK_NAME, os.O_RDWR | os.O_CREAT, 0o644)
    try:
        _acquire(fd)
        _held = True
        try:
            yield
        finally:
            _held = False
    finally:
        os.close(fd)  # closing releases the lock; so does the process dying mid-compile


def _acquire(fd: int) -> None:
    if os.name != "nt":
        import fcntl

        fcntl.flock(fd, fcntl.LOCK_EX)
        return
    import msvcrt

    while True:
        try:
            msvcrt.locking(fd, msvcrt.LK_LOCK, 1)
            return
        except OSError:  # LK_LOCK gives up after ~10 s; a cold TeX compile can take longer
            continue
