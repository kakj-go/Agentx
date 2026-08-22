from __future__ import annotations

import json
import sys
import tempfile
import uuid
from datetime import UTC, datetime
from pathlib import Path, PurePosixPath
from typing import Any

import yaml
from jsonschema import Draft202012Validator

from agentx_deploy.config import RELEASES, DeploymentConfig, selected_targets
from agentx_deploy.helm import (
    INGRESS_RELEASE,
    install_ingress,
    lint,
    namespace_for,
    release_status,
    template,
    test_release,
    tool_versions,
    upgrade_install,
)
from agentx_deploy.process import require_tool, run
from agentx_deploy.secrets import (
    ensure_existing_secret_references,
    ensure_local_secrets,
    rotate_egress_keys,
    sync_existing_mirrors,
)


def emit(payload: Any, output: str) -> None:
    if output == "json":
        print(json.dumps(payload, ensure_ascii=False, indent=2))
        return
    if isinstance(payload, str):
        print(payload)
        return
    if isinstance(payload, dict):
        for key, value in payload.items():
            rendered = json.dumps(value, ensure_ascii=False) if isinstance(value, dict | list) else value
            print(f"{key}: {rendered}")
        return
    print(payload)


def validate(config: DeploymentConfig, targets: tuple[str, ...], *, cluster: bool) -> dict[str, Any]:
    versions = tool_versions()
    lint(config, targets)
    if cluster:
        result = run(("kubectl", "cluster-info"), timeout=30)
        versions["cluster"] = result.stdout.splitlines()[0] if result.stdout else "reachable"
        secret_status = ensure_existing_secret_references(
            config, (template(config, target).stdout for target in targets)
        )
    else:
        secret_status = {"status": "not-checked"}
    return {
        "status": "valid",
        "environment": config.environment,
        "targets": list(targets),
        "namespaces": config.namespaces,
        "tools": versions,
        "secrets": secret_status,
    }


def render(config: DeploymentConfig, targets: tuple[str, ...]) -> str:
    return "\n---\n".join(template(config, target).stdout.rstrip() for target in targets) + "\n"


def _namespace_manifest(name: str, plane: str) -> dict[str, Any]:
    labels = {"agentx.io/plane": plane, "app.kubernetes.io/managed-by": "agentx-deploy"}
    if plane != "dependencies":
        labels.update(
            {
                "pod-security.kubernetes.io/enforce": "restricted",
                "pod-security.kubernetes.io/audit": "restricted",
                "pod-security.kubernetes.io/warn": "restricted",
            }
        )
    return {"apiVersion": "v1", "kind": "Namespace", "metadata": {"name": name, "labels": labels}}


def ensure_namespaces(config: DeploymentConfig, targets: tuple[str, ...] | None = None) -> None:
    planes = []
    for target in targets or ("control", "runtime", "dependencies"):
        plane = "runtime" if target == "observability" else target
        if plane not in planes:
            planes.append(plane)
    for plane in planes:
        run(
            ("kubectl", "apply", "-f", "-"),
            input_text=json.dumps(_namespace_manifest(config.namespaces[plane], plane)),
            timeout=60,
        )


REPLICA_PATHS = {
    "control": {
        "web-console": "control.services.webConsole.replicas",
        "platform-control": "control.services.platformControl.replicas",
    },
    "runtime": {
        "runtime-gateway": "runtime.services.runtimeGateway.replicas",
        "workflow-runtime": "runtime.services.workflowRuntime.replicas",
        "workflow-worker": "runtime.services.workflowWorker.replicas",
        "sandbox-manager": "runtime.services.sandboxManager.replicas",
    },
    "observability": {"observability": "observability.services.observability.replicas"},
    "dependencies": {"agentx-egress-gateway": "dependencies.services.egressGateway.replicas"},
}


