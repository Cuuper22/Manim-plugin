#!/usr/bin/env python3
"""Build and install Manim Director from a source checkout."""

from __future__ import annotations

import argparse
import hashlib
import hmac
import json
import os
import platform
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import urllib.error
import urllib.request
import zipfile
from pathlib import Path, PurePosixPath
from typing import BinaryIO

ROOT = Path(__file__).resolve().parents[1]
BINARY_NAME = "manim-director.exe" if os.name == "nt" else "manim-director"
MAX_CHECKSUM_BYTES = 128 * 1024
MAX_RELEASE_BYTES = 512 * 1024 * 1024
MAX_NOTICE_BYTES = 1024 * 1024
NOTICE_MEMBERS = {"LICENSE", "THIRD_PARTY_NOTICES.md"}


def run(*args: str, cwd: Path = ROOT) -> None:
    print("+", " ".join(args), flush=True)
    subprocess.run(args, cwd=cwd, check=True)


def require(command: str, hint: str) -> None:
    if shutil.which(command) is None:
        raise SystemExit(f"Missing {command}. {hint}")


def venv_python(venv: Path) -> Path:
    return venv / ("Scripts/python.exe" if os.name == "nt" else "bin/python")


def release_version() -> str:
    manifest = json.loads((ROOT / ".codex-plugin" / "plugin.json").read_text())
    return str(manifest["version"])


def release_target() -> tuple[str, str]:
    machine = platform.machine().lower()
    if machine in {"amd64", "x86_64"}:
        arch = "x86_64"
    elif machine in {"arm64", "aarch64"}:
        arch = "aarch64"
    else:
        raise RuntimeError(f"No prebuilt release for architecture {machine}")

    if sys.platform.startswith("linux") and arch == "x86_64":
        return "x86_64-unknown-linux-musl", "tar.gz"
    if sys.platform == "darwin":
        return f"{arch}-apple-darwin", "tar.gz"
    if os.name == "nt" and arch == "x86_64":
        return "x86_64-pc-windows-msvc", "zip"
    raise RuntimeError(f"No prebuilt release for {sys.platform}/{arch}")


def download_file(url: str, destination: Path, max_bytes: int) -> None:
    request = urllib.request.Request(url, headers={"User-Agent": "manim-director-installer"})
    with urllib.request.urlopen(request, timeout=60) as response:
        content_length = response.headers.get("Content-Length")
        if content_length is not None:
            try:
                declared_size = int(content_length)
            except ValueError as exc:
                raise RuntimeError(f"Invalid Content-Length for {url}") from exc
            if declared_size < 0 or declared_size > max_bytes:
                raise RuntimeError(f"Download exceeds the {max_bytes}-byte limit: {url}")

        total = 0
        with destination.open("xb") as output:
            while chunk := response.read(1024 * 1024):
                total += len(chunk)
                if total > max_bytes:
                    raise RuntimeError(f"Download exceeds the {max_bytes}-byte limit: {url}")
                output.write(chunk)


def expected_sha256(manifest: str, asset: str) -> str:
    matches: list[str] = []
    for line_number, raw_line in enumerate(manifest.splitlines(), start=1):
        line = raw_line.strip()
        if not line:
            continue
        fields = line.split(maxsplit=1)
        if len(fields) != 2 or len(fields[0]) != 64:
            raise RuntimeError(f"Malformed SHA256SUMS line {line_number}")
        digest, filename = fields
        if any(character not in "0123456789abcdefABCDEF" for character in digest):
            raise RuntimeError(f"Malformed SHA256SUMS line {line_number}")
        filename = filename.removeprefix("*")
        if filename == asset:
            matches.append(digest.lower())
    if len(matches) != 1:
        raise RuntimeError(f"SHA256SUMS must contain exactly one checksum for {asset}")
    return matches[0]


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _validate_member_name(name: str, allowed_names: set[str]) -> None:
    member = PurePosixPath(name)
    if (
        not name
        or "\\" in name
        or "\0" in name
        or member.is_absolute()
        or len(member.parts) != 1
        or member.parts[0] in {".", ".."}
    ):
        raise RuntimeError(f"Release archive contains an unsafe path: {name!r}")
    if name not in allowed_names:
        raise RuntimeError(f"Release archive contains unexpected member: {name!r}")


