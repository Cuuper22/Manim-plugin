from __future__ import annotations

import hashlib
import io
from pathlib import Path
import stat
import sys
import tarfile
import tempfile
import unittest
from unittest import mock
import zipfile


sys.path.insert(0, str(Path(__file__).resolve().parent))

import install  # noqa: E402
import package_release  # noqa: E402
import release_integrity  # noqa: E402


class VersionAndChecksumTests(unittest.TestCase):
    def test_repository_versions_match_release_tag(self) -> None:
        self.assertEqual(release_integrity.validate_versions(tag="v1.0.0"), "1.0.0")
        with self.assertRaisesRegex(ValueError, "must exactly equal"):
            release_integrity.validate_versions(tag="1.0.0")

    def test_component_drift_is_rejected(self) -> None:
        versions = {"plugin": "1.0.0", "workbench": "1.0.1"}
        with mock.patch.object(release_integrity, "component_versions", return_value=versions):
            with self.assertRaisesRegex(ValueError, "versions differ"):
                release_integrity.validate_versions()

    def test_release_package_names_are_allowlisted(self) -> None:
        self.assertEqual(
            package_release.validate_release_name("1.0.0", "x86_64-unknown-linux-musl"),
            "tar.gz",
        )
        self.assertEqual(
            package_release.validate_release_name("1.0.0", "x86_64-pc-windows-msvc"),
            "zip",
        )
        for version, target in (
            ("../1.0.0", "x86_64-pc-windows-msvc"),
            ("1.0.0", "../../unexpected"),
        ):
            with self.subTest(version=version, target=target), self.assertRaises(ValueError):
                package_release.validate_release_name(version, target)

    def test_checksum_parser_requires_one_exact_well_formed_entry(self) -> None:
        digest = "a" * 64
        asset = "manim-director-v1.0.0-test.tar.gz"
        self.assertEqual(install.expected_sha256(f"{digest}  {asset}\n", asset), digest)
        for manifest in (
            "",
            f"{digest}  another-file\n",
            f"{digest}  {asset}\n{digest}  {asset}\n",
            f"not-a-digest  {asset}\n",
        ):
            with self.subTest(manifest=manifest), self.assertRaises(RuntimeError):
                install.expected_sha256(manifest, asset)

    def test_manifest_covers_exact_release_matrix_in_sorted_order(self) -> None:
        with tempfile.TemporaryDirectory() as raw_tmp:
            directory = Path(raw_tmp)
            names = {
                f"manim-director-v1.0.0-{target}.{extension}"
                for target, extension in release_integrity.RELEASE_TARGETS.items()
            }
            for name in names:
                (directory / name).write_bytes(name.encode("ascii"))
            output = directory / "SHA256SUMS"
            release_integrity.write_checksums(directory, output)
            lines = output.read_text(encoding="ascii").splitlines()
            self.assertEqual([line.split("  ", 1)[1] for line in lines], sorted(names))
            for line in lines:
                digest, name = line.split("  ", 1)
                self.assertEqual(digest, hashlib.sha256(name.encode("ascii")).hexdigest())

            (directory / next(iter(names))).unlink()
            with self.assertRaisesRegex(ValueError, "incomplete"):
                release_integrity.write_checksums(directory, output)


