#!/usr/bin/env python3
"""Create a deterministic platform release archive for Manim Director."""

from __future__ import annotations

import argparse
import gzip
import io
from pathlib import Path
import stat
import tarfile
import zipfile

from release_integrity import RELEASE_TARGETS, ROOT, SEMVER


LEGAL_FILES = ("LICENSE", "THIRD_PARTY_NOTICES.md")


def validate_release_name(version: str, target: str) -> str:
    if not SEMVER.fullmatch(version):
        raise ValueError(f"Invalid semantic release version: {version!r}")
    try:
        return RELEASE_TARGETS[target]
    except KeyError as exc:
        raise ValueError(f"Unsupported release target: {target!r}") from exc


def release_members(binary: Path, binary_name: str) -> list[tuple[str, bytes, int]]:
    members = [(binary_name, binary.read_bytes(), 0o755)]
    for name in LEGAL_FILES:
        path = ROOT / name
        if not path.is_file() or path.is_symlink():
            raise ValueError(f"Release notice must be a regular non-symlink file: {path}")
        members.append((name, path.read_bytes(), 0o644))
    return members


def write_zip(output: Path, members: list[tuple[str, bytes, int]]) -> None:
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for name, payload, mode in members:
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.create_system = 3
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = (stat.S_IFREG | mode) << 16
            archive.writestr(info, payload, compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)


def write_tar_gz(output: Path, members: list[tuple[str, bytes, int]]) -> None:
    with output.open("wb") as raw_output:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw_output, mtime=0, compresslevel=9) as compressed:
            with tarfile.open(fileobj=compressed, mode="w", format=tarfile.GNU_FORMAT) as archive:
                for name, payload, mode in members:
                    info = tarfile.TarInfo(name)
                    info.size = len(payload)
                    info.mode = mode
                    info.mtime = 0
                    info.uid = 0
                    info.gid = 0
                    info.uname = ""
                    info.gname = ""
                    archive.addfile(info, io.BytesIO(payload))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--out", type=Path, default=Path("dist"))
    args = parser.parse_args()

    if not args.binary.is_file() or args.binary.is_symlink():
        raise SystemExit(f"Release binary must be a regular non-symlink file: {args.binary}")
    binary = args.binary.resolve()
    try:
        archive_kind = validate_release_name(args.version, args.target)
    except ValueError as exc:
        raise SystemExit(str(exc)) from exc
    args.out.mkdir(parents=True, exist_ok=True)
    stem = f"manim-director-v{args.version}-{args.target}"
    binary_name = "manim-director.exe" if archive_kind == "zip" else "manim-director"
    try:
        members = release_members(binary, binary_name)
    except (OSError, ValueError) as exc:
        raise SystemExit(str(exc)) from exc

    if archive_kind == "zip":
        output = args.out / f"{stem}.zip"
        write_zip(output, members)
    else:
        output = args.out / f"{stem}.tar.gz"
        write_tar_gz(output, members)

    print(output)


if __name__ == "__main__":
    main()
