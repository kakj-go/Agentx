from __future__ import annotations

import os
import subprocess
import tempfile
from collections.abc import Sequence
from pathlib import Path

import pytest


def _run_playwright(
    pnpm: str,
    suite: str,
    tests: Sequence[str],
    environment: dict[str, str],
    root: str,
) -> None:
    subprocess.run(
        [pnpm, "--filter", "@agentx/e2e", "exec", "playwright", "test", *tests],
        check=True,
        shell=False,
        env={**environment, "AGENTX_E2E_SUITE": suite},
        cwd=root,
    )


@pytest.mark.cluster
@pytest.mark.product
def test_product_playwright_suite(
    installed_agentx: dict[str, str], service_urls: dict[str, str], e2e_providers: dict[str, str]
) -> None:
    environment = os.environ.copy()
    environment["AGENTX_E2E_RUN_ID"] = installed_agentx["run_id"]
    environment["AGENTX_E2E_STAGE"] = "helm-agentxctl"
    environment["AGENTX_E2E_BASE_URL"] = service_urls["web"]
    environment["AGENTX_E2E_RUNTIME_URL"] = service_urls["runtime"]
    environment["AGENTX_E2E_ECHO_BASE_URL"] = e2e_providers["echo_mcp"]
    environment["AGENTX_E2E_REMOTE_NODE_ENDPOINT"] = e2e_providers["echo_node"]
    environment["AGENTX_E2E_LIGHTRAG_BASE_URL"] = e2e_providers["lightrag"]
    environment["AGENTX_E2E_MEM0_BASE_URL"] = e2e_providers["mem0"]
    pnpm = "pnpm.cmd" if os.name == "nt" else "pnpm"
    with tempfile.TemporaryDirectory(prefix="agentx-e2e-context-") as context_dir:
        environment["AGENTX_V2_08_CONTEXT_OUTPUT"] = str(Path(context_dir) / "v2-08-context.json")
        root = installed_agentx["root"]
        suites = (
            ("api-first", ("tests/v2-08-api-first.spec.ts",)),
            ("application-docs", ("tests/application-integration-docs.spec.ts",)),
            ("execution-filters", ("tests/execution-filters.spec.ts",)),
            ("workflow4", ("tests/workflow4-closure.spec.ts", "--retries=1")),
            (
                "control-ui",
                (
                    "tests/m2.1-control-plane.spec.ts",
                    "tests/resource-grant-requests.spec.ts",
                ),
            ),
            (
                "product-closure",
                (
                    "tests/m6-workflow-studio.spec.ts",
                    "tests/m6-local-builtins.spec.ts",
                    "tests/m7-business-closure.spec.ts",
                    "tests/safe-deletion.spec.ts",
                ),
            ),
        )
        for suite, tests in suites:
            _run_playwright(pnpm, suite, tests, environment, root)
