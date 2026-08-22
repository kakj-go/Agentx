from __future__ import annotations

import subprocess

import pytest


@pytest.mark.cluster
@pytest.mark.gateway
def test_runtime_public_and_internal_services_exist(installed_agentx: dict[str, str]) -> None:
    namespace = installed_agentx["runtime_namespace"]
    for name in ("runtime-gateway-public", "runtime-gateway-internal"):
        subprocess.run(["kubectl", "-n", namespace, "get", "service", name], check=True, shell=False)