def _current_replica_values(config: DeploymentConfig, target: str) -> dict[str, int]:
    if release_status(config, target) is None:
        return {}
    result = run(
        (
            "kubectl",
            "-n",
            namespace_for(config, target),
            "get",
            "deployment",
            "-l",
            f"agentx.io/plane={target}",
            "-o",
            "json",
        ),
        timeout=60,
    )
    values: dict[str, int] = {}
    for item in result.json().get("items", []):
        name = item.get("metadata", {}).get("name")
        path = REPLICA_PATHS[target].get(name)
        if path:
            values[path] = int(item.get("spec", {}).get("replicas", 1))
    return values


def install(
    config: DeploymentConfig,
    targets: tuple[str, ...],
    *,
    run_doctor: bool,
    preserve_replicas: bool = False,
) -> dict[str, Any]:
    validate(config, targets, cluster=True)
    if "dependencies" not in targets and release_status(config, "dependencies") is None:
        raise RuntimeError(f"{targets[0]} requires the agentx-dependencies release")
    ensure_namespaces(config, targets)
    if config.values["global"]["secrets"]["mode"] == "generated-local":
        ensure_local_secrets(config, targets)
    if "dependencies" in targets:
        install_ingress(config)
    for target in targets:
        replicas = _current_replica_values(config, target) if preserve_replicas else {}
        upgrade_install(config, target, set_values=replicas)
    if run_doctor:
        doctor(config, targets)
    result = status(config, targets, expected="ready")
    if config.environment == "production":
        result["releaseManifest"] = str(_write_release_manifest(config))
    return result


def doctor(config: DeploymentConfig, targets: tuple[str, ...]) -> dict[str, Any]:
    checked: list[str] = []
    for target in targets:
        if release_status(config, target) is None:
            raise RuntimeError(f"release is not installed: {RELEASES[target]}")
        test_release(config, target)
        checked.append(target)
    return {"status": "healthy", "targets": checked}


def status(config: DeploymentConfig, targets: tuple[str, ...], *, expected: str | None = None) -> dict[str, Any]:
    payload: dict[str, Any] = {
        "status": expected or "observed",
        "environment": config.environment,
        "namespaces": config.namespaces,
        "releases": [],
        "images": {
            service: image_reference(config, service) for service in config.values["global"]["images"]["services"]
        },
        "endpoints": {
            "control": f"{'https' if config.values['global']['ingress'].get('controlTlsSecretName') else 'http'}://{config.values['global']['ingress']['controlHost']}",
            "runtime": f"{'https' if config.values['global']['ingress'].get('runtimeTlsSecretName') else 'http'}://{config.values['global']['ingress']['runtimeHost']}",
        },
    }
    for target in targets:
        helm_status = release_status(config, target)
        release = {
            "target": target,
            "name": RELEASES[target],
            "namespace": namespace_for(config, target),
            "installed": helm_status is not None,
        }
        if helm_status:
            info = helm_status.get("info", {})
            release.update(
                {
                    "revision": helm_status.get("version"),
                    "state": info.get("status"),
                    "updated": info.get("last_deployed"),
                }
            )
            resources = run(
                (
                    "kubectl",
                    "-n",
                    namespace_for(config, target),
                    "get",
                    "deployment,statefulset,job,pdb",
                    "-l",
                    f"agentx.io/plane={target}",
                    "-o",
                    "json",
                ),
                check=False,
                timeout=60,
            )
            if resources.returncode == 0:
                release["resources"] = json.loads(resources.stdout).get("items", [])
        payload["releases"].append(release)
    return payload


def _write_release_manifest(config: DeploymentConfig) -> Path:
    images = config.values["global"]["images"]
    manifest = {
        "schemaVersion": "agentx.io/v2-release/v1",
        "version": images["tag"],
        "gitCommit": run(("git", "rev-parse", "HEAD"), cwd=config.root, timeout=30).stdout.strip(),
        "generatedAt": datetime.now(UTC).isoformat(),
        "protocolVersion": 1,
        "compatibleProtocolVersions": [1],
        "images": [{"name": name, "digest": images["digests"][name]} for name in images["services"]],
    }
    schema_path = config.root / "deploy" / "release" / "v2-release-manifest.schema.json"
    schema = json.loads(schema_path.read_text(encoding="utf-8"))
    Draft202012Validator(schema, format_checker=Draft202012Validator.FORMAT_CHECKER).validate(manifest)
    output_dir = config.root / "artifacts" / "releases"
    output_dir.mkdir(parents=True, exist_ok=True)
    output = output_dir / f"{datetime.now(UTC).strftime('%Y%m%dT%H%M%SZ')}-{images['tag']}.json"
    output.write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8")
    return output


