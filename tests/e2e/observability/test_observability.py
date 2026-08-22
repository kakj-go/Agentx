from __future__ import annotations

import subprocess

import pytest


@pytest.mark.cluster
@pytest.mark.observability
def test_observability_is_available(installed_agentx: dict[str, str]) -> None:
    subprocess.run(
        [
            "kubectl",
            "-n",
            installed_agentx["runtime_namespace"],
            "rollout",
            "status",
            "deployment/observability",
            "--timeout=300s",
        ],
        check=True,
        shell=False,
    )
