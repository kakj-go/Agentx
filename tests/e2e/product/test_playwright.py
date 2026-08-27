from __future__ import annotations

import json
import os
import subprocess
import tempfile
import uuid
from collections.abc import Sequence
from pathlib import Path

import pytest

from tests.e2e.support import run


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


def _assert_execution_context_snapshot(installed_agentx: dict[str, str], evidence_path: Path) -> None:
    evidence = json.loads(evidence_path.read_text(encoding="utf-8"))
    execution_id = str(uuid.UUID(evidence["executionId"]))
    fields = (
        "'executionId',JSON_UNQUOTE(JSON_EXTRACT(execution_context_json,'$.id'))",
        "'workflowName',JSON_UNQUOTE(JSON_EXTRACT(execution_context_json,'$.workflow.name'))",
        "'departmentName',JSON_UNQUOTE(JSON_EXTRACT(execution_context_json,'$.initiator.department.name'))",
        "'roleCodes',JSON_EXTRACT(execution_context_json,'$.initiator.roles.codes')",
        "'initiatorType',JSON_UNQUOTE(JSON_EXTRACT(execution_context_json,'$.initiator.type'))",
        "'contextWorkflowName',JSON_UNQUOTE(JSON_EXTRACT(e.output_json,'$.context_workflow_name'))",
    )
    query = (
        f"SELECT JSON_OBJECT({','.join(fields)}) FROM execution_snapshots s "  # noqa: S608 -- UUID is normalized.
        "JOIN workflow_executions e ON e.tenant_id=s.tenant_id AND e.id=s.execution_id "
        f"WHERE s.execution_id=UUID_TO_BIN('{execution_id}')"
    )
    result = run(
        (
            "kubectl",
            "-n",
            installed_agentx["runtime_namespace"],
            "exec",
            "statefulset/runtime-mysql",
            "--",
            "sh",
            "-ec",
            'MYSQL_PWD="$(cat /run/secrets/agentx/root-password)" mysql --ssl-mode=DISABLED '
            '--batch --skip-column-names -uroot agentx_runtime -e "$1"',
            "agentx-e2e-query",
            query,
        ),
        timeout=60,
    )
    snapshot = json.loads(result.stdout.strip())
    assert snapshot == {
        "executionId": execution_id,
        "workflowName": evidence["workflowName"],
        "departmentName": evidence["departmentName"],
        "roleCodes": evidence["roleCodes"],
        "initiatorType": "user",
        "contextWorkflowName": evidence["contextWorkflowName"],
    }


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
        execution_context_evidence = Path(context_dir) / "execution-context.json"
        environment["AGENTX_EXECUTION_CONTEXT_EVIDENCE_OUTPUT"] = str(execution_context_evidence)
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
        only_suite = os.environ.get("AGENTX_E2E_ONLY_SUITE")
        for suite, tests in suites:
            if only_suite and suite != only_suite:
                continue
            _run_playwright(pnpm, suite, tests, environment, root)
            if suite == "product-closure":
                _assert_execution_context_snapshot(installed_agentx, execution_context_evidence)
        if not only_suite or only_suite == "session-diagnostics":
            _run_playwright(
                pnpm,
                "session-diagnostics",
                ("tests/agent-sessions-diagnostics.spec.ts",),
                environment,
                root,
            )
