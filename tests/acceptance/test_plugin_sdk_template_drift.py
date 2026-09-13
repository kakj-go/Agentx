from __future__ import annotations

import os
import re
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]


@pytest.mark.parametrize(
    ("workspace_package", "vendor"),
    (
        ("@agentx/plugin-sdk", "plugin-sdk"),
        ("@agentx/plugin-ui", "plugin-ui"),
    ),
)
def test_template_sdk_declarations_are_generated_from_public_packages(
    tmp_path: Path, workspace_package: str, vendor: str
) -> None:
    pnpm = "corepack.cmd" if os.name == "nt" else "corepack"
    output = tmp_path / vendor
    output.mkdir()
    subprocess.run(
        (
            pnpm,
            "pnpm",
            "--filter",
            workspace_package,
            "exec",
            "tsc",
            "--declaration",
            "--emitDeclarationOnly",
            "--noEmit",
            "false",
            "--rootDir",
            "src",
            "--outDir",
            str(output),
        ),
        cwd=ROOT,
        check=True,
        shell=False,
        timeout=120,
    )
    generated = (output / "index.d.ts").read_text(encoding="utf-8").replace("\r\n", "\n")
    checked_in = (
        (ROOT / "src" / "plugins" / "templates" / "canvas-plugin" / "vendor" / vendor / "index.d.ts")
        .read_text(encoding="utf-8")
        .replace("\r\n", "\n")
    )
    assert checked_in == generated


def test_template_agents_document_links_and_versions_are_current() -> None:
    template = ROOT / "src" / "plugins" / "templates" / "canvas-plugin"
    agents = (template / "AGENTS.md").read_text(encoding="utf-8")
    for relative in re.findall(r"`(docs/[^`]+\.md)`", agents):
        assert (template / relative).is_file(), relative
    manifest = (template / "manifest.json").read_text(encoding="utf-8")
    assert '"protocolVersion": 1' in manifest
    assert '"sdkApiVersion": 2' in manifest