def _copy_regular_member(source: BinaryIO, destination: Path, size: int) -> None:
    if size <= 0 or size > MAX_RELEASE_BYTES:
        raise RuntimeError("Release binary has an invalid size")
    total = 0
    with destination.open("xb") as output:
        while chunk := source.read(1024 * 1024):
            total += len(chunk)
            if total > size or total > MAX_RELEASE_BYTES:
                raise RuntimeError("Release binary exceeds its declared size")
            output.write(chunk)
    if total != size:
        raise RuntimeError("Release binary did not match its declared size")


def extract_binary(archive: Path, archive_kind: str, binary_name: str, destination: Path) -> None:
    expected_names = NOTICE_MEMBERS | {binary_name}
    try:
        if archive_kind == "zip":
            with zipfile.ZipFile(archive) as bundle:
                members = bundle.infolist()
                names = [member.filename for member in members]
                if len(names) != len(expected_names) or set(names) != expected_names:
                    raise RuntimeError(
                        "Release archive does not contain the exact signed release layout"
                    )
                for member in members:
                    _validate_member_name(member.filename, expected_names)
                    unix_mode = (member.external_attr >> 16) & 0xFFFF
                    file_type = stat.S_IFMT(unix_mode)
                    if member.is_dir() or file_type not in {0, stat.S_IFREG}:
                        raise RuntimeError("Release archive member is not a regular file")
                    if member.flag_bits & 0x1:
                        raise RuntimeError("Encrypted release archives are not supported")
                    if member.filename != binary_name and not (
                        0 < member.file_size <= MAX_NOTICE_BYTES
                    ):
                        raise RuntimeError("Release notice has an invalid size")
                binary = next(member for member in members if member.filename == binary_name)
                with bundle.open(binary, "r") as source:
                    _copy_regular_member(source, destination, binary.file_size)
        elif archive_kind == "tar.gz":
            with tarfile.open(archive, "r:gz") as bundle:
                members = bundle.getmembers()
                names = [member.name for member in members]
                if len(names) != len(expected_names) or set(names) != expected_names:
                    raise RuntimeError(
                        "Release archive does not contain the exact signed release layout"
                    )
                for member in members:
                    _validate_member_name(member.name, expected_names)
                    if not member.isfile():
                        raise RuntimeError("Release archive member is not a regular file")
                    if member.name != binary_name and not (0 < member.size <= MAX_NOTICE_BYTES):
                        raise RuntimeError("Release notice has an invalid size")
                binary = next(member for member in members if member.name == binary_name)
                source = bundle.extractfile(binary)
                if source is None:
                    raise RuntimeError("Release archive member could not be read")
                with source:
                    _copy_regular_member(source, destination, binary.size)
        else:
            raise RuntimeError(f"Unsupported release archive type: {archive_kind}")
    except (tarfile.TarError, zipfile.BadZipFile) as exc:
        raise RuntimeError("Release archive is invalid") from exc


def download_binary(destination: Path) -> None:
    version = release_version()
    target, archive_kind = release_target()
    asset = f"manim-director-v{version}-{target}.{archive_kind}"
    base = os.environ.get(
        "MANIM_DIRECTOR_RELEASE_BASE",
        f"https://github.com/Cuuper22/Manim-plugin/releases/download/v{version}",
    ).rstrip("/")
    archive_url = f"{base}/{asset}"
    checksum_url = f"{base}/SHA256SUMS"
    print(f"+ download {archive_url}", flush=True)

    with tempfile.TemporaryDirectory(prefix="manim-director-") as raw_tmp:
        tmp = Path(raw_tmp)
        archive = tmp / asset
        checksum_file = tmp / "SHA256SUMS"
        download_file(checksum_url, checksum_file, MAX_CHECKSUM_BYTES)
        try:
            checksum_manifest = checksum_file.read_text(encoding="ascii")
        except UnicodeDecodeError as exc:
            raise RuntimeError("SHA256SUMS is not ASCII text") from exc
        expected = expected_sha256(checksum_manifest, asset)
        download_file(archive_url, archive, MAX_RELEASE_BYTES)
        actual = sha256_file(archive)
        if not hmac.compare_digest(actual, expected):
            raise RuntimeError(f"SHA-256 mismatch for {asset}")
        print(f"+ verified SHA-256 {asset}", flush=True)

        extracted = tmp / BINARY_NAME
        extract_binary(archive, archive_kind, BINARY_NAME, extracted)
        install_executable(extracted, destination)


