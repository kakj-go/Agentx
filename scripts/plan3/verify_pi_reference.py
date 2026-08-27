from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import tempfile
from pathlib import Path

EXPECTED_COMMIT = "a69bef789bc95abf0acee16f7b4660b70b650bb9"
EXPECTED_VERSION = "0.84.2"
REPOSITORY = "https://github.com/earendil-works/pi.git"
EXPECTED_BLOBS = {
    "packages/agent/src/agent-loop.ts": "a251fede0a9adb9c6cf5e2ba57b4ea2f65b8100b",
    "packages/agent/src/agent.ts": "0de7edd83029743e59a174c8b9b994282fd5f8a3",
    "packages/agent/src/harness/compaction/compaction.ts": "06ae8afb1dd0dc21215fef60ed8266e5a48649e6",
    "packages/agent/src/harness/session/context.ts": "d219b541ae1fbb74ac64e07201bc9d062972ea55",
    "packages/agent/src/harness/tools/read.ts": "5fdbdf6c04e3cb03b2d75d6f2eb83b0ec3f50ac3",
    "packages/agent/src/harness/tools/write.ts": "f7175284e83b7606a608254e249c8935b157eb40",
    "packages/agent/src/harness/tools/edit.ts": "5473c48b8a0d98829194bca542cad942272d4b1c",
    "packages/agent/src/harness/tools/bash.ts": "c0e1f19da9f2796aa9b1bcab1145842c360afd7a",
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Verify the fixed Pi reference used by Agentx Plan3.")
    parser.add_argument("--source", type=Path, help="Existing Pi checkout; defaults to a retained temporary checkout.")
    parser.add_argument("--run-upstream-tests", action="store_true", help="Run the selected upstream npm tests.")
    parser.add_argument(
        "--run-bash-tests",
        action="store_true",
        help="Also run upstream Bash tool tests; requires --run-upstream-tests and a working /bin/bash.",
    )
    args = parser.parse_args()
    if args.run_bash_tests and not args.run_upstream_tests:
        parser.error("--run-bash-tests requires --run-upstream-tests")
    return args


def executable(name: str) -> str:
    path = shutil.which(name)
    if path is None:
        raise RuntimeError(f"required executable not found: {name}")
    return path


def run(command: list[str], *, capture_output: bool = False, error: str) -> str:
    try:
        completed = subprocess.run(
            command,
            check=True,
            capture_output=capture_output,
            text=True,
        )
    except subprocess.CalledProcessError as exc:
        raise RuntimeError(error) from exc
    return completed.stdout.strip() if capture_output else ""


def prepare_source(requested_source: Path | None, git: str) -> tuple[Path, bool]:
    if requested_source is not None:
        source = requested_source.expanduser().resolve()
        if not source.is_dir():
            raise RuntimeError(f"Pi source directory does not exist: {source}")
        return source, False

    source = Path(tempfile.gettempdir()) / f"agentx-plan3-pi-{EXPECTED_COMMIT}"
    if source.exists() and not source.is_dir():
        raise RuntimeError(f"Pi checkout path is not a directory: {source}")
    if source.is_dir():
        return source.resolve(), False

    run(
        [git, "clone", "--filter=blob:none", "--no-checkout", REPOSITORY, str(source)],
        error="clone Pi reference failed",
    )
    return source.resolve(), True


def verify_source(source: Path, git: str) -> str:
    run(
        [git, "-C", str(source), "checkout", "--detach", EXPECTED_COMMIT],
        capture_output=True,
        error="checkout Pi reference failed",
    )
    actual_commit = run(
        [git, "-C", str(source), "rev-parse", "HEAD"],
        capture_output=True,
        error="read Pi reference commit failed",
    )
    if actual_commit != EXPECTED_COMMIT:
        raise RuntimeError(f"unexpected Pi commit: {actual_commit}")

    package_path = source / "packages" / "agent" / "package.json"
    package = json.loads(package_path.read_text(encoding="utf-8"))
    if package.get("name") != "@earendil-works/pi-agent-core" or package.get("version") != EXPECTED_VERSION:
        raise RuntimeError(f"unexpected Pi agent package: {package.get('name')}@{package.get('version')}")
    if package.get("license") != "MIT":
        raise RuntimeError(f"unexpected Pi agent package license: {package.get('license')}")

    license_text = (source / "LICENSE").read_text(encoding="utf-8")
    if not license_text.startswith("MIT License") or "Copyright (c) 2025 Mario Zechner" not in license_text:
        raise RuntimeError("Pi license or attribution changed")

    for relative_path, expected_blob in EXPECTED_BLOBS.items():
        actual_blob = run(
            [git, "-C", str(source), "rev-parse", f"HEAD:{relative_path}"],
            capture_output=True,
            error=f"read Pi reference blob failed: {relative_path}",
        )
        if actual_blob != expected_blob:
            raise RuntimeError(f"Pi reference blob drift: {relative_path}={actual_blob}")
    return actual_commit


def verify_fixtures() -> int:
    repository_root = Path(__file__).resolve().parents[2]
    fixture_path = repository_root / "docs" / "plan3" / "fixtures" / "agent-core" / "cases.jsonl"
    case_ids: set[str] = set()
    for line in fixture_path.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        case = json.loads(line)
        case_id = case.get("case_id")
        if case.get("reference_commit") != EXPECTED_COMMIT or case.get("reference_package_version") != EXPECTED_VERSION:
            raise RuntimeError(f"fixture {case_id} is not bound to the fixed Pi reference")
        if case_id in case_ids:
            raise RuntimeError(f"duplicate fixture case: {case_id}")
        case_ids.add(case_id)
    return len(case_ids)


def run_upstream_tests(source: Path, *, include_bash: bool) -> None:
    npm = executable("npm")
    if not (source / "node_modules").is_dir():
        run([npm, "ci", "--prefix", str(source)], error="Pi npm ci failed")

    agent_package = source / "packages" / "agent"
    run(
        [
            npm,
            "test",
            "--prefix",
            str(agent_package),
            "--",
            "test/agent-loop.test.ts",
            "test/harness/compaction.test.ts",
            "test/harness/session/context.test.ts",
        ],
        error="Pi upstream loop/context/compaction tests failed",
    )
    run(
        [npm, "test", "--prefix", str(agent_package), "--", "test/harness/tools.test.ts", "-t", "read|write|edit"],
        error="Pi upstream portable file tool tests failed",
    )
    if include_bash:
        run(
            [npm, "test", "--prefix", str(agent_package), "--", "test/harness/tools.test.ts", "-t", "bash"],
            error="Pi upstream bash tests failed; verify WSL /bin/bash availability",
        )


def main() -> None:
    args = parse_args()
    git = executable("git")
    source, owned_temporary_checkout = prepare_source(args.source, git)
    actual_commit = verify_source(source, git)
    fixture_count = verify_fixtures()
    if args.run_upstream_tests:
        run_upstream_tests(source, include_bash=args.run_bash_tests)

    print(
        f"Pi reference verified: {actual_commit} / "
        f"@earendil-works/pi-agent-core@{EXPECTED_VERSION} / {fixture_count} fixtures"
    )
    if owned_temporary_checkout:
        print(f"Reference checkout retained for repeatability: {source}")


if __name__ == "__main__":
    main()