def rollback(config: DeploymentConfig, target: str, revision: int) -> dict[str, Any]:
    if target == "all":
        raise ValueError("rollback requires one explicit target")
    run(
        (
            "helm",
            "rollback",
            RELEASES[target],
            str(revision),
            "--namespace",
            namespace_for(config, target),
            "--atomic",
            "--wait",
            "--timeout",
            "10m",
        ),
        timeout=720,
    )
    test_release(config, target)
    return status(config, (target,), expected="rolled-back")


def uninstall(
    config: DeploymentConfig, targets: tuple[str, ...], *, purge_data: bool, confirmed: bool
) -> dict[str, Any]:
    if purge_data and (config.environment == "production" or targets != selected_targets("all") or not confirmed):
        raise ValueError("data purge requires local/test values, --target all, and --yes")
    if targets == ("dependencies",):
        runtime = release_status(config, "runtime")
        if runtime is not None:
            raise RuntimeError("dependencies uninstall is refused while the runtime release exists")
    for target in reversed(targets):
        run(
            ("helm", "uninstall", RELEASES[target], "--namespace", namespace_for(config, target), "--ignore-not-found"),
            timeout=300,
        )
    if "dependencies" in targets:
        ingress_class = config.values["global"]["ingress"]["className"]
        ingresses = run(("kubectl", "get", "ingress", "--all-namespaces", "-o", "json"), check=False, timeout=60)
        users = []
        if ingresses.returncode == 0:
            users = [
                f"{item['metadata']['namespace']}/{item['metadata']['name']}"
                for item in ingresses.json().get("items", [])
                if item.get("spec", {}).get("ingressClassName") == ingress_class
            ]
        if users:
            if purge_data:
                raise RuntimeError(f"cannot purge while IngressClass {ingress_class} is still used: {', '.join(users)}")
        else:
            run(
                (
                    "helm",
                    "uninstall",
                    INGRESS_RELEASE,
                    "--namespace",
                    config.namespaces["dependencies"],
                    "--ignore-not-found",
                ),
                timeout=300,
                check=False,
            )
    if purge_data:
        for namespace in dict.fromkeys(config.namespaces.values()):
            run(("kubectl", "delete", "namespace", namespace, "--ignore-not-found", "--wait=true"), timeout=600)
        return {"status": "purged", "namespaces": list(dict.fromkeys(config.namespaces.values()))}
    return {"status": "uninstalled", "namespacesPreserved": True, "targets": list(targets)}


def migrate(config: DeploymentConfig, target: str, phase: str, timeout: int) -> dict[str, Any]:
    if target not in ("control", "runtime", "observability"):
        raise ValueError("migrate target must be control, runtime, or observability")
    namespace = namespace_for(config, target)
    if phase == "contract":
        result = run(("kubectl", "-n", namespace, "get", "replicaset", "-o", "json"), timeout=60)
        old = [
            item["metadata"]["name"]
            for item in result.json().get("items", [])
            if item.get("spec", {}).get("replicas", 0) and not item.get("status", {}).get("availableReplicas", 0)
        ]
        if old:
            raise RuntimeError(f"contract migration refused while old ReplicaSets exist: {', '.join(old)}")
        return {"status": "contract-ready", "target": target, "schemaRollback": False}
    manifest = template(config, target).stdout
    docs = [item for item in yaml.safe_load_all(manifest) if isinstance(item, dict)]
    jobs = [
        item for item in docs if item.get("kind") == "Job" and "migrate" in item.get("metadata", {}).get("name", "")
    ]
    if len(jobs) != 1:
        raise RuntimeError(f"rendered {target} chart must contain exactly one migration Job")
    job = jobs[0]
    base = job["metadata"]["name"].rsplit("-", 1)[0]
    job["metadata"]["name"] = f"{base}-manual-{datetime.now(UTC).strftime('%Y%m%d%H%M%S')}"
    run(("kubectl", "apply", "-f", "-"), input_text=yaml.safe_dump(job), timeout=60)
    run(
        (
            "kubectl",
            "-n",
            namespace,
            "wait",
            "--for=condition=complete",
            f"job/{job['metadata']['name']}",
            f"--timeout={timeout}s",
        ),
        timeout=timeout + 30,
    )
    return {"status": "expanded", "target": target, "job": job["metadata"]["name"]}


