from __future__ import annotations

import argparse
import sys
from pathlib import Path

from agentx_deploy.config import load_values, repository_root
from agentx_deploy.helm import lint, template
from agentx_deploy.process import run


def _line_limit(root: Path) -> None:
    candidates = [
        *root.glob("apps/web/src/**/*.ts"),
        *root.glob("apps/web/src/**/*.tsx"),
        *root.glob("services/**/*.rs"),
        *root.glob("crates/**/*.rs"),
        *root.glob("deploy/python/**/*.py"),
        *root.glob("tools/python/**/*.py"),
        *root.glob("tests/**/*.py"),
    ]
    oversized = [
        (path, len(path.read_text(encoding="utf-8").splitlines()))
        for path in candidates
        if len(path.read_text(encoding="utf-8").splitlines()) > 2000
    ]
    if oversized:
        details = ", ".join(f"{path.relative_to(root)}={lines}" for path, lines in oversized)
        raise RuntimeError(f"2000-line limit exceeded: {details}")


def main(argv: list[str] | None = None) -> None:
    parser = argparse.ArgumentParser(prog="agentx-check")
    parser.add_argument("--fast", action="store_true", help="skip Rust and Web test suites")
    args = parser.parse_args(argv)
    root = repository_root()
    run((sys.executable, "-m", "ruff", "check", "."), cwd=root, timeout=300)
    run((sys.executable, "-m", "ruff", "format", "--check", "."), cwd=root, timeout=300)
    run(("uv", "lock", "--check"), cwd=root, timeout=300)
    run((sys.executable, "-m", "pytest", "deploy/tests"), cwd=root, timeout=600)
    for file in ("local.yaml", "dockerhub-beta.yaml", "production.example.yaml"):
        config = load_values(root / "deploy" / "values" / file)
        lint(config, ("dependencies", "control", "runtime", "observability"))
        for target in ("dependencies", "control", "runtime", "observability"):
            template(config, target)
    for directory in (
        "deploy/kustomize/addons/lightrag",
        "deploy/kustomize/addons/mem0",
        "deploy/kustomize/e2e-fixtures/runtime-providers",
    ):
        run(("kubectl", "kustomize", directory), cwd=root, timeout=120)
    _line_limit(root)
    if not args.fast:
        run(("cargo", "fmt", "--all", "--", "--check"), cwd=root, timeout=600)
        run(("cargo", "clippy", "--workspace", "--all-targets", "--", "-D", "warnings"), cwd=root, timeout=3600)
        run(("cargo", "test", "--workspace"), cwd=root, timeout=7200)
        run(("cargo", "run", "--quiet", "-p", "agentx-boundary-check", "--", "check", "."), cwd=root, timeout=1800)
        run(("pnpm", "lint:web"), cwd=root, timeout=1200)
        run(("pnpm", "--filter", "@agentx/web", "test"), cwd=root, timeout=1800)
        run(("pnpm", "build:web"), cwd=root, timeout=1800)
    run(("git", "diff", "--check"), cwd=root, timeout=300)
    print("Agentx checks passed")
