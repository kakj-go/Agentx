from __future__ import annotations

import json
import shutil
from pathlib import Path

import pytest
import yaml
from agentx_deploy.config import load_values
from agentx_deploy.helm import lint, template
from agentx_deploy.process import run

TARGETS = ("dependencies", "control", "runtime", "observability")


def test_chart_schemas_are_identical() -> None:
    canonical = json.loads(Path("deploy/values/values.schema.json").read_text(encoding="utf-8"))
    for target in TARGETS:
        schema = json.loads(Path(f"deploy/helm/agentx-{target}/values.schema.json").read_text(encoding="utf-8"))
        assert schema == canonical


@pytest.mark.skipif(shutil.which("helm") is None, reason="Helm is not installed")
@pytest.mark.parametrize("values_name", ["local.yaml", "dockerhub-beta.yaml", "production.example.yaml"])
def test_all_charts_lint_and_render(values_name: str) -> None:
    config = load_values(f"deploy/values/{values_name}")
    lint(config, TARGETS)
    rendered = "\n".join(template(config, target).stdout for target in TARGETS)
    assert "agentx.io/deployment/v2alpha3" not in rendered
    assert "agentx-agentx-" not in rendered
    assert "kind: HorizontalPodAutoscaler" not in rendered


@pytest.mark.skipif(shutil.which("helm") is None, reason="Helm is not installed")
def test_kustomize_does_not_own_core_resources() -> None:
    config = load_values("deploy/values/local.yaml")
    core_names: set[tuple[str, str]] = set()
    for target in TARGETS:
        for document in yaml.safe_load_all(template(config, target).stdout):
            if isinstance(document, dict) and document.get("metadata", {}).get("name"):
                core_names.add((document.get("kind", ""), document["metadata"]["name"]))
    kustomize_names: set[tuple[str, str]] = set()
    for directory in (
        "deploy/kustomize/addons/lightrag",
        "deploy/kustomize/addons/mem0",
        "deploy/kustomize/e2e-fixtures/runtime-providers",
    ):
        for document in yaml.safe_load_all(run(("kubectl", "kustomize", directory), timeout=120).stdout):
            if not isinstance(document, dict) or not document.get("metadata", {}).get("name"):
                continue
            kustomize_names.add((document.get("kind", ""), document["metadata"]["name"]))
            labels = document["metadata"].get("labels", {})
            assert labels.get("app.kubernetes.io/managed-by") != "Helm"
    assert core_names.isdisjoint(kustomize_names)
