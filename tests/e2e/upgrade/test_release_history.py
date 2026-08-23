from __future__ import annotations

import json
import subprocess

import pytest

from tests.e2e.support import agentxctl, run


@pytest.mark.cluster
@pytest.mark.upgrade
def test_every_plane_has_helm_history(installed_agentx: dict[str, str]) -> None:
    releases = (
        ("agentx-dependencies", installed_agentx["dependencies_namespace"]),
        ("agentx-control", installed_agentx["control_namespace"]),
        ("agentx-runtime", installed_agentx["runtime_namespace"]),
        ("agentx-observability", installed_agentx["runtime_namespace"]),
    )
    for release, namespace in releases:
        result = subprocess.run(
            ["helm", "history", release, "-n", namespace, "-o", "json"],
            check=True,
            capture_output=True,
            text=True,
            shell=False,
        )
        assert json.loads(result.stdout)


@pytest.mark.cluster
@pytest.mark.upgrade
def test_runtime_upgrade_preserves_replicas_and_can_rollback(installed_agentx: dict[str, str]) -> None:
    namespace = installed_agentx["runtime_namespace"]
    history = run(("helm", "history", "agentx-runtime", "-n", namespace, "-o", "json"), timeout=60).json()
    previous_revision = int(history[-1]["revision"])
    run(("kubectl", "-n", namespace, "scale", "deployment/workflow-runtime", "--replicas=2"), timeout=60)
    run(
        (
            agentxctl(),
            "upgrade",
            "--values",
            installed_agentx["values"],
            "--run-id",
            installed_agentx["run_id"],
            "--target",
            "runtime",
            "--output",
            "json",
        ),
        timeout=1200,
    )
    deployment = run(
        ("kubectl", "-n", namespace, "get", "deployment", "workflow-runtime", "-o", "json"), timeout=60
    ).json()
    assert deployment["spec"]["replicas"] == 2
    canonical_before = run(
        (
            "kubectl",
            "-n",
            installed_agentx["dependencies_namespace"],
            "get",
            "secret",
            "agentx-dependencies-secrets",
            "-o",
            "json",
        ),
        timeout=60,
    ).json()["data"]
    run(
        (
            agentxctl(),
            "rollback",
            "--values",
            installed_agentx["values"],
            "--run-id",
            installed_agentx["run_id"],
            "--target",
            "runtime",
            "--revision",
            str(previous_revision),
            "--output",
            "json",
        ),
        timeout=1200,
    )
    canonical_after = run(
        (
            "kubectl",
            "-n",
            installed_agentx["dependencies_namespace"],
            "get",
            "secret",
            "agentx-dependencies-secrets",
            "-o",
            "json",
        ),
        timeout=60,
    ).json()["data"]
    assert canonical_after == canonical_before


@pytest.mark.cluster
@pytest.mark.upgrade
def test_dependencies_uninstall_is_refused_while_runtime_exists(installed_agentx: dict[str, str]) -> None:
    result = run(
        (
            agentxctl(),
            "uninstall",
            "--values",
            installed_agentx["values"],
            "--run-id",
            installed_agentx["run_id"],
            "--target",
            "dependencies",
        ),
        check=False,
        timeout=60,
    )
    assert result.returncode != 0
    assert "refused" in result.stderr