def _docker_build(command: list[str]) -> None:
    last_error: Exception | None = None
    for _ in range(3):
        try:
            run(command, timeout=3600)
            return
        except Exception as error:
            last_error = error
    if last_error is None:
        raise RuntimeError("Docker build failed without an error result")
    raise last_error


def _import_images(config: DeploymentConfig, images: list[str]) -> None:
    require_tool("kubectl")
    archive = Path(tempfile.gettempdir()) / f"agentx-images-{uuid.uuid4().hex}.tar"
    remote_archive = str(PurePosixPath("/", "tmp", archive.name))
    loader_name = f"agentx-image-loader-{uuid.uuid4().hex[:8]}"
    node = run(
        ("docker", "ps", "--filter", "name=^/desktop-control-plane$", "--format", "{{.Names}}"),
        check=False,
        timeout=30,
    ).stdout.strip()
    try:
        run(("docker", "save", "--output", archive, *images), timeout=1800)
        if node:
            run(("docker", "cp", archive, f"{node}:{remote_archive}"), timeout=300)
            for image in images:
                containerd_image = image if "/" not in image else f"docker.io/{image}"
                run(
                    ("docker", "exec", node, "ctr", "--namespace", "k8s.io", "images", "remove", containerd_image),
                    check=False,
                    timeout=60,
                )
            run(
                ("docker", "exec", node, "ctr", "--namespace", "k8s.io", "images", "import", remote_archive),
                timeout=600,
            )
            run(("docker", "exec", node, "rm", "-f", remote_archive), check=False, timeout=30)
            return

        namespace = config.namespaces["dependencies"]
        ensure_namespaces(config)
        loader = {
            "apiVersion": "v1",
            "kind": "Pod",
            "metadata": {"name": loader_name, "namespace": namespace},
            "spec": {
                "restartPolicy": "Never",
                "containers": [
                    {
                        "name": "loader",
                        "image": "ghcr.io/containerd/nerdctl:v2.3.5",
                        "command": ["sleep", "3600"],
                        "securityContext": {"privileged": True},
                        "volumeMounts": [{"name": "containerd", "mountPath": "/run/containerd/containerd.sock"}],
                    }
                ],
                "volumes": [
                    {
                        "name": "containerd",
                        "hostPath": {"path": "/run/containerd/containerd.sock", "type": "Socket"},
                    }
                ],
            },
        }
        run(("kubectl", "apply", "-f", "-"), input_text=json.dumps(loader), timeout=60)
        run(
            ("kubectl", "-n", namespace, "wait", "--for=condition=Ready", f"pod/{loader_name}", "--timeout=180s"),
            timeout=210,
        )
        run(("kubectl", "-n", namespace, "cp", archive, f"{loader_name}:{remote_archive}"), timeout=600)
        run(
            (
                "kubectl",
                "-n",
                namespace,
                "exec",
                loader_name,
                "--",
                "ctr",
                "--address",
                "/run/containerd/containerd.sock",
                "--namespace",
                "k8s.io",
                "images",
                "import",
                remote_archive,
            ),
            timeout=600,
        )
    finally:
        archive.unlink(missing_ok=True)
        if not node:
            run(
                (
                    "kubectl",
                    "-n",
                    config.namespaces["dependencies"],
                    "delete",
                    "pod",
                    loader_name,
                    "--ignore-not-found",
                    "--wait=true",
                ),
                check=False,
                timeout=180,
            )


