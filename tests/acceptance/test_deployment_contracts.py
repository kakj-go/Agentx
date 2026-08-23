from __future__ import annotations

import json
import shutil
import tomllib
from pathlib import Path

import pytest
import yaml

from tests.e2e.support import render, run

TARGETS = ("dependencies", "control", "runtime", "observability")


def test_release_exposes_standalone_binaries_and_direct_download_links() -> None:
    workflow = Path(".github/workflows/agentxctl-release.yml").read_text(encoding="utf-8")
    readme = Path("README.md").read_text(encoding="utf-8")
    for asset in ("agentxctl-linux-x86_64", "agentxctl-windows-x86_64.exe"):
        assert f"standalone: {asset}" in workflow
        assert f"/agentxctl-v0.0.3-beta/{asset}" in readme
    assert "& $standalone validate --output json" in workflow
    assert "& $standalone render --target runtime" in workflow


def test_backend_builder_copies_every_workspace_member_root() -> None:
    workspace = tomllib.loads(Path("Cargo.toml").read_text(encoding="utf-8"))["workspace"]["members"]
    dockerfile = Path("deploy/docker/backend.Dockerfile").read_text(encoding="utf-8")
    copied = {
        line.split()[1].rstrip("/")
        for line in dockerfile.splitlines()
        if line.startswith("COPY ") and len(line.split()) >= 3
    }
    for member in workspace:
        assert any(member == source or member.startswith(f"{source}/") for source in copied), member


def test_chart_schemas_are_identical() -> None:
    canonical = json.loads(Path("deploy/values/values.schema.json").read_text(encoding="utf-8"))
    for target in TARGETS:
        schema = json.loads(Path(f"deploy/helm/agentx-{target}/values.schema.json").read_text(encoding="utf-8"))
        assert schema == canonical


@pytest.mark.skipif(shutil.which("helm") is None, reason="Helm is not installed")
@pytest.mark.parametrize("values_name", ["local.yaml", "dockerhub-beta.yaml", "production.example.yaml"])
def test_all_charts_lint_and_render(values_name: str) -> None:
    rendered = "\n".join(render(f"deploy/values/{values_name}", target) for target in TARGETS)
    for document in yaml.safe_load_all(rendered):
        if not isinstance(document, dict):
            continue
        pod_spec = document.get("spec", {})
        if document.get("kind") in {"Deployment", "StatefulSet", "DaemonSet", "Job"}:
            pod_spec = pod_spec.get("template", {}).get("spec", {})
        for container in (*pod_spec.get("initContainers", []), *pod_spec.get("containers", [])):
            assert all(isinstance(argument, str) for argument in container.get("args", [])), (
                document.get("kind"),
                document.get("metadata", {}).get("name"),
                container.get("name"),
            )
    assert "agentx.io/deployment/v2alpha3" not in rendered
    assert "agentx-agentx-" not in rendered
    assert "kind: HorizontalPodAutoscaler" not in rendered
    if values_name == "production.example.yaml":
        assert "--get-server-public-key" not in rendered
    else:
        assert "--get-server-public-key" in rendered
    assert "wait-for-control-schema" in rendered
    assert "wait-for-runtime-schema" in rendered
    assert "wait-for-observability-schema" in rendered
    assert "database=$AGENTX_CLICKHOUSE_DATABASE" in rendered


@pytest.mark.skipif(shutil.which("helm") is None, reason="Helm is not installed")
def test_kustomize_does_not_own_core_resources() -> None:
    core_names: set[tuple[str, str]] = set()
    for target in TARGETS:
        for document in yaml.safe_load_all(render("deploy/values/local.yaml", target)):
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
