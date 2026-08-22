from __future__ import annotations

import json
import subprocess

import pytest


@pytest.mark.cluster
@pytest.mark.security
def test_each_namespace_has_default_deny(installed_agentx: dict[str, str]) -> None:
    for key in ("control_namespace", "runtime_namespace", "dependencies_namespace"):
        result = subprocess.run(
            ["kubectl", "-n", installed_agentx[key], "get", "networkpolicy", "-o", "json"],
            check=True,
            capture_output=True,
            text=True,
            shell=False,
        )
        policies = json.loads(result.stdout)["items"]
        assert any("default-deny" in item["metadata"]["name"] for item in policies)