def build_images(
    config: DeploymentConfig, services: list[str], *, push: bool, skip_kubernetes_import: bool
) -> dict[str, Any]:
    if config.environment == "production":
        raise ValueError("production images must be supplied by immutable digest")
    require_tool("docker")
    images = config.values["global"]["images"]
    known = images["services"]
    selected = services or list(known)
    unknown = set(selected) - set(known)
    if unknown:
        raise ValueError(f"unknown image services: {', '.join(sorted(unknown))}")
    built: list[str] = []
    for service in selected:
        image = image_reference(config, service)
        if service == "web-console":
            args = [
                "docker",
                "build",
                "-f",
                str(config.root / "deploy" / "docker" / "web.Dockerfile"),
                "--build-arg",
                "NGINX_CONFIG=deploy/docker/nginx-v2.conf",
                "-t",
                image,
                str(config.root),
            ]
        else:
            application = "agentx-observability" if service == "observability" else service
            args = [
                "docker",
                "build",
                "-f",
                str(config.root / "deploy" / "docker" / "backend.Dockerfile"),
                "--build-arg",
                f"APP={application}",
            ]
            cargo_packages = {
                "observability": "agentx-observability",
                "workflow-worker": "agentx-v2-runtime",
                "sandbox-manager": "agentx-v2-runtime",
                "agentx-egress-gateway": "agentx-egress-gateway",
            }
            if cargo_package := cargo_packages.get(service):
                args.extend(("--build-arg", f"CARGO_PACKAGE={cargo_package}"))
            args.extend(("-t", image, str(config.root)))
        _docker_build(args)
        if push:
            run(("docker", "push", image), timeout=1800)
        built.append(image)
    if not skip_kubernetes_import:
        _import_images(config, built)
    return {"status": "built", "images": built, "pushed": push, "imported": not skip_kubernetes_import}


def image_reference(config: DeploymentConfig, service: str) -> str:
    images = config.values["global"]["images"]
    prefix = images.get("repositoryPrefix", "")
    repository = service if service.startswith(prefix) else f"{prefix}{service}"
    base = f"{images['registry'].rstrip('/')}/{repository}"
    digest = images.get("digests", {}).get(service)
    return f"{base}@{digest}" if digest else f"{base}:{images['tag']}"


