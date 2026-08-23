from __future__ import annotations

import os
import socket
import time
import uuid
from collections.abc import Iterator
from datetime import UTC, datetime
from pathlib import Path

import httpx
import pytest

from tests.e2e.support import ManagedProcess, agentxctl, deployment_config, redact, run, start_process


def pytest_addoption(parser: pytest.Parser) -> None:
    parser.addoption("--values", action="store", default=os.getenv("AGENTX_E2E_VALUES"))
    parser.addoption("--keep-on-failure", action="store_true", default=False)
    parser.addoption("--scale-down-development", action="store_true", default=False)


@pytest.fixture(scope="session")
def deployment_values(pytestconfig: pytest.Config) -> Path:
    value = pytestconfig.getoption("--values")
    if not value:
        pytest.skip("cluster E2E requires --values or AGENTX_E2E_VALUES")
    return Path(value).resolve()


@pytest.fixture(scope="session")
def run_id() -> str:
    return uuid.uuid4().hex[:10]


def _timeline(timeline: list[str], message: str) -> None:
    timeline.append(f"{datetime.now(UTC).isoformat()} {message}")


def _must_remain_available_during_scale_down(deployment: dict[str, object]) -> bool:
    labels = deployment.get("metadata", {}).get("labels", {})
    return (
        labels.get("app.kubernetes.io/name") == "ingress-nginx"
        and labels.get("app.kubernetes.io/component") == "controller"
    )


def _scale_development(values: Path, enabled: bool) -> dict[tuple[str, str], int]:
    if not enabled:
        return {}
    config = deployment_config(values)
    replicas: dict[tuple[str, str], int] = {}
    for namespace in dict.fromkeys(config["namespaces"].values()):
        result = run(("kubectl", "-n", namespace, "get", "deployment", "-o", "json"), check=False, timeout=60)
        if result.returncode != 0:
            continue
        for deployment in result.json().get("items", []):
            if _must_remain_available_during_scale_down(deployment):
                continue
            name = deployment["metadata"]["name"]
            replicas[(namespace, name)] = int(deployment.get("spec", {}).get("replicas", 1))
            run(("kubectl", "-n", namespace, "scale", f"deployment/{name}", "--replicas=0"), timeout=60)
    return replicas


def _restore_development(replicas: dict[tuple[str, str], int]) -> None:
    for (namespace, name), count in replicas.items():
        run(
            ("kubectl", "-n", namespace, "scale", f"deployment/{name}", f"--replicas={count}"),
            check=False,
            timeout=60,
        )


def _collect_artifacts(context: dict[str, str], artifact_dir: Path, timeline: list[str]) -> None:
    for plane in ("control", "runtime", "dependencies"):
        namespace = context[f"{plane}_namespace"]
        commands = {
            "resources": ("kubectl", "-n", namespace, "get", "all,ingress,networkpolicy,pdb", "-o", "yaml"),
            "events": ("kubectl", "-n", namespace, "get", "events", "-o", "yaml"),
            "logs": (
                "kubectl",
                "-n",
                namespace,
                "logs",
                "-l",
                f"agentx.io/plane={plane}",
                "--all-containers=true",
                "--prefix=true",
                "--tail=1000",
            ),
        }
        for label, command in commands.items():
            result = run(command, check=False, timeout=180)
            content = result.stdout + (f"\n{result.stderr}" if result.stderr else "")
            (artifact_dir / f"{plane}-{label}.txt").write_text(redact(content), encoding="utf-8")
    (artifact_dir / "timeline.txt").write_text("\n".join(timeline) + "\n", encoding="utf-8")


