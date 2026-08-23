from __future__ import annotations

import json
import subprocess

import pytest
import yaml

from tests.e2e.support import agentxctl, render, run


@pytest.mark.cluster
@pytest.mark.infrastructure
def test_all_planes_are_ready(installed_agentx: dict[str, str]) -> None:
    result = subprocess.run(
        [
            agentxctl(),
            "status",
            "--values",
            installed_agentx["values"],
            "--run-id",
            installed_agentx["run_id"],
            "--output",
            "json",
        ],
        check=True,
        capture_output=True,
        text=True,
        shell=False,
    )
    payload = json.loads(result.stdout)
    assert {release["target"] for release in payload["releases"] if release["installed"]} == {
        "dependencies",
        "control",
        "runtime",
        "observability",
    }


@pytest.mark.cluster
@pytest.mark.infrastructure
def test_bootstrap_is_idempotent(installed_agentx: dict[str, str]) -> None:
    for target in ("control", "runtime", "observability"):
        documents = [
            document
            for document in yaml.safe_load_all(render(installed_agentx["values"], target, installed_agentx["run_id"]))
            if isinstance(document, dict)
        ]
        bootstrap = next(
            document
            for document in documents
            if document.get("kind") == "Job" and "bootstrap" in document.get("metadata", {}).get("name", "")
        )
        for replay in (1, 2):
            job = json.loads(json.dumps(bootstrap))
            job["metadata"]["name"] = f"{target}-bootstrap-idempotency-{replay}"
            run(("kubectl", "apply", "-f", "-"), input_text=yaml.safe_dump(job), timeout=60)
            run(
                (
                    "kubectl",
                    "-n",
                    job["metadata"]["namespace"],
                    "wait",
                    "--for=condition=complete",
                    f"job/{job['metadata']['name']}",
                    "--timeout=300s",
                ),
                timeout=330,
            )
