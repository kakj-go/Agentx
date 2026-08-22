from __future__ import annotations

import json
import re
import sys
import tempfile
from collections.abc import Iterable
from pathlib import Path
from typing import Any

import yaml

from agentx_deploy.config import RELEASES, DeploymentConfig
from agentx_deploy.process import Result, require_tool, run

INGRESS_RELEASE = "agentx-ingress-nginx"
INGRESS_CHART = (
    "https://github.com/kubernetes/ingress-nginx/releases/download/helm-chart-4.15.1/ingress-nginx-4.15.1.tgz"
)


def tool_versions() -> dict[str, str]:
    if sys.version_info[:2] != (3, 12):
        raise RuntimeError("agentx-deploy requires Python 3.12")
    versions: dict[str, str] = {"python": ".".join(str(part) for part in sys.version_info[:3])}
    for tool, args in (
        ("uv", ("--version",)),
        ("helm", ("version", "--short")),
        ("kubectl", ("version", "--client", "-o", "json")),
    ):
        executable = require_tool(tool)
        result = run((executable, *args), timeout=30)
        versions[tool] = result.stdout.strip()
    if not re.search(r"(?:^|\s)v?3\.", versions["helm"]):
        raise RuntimeError(f"Helm 3 is required, found: {versions['helm']}")
    kubectl_version = json.loads(versions["kubectl"])["clientVersion"]["gitVersion"]
    versions["kubectl"] = kubectl_version
    return versions


def helm_values_file(config: DeploymentConfig) -> Path:
    with tempfile.NamedTemporaryFile(mode="w", suffix=".yaml", encoding="utf-8", delete=False) as handle:
        yaml.safe_dump(config.values, handle, sort_keys=False, allow_unicode=True)
        return Path(handle.name)


def chart_path(config: DeploymentConfig, target: str) -> Path:
    return config.root / "deploy" / "helm" / f"agentx-{target}"


def namespace_for(config: DeploymentConfig, target: str) -> str:
    if target == "observability":
        return config.namespaces["runtime"]
    return config.namespaces[target]


def template(config: DeploymentConfig, target: str) -> Result:
    require_tool("helm")
    values = helm_values_file(config)
    try:
        return run(
            (
                "helm",
                "template",
                RELEASES[target],
                chart_path(config, target),
                "--namespace",
                namespace_for(config, target),
                "--values",
                values,
            ),
            timeout=120,
        )
    finally:
        values.unlink(missing_ok=True)


def lint(config: DeploymentConfig, targets: Iterable[str]) -> None:
    require_tool("helm")
    values = helm_values_file(config)
    try:
        for target in targets:
            run(("helm", "lint", chart_path(config, target), "--values", values), timeout=120)
    finally:
        values.unlink(missing_ok=True)


def upgrade_install(config: DeploymentConfig, target: str, *, set_values: dict[str, int] | None = None) -> None:
    values = helm_values_file(config)
    try:
        command: list[str | Path] = [
            "helm",
            "upgrade",
            "--install",
            RELEASES[target],
            chart_path(config, target),
            "--namespace",
            namespace_for(config, target),
            "--values",
            values,
            "--atomic",
            "--wait",
            "--wait-for-jobs",
            "--timeout",
            "10m",
        ]
        for key, value in (set_values or {}).items():
            command.extend(("--set", f"{key}={value}"))
        run(command, timeout=720)
    finally:
        values.unlink(missing_ok=True)


def install_ingress(config: DeploymentConfig) -> None:
    ingress = config.values["global"]["ingress"]
    namespace = config.namespaces["dependencies"]
    class_name = ingress["className"]
    args = [
        "helm",
        "upgrade",
        "--install",
        INGRESS_RELEASE,
        INGRESS_CHART,
        "--namespace",
        namespace,
        "--create-namespace",
        "--atomic",
        "--wait",
        "--timeout",
        "10m",
        "-f",
        str(config.root / "deploy" / "ingress-nginx" / "values.yaml"),
        "--set-string",
        f"controller.ingressClass={class_name}",
        "--set-string",
        f"controller.ingressClassResource.name={class_name}",
        "--set-string",
        f"controller.ingressClassResource.controllerValue=k8s.io/{class_name}",
    ]
    if config.environment == "test" or config.namespaces["dependencies"].startswith("agentx-e2e-"):
        args.extend(
            ("--set-string", f"fullnameOverride={class_name}", "--set-string", "controller.service.type=ClusterIP")
        )
    run(args, timeout=720)


def release_status(config: DeploymentConfig, target: str) -> dict[str, Any] | None:
    result = run(
        ("helm", "status", RELEASES[target], "--namespace", namespace_for(config, target), "-o", "json"),
        timeout=60,
        check=False,
    )
    if result.returncode != 0:
        return None
    return json.loads(result.stdout)


def test_release(config: DeploymentConfig, target: str) -> None:
    run(
        ("helm", "test", RELEASES[target], "--namespace", namespace_for(config, target), "--logs", "--timeout", "5m"),
        timeout=360,
    )