@pytest.fixture(scope="session")
def installed_agentx(
    deployment_values: Path,
    run_id: str,
    pytestconfig: pytest.Config,
    request: pytest.FixtureRequest,
) -> Iterator[dict[str, str]]:
    timeline: list[str] = []
    failures_before = request.session.testsfailed
    development = _scale_development(deployment_values, pytestconfig.getoption("--scale-down-development"))
    config = deployment_config(deployment_values, run_id=run_id)
    artifact_dir = Path(__file__).resolve().parents[2] / "artifacts" / "e2e" / run_id
    artifact_dir.mkdir(parents=True, exist_ok=True)
    _timeline(timeline, "install started")
    install_command = (
        agentxctl(),
        "install",
        "--values",
        deployment_values,
        "--run-id",
        run_id,
        "--output",
        "json",
    )
    try:
        installed = run(install_command, timeout=3600)
    except Exception:
        run(
            (
                agentxctl(),
                "uninstall",
                "--values",
                deployment_values,
                "--run-id",
                run_id,
                "--purge-data",
                "--yes",
            ),
            check=False,
            timeout=1200,
        )
        _restore_development(development)
        raise
    (artifact_dir / "install.json").write_text(redact(installed.stdout), encoding="utf-8")
    _timeline(timeline, "install and Helm Doctor completed")
    context = {
        "values": str(deployment_values),
        "run_id": run_id,
        "control_namespace": config["namespaces"]["control"],
        "runtime_namespace": config["namespaces"]["runtime"],
        "dependencies_namespace": config["namespaces"]["dependencies"],
        "artifact_dir": str(artifact_dir),
        "root": str(Path(__file__).resolve().parents[2]),
    }
    try:
        yield context
    finally:
        failed = request.session.testsfailed > failures_before
        _timeline(timeline, f"test session completed failed={str(failed).lower()}")
        _collect_artifacts(context, artifact_dir, timeline)
        keep = failed and pytestconfig.getoption("--keep-on-failure")
        if not keep:
            _timeline(timeline, "purge started")
            result = run(
                (
                    agentxctl(),
                    "uninstall",
                    "--values",
                    deployment_values,
                    "--run-id",
                    run_id,
                    "--purge-data",
                    "--yes",
                    "--output",
                    "json",
                ),
                check=False,
                timeout=1200,
            )
            (artifact_dir / "uninstall.json").write_text(redact(result.stdout + result.stderr), encoding="utf-8")
        _restore_development(development)
        (artifact_dir / "timeline.txt").write_text("\n".join(timeline) + "\n", encoding="utf-8")


@pytest.fixture(scope="session")
def e2e_providers(installed_agentx: dict[str, str]) -> dict[str, str]:
    namespace = installed_agentx["dependencies_namespace"]
    fixture = Path(installed_agentx["root"]) / "deploy" / "kustomize" / "e2e-fixtures" / "runtime-providers"
    run(("kubectl", "-n", namespace, "apply", "-k", fixture), timeout=300)
    run(
        (
            "kubectl",
            "-n",
            namespace,
            "wait",
            "--for=condition=complete",
            "job/lightrag-tokenizer-cache",
            "--timeout=300s",
        ),
        timeout=330,
    )
    for deployment in ("echo-mcp", "echo-node", "lightrag", "mem0", "mem0-postgres"):
        rollout_timeout = 600 if deployment == "lightrag" else 300
        result = run(
            (
                "kubectl",
                "-n",
                namespace,
                "rollout",
                "status",
                f"deployment/{deployment}",
                f"--timeout={rollout_timeout}s",
            ),
            check=False,
            timeout=rollout_timeout + 30,
        )
        if result.returncode != 0:
            raise RuntimeError(f"required E2E provider did not become ready: {namespace}/{deployment}")
    return {
        "echo_mcp": f"http://echo-mcp.{namespace}.svc:8090",
        "echo_node": f"http://echo-node.{namespace}.svc:8080",
        "lightrag": f"http://lightrag.{namespace}.svc:9621",
        "mem0": f"http://mem0.{namespace}.svc:8000",
    }


def _free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.bind(("127.0.0.1", 0))
        return int(listener.getsockname()[1])


def _wait_http(process: ManagedProcess, url: str) -> None:
    deadline = time.monotonic() + 60
    while time.monotonic() < deadline:
        if process.process.poll() is not None:
            raise RuntimeError(f"port-forward exited before {url} became ready")
        try:
            response = httpx.get(url, timeout=2)
            if response.status_code < 500:
                return
        except httpx.HTTPError:
            pass
        time.sleep(0.5)
    raise RuntimeError(f"timed out waiting for {url}")


@pytest.fixture(scope="session")
def service_urls(installed_agentx: dict[str, str]) -> Iterator[dict[str, str]]:
    artifact_dir = Path(installed_agentx["artifact_dir"])
    web_port, runtime_port = _free_port(), _free_port()
    forwards = [
        start_process(
            (
                "kubectl",
                "-n",
                installed_agentx["control_namespace"],
                "port-forward",
                "service/web-console",
                f"{web_port}:8080",
            ),
            stdout_path=artifact_dir / "port-forward-web.log",
            stderr_path=artifact_dir / "port-forward-web-error.log",
        ),
        start_process(
            (
                "kubectl",
                "-n",
                installed_agentx["runtime_namespace"],
                "port-forward",
                "service/runtime-gateway-public",
                f"{runtime_port}:8080",
            ),
            stdout_path=artifact_dir / "port-forward-runtime.log",
            stderr_path=artifact_dir / "port-forward-runtime-error.log",
        ),
    ]
    urls = {"web": f"http://127.0.0.1:{web_port}", "runtime": f"http://127.0.0.1:{runtime_port}"}
    try:
        _wait_http(forwards[0], f"{urls['web']}/health/live")
        _wait_http(forwards[1], f"{urls['runtime']}/health/live")
        yield urls
    finally:
        for process in reversed(forwards):
            process.stop()