class ArchiveExtractionTests(unittest.TestCase):
    @staticmethod
    def _zip_release_members(binary_name: str, contents: bytes) -> list[tuple[zipfile.ZipInfo | str, bytes]]:
        return [
            (binary_name, contents),
            ("LICENSE", b"MIT license"),
            ("THIRD_PARTY_NOTICES.md", b"Third-party notices"),
        ]

    @staticmethod
    def _tar_release_members(binary_name: str, contents: bytes) -> list[tuple[tarfile.TarInfo, bytes]]:
        return [
            (tarfile.TarInfo(binary_name), contents),
            (tarfile.TarInfo("LICENSE"), b"MIT license"),
            (tarfile.TarInfo("THIRD_PARTY_NOTICES.md"), b"Third-party notices"),
        ]

    @staticmethod
    def _write_zip(path: Path, members: list[tuple[zipfile.ZipInfo | str, bytes]]) -> None:
        with zipfile.ZipFile(path, "w", compression=zipfile.ZIP_DEFLATED) as bundle:
            for member, contents in members:
                bundle.writestr(member, contents)

    @staticmethod
    def _write_tar(path: Path, members: list[tuple[tarfile.TarInfo, bytes]]) -> None:
        with tarfile.open(path, "w:gz") as bundle:
            for member, contents in members:
                if member.isreg():
                    member.size = len(contents)
                    bundle.addfile(member, io.BytesIO(contents))
                else:
                    bundle.addfile(member)

    def test_extracts_binary_from_exact_signed_release_layout(self) -> None:
        with tempfile.TemporaryDirectory() as raw_tmp:
            directory = Path(raw_tmp)
            zip_path = directory / "release.zip"
            self._write_zip(zip_path, self._zip_release_members("manim-director", b"zip-binary"))
            zip_output = directory / "from-zip"
            install.extract_binary(zip_path, "zip", "manim-director", zip_output)
            self.assertEqual(zip_output.read_bytes(), b"zip-binary")

            tar_path = directory / "release.tar.gz"
            self._write_tar(tar_path, self._tar_release_members("manim-director", b"tar-binary"))
            tar_output = directory / "from-tar"
            install.extract_binary(tar_path, "tar.gz", "manim-director", tar_output)
            self.assertEqual(tar_output.read_bytes(), b"tar-binary")

    def test_zip_rejects_unsafe_paths_symlinks_and_extra_members(self) -> None:
        with tempfile.TemporaryDirectory() as raw_tmp:
            directory = Path(raw_tmp)
            notices = [("LICENSE", b"MIT"), ("THIRD_PARTY_NOTICES.md", b"notices")]
            unsafe_members: list[list[tuple[zipfile.ZipInfo | str, bytes]]] = [
                [("../manim-director", b"binary"), *notices],
                [("/manim-director", b"binary"), *notices],
                [("nested/manim-director", b"binary"), *notices],
                [("..\\manim-director", b"binary"), *notices],
                [*self._zip_release_members("manim-director", b"binary"), ("extra", b"extra")],
            ]
            symlink = zipfile.ZipInfo("manim-director")
            symlink.create_system = 3
            symlink.external_attr = (stat.S_IFLNK | 0o777) << 16
            unsafe_members.append([(symlink, b"elsewhere"), *notices])
            directory_member = zipfile.ZipInfo("manim-director/")
            directory_member.external_attr = (stat.S_IFDIR | 0o755) << 16
            unsafe_members.append([(directory_member, b""), *notices])

            for index, members in enumerate(unsafe_members):
                archive = directory / f"unsafe-{index}.zip"
                self._write_zip(archive, members)
                with self.subTest(index=index), self.assertRaises(RuntimeError):
                    install.extract_binary(archive, "zip", "manim-director", directory / f"output-{index}")

    def test_tar_rejects_unsafe_paths_and_non_regular_members(self) -> None:
        with tempfile.TemporaryDirectory() as raw_tmp:
            directory = Path(raw_tmp)
            members: list[tarfile.TarInfo] = [
                tarfile.TarInfo("../manim-director"),
                tarfile.TarInfo("/manim-director"),
                tarfile.TarInfo("nested/manim-director"),
            ]
            for member_type in (tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.CHRTYPE):
                member = tarfile.TarInfo("manim-director")
                member.type = member_type
                member.linkname = "elsewhere"
                members.append(member)

            for index, member in enumerate(members):
                archive = directory / f"unsafe-{index}.tar.gz"
                release_members = [(member, b"binary"), *self._tar_release_members("placeholder", b"")[1:]]
                self._write_tar(archive, release_members)
                with self.subTest(index=index), self.assertRaises(RuntimeError):
                    install.extract_binary(archive, "tar.gz", "manim-director", directory / f"output-{index}")

            multiple = directory / "multiple.tar.gz"
            extra = tarfile.TarInfo("extra")
            self._write_tar(multiple, [*self._tar_release_members("manim-director", b"x"), (extra, b"x")])
            with self.assertRaisesRegex(RuntimeError, "exact signed release layout"):
                install.extract_binary(multiple, "tar.gz", "manim-director", directory / "multiple-output")

    def test_release_archives_are_deterministic_and_include_notices(self) -> None:
        members = [
            ("manim-director", b"binary", 0o755),
            ("LICENSE", b"MIT license", 0o644),
            ("THIRD_PARTY_NOTICES.md", b"notices", 0o644),
        ]
        with tempfile.TemporaryDirectory() as raw_tmp:
            directory = Path(raw_tmp)
            first = directory / "first.tar.gz"
            second = directory / "second.tar.gz"
            package_release.write_tar_gz(first, members)
            package_release.write_tar_gz(second, members)
            self.assertEqual(first.read_bytes(), second.read_bytes())
            with tarfile.open(first, "r:gz") as bundle:
                archived = bundle.getmembers()
            self.assertEqual([member.name for member in archived], [name for name, _, _ in members])
            self.assertTrue(all(member.uid == 0 and member.gid == 0 and member.mtime == 0 for member in archived))

    def test_checksum_mismatch_never_opens_archive_or_replaces_destination(self) -> None:
        with tempfile.TemporaryDirectory() as raw_tmp:
            directory = Path(raw_tmp)
            destination = directory / "installed"
            destination.write_bytes(b"existing-binary")

            def fake_download(url: str, path: Path, _max_bytes: int) -> None:
                if url.endswith("SHA256SUMS"):
                    asset = "manim-director-v1.0.0-test.tar.gz"
                    path.write_text(f"{'0' * 64}  {asset}\n", encoding="ascii")
                else:
                    path.write_bytes(b"not-the-published-archive")

            with (
                mock.patch.object(install, "release_version", return_value="1.0.0"),
                mock.patch.object(install, "release_target", return_value=("test", "tar.gz")),
                mock.patch.object(install, "download_file", side_effect=fake_download),
                mock.patch.object(install, "extract_binary") as extract,
            ):
                with self.assertRaisesRegex(RuntimeError, "SHA-256 mismatch"):
                    install.download_binary(destination)
                extract.assert_not_called()
            self.assertEqual(destination.read_bytes(), b"existing-binary")

    def test_declared_oversize_member_is_rejected_before_write(self) -> None:
        with tempfile.TemporaryDirectory() as raw_tmp:
            destination = Path(raw_tmp) / "binary"
            with self.assertRaisesRegex(RuntimeError, "invalid size"):
                install._copy_regular_member(io.BytesIO(b"x"), destination, install.MAX_RELEASE_BYTES + 1)
            self.assertFalse(destination.exists())


if __name__ == "__main__":
    unittest.main()
