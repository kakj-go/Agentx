from __future__ import annotations

import zipfile
from pathlib import Path

import pytest

from tools.scripts.release.package_agentxctl import TARGETS, checksum, create_package, unpack_package


@pytest.mark.parametrize("target", TARGETS)
def test_release_archive_preserves_the_binary_and_exact_contents(tmp_path: Path, target: str) -> None:
    source = tmp_path / "source-binary"
    source.write_bytes(b"release-fixture-binary\x00\x01")
    source.chmod(0o755)
    archive, standalone = create_package(source, "1.2.3-test", target, tmp_path / "output")
    package = unpack_package(archive, tmp_path / "extracted", "1.2.3-test", target)
    assert (package / TARGETS[target][0]).read_bytes() == source.read_bytes()
    assert standalone.read_bytes() == source.read_bytes()
    assert (package / "LICENSE").is_file()
    assert len([path for path in package.rglob("*") if path.is_file()]) == 5
    for artifact in (archive, standalone):
        assert artifact.with_name(artifact.name + ".sha256").read_text(encoding="utf-8") == (
            f"{checksum(artifact)}  {artifact.name}\n"
        )


def test_release_archive_rejects_extra_files(tmp_path: Path) -> None:
    source = tmp_path / "source-binary"
    source.write_bytes(b"release-fixture")
    target = "x86_64-pc-windows-msvc"
    archive, _ = create_package(source, "1.2.3-test", target, tmp_path / "output")
    with zipfile.ZipFile(archive, "a") as stream:
        stream.writestr("unexpected.txt", "unexpected")
    with pytest.raises(ValueError, match="five-file contract"):
        unpack_package(archive, tmp_path / "extracted", "1.2.3-test", target)
