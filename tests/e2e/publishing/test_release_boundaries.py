from __future__ import annotations

import subprocess

import pytest


@pytest.mark.cluster
@pytest.mark.publishing
def test_control_and_runtime_have_independent_releases(installed_agentx: dict[str, str]) -> None:
    for release, namespace in (
        ("agentx-control", installed_agentx["control_namespace"]),
        ("agentx-runtime", installed_agentx["runtime_namespace"]),
    ):
        subprocess.run(["helm", "status", release, "-n", namespace], check=True, shell=False)
