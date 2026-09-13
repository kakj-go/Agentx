from __future__ import annotations

import json
import tomllib
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[2]


def test_workspace_members_and_local_dependencies_resolve_inside_the_new_layout() -> None:
    cargo = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]
    for member in cargo["members"]:
        assert member.startswith(("src/crates/", "src/services/", "tools/", "tests/fixtures/")), member
        assert (ROOT / member / "Cargo.toml").is_file(), member
    for dependency in cargo["dependencies"].values():
        if isinstance(dependency, dict) and "path" in dependency:
            assert (ROOT / dependency["path"] / "Cargo.toml").is_file(), dependency

    lock = yaml.safe_load((ROOT / "pnpm-lock.yaml").read_text(encoding="utf-8"))
    for importer, definition in lock["importers"].items():
        assert (ROOT / importer / "package.json").is_file(), importer
        for group in ("dependencies", "devDependencies", "optionalDependencies"):
            for dependency in definition.get(group, {}).values():
                version = dependency["version"]
                if version.startswith("link:"):
                    assert (ROOT / importer / version.removeprefix("link:") / "package.json").is_file(), dependency
                elif version.startswith("file:"):
                    assert (ROOT / version.removeprefix("file:") / "package.json").is_file(), dependency


def test_builtin_manifest_runtime_entries_are_present_in_the_source_checkout() -> None:
    for directory in (ROOT / "src/plugins/builtin").iterdir():
        manifest = json.loads((directory / "manifest.json").read_text(encoding="utf-8"))
        if (entry := manifest.get("runtimeEntry")) and entry != "native":
            assert (directory / entry).is_file(), directory


def test_retired_root_source_directories_are_absent() -> None:
    for name in (
        "apps",
        "crates",
        "services",
        "packages",
        "plugins",
        "templates",
        "openapi",
        "schemas",
        "vendor",
        "migrations",
        "scripts",
        "xtask",
    ):
        assert not (ROOT / name).exists(), name
    assert not (ROOT / "skills-lock.json").exists()
    assert (ROOT / "docs/todolist.md").is_file()