def backup_operation(
    config: DeploymentConfig,
    *,
    action: str,
    target: str,
    backup_id: str,
    adapter: Path,
    restore_target: str | None,
    artifact_dir: Path,
    allow_in_place_restore: bool,
) -> dict[str, Any]:
    if config.environment != "production":
        raise ValueError("backup and restore acceptance requires production values")
    if not adapter.is_file():
        raise ValueError(f"backup adapter does not exist: {adapter}")
    allowed_targets = {
        "control-mysql",
        "runtime-mysql",
        "control-objects",
        "runtime-objects",
        "observability-objects",
        "clickhouse",
    }
    if target not in allowed_targets:
        raise ValueError(f"unsupported backup target: {target}")
    if action == "restore" and not restore_target:
        raise ValueError("restore requires --restore-target")
    authoritative = {
        "control-mysql": config.values["global"]["components"]["controlMysql"]["host"],
        "runtime-mysql": config.values["global"]["components"]["runtimeMysql"]["host"],
        "clickhouse": config.values["global"]["components"]["clickhouse"]["url"],
    }.get(target)
    if action == "restore" and authoritative == restore_target and not allow_in_place_restore:
        raise ValueError("in-place restore requires --allow-in-place-restore")
    started = datetime.now(UTC)
    command = [
        *([sys.executable] if adapter.suffix.lower() == ".py" else []),
        str(adapter),
        "--action",
        action,
        "--target",
        target,
        "--backup-id",
        backup_id,
        "--values",
        str(config.path),
    ]
    if restore_target:
        command.extend(("--restore-target", restore_target))
    receipt = run(command, timeout=3600).json()
    if receipt.get("status") != "passed":
        raise RuntimeError("backup provider adapter did not return status=passed")
    receipt_fields = {
        "status",
        "recoveryPointUtc",
        "objectCount",
        "contentSha256",
        "schemaVersionObserved",
    }
    if set(receipt) != receipt_fields:
        raise RuntimeError("backup provider receipt has missing or unapproved fields")
    completed = datetime.now(UTC)
    recovery_point = datetime.fromisoformat(str(receipt["recoveryPointUtc"]).replace("Z", "+00:00"))
    if recovery_point.tzinfo is None:
        raise RuntimeError("provider recoveryPointUtc must include a timezone")
    age_minutes = (completed - recovery_point).total_seconds() / 60
    if age_minutes < -1:
        raise RuntimeError("provider recovery point is unexpectedly in the future")
    limits = config.values["global"]["backup"]
    if target.endswith("-mysql"):
        rpo, rto = limits["mysqlRpoMinutes"], limits["mysqlRtoMinutes"]
    elif target.endswith("-objects"):
        rpo, rto = limits["objectRpoMinutes"], limits["objectRtoMinutes"]
    else:
        rpo, rto = limits["clickhouseRpoMinutes"], limits["clickhouseRtoMinutes"]
    elapsed_minutes = (completed - started).total_seconds() / 60
    if action == "backup" and age_minutes > rpo:
        raise RuntimeError(f"RPO exceeded: {age_minutes:.2f}m > {rpo}m")
    if action == "restore" and elapsed_minutes > rto:
        raise RuntimeError(f"RTO exceeded: {elapsed_minutes:.2f}m > {rto}m")
    evidence = {
        "schemaVersion": "agentx.io/backup-manifest/v1",
        "backupId": backup_id,
        "target": target,
        "operation": action,
        "status": "passed",
        "startedAt": started.isoformat(),
        "completedAt": completed.isoformat(),
        "recoveryPointUtc": recovery_point.isoformat(),
        "objectCount": int(receipt["objectCount"]),
        "contentSha256": str(receipt["contentSha256"]),
        "schemaVersionObserved": str(receipt["schemaVersionObserved"]),
        "restoreTarget": restore_target,
        "providerReceipt": receipt,
    }
    schema = json.loads(
        (config.root / "deploy" / "release" / "backup-manifest.schema.json").read_text(encoding="utf-8")
    )
    Draft202012Validator(schema, format_checker=Draft202012Validator.FORMAT_CHECKER).validate(evidence)
    artifact_dir.mkdir(parents=True, exist_ok=True)
    output = artifact_dir / f"{backup_id}-{target}-{action}.json"
    output.write_text(json.dumps(evidence, ensure_ascii=False, indent=2), encoding="utf-8")
    evidence["path"] = str(output)
    return evidence


def sync_secrets(config: DeploymentConfig) -> dict[str, Any]:
    updated = sync_existing_mirrors(config)
    restarts = {
        "control": ("platform-control",),
        "runtime": ("runtime-gateway", "workflow-runtime", "workflow-worker", "sandbox-manager"),
        "observability": ("observability",),
        "dependencies": ("agentx-egress-gateway",),
    }
    restarted: list[str] = []
    for target, names in restarts.items():
        if release_status(config, target) is None:
            continue
        for name in names:
            run(
                ("kubectl", "-n", namespace_for(config, target), "rollout", "restart", f"deployment/{name}"),
                timeout=60,
                check=False,
            )
            run(
                (
                    "kubectl",
                    "-n",
                    namespace_for(config, target),
                    "rollout",
                    "status",
                    f"deployment/{name}",
                    "--timeout=300s",
                ),
                timeout=330,
                check=False,
            )
            restarted.append(f"{namespace_for(config, target)}/{name}")
    return {"status": "synced", "updatedSecrets": updated, "restarted": restarted}


def rotate_keys(config: DeploymentConfig, *, apply: bool) -> dict[str, object]:
    return rotate_egress_keys(config, apply=apply)
