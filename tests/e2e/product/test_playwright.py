from __future__ import annotations

import os
import subprocess

import pytest


@pytest.mark.cluster
@pytest.mark.product
def test_product_playwright_suite(
    installed_agentx: dict[str, str], service_urls: dict[str, str], e2e_providers: dict[str, str]
) -> None:
    environment = os.environ.copy()
    environment["AGENTX_E2E_RUN_ID"] = installed_agentx["run_id"]
    environment["AGENTX_E2E_STAGE"] = "helm-python"
    environment["AGENTX_E2E_BASE_URL"] = service_urls["web"]
    environment["AGENTX_E2E_RUNTIME_URL"] = service_urls["runtime"]
    environment["AGENTX_E2E_ECHO_BASE_URL"] = e2e_providers["echo_mcp"]
    environment["AGENTX_E2E_REMOTE_NODE_ENDPOINT"] = e2e_providers["echo_node"]
    environment["AGENTX_E2E_LIGHTRAG_BASE_URL"] = e2e_providers["lightrag"]
    environment["AGENTX_E2E_MEM0_BASE_URL"] = e2e_providers["mem0"]
    subprocess.run(
        ["pnpm", "--filter", "@agentx/e2e", "test"],
        check=True,
        shell=False,
        env=environment,
        cwd=installed_agentx["root"],
    )
