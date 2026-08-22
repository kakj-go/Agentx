from __future__ import annotations

import subprocess

import pytest


@pytest.mark.cluster
@pytest.mark.runtime
def test_runtime_workloads_are_available(installed_agentx: dict[str, str]) -> None:
    namespace = installed_agentx["runtime_namespace"]
    for name in ("runtime-gateway", "workflow-runtime", "workflow-worker", "sandbox-manager"):
        subprocess.run(
            ["kubectl", "-n", namespace, "rollout", "status", f"deployment/{name}", "--timeout=300s"],
            check=True,
            shell=False,
        )