def build_binary(destination: Path) -> None:
    require("cargo", "Install Rust from https://rustup.rs.")
    require("npm", "Install Node.js 22 or newer.")
    run("npm", "ci", cwd=ROOT / "workbench")
    run("npm", "run", "build", cwd=ROOT / "workbench")
    run(
        "cargo",
        "build",
        "--release",
        "--locked",
        "-p",
        "manim-director-cli",
        "--bin",
        "manim-director",
    )
    binary = ROOT / "target" / "release" / BINARY_NAME
    if not binary.is_file():
        raise SystemExit(f"Build completed without expected binary: {binary}")
    install_executable(binary, destination)


def install_executable(source: Path, destination: Path) -> None:
    """Replace `destination` by rename: copying over a running engine fails ("text file busy")."""
    destination.parent.mkdir(parents=True, exist_ok=True)
    descriptor, raw_install = tempfile.mkstemp(
        prefix=f".{destination.name}.", dir=destination.parent
    )
    os.close(descriptor)
    install_tmp = Path(raw_install)
    try:
        shutil.copy2(source, install_tmp)
        install_tmp.chmod(install_tmp.stat().st_mode | 0o111)
        install_tmp.replace(destination)
    finally:
        install_tmp.unlink(missing_ok=True)


def install_runtime(venv: Path, with_manim: bool) -> None:
    if not venv_python(venv).exists():
        run(sys.executable, "-m", "venv", str(venv))
    command = [str(venv_python(venv)), "-m", "pip", "install"]
    if with_manim:
        command += [
            "--constraint",
            str(ROOT / "runtime" / "constraints-full.txt"),
            f"{ROOT / 'runtime'}[full]",
        ]
    else:
        command.append(str(ROOT / "runtime"))
    run(*command)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--with-manim",
        action="store_true",
        help="Install Manim and the full visual/math runtime extras.",
    )
    parser.add_argument(
        "--from-source",
        action="store_true",
        help="Compile locally instead of downloading the release binary.",
    )
    parser.add_argument(
        "--prefix",
        type=Path,
        default=Path(os.environ.get("MANIM_DIRECTOR_PREFIX", Path.home() / ".local")),
        help="Installation prefix (default: ~/.local).",
    )
    args = parser.parse_args()

    if sys.version_info < (3, 11):
        raise SystemExit("Manim Director requires Python 3.11 or newer.")

    prefix = args.prefix.expanduser().resolve()
    bin_dir = prefix / "bin"
    installed = bin_dir / BINARY_NAME
    runtime_venv = prefix / "share" / "manim-director" / "venv"

    if args.from_source:
        build_binary(installed)
    else:
        try:
            download_binary(installed)
        except (OSError, RuntimeError, urllib.error.URLError) as error:
            if not (shutil.which("cargo") and shutil.which("npm")):
                raise SystemExit(
                    f"Could not install a release binary: {error}. Re-run with --from-source."
                ) from error
            print(f"Release download failed ({error}); building locally instead.", flush=True)
            build_binary(installed)
    install_runtime(runtime_venv, args.with_manim)

    print(f"\nInstalled {installed} with its runtime in {runtime_venv}")
    if str(bin_dir) not in os.environ.get("PATH", "").split(os.pathsep):
        print(f"Add {bin_dir} to PATH.")
    print("Start a project: manim-director init my-film && cd my-film && manim-director doctor")


if __name__ == "__main__":
    main()
